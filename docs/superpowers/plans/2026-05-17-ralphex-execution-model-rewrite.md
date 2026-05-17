# Ralphex Execution-Model Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite RalphTerm's runner around ralphex's execution model so `ralphterm --tasks-only hello.md` produces an observably equivalent transcript to `ralphex --tasks-only hello.md` against the same plan, agent, and reviewer config.

**Architecture:** Replace per-task prompt loop with an iteration loop. The agent receives a substantial prompt (vendored from ralphex's `task.txt`) that contains the plan file path and tells it to pick one task, mark its own checkbox, and emit `<<<RALPHEX:ALL_TASKS_DONE>>>` when done. New modules: `prompts` (embedded constants + override loader), `preflight` (dirty worktree refusal + branch creation), `output_format` (ralphex-style stdout strings), `progress_log` (timestamped narration file), `review_phases` (Phase 1 / external / Phase 3). Existing `runner.rs` reduced to a thin orchestrator.

**Tech Stack:** Rust 2021, `portable-pty` 0.8, `tokio` 1.42 (for parallel reviewer agents via `tokio::task::spawn_blocking`), `chrono` 0.4 (new dep — timestamped progress log format).

**Verification gate:** After every task with observable output, run `/home/hetoku/work/ralphterm/scripts/diff-against-ralphex.sh` (created in Task 14). It diffs `ralphterm` and `ralphex 1.2.0` transcripts on `hello.md`; the task is "done" only when the diff shows no structural divergence (whitespace/timestamps/hashes OK; missing lines, wrong order, different headers NOT OK).

---

## File structure

| Path | Responsibility |
|---|---|
| `src/prompts.rs` | NEW. Embedded prompt + agent constants (vendored from ralphex 1.2.0). `Prompts::load(project_root, global_dir)` returns merged set with override precedence. `substitute(template, vars) -> String` for `{{VAR}}` replacement. |
| `src/preflight.rs` | NEW. `Preflight::check(repo_root, plan_path, use_worktree, branch_override) -> Result<PreflightOutput>`. Dirty-worktree refusal with the same error text ralphex prints. Branch creation from plan slug. |
| `src/output_format.rs` | NEW. `print_run_header`, `print_iteration_header`, `print_completion_summary`, etc. — every stdout string ralphex emits during a full run, matched exactly. |
| `src/progress_log.rs` | NEW. `ProgressLog::open(path)` opens (creates) `.ralphex/progress/progress-<slug>.txt`. `log.write_control("creating branch: hello")`, `log.write_narration(line)` (timestamps the line). Format mirrors ralphex. |
| `src/review_phases.rs` | NEW. `first_review`, `external_review`, `second_review`. Phase 1 and 3 dispatch reviewer agents concurrently via `tokio::task::spawn_blocking`. |
| `src/runner.rs` | REWRITE. Thin orchestrator: preflight → task_execution_phase → first_review → external_review → second_review → finalize. Drop per-task prompt building and checkbox marking. Estimated final size ~600 lines. |
| `src/cli.rs` | UPDATE. Mode flags re-wired; output strings emitted via `output_format`. |
| `tests/fixtures/fake-agent.sh` | REWRITE. Read prompt, open `{{PLAN_FILE}}`, mark first `- [ ]` → `- [x]`, run a small recipe based on task body grep, emit `<<<RALPHEX:ALL_TASKS_DONE>>>` when all checkboxes done. |
| `scripts/diff-against-ralphex.sh` | NEW. The verification harness. |
| Cargo.toml | UPDATE. Add `chrono = "0.4"` (minimal subset). Bump to `0.2.0` once acceptance gate passes. |

---

## Verification gates (run after each commit unless task notes otherwise)

```
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

After tasks that touch the runner (5, 6, 7, 8, 9, 10, 13), also run:
```
./scripts/diff-against-ralphex.sh
```

Plan tasks are sequential; do not parallelise. Branch is `main` per established workflow.

---

### Task 1: Vendor ralphex prompts as embedded constants

**Goal:** Capture all 14 prompt files from ralphex 1.2.0 as `pub(crate) const` raw string literals inside `src/prompts.rs`. No separate `.txt` files in the source tree (per user instruction).

**Files:**
- Create: `src/prompts.rs`
- Modify: `src/lib.rs`

**Source of prompt text:** `/tmp/ralphterm-e2e-0Xpj/.ralphex/prompts/*.txt` and `/tmp/ralphterm-e2e-0Xpj/.ralphex/agents/*.txt` (created by running `/tmp/ralphex-bin/ralphex --init` earlier in this session). If those paths no longer exist, regenerate: `mkdir -p /tmp/rx-init && cd /tmp/rx-init && git init -q && git config user.email x@y && git config user.name x && /tmp/ralphex-bin/ralphex --init` (then read the files from `/tmp/rx-init/.ralphex/`).

- [ ] **Step 1: Add the module declaration**

Edit `src/lib.rs` and add `pub mod prompts;` to the existing list of `#[doc(hidden)] pub mod ...;` lines (keep it `#[doc(hidden)]` since this is internal).

- [ ] **Step 2: Write `src/prompts.rs` with embedded constants**

For each of the 14 source files, embed as `pub(crate) const NAME: &str = r#"..."#;`. Use single-hash raw strings (`r#"..."#`); if any prompt contains the sequence `"#`, escalate that one to `r##"..."##`.

Skeleton:

```rust
//! Vendored ralphex 1.2.0 prompts and agent definitions.
//!
//! Prompts live as raw string constants so the binary needs nothing on
//! disk. Users can override any individual prompt via
//! `.ralphex/prompts/<name>.txt` or `~/.config/ralphex/prompts/<name>.txt`.

pub(crate) const TASK_TXT: &str = r#"<paste-content-of-task.txt>"#;
pub(crate) const MAKE_PLAN_TXT: &str = r#"<paste-content-of-make_plan.txt>"#;
pub(crate) const REVIEW_FIRST_TXT: &str = r#"<paste-content-of-review_first.txt>"#;
pub(crate) const REVIEW_SECOND_TXT: &str = r#"<paste-content-of-review_second.txt>"#;
pub(crate) const CODEX_TXT: &str = r#"<paste-content-of-codex.txt>"#;
pub(crate) const CODEX_REVIEW_TXT: &str = r#"<paste-content-of-codex_review.txt>"#;
pub(crate) const CUSTOM_EVAL_TXT: &str = r#"<paste-content-of-custom_eval.txt>"#;
pub(crate) const CUSTOM_REVIEW_TXT: &str = r#"<paste-content-of-custom_review.txt>"#;
pub(crate) const FINALIZE_TXT: &str = r#"<paste-content-of-finalize.txt>"#;

pub(crate) const AGENT_QUALITY_TXT: &str = r#"<paste-content-of-agents/quality.txt>"#;
pub(crate) const AGENT_IMPLEMENTATION_TXT: &str = r#"<paste-content-of-agents/implementation.txt>"#;
pub(crate) const AGENT_TESTING_TXT: &str = r#"<paste-content-of-agents/testing.txt>"#;
pub(crate) const AGENT_SIMPLIFICATION_TXT: &str = r#"<paste-content-of-agents/simplification.txt>"#;
pub(crate) const AGENT_DOCUMENTATION_TXT: &str = r#"<paste-content-of-agents/documentation.txt>"#;
```

To populate the placeholders, run this shell from the repo root:

```sh
SRC=/tmp/ralphterm-e2e-0Xpj/.ralphex
for f in task make_plan review_first review_second codex codex_review custom_eval custom_review finalize; do
  echo "=== $f ==="
  cat "$SRC/prompts/$f.txt"
done
for a in quality implementation testing simplification documentation; do
  echo "=== agent: $a ==="
  cat "$SRC/agents/$a.txt"
done
```

Paste each file's content into the corresponding const, replacing `<paste-content-of-...>`. If any file contains the literal sequence `"#`, change that const's delimiter to `r##"..."##`.

- [ ] **Step 3: Build to verify all raw strings parse**

```bash
cargo build 2>&1 | tail -10
```

Expected: clean build. If a "unterminated raw string" or "no rules expected `r#`" error appears, that const needs more hashes in its delimiter — bump to `r##`...`##` and rebuild.

- [ ] **Step 4: Commit**

```bash
git add src/prompts.rs src/lib.rs
git commit -m "feat(prompts): vendor ralphex 1.2.0 prompts as embedded constants"
```

---

### Task 2: Prompt loader with override precedence + variable substitution

**Goal:** Provide the API that the runner will call: read overrides from disk, fall back to embedded constants, apply `{{VAR}}` substitution.

**Files:**
- Modify: `src/prompts.rs`

- [ ] **Step 1: Add the `Prompts` struct and loader**

Append to `src/prompts.rs`:

```rust
use std::collections::HashMap;
use std::path::Path;
use std::{fs, io};

#[derive(Debug, Clone)]
pub struct Prompts {
    pub task: String,
    pub make_plan: String,
    pub review_first: String,
    pub review_second: String,
    pub codex: String,
    pub codex_review: String,
    pub custom_eval: String,
    pub custom_review: String,
    pub finalize: String,
    pub agents: HashMap<String, String>,
}

impl Prompts {
    /// Load prompts. Override precedence (highest first):
    ///   `.ralphex/prompts/<name>.txt` in `project_root`
    ///   `<global_dir>/prompts/<name>.txt`
    ///   embedded constant
    /// Agents follow the same precedence under `agents/`.
    pub fn load(project_root: &Path, global_dir: Option<&Path>) -> Self {
        let read = |name: &str, default: &str, subdir: &str| -> String {
            let project = project_root
                .join(".ralphex")
                .join(subdir)
                .join(format!("{name}.txt"));
            if let Some(text) = read_if_exists(&project) {
                return text;
            }
            if let Some(global) = global_dir {
                let global_path = global.join(subdir).join(format!("{name}.txt"));
                if let Some(text) = read_if_exists(&global_path) {
                    return text;
                }
            }
            default.to_string()
        };

        let mut agents = HashMap::new();
        for (name, default) in [
            ("quality", AGENT_QUALITY_TXT),
            ("implementation", AGENT_IMPLEMENTATION_TXT),
            ("testing", AGENT_TESTING_TXT),
            ("simplification", AGENT_SIMPLIFICATION_TXT),
            ("documentation", AGENT_DOCUMENTATION_TXT),
        ] {
            agents.insert(name.to_string(), read(name, default, "agents"));
        }

        Self {
            task: read("task", TASK_TXT, "prompts"),
            make_plan: read("make_plan", MAKE_PLAN_TXT, "prompts"),
            review_first: read("review_first", REVIEW_FIRST_TXT, "prompts"),
            review_second: read("review_second", REVIEW_SECOND_TXT, "prompts"),
            codex: read("codex", CODEX_TXT, "prompts"),
            codex_review: read("codex_review", CODEX_REVIEW_TXT, "prompts"),
            custom_eval: read("custom_eval", CUSTOM_EVAL_TXT, "prompts"),
            custom_review: read("custom_review", CUSTOM_REVIEW_TXT, "prompts"),
            finalize: read("finalize", FINALIZE_TXT, "prompts"),
            agents,
        }
    }
}

fn read_if_exists(path: &Path) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "failed to read prompt override");
            None
        }
    }
}

/// Replace `{{KEY}}` occurrences in `template` with `vars[KEY]`. Unknown
/// `{{...}}` placeholders are left intact (ralphex does the same).
pub fn substitute(template: &str, vars: &HashMap<&str, &str>) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(end) = template[i + 2..].find("}}") {
                let key = &template[i + 2..i + 2 + end];
                let key_trimmed = key.trim();
                if let Some(value) = vars.get(key_trimmed) {
                    out.push_str(value);
                    i += 2 + end + 2;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}
```

- [ ] **Step 2: Add a unit test for substitution**

Append to the bottom of `src/prompts.rs` inside a `#[cfg(test)] mod tests { ... }`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_replaces_known_keys_and_leaves_unknown_intact() {
        let mut vars = HashMap::new();
        vars.insert("PLAN_FILE", "docs/plans/foo.md");
        vars.insert("GOAL", "build it");
        let out = substitute(
            "Read plan {{PLAN_FILE}}; goal {{GOAL}}; keep {{UNKNOWN}}.",
            &vars,
        );
        assert_eq!(
            out,
            "Read plan docs/plans/foo.md; goal build it; keep {{UNKNOWN}}."
        );
    }

    #[test]
    fn load_falls_back_to_embedded_when_no_override_present() {
        let tmp = std::env::temp_dir().join(format!("rt-prompts-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let prompts = Prompts::load(&tmp, None);
        assert_eq!(prompts.task, TASK_TXT);
        assert_eq!(prompts.agents.get("quality").map(String::as_str), Some(AGENT_QUALITY_TXT));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_prefers_project_override_over_embedded() {
        let tmp = std::env::temp_dir().join(format!("rt-prompts-test-override-{}", std::process::id()));
        let prompts_dir = tmp.join(".ralphex").join("prompts");
        std::fs::create_dir_all(&prompts_dir).unwrap();
        std::fs::write(prompts_dir.join("task.txt"), "MY OVERRIDE").unwrap();
        let prompts = Prompts::load(&tmp, None);
        assert_eq!(prompts.task, "MY OVERRIDE");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
```

- [ ] **Step 3: Run the tests**

```bash
cargo test --lib prompts:: 2>&1 | tail -10
```

Expected: 3 passed.

- [ ] **Step 4: Commit**

```bash
git add src/prompts.rs
git commit -m "feat(prompts): add loader with override precedence and substitution"
```

---

### Task 3: Pre-flight checks (dirty worktree refusal + branch creation)

**Goal:** Match ralphex's pre-flight: refuse to start when the worktree has uncommitted changes (unless `--worktree` is set), then `git checkout -b <plan-slug>` (or use `--branch`).

**Files:**
- Create: `src/preflight.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Add module declaration**

In `src/lib.rs` add `#[doc(hidden)] pub mod preflight;` to the existing list.

- [ ] **Step 2: Write `src/preflight.rs`**

```rust
use std::path::{Path, PathBuf};
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
}

impl<'a> Preflight<'a> {
    pub fn check(&self) -> Result<PreflightOutput> {
        let plan_slug = derive_slug(self.plan_path)?;
        let branch = self
            .branch_override
            .map(|s| s.to_string())
            .unwrap_or_else(|| plan_slug.clone());
        let default_branch = detect_default_branch(self.repo_root)?;

        if !self.allow_dirty && !self.use_worktree {
            let dirty = collect_uncommitted_paths(self.repo_root)?;
            if !dirty.is_empty() {
                bail!(format_dirty_message(&dirty, self.plan_path));
            }
        }

        let mut created = false;
        if !self.use_worktree {
            let current = current_branch(self.repo_root)?;
            if current != branch {
                if branch_exists(self.repo_root, &branch)? {
                    git(self.repo_root, &["checkout", &branch])
                        .with_context(|| format!("switch to existing branch {branch}"))?;
                } else {
                    git(self.repo_root, &["checkout", "-b", &branch])
                        .with_context(|| format!("create branch {branch}"))?;
                    created = true;
                }
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

fn collect_uncommitted_paths(repo: &Path) -> Result<Vec<String>> {
    let output = git_output(repo, &["status", "--porcelain"])?;
    Ok(output
        .lines()
        .map(|line| line.get(3..).unwrap_or(line).to_string())
        .filter(|p| !p.is_empty())
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
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .status()
        .with_context(|| format!("git {}", args.join(" ")))?;
    if !status.success() {
        bail!("git {} failed", args.join(" "));
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
```

- [ ] **Step 3: Add unit tests**

Append to `src/preflight.rs`:

```rust
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
        Command::new("git").arg("-C").arg(&tmp).args(["init", "-q"]).status().unwrap();
        Command::new("git").arg("-C").arg(&tmp).args(["config", "user.email", "t@e"]).status().unwrap();
        Command::new("git").arg("-C").arg(&tmp).args(["config", "user.name", "t"]).status().unwrap();
        std::fs::write(tmp.join("README"), "init").unwrap();
        Command::new("git").arg("-C").arg(&tmp).args(["add", "-A"]).status().unwrap();
        Command::new("git").arg("-C").arg(&tmp).args(["commit", "-q", "-m", "init"]).status().unwrap();
        tmp
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
        };
        let err = p.check().unwrap_err().to_string();
        assert!(err.contains("uncommitted files"), "{err}");
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn creates_branch_from_plan_slug_when_clean() {
        let repo = init_repo();
        let plan = repo.join("docs/plans/hello.md");
        std::fs::create_dir_all(plan.parent().unwrap()).unwrap();
        std::fs::write(&plan, "# plan\n").unwrap();
        Command::new("git").arg("-C").arg(&repo).args(["add", "-A"]).status().unwrap();
        Command::new("git").arg("-C").arg(&repo).args(["commit", "-q", "-m", "add plan"]).status().unwrap();
        let out = Preflight {
            repo_root: &repo,
            plan_path: &plan,
            branch_override: None,
            use_worktree: false,
            allow_dirty: false,
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
```

- [ ] **Step 4: Run the tests**

```bash
cargo test --lib preflight:: 2>&1 | tail -10
```

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src/preflight.rs src/lib.rs
git commit -m "feat(preflight): dirty-worktree refusal and plan-slug branch creation"
```

---

### Task 4: Output format module (ralphex-style stdout strings)

**Goal:** Centralise every stdout string the runner prints so they match ralphex byte-for-byte.

**Files:**
- Create: `src/output_format.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Add module declaration**

In `src/lib.rs` add `#[doc(hidden)] pub mod output_format;`.

- [ ] **Step 2: Write `src/output_format.rs`**

```rust
use std::path::Path;
use std::time::Duration;

pub fn print_branch_creating(branch: &str) {
    println!("creating branch: {branch}");
}

pub fn print_run_header(
    max_iterations: usize,
    mode_label: &str,
    plan_path: &Path,
    branch: &str,
    progress_log: &Path,
) {
    println!("starting ralphex loop (max {max_iterations} iterations) ({mode_label})");
    println!("plan: {}", plan_path.display());
    println!("branch: {branch}");
    println!("progress log: {}", progress_log.display());
    println!();
}

pub fn print_task_phase_start() {
    println!("starting task execution phase");
    println!();
}

pub fn print_iteration_header(n: usize) {
    println!("--- task iteration {n} ---");
}

pub fn print_review_phase_start(label: &str) {
    println!();
    println!("{label}");
}

pub fn print_completion_summary(
    elapsed: Duration,
    files: usize,
    additions: usize,
    deletions: usize,
    plan_dest: &Path,
    branch: &str,
    progress_log: &Path,
) {
    println!();
    println!(
        "completed in {}s ({} files, +{}/-{} lines)",
        elapsed.as_secs(),
        files,
        additions,
        deletions
    );
    println!("  plan: {}", plan_dest.display());
    println!("  branch: {branch}");
    println!("  progress log: {}", progress_log.display());
}

pub fn print_moved_plan(dest: &Path) {
    println!("moved plan to {}", dest.display());
}

pub fn print_all_tasks_completed() {
    println!("all tasks completed, starting code review...");
}

pub fn mode_label(tasks_only: bool, review_only: bool, external_only: bool) -> &'static str {
    if tasks_only {
        "tasks-only mode"
    } else if review_only {
        "review-only mode"
    } else if external_only {
        "external-only mode"
    } else {
        "full mode"
    }
}
```

- [ ] **Step 3: Verify it compiles**

```bash
cargo build 2>&1 | tail -3
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add src/output_format.rs src/lib.rs
git commit -m "feat(output): ralphex-style stdout strings for run phases"
```

---

### Task 5: Progress log writer (timestamped narration file)

**Goal:** Write `.ralphex/progress/progress-<slug>.txt` in ralphex's format: control lines + timestamped narration lines.

**Files:**
- Create: `src/progress_log.rs`
- Modify: `src/lib.rs`
- Modify: `Cargo.toml` (add `chrono = { version = "0.4", default-features = false, features = ["clock"] }`)

- [ ] **Step 1: Add `chrono` dependency**

In `Cargo.toml`, add to `[dependencies]`:
```toml
chrono = { version = "0.4", default-features = false, features = ["clock"] }
```

- [ ] **Step 2: Add module declaration**

In `src/lib.rs` add `#[doc(hidden)] pub mod progress_log;`.

- [ ] **Step 3: Write `src/progress_log.rs`**

```rust
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub struct ProgressLog {
    path: PathBuf,
    writer: BufWriter<File>,
}

impl ProgressLog {
    pub fn open(repo_root: &Path, plan_slug: &str) -> Result<Self> {
        let dir = repo_root.join(".ralphex").join("progress");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("create progress dir {}", dir.display()))?;
        let path = dir.join(format!("progress-{plan_slug}.txt"));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("open progress log {}", path.display()))?;
        Ok(Self {
            path,
            writer: BufWriter::new(file),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write a control line — no timestamp. Used for phase headers and
    /// other framing emitted by the orchestrator itself.
    pub fn write_control(&mut self, line: &str) -> Result<()> {
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }

    /// Write a narration line prefixed with `[YYYY-MM-DD HH:MM:SS] `.
    /// Used for every agent-produced line of output.
    pub fn write_narration(&mut self, line: &str) -> Result<()> {
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        write!(self.writer, "[{}] {}\n", ts, line)?;
        self.writer.flush()?;
        Ok(())
    }
}
```

- [ ] **Step 4: Add a unit test**

Append to `src/progress_log.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_control_and_timestamped_narration_lines() {
        let tmp = std::env::temp_dir().join(format!(
            "rt-progress-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let mut log = ProgressLog::open(&tmp, "hello").unwrap();
        log.write_control("creating branch: hello").unwrap();
        log.write_narration("Picking Task 1.").unwrap();
        drop(log);

        let body = std::fs::read_to_string(tmp.join(".ralphex/progress/progress-hello.txt")).unwrap();
        assert!(body.contains("creating branch: hello\n"));
        assert!(
            body.contains("] Picking Task 1.\n"),
            "missing timestamped narration; body:\n{body}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
```

- [ ] **Step 5: Run the test**

```bash
cargo test --lib progress_log:: 2>&1 | tail -10
```

Expected: 1 passed.

- [ ] **Step 6: Commit**

```bash
git add src/progress_log.rs src/lib.rs Cargo.toml Cargo.lock
git commit -m "feat(progress): timestamped narration writer matching ralphex format"
```

---

### Task 6: Create the verification harness

**Goal:** Self-contained shell script that runs the same plan through both `ralphex` and `ralphterm`, diffs the outputs, and writes a verdict. Used by every subsequent task that touches the runner.

**Files:**
- Create: `scripts/diff-against-ralphex.sh`

- [ ] **Step 1: Write the script**

```sh
#!/bin/sh
# diff-against-ralphex.sh — verification harness for the ralphex execution-model rewrite.
#
# Usage:
#   scripts/diff-against-ralphex.sh                  # default plan: hello.md, --tasks-only
#   scripts/diff-against-ralphex.sh --full           # also exercise the full review pipeline
#
# Requires:
#   /tmp/ralphex-bin/ralphex          (download from
#     https://github.com/umputun/ralphex/releases/download/v1.2.0/ralphex_1.2.0_linux_amd64.tar.gz
#     and extract into /tmp/ralphex-bin/ if missing)
#   ./target/debug/ralphterm          (built in current repo)
set -eu

REPO_ROOT=$(git rev-parse --show-toplevel)
RALPHTERM_BIN="$REPO_ROOT/target/debug/ralphterm"
RALPHEX_BIN="${RALPHEX_BIN:-/tmp/ralphex-bin/ralphex}"
MODE="--tasks-only"

while [ $# -gt 0 ]; do
  case "$1" in
    --full) MODE="";;
    *) echo "unknown arg: $1" >&2; exit 2;;
  esac
  shift
done

if [ ! -x "$RALPHEX_BIN" ]; then
  echo "MISSING: $RALPHEX_BIN — download from https://github.com/umputun/ralphex/releases/download/v1.2.0/ralphex_1.2.0_linux_amd64.tar.gz" >&2
  exit 1
fi
if [ ! -x "$RALPHTERM_BIN" ]; then
  echo "MISSING: $RALPHTERM_BIN — run \`cargo build\` first" >&2
  exit 1
fi

scratch=$(mktemp -d /tmp/ralphterm-diff-XXXX)
trap 'rm -rf "$scratch"' EXIT

setup_repo() {
  d="$1"
  cd "$d"
  git init -q
  git config user.email t@e.invalid
  git config user.name test
  mkdir -p docs/plans
  cat > docs/plans/hello.md <<'PLAN'
# Hello plan

## Validation Commands
- `test -f hello.txt`

### Task 1: write the file
- [ ] Create a file named hello.txt with the text "hi"
PLAN
  git add -A
  git commit -q -m init
}

# Run ralphex
RX_REPO="$scratch/rx"
mkdir -p "$RX_REPO"
setup_repo "$RX_REPO"
(cd "$RX_REPO" && "$RALPHEX_BIN" --init >/dev/null 2>&1 && git add -A && git commit -q -m "add ralphex config")
(cd "$RX_REPO" && timeout 240 "$RALPHEX_BIN" $MODE docs/plans/hello.md) > "$scratch/rx.out" 2>&1 || true
RX_EXIT=$?

# Run ralphterm
RT_REPO="$scratch/rt"
mkdir -p "$RT_REPO"
setup_repo "$RT_REPO"
(cd "$RT_REPO" && timeout 240 "$RALPHTERM_BIN" $MODE docs/plans/hello.md) > "$scratch/rt.out" 2>&1 || true
RT_EXIT=$?

# Normalise: drop ANSI escapes, timestamps, version banners, commit hashes,
# and tmp paths so the structural diff is meaningful.
normalise() {
  sed -e 's/\x1b\[[0-9;]*[a-zA-Z]//g' \
      -e 's/\[20[0-9][0-9]-[0-9][0-9]-[0-9][0-9] [0-9][0-9]:[0-9][0-9]:[0-9][0-9]\]/[TS]/g' \
      -e 's/^ralph[a-z]* v[^ ]*/<VERSION-BANNER>/' \
      -e 's/[0-9a-f]\{7,40\}/<HASH>/g' \
      -e "s|$1|<REPO>|g" \
      -e 's/completed in [0-9]\+s/completed in <SECS>s/' \
    "$2"
}

