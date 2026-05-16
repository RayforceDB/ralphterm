use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn run_command_marks_completed_tasks_and_commits() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
- [ ] Verify first.txt exists

Literal example: `- [ ] do not mark`
"#,
    )
    .expect("write plan");
    fs::write(repo.path.join("unrelated.txt"), "original\n").expect("write unrelated file");
    fs::write(repo.path.join("staged.txt"), "original\n").expect("write staged file");
    repo.git(["add", "plan.md", "unrelated.txt", "staged.txt"]);
    repo.git(["commit", "-m", "docs: add test plan"]);
    fs::write(repo.path.join("unrelated.txt"), "do not commit\n").expect("dirty unrelated file");
    fs::write(repo.path.join("staged.txt"), "do not commit\n").expect("dirty staged file");
    repo.git(["add", "staged.txt"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
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

    let plan = fs::read_to_string(&plan_path).expect("read updated plan");
    assert!(plan.contains("- [x] Write first.txt"), "{plan}");
    assert!(plan.contains("- [x] Verify first.txt exists"), "{plan}");
    assert!(
        plan.contains("Literal example: `- [ ] do not mark`"),
        "{plan}"
    );

    let log = repo.git_output(["log", "--oneline", "-1"]);
    assert!(
        log.contains("task: Create first file"),
        "latest commit should be task commit, got {log}"
    );
    let committed_files = repo.git_output(["show", "--name-only", "--format=", "HEAD"]);
    assert!(committed_files.contains("plan.md"), "{committed_files}");
    assert!(committed_files.contains("first.txt"), "{committed_files}");
    assert!(
        !committed_files.contains("unrelated.txt"),
        "unrelated dirty file should not be committed: {committed_files}"
    );
    assert!(
        !committed_files.contains("staged.txt"),
        "unrelated staged file should not be committed: {committed_files}"
    );
    assert_eq!(
        repo.git_output(["status", "--short"]),
        "M  staged.txt\n M unrelated.txt\n?? .ralphterm/\n"
    );

    let progress_log_path = repo.path.join(".ralphterm/progress/plan.log");
    let progress_log = fs::read_to_string(&progress_log_path).expect("read progress log");
    assert!(
        progress_log.contains("task_start number=1 title=Create first file"),
        "{progress_log}"
    );
    assert!(
        progress_log.contains("validation result=passed"),
        "{progress_log}"
    );
    let commit_hash = repo.git_output(["rev-parse", "--short", "HEAD"]);
    assert!(
        progress_log.contains(&format!("commit hash={}", commit_hash.trim())),
        "{progress_log}"
    );
    assert!(progress_log.contains("signal=COMPLETED"), "{progress_log}");
    assert!(progress_log.contains("task_end number=1"), "{progress_log}");

    let transcript_line = progress_log
        .lines()
        .find(|line| line.contains("transcript path="))
        .expect("transcript path logged");
    let transcript_path = transcript_line
        .split("transcript path=")
        .nth(1)
        .expect("transcript path value")
        .trim();
    let transcript = fs::read_to_string(repo.path.join(transcript_path)).expect("read transcript");
    assert!(transcript.contains("COMPLETED"), "{transcript}");
}

#[test]
fn run_command_prints_pending_tasks_in_order() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt

The agent should create first.txt.

### Task 2: Already finished
- [x] Nothing left here

### Task 3: Create second file
- [ ] Write second.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
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

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Executing plan.md"), "{stdout}");
    assert!(stdout.contains("Task 1: Create first file"), "{stdout}");
    assert!(stdout.contains("Task 3: Create second file"), "{stdout}");
    assert!(!stdout.contains("Already finished"), "{stdout}");
    assert!(stdout.contains("COMPLETED"), "{stdout}");
    assert!(stdout.contains("Validation: test -f first.txt"), "{stdout}");
    assert!(stdout.contains("Validation passed"), "{stdout}");

    assert_eq!(
        fs::read_to_string(repo.path.join("first.txt")).expect("first file created"),
        "created by fake agent\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path.join("second.txt")).expect("second file created"),
        "created by fake agent\n"
    );
}

#[test]
fn progress_signal_ignores_prompt_echo() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent-no-completed.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
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

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    assert!(progress_log.contains("signal=NONE"), "{progress_log}");
    assert!(!progress_log.contains("signal=COMPLETED"), "{progress_log}");
}

#[test]
fn commit_excludes_ralphterm_artifacts_even_when_logs_are_ignored() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    fs::write(repo.path.join(".gitignore"), "*.log\n").expect("write gitignore");
    repo.git(["add", "plan.md", ".gitignore"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
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

    let committed_files = repo.git_output(["show", "--name-only", "--format=", "HEAD"]);
    assert!(
        !committed_files.contains(".ralphterm/"),
        "RalphTerm artifacts must not be committed: {committed_files}"
    );
}

#[test]
fn validation_failure_is_logged_and_does_not_complete_task() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f missing.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
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

    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(plan.contains("- [ ] Write first.txt"), "{plan}");

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    assert!(
        progress_log.contains("validation result=failed"),
        "{progress_log}"
    );
    assert!(progress_log.contains("task_end number=1"), "{progress_log}");
    assert!(progress_log.contains("result=failed"), "{progress_log}");
}

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
