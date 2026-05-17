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

struct TestRepo {
    path: PathBuf,
}

impl TestRepo {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("ralphterm-worktree-{}", Uuid::new_v4()));
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
fn worktree_flag_creates_isolated_worktree_from_plan_slug() {
    let repo = TestRepo::new();
    let plans_dir = repo.path().join("docs").join("plans");
    fs::create_dir_all(&plans_dir).unwrap();
    let plan_path = plans_dir.join("foo.md");
    write_minimal_plan(&plan_path);
    git(repo.path(), ["add", "docs/plans/foo.md"]);
    git(repo.path(), ["commit", "-m", "add plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path())
        .args([
            "--tasks-only",
            "--worktree",
            "--claude-command",
            fixture_path("fake-agent.sh").to_str().unwrap(),
            "--no-commit",
            "docs/plans/foo.md",
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

    let workspace_path = repo
        .path()
        .join(".ralphterm")
        .join("workspaces")
        .join("foo");
    assert!(
        workspace_path.is_dir(),
        "expected worktree at {} after run",
        workspace_path.display()
    );

    let branch = git(&workspace_path, ["branch", "--show-current"]);
    // The new runner names the working branch after the plan slug (with no
    // `ralphterm/` prefix) since it must match what ralphex would create.
    assert_eq!(branch, "foo");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(workspace_path.to_str().unwrap()),
        "expected stdout to mention worktree path:\n{stdout}"
    );
}

#[test]
fn worktree_with_branch_override_uses_custom_branch() {
    let repo = TestRepo::new();
    let plans_dir = repo.path().join("docs").join("plans");
    fs::create_dir_all(&plans_dir).unwrap();
    let plan_path = plans_dir.join("bar.md");
    write_minimal_plan(&plan_path);
    git(repo.path(), ["add", "docs/plans/bar.md"]);
    git(repo.path(), ["commit", "-m", "add plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path())
        .args([
            "--tasks-only",
            "--worktree",
            "--branch",
            "my/custom",
            "--claude-command",
            fixture_path("fake-agent.sh").to_str().unwrap(),
            "--no-commit",
            "docs/plans/bar.md",
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

    let workspace_path = repo
        .path()
        .join(".ralphterm")
        .join("workspaces")
        .join("bar");
    assert!(workspace_path.is_dir());
    // The worktree is created on my/custom, but the new runner switches the
    // worktree to a plan-slug branch (bar) before iterating. Both branches
    // should now exist locally.
    let branches = git(&workspace_path, ["branch", "--list"]);
    assert!(
        branches.contains("my/custom"),
        "expected my/custom branch to exist:\n{branches}"
    );
    let current = git(&workspace_path, ["branch", "--show-current"]);
    assert_eq!(current, "bar", "runner should switch to plan-slug branch");
}

#[test]
fn worktree_combined_with_workspace_id_is_rejected() {
    let repo = TestRepo::new();
    let plan_path = repo.path().join("plan.md");
    write_minimal_plan(&plan_path);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path())
        .args(["--worktree", "--workspace-id", "anything", "plan.md"])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm should reject combining --worktree and --workspace-id"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--workspace-id") || stderr.contains("workspace-id"),
        "expected error to mention --workspace-id: {stderr}"
    );
}
