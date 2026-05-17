use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn ralphex_binary_runs_plan_with_ralphex_flags() {
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

    let output = Command::new(env!("CARGO_BIN_EXE_ralphex"))
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
        .expect("run ralphex");

    assert!(
        output.status.success(),
        "ralphex failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Executing plan.md"), "{stdout}");
    assert!(stdout.contains("Task 1: Create first file"), "{stdout}");
    assert!(repo.path.join("first.txt").exists());
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
        let path = std::env::temp_dir().join(format!("ralphex-alias-{unique}"));
        fs::create_dir(&path).expect("create temp repo");
        Self { path }
    }
}