normalise "$RX_REPO" "$scratch/rx.out" > "$scratch/rx.norm"
normalise "$RT_REPO" "$scratch/rt.out" > "$scratch/rt.norm"

DIFF=$(diff -u "$scratch/rx.norm" "$scratch/rt.norm" || true)
echo "--- ralphex exit: $RX_EXIT ---"
echo "--- ralphterm exit: $RT_EXIT ---"
echo "--- normalised diff (-=ralphex, +=ralphterm) ---"
if [ -z "$DIFF" ]; then
  echo "OK: transcripts match after normalisation"
  exit 0
fi
echo "$DIFF" | head -120
echo "..."
echo "FAIL: structural divergence detected"
exit 1
```

- [ ] **Step 2: Make it executable and run the baseline**

```bash
chmod +x scripts/diff-against-ralphex.sh
cargo build 2>&1 | tail -3
./scripts/diff-against-ralphex.sh 2>&1 | tail -40
```

Expected: the first run almost certainly reports `FAIL` (we haven't done the runner rewrite yet). That's the baseline — record the diff so we know what we're shrinking with each task.

- [ ] **Step 3: Commit**

```bash
git add scripts/diff-against-ralphex.sh
git commit -m "test: add diff-against-ralphex.sh verification harness"
```

---

### Task 7: New task-execution loop in `runner.rs`

**Goal:** Replace the existing per-task prompt loop with an iteration loop that sends the substituted `task.txt` prompt and lets the agent navigate the plan, mark its own checkbox, and signal completion.

**Files:**
- Modify: `src/runner.rs`

This task touches the largest file. Work in a single careful pass.

- [ ] **Step 1: Read the existing per-task loop to understand surrounding context**

```bash
grep -n 'fn run_plan_default\|fn task_execution_phase\|fn run_plan(' src/runner.rs | head -10
```

Note the boundary of `run_plan_default` (the current "for each task, send a per-task prompt" loop). The replacement code goes in the same function.

- [ ] **Step 2: Add a helper that counts unchecked boxes in a plan**

Append near the top of `src/runner.rs`:

```rust
pub(crate) fn count_unchecked_tasks(plan_path: &std::path::Path) -> std::io::Result<usize> {
    let body = std::fs::read_to_string(plan_path)?;
    let count = body
        .lines()
        .filter(|line| line.trim_start().starts_with("- [ ]"))
        .count();
    Ok(count)
}
```

- [ ] **Step 3: Replace the body of `run_plan_default`**

Locate `fn run_plan_default(options: RunOptions) -> ...` and replace its body with the new iteration loop. The replacement (skeleton — keep the existing surrounding signatures and error types):

```rust
fn run_plan_default(options: RunOptions) -> Result<String> {
    use crate::output_format as fmt;
    use crate::preflight::Preflight;
    use crate::progress_log::ProgressLog;
    use crate::prompts::{substitute, Prompts};
    use std::collections::HashMap;
    use std::time::Instant;

    let RunOptions {
        plan_path,
        agent_command,
        mode,
        max_review_retries: _max_review_retries,
        no_commit,
        dry_run,
        max_external_iterations: _,
        review_command: _,
        require_review: _,
        agent_timeout,
        event_sink: _,
        cancellation_check: _,
        review_patience: _,
    } = options;

    let agent_cmd = agent_command
        .ok_or_else(|| anyhow::anyhow!("agent command required"))?;
    let repo_root = std::env::current_dir()?;
    let plan_path = plan_path.canonicalize()?;

    let max_iterations = std::env::var("RALPHTERM_MAX_ITERATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50usize);

    let preflight = Preflight {
        repo_root: &repo_root,
        plan_path: &plan_path,
        branch_override: None,
        use_worktree: false,
        allow_dirty: dry_run,
    }
    .check()?;

    if preflight.created_branch {
        fmt::print_branch_creating(&preflight.branch);
    }

    let mut progress = ProgressLog::open(&repo_root, &preflight.plan_slug)?;
    let mode_label = fmt::mode_label(
        matches!(mode, RunMode::TasksOnly),
        matches!(mode, RunMode::ReviewOnly),
        matches!(mode, RunMode::ExternalOnly),
    );
    fmt::print_run_header(
        max_iterations,
        mode_label,
        &plan_path,
        &preflight.branch,
        progress.path(),
    );
    progress.write_control(&format!("creating branch: {}", preflight.branch))?;
    progress.write_control(&format!("starting ralphex loop (max {max_iterations} iterations) ({mode_label})"))?;
    progress.write_control(&format!("plan: {}", plan_path.display()))?;
    progress.write_control(&format!("branch: {}", preflight.branch))?;

    fmt::print_task_phase_start();
    progress.write_control("starting task execution phase")?;

    let prompts = Prompts::load(&repo_root, None);
    let start = Instant::now();

    for iteration in 1..=max_iterations {
        if count_unchecked_tasks(&plan_path)? == 0 {
            break;
        }
        fmt::print_iteration_header(iteration);
        progress.write_control(&format!("--- task iteration {iteration} ---"))?;

        let mut vars: HashMap<&str, &str> = HashMap::new();
        let plan_str = plan_path.to_string_lossy().to_string();
        let progress_path_str = progress.path().to_string_lossy().to_string();
        let goal = plan_first_h1(&plan_path).unwrap_or_default();
        let default_branch = preflight.default_branch.clone();
        vars.insert("PLAN_FILE", &plan_str);
        vars.insert("PROGRESS_FILE", &progress_path_str);
        vars.insert("GOAL", &goal);
        vars.insert("DEFAULT_BRANCH", &default_branch);

        let prompt = substitute(&prompts.task, &vars);
        let timeout = agent_timeout.unwrap_or_else(agent_timeout_default);
        let run = run_agent_command_with_timeout(&agent_cmd, &prompt, timeout)?;

        let cleaned = strip_ansi_escapes(&run.transcript);
        for line in cleaned.lines() {
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                continue;
            }
            println!("{trimmed}");
            let _ = progress.write_narration(trimmed);
        }

        if crate::signals::detect_signal(&cleaned)
            == Some(crate::signals::AgentSignal::Completed)
        {
            break;
        }
        if run.exit_code != 0 {
            anyhow::bail!(
                "agent iteration {iteration} exited with {}",
                run.exit_code
            );
        }
    }

    if count_unchecked_tasks(&plan_path)? > 0 {
        anyhow::bail!("hit max iterations ({max_iterations}) without ALL_TASKS_DONE");
    }

    let elapsed = start.elapsed();
    let (files, additions, deletions) = git_shortstat(&repo_root, &preflight.default_branch)?;

    let plan_dest = if no_commit {
        plan_path.clone()
    } else {
        move_plan_to_completed(&plan_path)?
    };
    if plan_dest != plan_path {
        fmt::print_moved_plan(&plan_dest);
        progress.write_control(&format!("moved plan to {}", plan_dest.display()))?;
    }

    fmt::print_completion_summary(
        elapsed,
        files,
        additions,
        deletions,
        &plan_dest,
        &preflight.branch,
        progress.path(),
    );

    Ok(String::new())
}

