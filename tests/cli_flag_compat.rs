use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

struct TempRepo {
    path: PathBuf,
}

impl TempRepo {
    fn new(label: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ralphterm-cli-flag-{label}-{unique}"));
        fs::create_dir(&path).expect("create temp repo");
        Self { path }
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

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

#[test]
fn help_lists_ralphex_flags() {
    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .arg("--help")
        .output()
        .expect("run ralphterm --help");
    assert!(
        output.status.success(),
        "ralphterm --help failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    for flag in [
        "--review",
        "--external-only",
        "--codex-only",
        "--max-iterations",
        "--max-external-iterations",
        "--review-patience",
        "--task-model",
        "--review-model",
        "--claude-args",
        "--base-ref",
        "--session-timeout",
        "--idle-timeout",
        "--wait",
        "--debug",
        "--no-color",
    ] {
        assert!(
            combined.contains(flag),
            "--help missing {flag}:\n{combined}"
        );
    }
}

#[test]
fn version_flag_prints_crate_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .arg("--version")
        .output()
        .expect("run ralphterm --version");
    assert!(
        output.status.success(),
        "ralphterm --version failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("0.1.1"), "stdout: {stdout}");
}

#[test]
fn ralphex_flags_run_tasks_only_plan_with_extras() {
    let repo = TempRepo::new("tasks-only");
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--claude-args",
            "",
            "--base-ref",
            "main",
            "--debug",
            "--no-color",
            "--session-timeout",
            "30s",
            "--max-iterations",
            "5",
            "--no-commit",
            plan_path.to_str().expect("utf8 plan path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        repo.path.join("first.txt").exists(),
        "fake agent should have produced first.txt"
    );
}

#[test]
fn invalid_session_timeout_exits_with_error() {
    let repo = TempRepo::new("bad-timeout");
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--session-timeout",
            "bogus",
            "--no-commit",
            plan_path.to_str().expect("utf8 plan path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm should reject bogus --session-timeout\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_ascii_lowercase().contains("session-timeout")
            || stderr.to_ascii_lowercase().contains("duration"),
        "stderr should mention the invalid duration: {stderr}"
    );
}

#[test]
fn task_model_flag_sets_claude_model_env_for_agent() {
    let repo = TempRepo::new("task-model");
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("env-capture-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--task-model",
            "claude-opus",
            "--review-model",
            "claude-sonnet",
            "--no-commit",
            plan_path.to_str().expect("utf8 plan path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let captured = fs::read_to_string(repo.path.join("claude-model.txt"))
        .expect("read claude-model.txt produced by fake agent");
    assert!(
        captured.contains("claude-opus"),
        "claude-model.txt should contain task model: {captured:?}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--task-model"),
        "stderr should warn about task-model forwarding: {stderr}"
    );
}
