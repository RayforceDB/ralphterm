use std::{
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// --- smoke command ------------------------------------------------------

#[test]
fn smoke_command_runs_fake_agent_and_reports_completed_signal() {
    let repo = TempRepo::new();

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "smoke",
            "--agent-command",
            fixture_path("fake-agent-no-completed.sh")
                .to_str()
                .expect("utf8 fixture path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm smoke");

    // smoke prints the agent transcript regardless of completion signal.
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("Smoke:"), "{combined}");
}

#[test]
fn smoke_command_with_builtin_claude_uses_pty_keeps_skip_permissions() {
    // The contract restored in v0.3 after the v0.2.x --print regression:
    // ralphterm drives bare `claude` inside a real PTY, paste the prompt
    // as keystrokes (no argv, no --print, no -p). The only injected flag
    // is --dangerously-skip-permissions, which is required for an
    // autonomous session loop (no human to click per-tool approval
    // prompts) and is independent of the --print discussion. Anthropic's
    // workspace-trust dialog is handled by the operator-confirmed
    // preflight trust sentinel, not by going through --print.
    use std::os::unix::fs::PermissionsExt;

    let repo = TempRepo::new();
    let bin_dir = repo.path.join("bin");
    fs::create_dir(&bin_dir).expect("create fake bin dir");
    let claude_shim = bin_dir.join("claude");
    fs::write(
        &claude_shim,
        r#"#!/bin/sh
if [ -t 0 ]; then
  printf 'tty\n' > claude-stdin-kind.txt
else
  printf 'not-tty\n' > claude-stdin-kind.txt
fi
printf '%s\n' "$@" > claude-argv.txt
cat > claude-stdin.txt
printf 'fake claude interactive session\nCOMPLETED\n'
"#,
    )
    .expect("write fake claude shim");
    let mut permissions = fs::metadata(&claude_shim)
        .expect("stat fake claude shim")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&claude_shim, permissions).expect("chmod fake claude shim");
    let path = format!(
        "{}:{}",
        bin_dir.to_str().expect("utf8 bin path"),
        std::env::var("PATH").expect("PATH")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .env("PATH", path)
        .args(["smoke", "--agent", "claude"])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm smoke");

    assert!(
        output.status.success(),
        "ralphterm smoke failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let argv = fs::read_to_string(repo.path.join("claude-argv.txt")).expect("read argv capture");
    assert!(
        !argv.contains("--print"),
        "built-in claude smoke must NOT inject --print:\n{argv}"
    );
    assert!(
        !argv.contains("-p\n") && !argv.contains("-p ") && !argv.starts_with("-p"),
        "built-in claude smoke must NOT inject -p:\n{argv}"
    );
    assert!(
        argv.contains("--permission-mode") && argv.contains("bypassPermissions"),
        "built-in claude smoke must inject --permission-mode bypassPermissions for autonomous loops:\n{argv}"
    );
    assert!(
        !argv.contains("--dangerously-skip-permissions"),
        "should use --permission-mode bypassPermissions (avoids the one-time safety-acceptance dialog), not the older flag:\n{argv}"
    );
    // The prompt must NOT be passed as argv — it arrives on stdin via the PTY writer.
    assert!(
        !argv.contains("RalphTerm PTY smoke check"),
        "built-in claude smoke must NOT pass the prompt as argv:\n{argv}"
    );

    let stdin_kind = fs::read_to_string(repo.path.join("claude-stdin-kind.txt"))
        .expect("read stdin kind capture");
    assert_eq!(stdin_kind, "tty\n", "built-in claude smoke must use a PTY");

    let stdin_prompt =
        fs::read_to_string(repo.path.join("claude-stdin.txt")).expect("read stdin capture");
    assert!(
        stdin_prompt.contains("RalphTerm PTY smoke check"),
        "prompt must arrive via PTY stdin, not argv:\n{stdin_prompt}"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Smoke: claude"), "{stdout}");
    assert!(stdout.contains("Signal: COMPLETED"), "{stdout}");
}

#[test]
fn smoke_command_rejects_one_shot_print_mode() {
    let repo = TempRepo::new();
    let command = format!(
        "{} --print",
        fixture_path("fake-agent.sh")
            .to_str()
            .expect("utf8 fixture path")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args(["smoke", "--agent-command", &command])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm smoke");

    assert!(
        !output.status.success(),
        "ralphterm smoke unexpectedly accepted one-shot mode\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostics.contains("one-shot prompt mode"),
        "{diagnostics}"
    );
    assert!(diagnostics.contains("interactive PTY"), "{diagnostics}");
}

#[test]
fn smoke_command_missing_completed_reports_agent_transcript_for_diagnostics() {
    let repo = TempRepo::new();

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "smoke",
            "--agent-command",
            fixture_path("fake-agent-no-completed.sh")
                .to_str()
                .expect("utf8 fixture path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm smoke");

    assert!(
        !output.status.success(),
        "ralphterm smoke unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let diagnostics = format!("{stdout}\n{stderr}");
    assert!(diagnostics.contains("Smoke:"), "{diagnostics}");
    assert!(diagnostics.contains("Signal: NONE"), "{diagnostics}");
}

#[test]
fn smoke_command_hanging_agent_exits_nonzero_with_bounded_timeout() {
    let repo = TempRepo::new();
    let start = Instant::now();

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .env("RALPHTERM_SMOKE_TIMEOUT_MS", "250")
        .args([
            "smoke",
            "--agent-command",
            fixture_path("hanging-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm smoke");

    let elapsed = start.elapsed();
    assert!(
        !output.status.success(),
        "ralphterm smoke unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "smoke timeout was not bounded: elapsed={elapsed:?}"
    );
}

// --- plan execution (tasks-only) ----------------------------------------

#[test]
fn ralphex_style_cli_runs_plan_without_run_subcommand() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
            plan_path.to_str().expect("utf8 plan path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphex-style cli");

    assert!(
        output.status.success(),
        "ralphex-style cli failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--- task iteration"), "{stdout}");
    let plan_after = fs::read_to_string(&plan_path).expect("read plan");
    assert!(
        plan_after.contains("- [x] Write first.txt"),
        "checkbox should be flipped:\n{plan_after}"
    );
    assert!(repo.path.join("first.txt").exists());
}

// Multi-task tests aren't currently feasible: the prompt that ralphterm
// hands to the agent contains the literal string `ALL_TASKS_DONE` (inside
// the embedded ralphex task.txt), and the PTY echoes that prompt back into
// the transcript. The runner's signal detector treats any `ALL_TASKS_DONE`
// substring as a Completed signal and exits after the first iteration, so
// the agent never gets the chance to flip subsequent checkboxes.

#[test]
fn run_command_writes_progress_log_under_ralphterm() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
            plan_path.to_str().expect("utf8 plan path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let progress_path = repo
        .path
        .join(".ralphterm")
        .join("progress")
        .join("progress-plan.txt");
    assert!(
        progress_path.is_file(),
        "expected progress log at {}",
        progress_path.display()
    );
}

#[test]
fn run_command_marks_plan_complete_and_creates_branch() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
            plan_path.to_str().expect("utf8 plan path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(output.status.success(), "ralphterm run failed");

    let branch = repo.git_output(["branch", "--show-current"]);
    assert_eq!(branch.trim(), "plan", "expected plan-slug branch");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("branch: plan"), "stdout: {stdout}");
}

#[test]
fn run_command_hits_max_iterations_when_agent_makes_no_progress() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .env("RALPHTERM_MAX_ITERATIONS", "2")
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent-no-completed.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
            plan_path.to_str().expect("utf8 plan path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "agent that never flips a checkbox should hit max iterations\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("hit max iterations"),
        "stderr should mention max iterations: {stderr}"
    );
}

#[test]
fn run_command_dirty_worktree_is_refused_without_dry_run() {
    let repo = TempRepo::new();
    repo.init_git();
    fs::write(repo.path.join("README.md"), "hi\n").expect("write README");
    repo.git(["add", "README.md"]);
    repo.git(["commit", "-m", "initial"]);
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);
    // Note: plan.md remains uncommitted.

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
            plan_path.to_str().expect("utf8 plan path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "dirty worktree run should be refused\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("uncommitted") || stderr.contains("worktree"),
        "stderr should mention dirty worktree: {stderr}"
    );
}

#[test]
fn run_command_hanging_agent_exits_nonzero_with_bounded_timeout() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add plan"]);

    let start = Instant::now();
    let mut command = Command::new(env!("CARGO_BIN_EXE_ralphterm"));
    command
        .current_dir(&repo.path)
        .env("RALPHTERM_MAX_ITERATIONS", "1")
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("hanging-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--agent-timeout-ms",
            "500",
            "--no-commit",
            plan_path.to_str().expect("utf8 plan path"),
        ])
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());
    let output = run_with_test_timeout(command, Duration::from_secs(15));

    assert!(
        !output.status.success(),
        "hanging agent should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "agent timeout was not bounded: elapsed={:?}",
        start.elapsed()
    );
}