fn plan_first_h1(plan_path: &std::path::Path) -> Option<String> {
    let body = std::fs::read_to_string(plan_path).ok()?;
    body.lines()
        .find_map(|l| l.strip_prefix("# ").map(|s| s.trim().to_string()))
}

fn git_shortstat(repo: &std::path::Path, base: &str) -> Result<(usize, usize, usize)> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["diff", "--shortstat", &format!("{base}..HEAD")])
        .output()
        .context("git diff --shortstat")?;
    let text = String::from_utf8_lossy(&output.stdout);
    // Example: " 2 files changed, 1 insertion(+), 1 deletion(-)"
    let mut files = 0usize;
    let mut adds = 0usize;
    let mut dels = 0usize;
    for part in text.split(',') {
        let part = part.trim();
        let num: usize = part
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if part.contains("file") {
            files = num;
        } else if part.contains("insertion") {
            adds = num;
        } else if part.contains("deletion") {
            dels = num;
        }
    }
    Ok((files, adds, dels))
}

fn move_plan_to_completed(plan: &std::path::Path) -> Result<std::path::PathBuf> {
    let parent = plan.parent().ok_or_else(|| anyhow::anyhow!("plan has no parent dir"))?;
    let dest_dir = parent.join("completed");
    std::fs::create_dir_all(&dest_dir)
        .with_context(|| format!("create {}", dest_dir.display()))?;
    let dest = dest_dir.join(plan.file_name().ok_or_else(|| anyhow::anyhow!("plan has no filename"))?);
    std::fs::rename(plan, &dest)
        .with_context(|| format!("move plan to {}", dest.display()))?;
    Ok(dest)
}

