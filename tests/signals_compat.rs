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
        let path = std::env::temp_dir().join(format!("ralphterm-signals-{label}-{unique}"));
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

#[test]
fn ralphex_review_done_signal_is_accepted_as_review_pass() {
    let repo = TempRepo::new("review-done");
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
            "--review-command",
            fixture_path("review-ralphex-pass.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm should accept <<<RALPHEX:REVIEW_DONE>>>\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(
        plan.contains("- [x] Write first.txt"),
        "task should be marked after RALPHEX:REVIEW_DONE:\n{plan}"
    );
}