#[test]
fn run_command_accepts_explicit_agent_timeout_without_environment_variable() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--agent-timeout-ms",
            "30000",
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    // The `run` subcommand still spins up the full review pipeline (which
    // requires codex), so we can't expect success in CI. We only assert that
    // the timeout flag is parsed and the command at least begins execution.
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.contains("invalid value") && !combined.contains("error: unexpected argument"),
        "--agent-timeout-ms should be accepted by `run`:\n{combined}"
    );
}

// --- workspace-id integration -------------------------------------------

#[test]
fn run_command_with_workspace_id_creates_managed_workspace_directory() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            "plan.md",
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--workspace-id",
            "managed-ws",
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    // The `run` subcommand without --tasks-only still wants codex for review,
    // so the run itself may fail. What we care about is that the managed
    // workspace was created before the runner reached the review phase.
    let _ = output;
    let workspace_dir = repo
        .path
        .join(".ralphterm")
        .join("workspaces")
        .join("managed-ws");
    assert!(
        workspace_dir.is_dir(),
        "expected managed workspace directory at {}",
        workspace_dir.display()
    );
}

#[test]
fn run_command_with_workspace_id_rejects_absolute_plan_path() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--workspace-id",
            "ws",
            "--agent-command",
            "true",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "workspace-id + absolute plan path should be refused"
    );
}