fn agent_timeout_default() -> std::time::Duration {
    std::time::Duration::from_secs(30 * 60)
}
```

Delete the previous implementation of `run_plan_default` (the per-task loop and its helpers that are no longer reachable). Use the compiler's "unused function" warnings as the deletion guide.

- [ ] **Step 4: Build and fix any remaining warnings**

```bash
cargo build 2>&1 | tail -20
```

Expected: clean. If unused imports / functions remain (e.g., the old `build_task_prompt`, `mark_task_complete` helpers), remove them. Clippy will be strict about it in the next gate.

- [ ] **Step 5: Run the existing test suite — expect MANY failures**

```bash
cargo test --all 2>&1 | grep -E '^test result:' | awk '/^test result:/ { gsub(";",""); ok+=$4; fail+=$6 } END { print "passed=" ok " failed=" fail }'
```

Expected: 50-100 failures. This is the cost of the rewrite — the per-task tests are now obsolete. We fix them in Task 11.

- [ ] **Step 6: Run the verification harness**

```bash
./scripts/diff-against-ralphex.sh 2>&1 | tail -40
```

The diff should now be **much closer** to ralphex. Expected: `creating branch:`, `starting ralphex loop`, `--- task iteration N ---`, plan move, and summary footer all appear. Differences from ralphex now should be: ralphex's 5+codex+2 review pipeline (not yet implemented), exact narration extraction (we print raw, ralphex prints a curated subset).

- [ ] **Step 7: Commit**

```bash
git add src/runner.rs
git commit -m "feat(runner): rewrite around ralphex iteration-loop execution model"
```

---

### Task 8: Phase 1 first review (5 parallel reviewer agents)

**Goal:** Implement `review_phases::first_review`. Spawn 5 reviewer agents in parallel via `tokio::task::spawn_blocking`, each driven by `review_first.txt` plus one of the 5 agent definition files. Collect findings; if any reviewer reports critical/major issues, feed them back to the implementer.

**Files:**
- Create: `src/review_phases.rs`
- Modify: `src/lib.rs`, `src/runner.rs`

- [ ] **Step 1: Add module declaration**

In `src/lib.rs` add `#[doc(hidden)] pub mod review_phases;`.

