use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use uuid::Uuid;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn write_minimal_plan(path: &Path) {
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

fn write_failing_plan(path: &Path) {
    fs::write(
        path,
        r#"# Example plan

## Validation Commands
- `test -f never.txt`

### Task 1: This task will fail
- [ ] Do something
"#,
    )
    .expect("write plan");
}

struct TestRepo {
    path: PathBuf,
}

impl TestRepo {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("ralphterm-plan-move-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        git(&path, ["init", "--initial-branch", "main"]);
        git(&path, ["config", "user.email", "test@example.com"]);
        git(&path, ["config", "user.name", "Test User"]);
        fs::write(path.join("README.md"), "hello\n").unwrap();
        git(&path, ["add", "README.md"]);
        git(&path, ["commit", "-m", "initial"]);
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn git<I, S>(cwd: &Path, args: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git command failed in {}\nstdout:\n{}\nstderr:\n{}",
        cwd.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn successful_run_with_move_completed_flag_moves_plan_to_completed_dir() {
    let repo = TestRepo::new();
    let plans_dir = repo.path().join("plans");
    fs::create_dir_all(&plans_dir).unwrap();
    let plan_path = plans_dir.join("2025-05-17-feature.md");
    write_minimal_plan(&plan_path);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path())
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent.sh").to_str().unwrap(),
            "--no-commit",
            "--move-completed",
            "plans/2025-05-17-feature.md",
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

    let dest = plans_dir.join("completed").join("2025-05-17-feature.md");
    assert!(dest.exists(), "expected plan moved to {}", dest.display());
    assert!(
        !plan_path.exists(),
        "original plan {} should be gone",
        plan_path.display()
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout.lines().last().unwrap_or("");
    assert!(
        last_line.starts_with("Moved plan to ") && last_line.contains(dest.to_str().unwrap()),
        "expected last line to be 'Moved plan to ...'; got: {last_line}\nfull:\n{stdout}"
    );
}

#[test]
fn config_move_plan_on_completion_triggers_move() {
    let repo = TestRepo::new();
    let plan_path = repo.path().join("plan.md");
    write_minimal_plan(&plan_path);

    let cfg_dir = repo.path().join(".ralphex");
    fs::create_dir_all(&cfg_dir).unwrap();
    fs::write(cfg_dir.join("config"), "move_plan_on_completion = true\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path())
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent.sh").to_str().unwrap(),
            "--no-commit",
            "plan.md",
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

    let dest = repo.path().join("completed").join("plan.md");
    assert!(dest.exists(), "expected plan moved to {}", dest.display());
    assert!(!plan_path.exists(), "original plan should be gone");
}

#[test]
fn failing_run_with_move_completed_flag_leaves_plan_in_place() {
    let repo = TestRepo::new();
    let plan_path = repo.path().join("plan.md");
    write_failing_plan(&plan_path);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path())
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent.sh").to_str().unwrap(),
            "--no-commit",
            "--move-completed",
            "plan.md",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(!output.status.success(), "failing run should exit non-zero");
    assert!(plan_path.exists(), "plan should not have moved on failure");
    let dest = repo.path().join("completed").join("plan.md");
    assert!(
        !dest.exists(),
        "destination should not exist for failed run"
    );
}

#[test]
fn move_completed_errors_when_destination_exists() {
    let repo = TestRepo::new();
    let plan_path = repo.path().join("plan.md");
    write_minimal_plan(&plan_path);
    let completed_dir = repo.path().join("completed");
    fs::create_dir_all(&completed_dir).unwrap();
    fs::write(completed_dir.join("plan.md"), "old contents\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path())
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent.sh").to_str().unwrap(),
            "--no-commit",
            "--move-completed",
            "plan.md",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "run should fail when destination already exists\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(plan_path.exists(), "plan should stay at source");
    let dest = repo.path().join("completed").join("plan.md");
    let dest_content = fs::read_to_string(&dest).unwrap();
    assert_eq!(dest_content, "old contents\n");
}
