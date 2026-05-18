use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

#[derive(Debug, Clone)]
pub struct PreflightOutput {
    pub branch: String,
    pub created_branch: bool,
    pub plan_slug: String,
    pub default_branch: String,
}

pub struct Preflight<'a> {
    pub repo_root: &'a Path,
    pub plan_path: &'a Path,
    pub branch_override: Option<&'a str>,
    pub use_worktree: bool,
    pub allow_dirty: bool,
    /// Skip the workspace-trust precondition. Used by `--tasks-only`
    /// runs against fixture agents (fake-agent.sh etc.) that don't
    /// need Claude Code's trust dialog, and any caller spawning a
    /// non-`claude` command via `--claude-command`.
    pub skip_trust_check: bool,
}

impl<'a> Preflight<'a> {
    pub fn check(&self) -> Result<PreflightOutput> {
        let plan_slug = derive_slug(self.plan_path)?;
        let branch = self
            .branch_override
            .map(|s| s.to_string())
            .unwrap_or_else(|| plan_slug.clone());
        let default_branch = detect_default_branch(self.repo_root)?;

        // One-time migration: previous releases stored state under
        // `.ralphex/`. Rename to `.ralphterm/` if the new dir doesn't
        // already exist. Safe no-op when there's nothing to migrate.
        migrate_dot_ralphex_state(self.repo_root)?;

        if !self.skip_trust_check {
            ensure_workspace_trusted(self.repo_root)?;
        }

        // Detect resume: we're already on the plan's branch from a
        // previous (probably interrupted) run. In that case the dirty
        // files in the worktree are the agent's partial work, not the
        // user's WIP, and we should pick up where we left off rather
        // than refuse to start. Only print a heads-up if files are
        // actually dirty so the operator knows something carried over.
        let current = if self.use_worktree {
            String::new()
        } else {
            current_branch(self.repo_root)?
        };
        let resuming = !self.use_worktree && current == branch;

        if !self.allow_dirty && !self.use_worktree && !resuming {
            let dirty = collect_uncommitted_paths(self.repo_root)?;
            if !dirty.is_empty() {
                bail!(format_dirty_message(&dirty, self.plan_path));
            }
        }

        if resuming {
            let dirty = collect_uncommitted_paths(self.repo_root)?;
            if !dirty.is_empty() {
                let plural = if dirty.len() == 1 { "" } else { "s" };
                eprintln!(
                    "{}",
                    crate::color::info_line(format!(
                        "resuming on branch {branch} with {} uncommitted file{plural} — the next iteration will pick up where the previous run was interrupted",
                        dirty.len()
                    ))
                );
            }
        }

        let mut created = false;
        if !self.use_worktree && !resuming {
            if branch_exists(self.repo_root, &branch)? {
                git(self.repo_root, &["checkout", &branch])
                    .with_context(|| format!("switch to existing branch {branch}"))?;
            } else {
                git(self.repo_root, &["checkout", "-b", &branch])
                    .with_context(|| format!("create branch {branch}"))?;
                created = true;
            }
        }

        Ok(PreflightOutput {
            branch,
            created_branch: created,
            plan_slug,
            default_branch,
        })
    }
}

/// Rename a legacy `.ralphex/` state dir to `.ralphterm/` so users
/// don't lose their trusted sentinel, progress logs, or iteration-output
/// files across the 0.4.x rename. Quiet success when the legacy dir
/// doesn't exist (the typical case) or when both dirs exist (operator
/// likely already migrated).
fn migrate_dot_ralphex_state(repo_root: &Path) -> Result<()> {
    let legacy = repo_root.join(".ralphex");
    let target = repo_root.join(".ralphterm");
    if !legacy.exists() || target.exists() {
        return Ok(());
    }
    std::fs::rename(&legacy, &target)
        .with_context(|| format!("migrate {} → {}", legacy.display(), target.display()))?;
    eprintln!(
        "{}",
        crate::color::info_line(format!(
            "migrated legacy state directory {} → {}",
            legacy.display(),
            target.display()
        ))
    );
    Ok(())
}