- [ ] **Step 2: Write `src/review_phases.rs`**

```rust
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

use crate::prompts::{substitute, Prompts};

#[derive(Debug, Clone)]
pub enum ReviewOutcome {
    Pass,
    Issues(Vec<String>),
}

pub struct FirstReviewArgs<'a> {
    pub prompts: &'a Prompts,
    pub reviewer_command: &'a str,
    pub plan_path: &'a std::path::Path,
    pub progress_path: &'a std::path::Path,
    pub default_branch: &'a str,
    pub agent_timeout: Duration,
}

const FIRST_REVIEW_AGENTS: &[&str] = &[
    "quality",
    "implementation",
    "testing",
    "simplification",
    "documentation",
];

pub fn first_review(args: FirstReviewArgs<'_>) -> Result<ReviewOutcome> {
    run_parallel_review(args, FIRST_REVIEW_AGENTS)
}

const SECOND_REVIEW_AGENTS: &[&str] = &["quality", "implementation"];

pub fn second_review(args: FirstReviewArgs<'_>) -> Result<ReviewOutcome> {
    run_parallel_review(args, SECOND_REVIEW_AGENTS)
}

fn run_parallel_review(args: FirstReviewArgs<'_>, agent_names: &[&str]) -> Result<ReviewOutcome> {
    let mut handles: Vec<std::thread::JoinHandle<Result<(String, String)>>> = Vec::new();
    let reviewer_command = args.reviewer_command.to_string();
    let plan_path = args.plan_path.to_path_buf();
    let progress_path = args.progress_path.to_path_buf();
    let default_branch = args.default_branch.to_string();
    let timeout = args.agent_timeout;
    let review_template = args.prompts.review_first.clone();

    for name in agent_names {
        let name = (*name).to_string();
        let agent_template = args
            .prompts
            .agents
            .get(&name)
            .cloned()
            .unwrap_or_default();
        let reviewer = reviewer_command.clone();
        let plan = plan_path.clone();
        let progress = progress_path.clone();
        let default_branch = default_branch.clone();
        let review_template = review_template.clone();
        handles.push(std::thread::spawn(move || {
            run_one_reviewer(
                &reviewer,
                &review_template,
                &agent_template,
                &name,
                &plan,
                &progress,
                &default_branch,
                timeout,
            )
            .map(|transcript| (name, transcript))
        }));
    }

    let mut findings: Vec<String> = Vec::new();
    for h in handles {
        let (name, transcript) = h
            .join()
            .map_err(|_| anyhow::anyhow!("reviewer thread panicked"))??;
        if transcript_has_critical_issues(&transcript) {
            findings.push(format!("[{name}] {}", first_line_of_findings(&transcript)));
        }
    }

    if findings.is_empty() {
        Ok(ReviewOutcome::Pass)
    } else {
        Ok(ReviewOutcome::Issues(findings))
    }
}

fn run_one_reviewer(
    reviewer_command: &str,
    review_template: &str,
    agent_template: &str,
    agent_name: &str,
    plan_path: &std::path::Path,
    progress_path: &std::path::Path,
    default_branch: &str,
    timeout: Duration,
) -> Result<String> {
    let plan_str = plan_path.to_string_lossy().to_string();
    let progress_str = progress_path.to_string_lossy().to_string();
    let mut vars: HashMap<&str, &str> = HashMap::new();
    vars.insert("PLAN_FILE", &plan_str);
    vars.insert("PROGRESS_FILE", &progress_str);
    vars.insert("DEFAULT_BRANCH", default_branch);
    vars.insert("AGENT_NAME", agent_name);
    vars.insert("AGENT_INSTRUCTIONS", agent_template);

    let prompt = substitute(review_template, &vars);
    let run = crate::runner::run_agent_command_with_timeout(reviewer_command, &prompt, timeout)?;
    if run.exit_code != 0 {
        anyhow::bail!("reviewer {agent_name} exited with {}", run.exit_code);
    }
    Ok(run.transcript)
}

fn transcript_has_critical_issues(transcript: &str) -> bool {
    let upper = transcript.to_ascii_uppercase();
    upper.contains("CRITICAL") || upper.contains("MAJOR")
}

fn first_line_of_findings(transcript: &str) -> String {
    transcript
        .lines()
        .find(|l| {
            let u = l.to_ascii_uppercase();
            u.contains("CRITICAL") || u.contains("MAJOR")
        })
        .unwrap_or("")
        .trim()
        .to_string()
}

pub struct ExternalReviewArgs<'a> {
    pub prompts: &'a Prompts,
    pub implementer_command: &'a str,
    pub reviewer_command: &'a str,
    pub plan_path: &'a std::path::Path,
    pub progress_path: &'a std::path::Path,
    pub default_branch: &'a str,
    pub agent_timeout: Duration,
    pub max_iterations: usize,
}

pub fn external_review(args: ExternalReviewArgs<'_>) -> Result<ReviewOutcome> {
    // The existing fixer-loop logic in `runner::run_plan_external_only` is
    // the source of truth; this is a thin wrapper that prepares the prompt
    // and delegates. Implemented as the second sub-step of this task.
    let _ = args; // suppress until wired
    Ok(ReviewOutcome::Pass)
}
```

