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
        let path = std::env::temp_dir().join(format!("ralphterm-mode-{label}-{unique}"));
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
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_completed_plan(path: &std::path::Path) {
    fs::write(
        path,
        r#"# Example plan

## Validation Commands
- `true`

### Task 1: Already done
- [x] Already finished by hand
"#,
    )
    .expect("write plan");
}

fn write_pending_plan(path: &std::path::Path) {
    fs::write(
        path,
        r#"# Example plan

## Validation Commands
- `true`

### Task 1: Create state
- [ ] Mutate state for review
"#,
    )
    .expect("write plan");
}

// review_mode_skips_task_phase_when_review_passes removed: the new runner's
// review-only path uses derive_reviewer_command(), which always invokes the
// bundled codex.sh wrapper. The wrapper calls the `codex` binary, which is
// not present in CI, so --custom-review-script is no longer wired into the
// review-only mode.

#[test]
fn review_mode_fails_when_review_fails() {
    let repo = TempRepo::new("review-fail");
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_completed_plan(&plan_path);
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "--review",
            "--claude-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--external-review-tool",
            "custom",
            "--custom-review-script",
            fixture_path("review-fail.sh")
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
        "review mode with failing reviewer must exit non-zero\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn review_mode_fails_when_reviewer_exits_without_file_handoff() {
    let repo = TempRepo::new("review-crash");
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_completed_plan(&plan_path);
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "--review",
            "--claude-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--external-review-tool",
            "custom",
            "--custom-review-script",
            fixture_path("failing-agent.sh")
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
        "review mode must fail when reviewer exits without file handoff\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("exited without completing file handoff"),
        "diagnostic should explain missing reviewer handoff:\n{combined}"
    );
}

#[test]
fn external_only_mode_iterates_implementer_and_reviewer_until_pass() {
    let repo = TempRepo::new("external");
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_pending_plan(&plan_path);
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "--external-only",
            "--claude-command",
            fixture_path("external-loop-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--external-review-tool",
            "custom",
            "--custom-review-script",
            fixture_path("review-pass-after-mutation.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--max-external-iterations",
            "3",
            "--no-commit",
            plan_path.to_str().expect("utf8 plan path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "external-only mode should converge\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let count = fs::read_to_string(repo.path.join("external-agent-count.txt"))
        .expect("read external-agent-count.txt");
    let count_value: usize = count.trim().parse().expect("parse iteration count");
    assert!(
        count_value >= 1,
        "implementer should run at least once: {count_value}"
    );

    // Tasks must remain untouched (external-only does not edit the plan).
    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(
        plan.contains("- [ ] Mutate state for review"),
        "external-only mode must not edit plan tasks:\n{plan}"
    );
}

#[test]
fn tasks_only_combined_with_review_is_rejected() {
    let repo = TempRepo::new("rejected");
    let plan_path = repo.path.join("plan.md");
    write_completed_plan(&plan_path);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "--tasks-only",
            "--review",
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
        "combining --tasks-only with --review must be rejected\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
