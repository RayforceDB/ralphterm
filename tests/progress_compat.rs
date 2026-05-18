use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
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

struct TestRepo {
    path: PathBuf,
}

impl TestRepo {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("ralphterm-progress-{}", Uuid::new_v4()));
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
fn after_run_progress_log_written_under_ralphterm_progress() {
    let repo = TestRepo::new();
    let plan_path = repo.path().join("plan.md");
    write_minimal_plan(&plan_path);
    git(repo.path(), ["add", "plan.md"]);
    git(repo.path(), ["commit", "-m", "docs: add plan"]);

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

    let progress_path = repo
        .path()
        .join(".ralphterm")
        .join("progress")
        .join("progress-plan.txt");
    let body = fs::read_to_string(&progress_path).unwrap_or_else(|err| {
        panic!(
            "expected progress log {} after run: {err}",
            progress_path.display()
        )
    });
    assert!(
        !body.is_empty(),
        "progress log should not be empty:\n{body}"
    );
}

// preexisting_ralphterm_progress_file_is_not_overwritten removed: the new
// runner writes per-plan files at .ralphterm/progress/progress-<slug>.txt
// instead of treating .ralphterm/progress as a single user-owned file, so
// the legacy "preserve external file" contract no longer applies.

#[test]
fn serve_with_watch_prints_watching_message() {
    let repo = TestRepo::new();
    let watch_dir = repo.path().join("watch-target");
    fs::create_dir_all(&watch_dir).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path())
        .args([
            "--serve",
            "--port",
            "0",
            "--host",
            "127.0.0.1",
            "--watch",
            watch_dir.to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ralphterm");

    let stdout = child.stdout.take().expect("capture stdout");
    let reader = BufReader::new(stdout);
    let deadline = Instant::now() + Duration::from_secs(10);

    let mut saw_watching = false;
    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        if line.contains("[watching]") && line.contains(watch_dir.to_str().unwrap()) {
            saw_watching = true;
            break;
        }
        if Instant::now() > deadline {
            break;
        }
    }

    // Stop the server.
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        saw_watching,
        "expected '[watching] {}' on stdout within 10s",
        watch_dir.display()
    );
}

#[test]
fn serve_combined_with_tasks_only_is_rejected() {
    let repo = TestRepo::new();
    let plan_path = repo.path().join("plan.md");
    write_minimal_plan(&plan_path);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path())
        .args(["--serve", "--tasks-only", "plan.md"])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "expected clap to reject --serve combined with --tasks-only"
    );
}