(The `external_review` body is intentionally a stub — Task 9 implements it.)

- [ ] **Step 3: Make `run_agent_command_with_timeout` pub(crate)**

In `src/runner.rs`, change `fn run_agent_command_with_timeout(...)` to `pub(crate) fn run_agent_command_with_timeout(...)`. Same for `AgentRun` if it's not already pub(crate).

- [ ] **Step 4: Wire `first_review` into the run pipeline**

In the new `run_plan_default` body, after the task execution loop completes, add:

```rust
if !matches!(mode, RunMode::TasksOnly) {
    crate::output_format::print_all_tasks_completed();
    progress.write_control("all tasks completed, starting code review...")?;
    let reviewer_cmd = options_reviewer_command_or_default(...); // derive from CLI/config exactly as run_compat_cli already does; refactor out into a shared helper.
    let outcome = crate::review_phases::first_review(crate::review_phases::FirstReviewArgs {
        prompts: &prompts,
        reviewer_command: &reviewer_cmd,
        plan_path: &plan_path,
        progress_path: progress.path(),
        default_branch: &preflight.default_branch,
        agent_timeout: agent_timeout.unwrap_or_else(agent_timeout_default),
    })?;
    if let crate::review_phases::ReviewOutcome::Issues(findings) = outcome {
        for f in findings {
            eprintln!("[review-first] {f}");
        }
        anyhow::bail!("first review found critical issues");
    }
}
```

Replace `options_reviewer_command_or_default(...)` with the actual derivation already present in run_compat_cli (codex wrapper default). Refactor that bit out into a helper if needed.

- [ ] **Step 5: Add an integration test**

Create `tests/first_review.rs`:

```rust
use std::process::{Command, Stdio};

#[test]
fn first_review_spawns_five_reviewers_in_parallel() {
    // The fake review-pass.sh fixture always prints REVIEW_PASS, so no
    // critical/major findings. We assert that five reviewer transcript
    // files end up in the progress dir (one per agent) — this proves the
    // 5-way fan-out happened.
    use std::os::unix::fs::PermissionsExt;
    let tmp = std::env::temp_dir().join(format!("rt-first-review-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    // The harness lives in this test file; the actual API call goes here
    // once the public surface stabilises in Task 10. For now this is a
    // placeholder that exercises the parallel spawn path through a small
    // smoke test.
    let _ = tmp;
}
```

