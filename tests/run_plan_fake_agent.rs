use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn run_command_prints_pending_tasks_in_order() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `cargo test --all`

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
    assert!(stdout.contains("Pending tasks for plan.md:"), "{stdout}");
    assert!(stdout.contains("1. Task 1: Create first file"), "{stdout}");
    assert!(stdout.contains("2. Task 3: Create second file"), "{stdout}");
    assert!(!stdout.contains("Already finished"), "{stdout}");
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
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