/// Verify the workspace has been trusted by Claude Code at least once.
///
/// Claude Code refuses to start (it shows a blocking "Is this project
/// you trust?" dialog) the first time it runs in any directory. The
/// dialog only auto-skips when claude is run via `--print` — which
/// ralphterm by design does NOT use, because Anthropic has signalled
/// they intend to sunset `--print`. So ralphterm requires the operator
/// to satisfy claude's trust check once per workspace, the same way
/// SSH `known_hosts` works: run `claude` in the directory manually,
/// accept the dialog, exit. Ralphterm then records a small sentinel
/// at `.ralphterm/trusted` so it doesn't ask again.
///
/// Escape hatches:
///   - `RALPHTERM_ASSUME_TRUSTED=1` env var — skip the check entirely
///     (for CI / power users who manage trust through their own tooling).
///   - `--claude-command <wrapper>` — when the operator overrides the
///     spawn command, ralphterm passes `skip_trust_check: true` to
///     preflight because the wrapper or alternate binary likely doesn't
///     consult claude's trust system.
pub fn ensure_workspace_trusted(repo_root: &Path) -> Result<()> {
    if std::env::var_os("RALPHTERM_ASSUME_TRUSTED").is_some_and(|v| !v.is_empty()) {
        return Ok(());
    }

    let sentinel = repo_root.join(".ralphterm").join("trusted");
    if sentinel.is_file() {
        return Ok(());
    }

    let stdin_is_tty = std::io::stdin().is_terminal();
    if !stdin_is_tty {
        bail!(format_trust_required_message(
            repo_root, /* interactive: */ false
        ));
    }

    // Interactive prompt.
    let prompt = format!(
        "\nRalphTerm drives the official `claude` CLI inside a real PTY, the way a human does.\n\
         Claude Code requires every workspace to be trusted manually the first time it runs there.\n\n\
         Workspace: {}\n\n\
         Have you already run `claude` in this directory and accepted the\n\
         \"Is this project you trust?\" dialog at least once? [y/N] ",
        repo_root.display()
    );
    let _ = std::io::stderr().write_all(prompt.as_bytes());
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .context("read trust confirmation")?;
    let answer = answer.trim().to_ascii_lowercase();
    if answer != "y" && answer != "yes" {
        bail!(format_trust_required_message(
            repo_root, /* interactive: */ true
        ));
    }

    // Record the sentinel so we never ask again.
    let dir = sentinel.parent().expect("sentinel has parent");
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let ts = chrono::Local::now().to_rfc3339();
    std::fs::write(
        &sentinel,
        format!("accepted_at: {ts}\nworkspace: {}\n", repo_root.display()),
    )
    .with_context(|| format!("write {}", sentinel.display()))?;
    Ok(())
}

fn format_trust_required_message(repo_root: &Path, interactive: bool) -> String {
    let preamble = if interactive {
        "Workspace trust not confirmed."
    } else {
        "Workspace trust required but stdin is not a TTY (no way to ask interactively)."
    };
    format!(
        "{preamble}\n\nRun the following in this directory, accept the trust dialog, then exit (Ctrl+D):\n\n  cd {}\n  claude\n\nThen re-run ralphterm. If you've already accepted trust in claude but want to\nbypass this check (CI, scripted runs), set RALPHTERM_ASSUME_TRUSTED=1.\n\nBackground: claude requires per-workspace trust acceptance and only auto-skips\nthat dialog when run with --print. RalphTerm intentionally does not use --print\n(Anthropic has signalled they intend to sunset it). See\nhttps://code.claude.com/docs/en/security",
        repo_root.display()
    )
}

fn derive_slug(plan_path: &Path) -> Result<String> {
    let stem = plan_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid plan filename: {}", plan_path.display()))?;
    let slug: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        bail!("plan filename produced empty slug: {}", plan_path.display());
    }
    Ok(slug)
}

fn detect_default_branch(repo: &Path) -> Result<String> {
    // Try common names in order; fall back to current.
    for candidate in ["main", "master", "trunk"] {
        if branch_exists(repo, candidate)? {
            return Ok(candidate.to_string());
        }
    }
    current_branch(repo)
}

fn branch_exists(repo: &Path, name: &str) -> Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--verify", "--quiet", name])
        .output()
        .context("git rev-parse")?;
    Ok(output.status.success())
}