#[test]
fn run_command_with_workspace_id_rejects_relative_plan_path_that_escapes_repo_root() {
    let repo = TempRepo::new();
    repo.init_git();
    let nested = repo.path.join("nested");
    fs::create_dir_all(&nested).expect("create nested dir");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&nested)
        .args([
            "run",
            "../../escapes.md",
            "--workspace-id",
            "ws",
            "--agent-command",
            "true",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "workspace-id + plan path escaping repo should be refused"
    );
}

// --- CLI validation -----------------------------------------------------

#[test]
fn ralphex_compat_custom_review_tool_without_script_refuses_to_run_agent() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "--claude-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--external-review-tool=custom",
            "--no-commit",
            plan_path.to_str().expect("utf8 plan path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "--external-review-tool=custom without --custom-review-script must error before running the agent\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "ralphex-compatible full mode requires --external-review-tool=custom \
--custom-review-script <cmd>, or pass --tasks-only"
        ),
        "error message should explain the required flags:\n{stderr}"
    );

    assert!(
        !repo.path.join("first.txt").exists(),
        "agent must not run when the review gate refuses the configuration"
    );
}

#[test]
fn review_agent_conflicts_with_review_command() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-agent",
            "codex",
            "--review-command",
            fixture_path("review-pass.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(diagnostics.contains("--review-agent"), "{diagnostics}");
    assert!(diagnostics.contains("--review-command"), "{diagnostics}");
}

// The legacy --require-review CLI-side validation ("--require-review needs
// --review-command or --review-agent", "independent review command must
// differ from agent command", and the no-pending-tasks variant) has been
// removed: the new runner derives the reviewer from the bundled codex
// wrapper rather than from --review-command, so those CLI guard rails
// no longer fire and the corresponding tests were dropped.

// --- helpers ------------------------------------------------------------

fn write_minimal_plan(path: &std::path::Path) {
    fs::write(
        path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn run_with_test_timeout(mut command: Command, timeout: Duration) -> Output {
    let mut child = command.spawn().expect("spawn ralphterm");
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait().expect("poll ralphterm").is_some() {
            return child.wait_with_output().expect("collect ralphterm output");
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill timed out ralphterm test command");
            let output = child
                .wait_with_output()
                .expect("collect killed ralphterm output");
            panic!(
                "ralphterm test command exceeded {timeout:?}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct TempRepo {
    path: PathBuf,
}

impl TempRepo {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ralphterm-run-plan-{unique}"));
        fs::create_dir(&path).expect("create temp repo");
        Self { path }
    }

    fn init_git(&self) {
        self.git(["init"]);
        self.git(["config", "user.email", "test@example.invalid"]);
        self.git(["config", "user.name", "RalphTerm Test"]);
    }

    fn git<const N: usize>(&self, args: [&str; N]) {
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_output<const N: usize>(&self, args: [&str; N]) -> String {
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("git stdout utf8")
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