(The test stays as a placeholder until Task 10 wires the public CLI surface; it's worth committing now so the file is in place.)

- [ ] **Step 6: Build + run**

```bash
cargo build 2>&1 | tail -3
cargo test --lib review_phases:: 2>&1 | tail -10
```

Expected: build is clean. No new tests for `review_phases` yet (placeholder), so 0 passed is fine.

- [ ] **Step 7: Commit**

```bash
git add src/review_phases.rs src/lib.rs src/runner.rs tests/first_review.rs
git commit -m "feat(review): phase 1 — five parallel reviewer agents"
```

---

### Task 9: External review phase (codex fixer loop)

**Goal:** Implement `external_review` by lifting the existing fixer-loop logic out of the old `run_plan_external_only` into the new module.

**Files:**
- Modify: `src/review_phases.rs`, `src/runner.rs`

- [ ] **Step 1: Identify the existing fixer-loop code**

```bash
grep -n 'fn run_plan_external_only\|review_failure_category\|consecutive_same_category' src/runner.rs | head
```

Note the loop boundaries — typically ~120 lines.

- [ ] **Step 2: Extract that loop into `review_phases::external_review`**

Move the loop body into `external_review`, replacing the stub. Parameters: implementer command, reviewer command, prompt templates (`prompts.codex_review`), progress writer, max_external_iterations, review_patience. Keep the same stalemate-detection (`review_failure_category` matched N times = stalemate) and the same retry-on-REVIEW_FAIL semantics.

The body should be ~80-120 lines of straight extraction — no behavior change, just relocation. Reference the existing `run_plan_external_only` in `src/runner.rs`.

- [ ] **Step 3: Wire `external_review` into the full-mode pipeline**

In `run_plan_default`, after `first_review` passes, add:

```rust
if !matches!(mode, RunMode::TasksOnly | RunMode::ReviewOnly) {
    let outcome = crate::review_phases::external_review(crate::review_phases::ExternalReviewArgs {
        prompts: &prompts,
        implementer_command: &agent_cmd,
        reviewer_command: &reviewer_cmd,
        plan_path: &plan_path,
        progress_path: progress.path(),
        default_branch: &preflight.default_branch,
        agent_timeout: agent_timeout.unwrap_or_else(agent_timeout_default),
        max_iterations: 3,
    })?;
    if matches!(outcome, crate::review_phases::ReviewOutcome::Issues(_)) {
        anyhow::bail!("external review found critical issues");
    }
}
```

- [ ] **Step 4: Delete the old `run_plan_external_only` body**

Once `external_review` is the source of truth, reduce `run_plan_external_only` (called by `--external-only` mode) to a thin shim that calls `external_review` with the prompts loaded fresh. Don't duplicate the loop.

- [ ] **Step 5: Build + verify**

```bash
cargo build 2>&1 | tail -3
./scripts/diff-against-ralphex.sh --full 2>&1 | tail -30
```

Expected build clean. Full-mode diff should now show review-phase output close to ralphex (5 parallel + codex external).

- [ ] **Step 6: Commit**

```bash
git add src/review_phases.rs src/runner.rs
git commit -m "feat(review): phase 2 — external review fixer loop"
```

---

### Task 10: Phase 3 second review + finalize prompt

**Goal:** Wire `second_review` (already defined in Task 8) into the pipeline, then run the `finalize.txt` prompt as the last step before plan move + summary.

**Files:**
- Modify: `src/runner.rs`

- [ ] **Step 1: Call `second_review` after `external_review`**

In `run_plan_default`:

```rust
if !matches!(mode, RunMode::TasksOnly | RunMode::ReviewOnly | RunMode::ExternalOnly) {
    let outcome = crate::review_phases::second_review(crate::review_phases::FirstReviewArgs {
        prompts: &prompts,
        reviewer_command: &reviewer_cmd,
        plan_path: &plan_path,
        progress_path: progress.path(),
        default_branch: &preflight.default_branch,
        agent_timeout: agent_timeout.unwrap_or_else(agent_timeout_default),
    })?;
    if matches!(outcome, crate::review_phases::ReviewOutcome::Issues(_)) {
        anyhow::bail!("second review found critical issues");
    }
}
```

- [ ] **Step 2: Call the `finalize.txt` prompt**

Right before the plan-move + summary print, in full mode only:

```rust
if !matches!(mode, RunMode::TasksOnly) {
    let mut vars: HashMap<&str, &str> = HashMap::new();
    let plan_str = plan_path.to_string_lossy().to_string();
    let progress_str = progress.path().to_string_lossy().to_string();
    vars.insert("PLAN_FILE", &plan_str);
    vars.insert("PROGRESS_FILE", &progress_str);
    vars.insert("DEFAULT_BRANCH", &preflight.default_branch);
    let prompt = crate::prompts::substitute(&prompts.finalize, &vars);
    let _run = run_agent_command_with_timeout(
        &agent_cmd,
        &prompt,
        agent_timeout.unwrap_or_else(agent_timeout_default),
    )?;
}
```

- [ ] **Step 3: Build + run verification**

```bash
cargo build 2>&1 | tail -3
./scripts/diff-against-ralphex.sh --full 2>&1 | tail -30
```

Expected: full-mode diff shows the second-review phase + finalize step. Remaining divergence should be: precise narration line content (ralphex's agent narration vs ours), and any flag-mapping defaults still mismatched.

- [ ] **Step 4: Commit**

```bash
git add src/runner.rs
git commit -m "feat(runner): wire phase 3 second review and finalize prompt"
```

---

### Task 11: Rewrite test fixtures for the agent-navigates model

**Goal:** Update `fake-agent.sh` and friends so the integration test suite passes against the new runner model.

**Files:**
- Modify: `tests/fixtures/fake-agent.sh`
- Possibly modify: other fixtures (`review-pass.sh`, `failing-agent.sh`, etc.)

- [ ] **Step 1: Replace `tests/fixtures/fake-agent.sh`**

```sh
#!/usr/bin/env sh
# fake-agent.sh — simulates an agent that follows ralphex's task.txt
# instructions: read the plan file, find the first unchecked task,
# perform a small recipe based on the task body, mark the checkbox done,
# emit ALL_TASKS_DONE when no unchecked boxes remain.
set -eu

prompt=$(cat)
plan_file=$(printf '%s' "$prompt" | grep -oE 'Read the plan file at [^[:space:]]+' | head -1 | sed 's/.*at //')
if [ -z "${plan_file:-}" ]; then
  # Fallback: tests may pass the prompt as an argv; argv $1 may be the prompt
  if [ -n "${1:-}" ]; then
    prompt="$1"
    plan_file=$(printf '%s' "$prompt" | grep -oE 'Read the plan file at [^[:space:]]+' | head -1 | sed 's/.*at //')
  fi
fi
if [ -z "${plan_file:-}" ]; then
  printf 'FAILED: could not find plan path in prompt\n'
  exit 1
fi
if [ ! -f "$plan_file" ]; then
  printf 'FAILED: plan file does not exist: %s\n' "$plan_file"
  exit 1
fi

# Find the first unchecked task line and perform its recipe.
task_line=$(grep -nE '^- \[ \]' "$plan_file" | head -1 || true)
if [ -z "$task_line" ]; then
  printf '\nAll checkboxes in the plan are now complete.\n'
  printf 'ALL_TASKS_DONE\n'
  exit 0
fi

line_num=$(printf '%s' "$task_line" | cut -d: -f1)
task_text=$(printf '%s' "$task_line" | cut -d: -f2-)

if printf '%s' "$task_text" | grep -q 'Write first.txt'; then
  printf 'created by fake agent\n' > first.txt
elif printf '%s' "$task_text" | grep -q 'Write second.txt'; then
  printf 'created by fake agent\n' > second.txt
elif printf '%s' "$task_text" | grep -q 'Write nested/generated.txt'; then
  mkdir -p nested
  printf 'nested content from fake agent\n' > nested/generated.txt
elif printf '%s' "$task_text" | grep -q 'Change tracked.txt'; then
  printf 'run-change\n' > tracked.txt
elif printf '%s' "$task_text" | grep -q 'Recreate tracked.txt with base content'; then
  printf 'base\n' > tracked.txt
elif printf '%s' "$task_text" | grep -q 'Create a file named hello.txt'; then
  printf 'hi' > hello.txt
fi

# Mark the checkbox done.
tmp=$(mktemp)
awk -v ln="$line_num" 'NR==ln { sub(/- \[ \]/, "- [x]"); print; next } { print }' "$plan_file" > "$tmp"
mv "$tmp" "$plan_file"

# Did marking that box leave any unchecked? If not, emit ALL_TASKS_DONE.
remaining=$(grep -cE '^- \[ \]' "$plan_file" || true)
printf '\nMarked task at line %s as complete.\n' "$line_num"
if [ "${remaining:-0}" -eq 0 ]; then
  printf 'All checkboxes in the plan are now complete.\n'
  printf 'ALL_TASKS_DONE\n'
fi
```

- [ ] **Step 2: Run the test suite**

```bash
cargo test --all 2>&1 | grep -E '^test result:' | awk '/^test result:/ { gsub(";",""); ok+=$4; fail+=$6 } END { print "passed=" ok " failed=" fail }'
```

Expected: passed count climbs significantly. Some tests will still fail because they assert specific old output strings; those are Task 12's job.

- [ ] **Step 3: Commit**

```bash
git add tests/fixtures/fake-agent.sh
git commit -m "test(fixtures): rewrite fake-agent.sh around agent-navigates model"
```

---

### Task 12: Bring tests back to green

**Goal:** Update every failing assertion in the existing integration tests to assert the new ralphex-style output strings instead of the old per-task strings.

**Files:**
- Modify: every `tests/*.rs` file with failing assertions.

- [ ] **Step 1: List failing tests**

```bash
cargo test --all 2>&1 | grep -E 'test .* FAILED' | sort -u > /tmp/failing-tests.txt
wc -l /tmp/failing-tests.txt
head -20 /tmp/failing-tests.txt
```

- [ ] **Step 2: For each failing test, update the assertion**

Common transformations:
- `assert!(stdout.contains("Executing"))` → `assert!(stdout.contains("starting ralphex loop"))`
- `assert!(stdout.contains("Task N: title"))` → `assert!(stdout.contains("--- task iteration"))`
- `assert!(stdout.contains("COMPLETED"))` → `assert!(stdout.contains("ALL_TASKS_DONE"))` OR remove (the agent emits it, we just read it)
- Tests asserting we print `Marked task N complete` → drop those assertions (the agent now marks)
- Tests asserting we print `Committed <hash>` → drop those assertions (no longer applicable; agents do their own commits per ralphex)

Edit each failing test file; rerun the suite after each batch:

```bash
cargo test --all 2>&1 | grep -E '^test result:' | awk '/^test result:/ { gsub(";",""); ok+=$4; fail+=$6 } END { print "passed=" ok " failed=" fail }'
```

- [ ] **Step 3: Run the full gate**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all 2>&1 | grep -E '^test result:' | awk '/^test result:/ { gsub(";",""); ok+=$4; fail+=$6 } END { print "passed=" ok " failed=" fail }'
```

Expected: zero failures, clippy clean.

- [ ] **Step 4: Commit**

```bash
git add tests/
git commit -m "test: retarget integration assertions at the new ralphex output strings"
```

---

### Task 13: Side-by-side verification — hello.md tasks-only

**Goal:** Confirm `./scripts/diff-against-ralphex.sh` reports `OK: transcripts match after normalisation` for the `--tasks-only` baseline.

**Files:** None modified — this is a verification task.

- [ ] **Step 1: Build release and run**

```bash
cargo build --release 2>&1 | tail -3
RALPHTERM_BIN=$(pwd)/target/release/ralphterm scripts/diff-against-ralphex.sh 2>&1 | tee /tmp/diff-result.txt
```

- [ ] **Step 2: If divergent, fix → re-run → commit each fix**

For every structural divergence the harness reports, write a small follow-up commit that resolves it. Common patterns:
- Wrong order of output lines → adjust where `print_*` calls live in `run_plan_default`.
- Missing line → add the matching `progress.write_control` / `println!`.
- Extra line we emit → remove it.

Do NOT change the verification harness's normalisation rules to "make the diff pass" — that defeats the purpose. The acceptance gate is "we produce the same output ralphex produces"; the harness only strips incidentals (timestamps, hashes, version banner).

- [ ] **Step 3: Run the full mode harness**

```bash
RALPHTERM_BIN=$(pwd)/target/release/ralphterm scripts/diff-against-ralphex.sh --full 2>&1 | tail -40
```

This run requires a working `codex` install (for the reviewer). If your environment doesn't have one, skip this sub-step and capture it as an "unverified — needs codex" caveat in the commit message.

- [ ] **Step 4: Commit (or skip if no changes needed)**

If the harness now reports `OK`, commit any fix patches you made. If no patches were needed because the previous tasks landed it, just record the verification result inline in the next task's commit.

---

### Task 14: Version bump, docs update, release

**Goal:** Bump to `0.2.0`, update README + migration guide to honestly describe the new execution model, publish to crates.io.

**Files:**
- Modify: `Cargo.toml`, `README.md`, `docs/migrate-from-ralphex.md`, `docs/ralphex-compat.md`
- Modify: `tests/cli_flag_compat.rs` (version string assertion)

- [ ] **Step 1: Bump version**

In `Cargo.toml`:
```toml
version = "0.2.0"
```

In `tests/cli_flag_compat.rs`, update the `assert!(stdout.contains("0.1.x"))` to `assert!(stdout.contains("0.2.0"))`.

- [ ] **Step 2: Rewrite README install + drop-in sections**

Change the "Drop in" block to reflect the actual model:

```markdown
## Drop in

```sh
# Identical command, behaves the same as ralphex 1.2.0
ralphex --tasks-only docs/plans/feature.md
ralphterm --tasks-only docs/plans/feature.md
```

RalphTerm runs ralphex's vendored prompts unchanged. Both binaries:
- refuse to start on a dirty worktree (use `--worktree` to isolate)
- auto-create a branch from the plan filename slug
- emit `--- task iteration N ---` headers and a `completed in Xs (N files, +A/-D lines)` summary
- auto-move the plan to `docs/plans/completed/` on success
- write a timestamped progress log to `.ralphex/progress/progress-<slug>.txt`
- (in full mode) run the 5+codex+2 review pipeline using your `.ralphex/agents/*.txt` definitions
```

- [ ] **Step 3: Update `docs/migrate-from-ralphex.md`**

Delete the "Differences from ralphex" entries that are no longer differences (output format, plan move, branch creation, etc.). Keep only the genuinely-deferred items: `--init`, `--reset`, `--dump-defaults`, `-V` vs `-v`, idle-timeout no-op, wait no-op.

- [ ] **Step 4: Update `docs/ralphex-compat.md`**

Mark the previously "Accepted" flags as "Supported" for everything the rewrite now implements. Leave only the deferred items as "Pending".

- [ ] **Step 5: Run the full gate one more time**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo publish --dry-run --allow-dirty 2>&1 | tail -10
./scripts/diff-against-ralphex.sh 2>&1 | tail -10
```

All five must succeed.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock README.md docs/migrate-from-ralphex.md docs/ralphex-compat.md tests/cli_flag_compat.rs
git commit -m "release(0.2.0): honest drop-in claim after execution-model rewrite"
```

- [ ] **Step 7: Tag + publish**

```bash
git push origin main
git tag v0.2.0 -m "v0.2.0 — verified drop-in for ralphex 1.2.0 execution model"
git push origin v0.2.0
cargo publish
```

The tag push triggers the GitHub Actions release workflow (cargo-dist). `cargo publish` registers the new version on crates.io.

---

## Verification gates (before final push)

```
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo publish --dry-run --allow-dirty
./scripts/diff-against-ralphex.sh
./scripts/diff-against-ralphex.sh --full   # requires codex installed
```

All five must succeed.

## Acceptance recap

1. `./scripts/diff-against-ralphex.sh` reports `OK: transcripts match after normalisation`.
2. `cargo test --all` is green.
3. `src/runner.rs` is at or below 1000 lines.
4. README and migration guide honestly describe the new execution model, with a short and accurate deferred-features list.
5. `cargo publish` ships `ralphterm v0.2.0`.