fn current_branch(repo: &Path) -> Result<String> {
    let output = git_output(repo, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    Ok(output.trim().to_string())
}

/// Directory / file entries that represent per-machine tool state, not
/// user work. We filter these out of the preflight dirty-worktree check
/// because they bootstrap themselves (`.ralphterm/trusted` is written by
/// our own trust prompt) or get created by adjacent CLI tools that
/// users run alongside ralphterm. Most are already in users' global
/// gitignore but plenty of fresh workspaces don't ignore them locally.
const TOOL_STATE_PATHS: &[&str] = &[
    // ralphterm's own state.
    ".ralphterm",
    // Claude Code / Codex / other interactive AI CLIs that drop a
    // dot-dir into the workspace on first run.
    ".claude",
    ".codex",
    ".cursor",
    ".aider",
    // OS noise.
    ".DS_Store",
    "Thumbs.db",
];

fn is_tool_state_path(p: &str) -> bool {
    let p = p.trim_start_matches("./");
    TOOL_STATE_PATHS.iter().any(|prefix| {
        p == *prefix || p.starts_with(&format!("{prefix}/")) || p.starts_with(&format!("{prefix}."))
    })
}

fn collect_uncommitted_paths(repo: &Path) -> Result<Vec<String>> {
    let output = git_output(repo, &["status", "--porcelain"])?;
    Ok(output
        .lines()
        .map(|line| line.get(3..).unwrap_or(line).to_string())
        .filter(|p| !p.is_empty())
        .filter(|p| !is_tool_state_path(p))
        .collect())
}

fn format_dirty_message(paths: &[String], plan_path: &Path) -> String {
    let mut buf = String::from(
        "create branch for plan: cannot create branch \"<derived>\": worktree has uncommitted changes\n\nuncommitted files:\n",
    );
    let shown: Vec<&String> = paths.iter().take(10).collect();
    for p in &shown {
        buf.push_str("  ");
        buf.push_str(p);
        buf.push('\n');
    }
    if paths.len() > shown.len() {
        buf.push_str(&format!("  ... and {} more\n", paths.len() - shown.len()));
    }
    buf.push_str(
        "\nralphterm needs to create a feature branch from master to isolate plan work.\n\noptions:\n",
    );
    buf.push_str(&format!(
        "  git stash && ralphterm {} && git stash pop   # stash changes temporarily\n",
        plan_path.display()
    ));
    buf.push_str("  git commit -am \"wip\"                       # commit changes first\n");
    buf.push_str("  ralphterm --review                           # skip branch creation (review-only mode)\n");
    buf
}

fn git(repo: &Path, args: &[&str]) -> Result<()> {
    // Use `output()` not `status()` so git's stderr (e.g. "Switched to a
    // new branch 'hello'") doesn't leak into our stdout/stderr — ralphex
    // suppresses these messages and our verification harness diffs the
    // user-visible output.
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .with_context(|| format!("git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn git_output(repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .with_context(|| format!("git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} exited {}: {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8(output.stdout).context("git output non-utf8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn init_repo() -> PathBuf {
        let tmp = std::env::temp_dir().join(format!(
            "rt-preflight-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&tmp)
            .args(["init", "-q"])
            .status()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&tmp)
            .args(["config", "user.email", "t@e"])
            .status()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&tmp)
            .args(["config", "user.name", "t"])
            .status()
            .unwrap();
        std::fs::write(tmp.join("README"), "init").unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&tmp)
            .args(["add", "-A"])
            .status()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&tmp)
            .args(["commit", "-q", "-m", "init"])
            .status()
            .unwrap();
        tmp
    }

    #[test]
    fn ensure_workspace_trusted_passes_when_sentinel_exists() {
        let repo = init_repo();
        std::fs::create_dir_all(repo.join(".ralphterm")).unwrap();
        std::fs::write(
            repo.join(".ralphterm").join("trusted"),
            "accepted_at: 2026-05-18T00:00:00Z\n",
        )
        .unwrap();
        ensure_workspace_trusted(&repo).expect("trusted sentinel should pass");
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn ensure_workspace_trusted_passes_when_env_override_set() {
        let repo = init_repo();
        // SAFETY: Tests in this binary may run multi-threaded; the env
        // var is the public contract of the helper and other tests do
        // not depend on it being unset. We restore on the way out.
        let prev = std::env::var_os("RALPHTERM_ASSUME_TRUSTED");
        unsafe {
            std::env::set_var("RALPHTERM_ASSUME_TRUSTED", "1");
        }
        let result = ensure_workspace_trusted(&repo);
        unsafe {
            match prev {
                Some(v) => std::env::set_var("RALPHTERM_ASSUME_TRUSTED", v),
                None => std::env::remove_var("RALPHTERM_ASSUME_TRUSTED"),
            }
        }
        result.expect("env override should pass");
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn ensure_workspace_trusted_refuses_when_no_sentinel_and_not_a_tty() {
        let repo = init_repo();
        // stdin is a pipe in `cargo test`, never a TTY → expect refusal
        // with the "not a TTY" branch of the error message.
        let prev = std::env::var_os("RALPHTERM_ASSUME_TRUSTED");
        unsafe {
            std::env::remove_var("RALPHTERM_ASSUME_TRUSTED");
        }
        let err = ensure_workspace_trusted(&repo).unwrap_err().to_string();
        unsafe {
            if let Some(v) = prev {
                std::env::set_var("RALPHTERM_ASSUME_TRUSTED", v);
            }
        }
        assert!(
            err.contains("Workspace trust required") && err.contains("not a TTY"),
            "expected trust-required error, got: {err}"
        );
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn slug_derivation_lowercases_and_strips_non_alnum() {
        let s = derive_slug(Path::new("docs/plans/My Feature_Plan!.md")).unwrap();
        assert_eq!(s, "my-feature_plan");
    }

    #[test]
    fn refuses_dirty_worktree_unless_worktree_or_allow_dirty() {
        let repo = init_repo();
        std::fs::write(repo.join("dirty.txt"), "x").unwrap();
        let plan = repo.join("docs/plans/hello.md");
        std::fs::create_dir_all(plan.parent().unwrap()).unwrap();
        std::fs::write(&plan, "# plan\n").unwrap();
        let p = Preflight {
            repo_root: &repo,
            plan_path: &plan,
            branch_override: None,
            use_worktree: false,
            allow_dirty: false,
            skip_trust_check: true,
        };
        let err = p.check().unwrap_err().to_string();
        assert!(err.contains("uncommitted files"), "{err}");
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn already_on_plan_branch_resumes_without_dirty_refusal() {
        // Repro: user runs ralphterm against a plan, agent makes
        // changes mid-iteration, user hits Ctrl+C. Re-running the
        // same command should pick up where it left off, not bail
        // because of the agent's own uncommitted files. Fix: when
        // current_branch == plan-derived branch we treat the run as
        // a resume and skip the dirty refusal.
        let repo = init_repo();
        let plan = repo.join("docs/plans/hello.md");
        std::fs::create_dir_all(plan.parent().unwrap()).unwrap();
        std::fs::write(&plan, "# plan\n").unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "-A"])
            .status()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-q", "-m", "init"])
            .status()
            .unwrap();
        // Simulate a previous interrupted run: branch exists, we're
        // on it, and there's an uncommitted agent edit.
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["checkout", "-q", "-b", "hello"])
            .status()
            .unwrap();
        std::fs::write(
            repo.join("partial-work.txt"),
            "agent left this mid-iteration\n",
        )
        .unwrap();

        let out = Preflight {
            repo_root: &repo,
            plan_path: &plan,
            branch_override: None,
            use_worktree: false,
            allow_dirty: false,
            skip_trust_check: true,
        }
        .check()
        .expect("resume on plan branch should not bail on dirty worktree");
        assert_eq!(out.branch, "hello");
        // We did not CREATE the branch this run — we were already on it.
        assert!(!out.created_branch);
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn tool_state_dirs_do_not_count_as_dirty_worktree() {
        // Repro: trust prompt creates .ralphterm/trusted, dirty check
        // sees .ralphterm/ as untracked, run bails before ever spawning
        // the agent. Reported by user on 2026-05-18 in a fresh
        // workspace. Same class of bug for .claude/, .codex/, etc.
        // dropped by adjacent AI CLIs. Fix: collect_uncommitted_paths
        // filters TOOL_STATE_PATHS out of the porcelain output.
        let repo = init_repo();
        let plan = repo.join("docs/plans/hello.md");
        std::fs::create_dir_all(plan.parent().unwrap()).unwrap();
        std::fs::write(&plan, "# plan\n").unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "-A"])
            .status()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-q", "-m", "add plan"])
            .status()
            .unwrap();
        // Simulate ensure_workspace_trusted + drive_agent state, plus
        // adjacent AI/IDE/OS noise that fresh workspaces often have.
        std::fs::create_dir_all(repo.join(".ralphterm/iteration-output")).unwrap();
        std::fs::write(repo.join(".ralphterm/trusted"), "accepted_at: 2026-05-18\n").unwrap();
        std::fs::write(
            repo.join(".ralphterm/iteration-output/abc123.md"),
            "<<<BEGIN>>>\nfoo\n<<<END>>>\n",
        )
        .unwrap();
        std::fs::create_dir_all(repo.join(".claude")).unwrap();
        std::fs::write(repo.join(".claude/settings.local.json"), "{}\n").unwrap();
        std::fs::create_dir_all(repo.join(".codex")).unwrap();
        std::fs::write(repo.join(".codex/state"), "x\n").unwrap();
        std::fs::write(repo.join(".DS_Store"), "\0\0").unwrap();
        std::fs::write(repo.join(".aider.chat.history.md"), "x\n").unwrap();

        Preflight {
            repo_root: &repo,
            plan_path: &plan,
            branch_override: None,
            use_worktree: false,
            allow_dirty: false,
            skip_trust_check: true,
        }
        .check()
        .expect("tool-state paths should not block the preflight clean-worktree check");
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn creates_branch_from_plan_slug_when_clean() {
        let repo = init_repo();
        let plan = repo.join("docs/plans/hello.md");
        std::fs::create_dir_all(plan.parent().unwrap()).unwrap();
        std::fs::write(&plan, "# plan\n").unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "-A"])
            .status()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-q", "-m", "add plan"])
            .status()
            .unwrap();
        let out = Preflight {
            repo_root: &repo,
            plan_path: &plan,
            branch_override: None,
            use_worktree: false,
            allow_dirty: false,
            skip_trust_check: true,
        }
        .check()
        .unwrap();
        assert_eq!(out.branch, "hello");
        assert!(out.created_branch);
        let current = current_branch(&repo).unwrap();
        assert_eq!(current, "hello");
        let _ = std::fs::remove_dir_all(&repo);
    }
}
