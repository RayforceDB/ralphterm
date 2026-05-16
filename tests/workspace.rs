use std::{fs, path::Path, process::Command};

use ralphterm::workspace::{Workspace, WorkspaceManager};
use uuid::Uuid;

#[test]
fn discovers_git_repository_from_nested_directory() {
    let repo = TestRepo::new();
    let nested = repo.path().join("crates").join("app");
    fs::create_dir_all(&nested).unwrap();

    let manager = WorkspaceManager::discover(&nested).unwrap();

    assert_eq!(manager.repo_root(), repo.path());
}

#[test]
fn rejects_paths_outside_a_git_repository() {
    let dir = TestDir::new("ralphterm-not-git");

    let error = WorkspaceManager::discover(dir.path())
        .unwrap_err()
        .to_string();

    assert!(error.contains("not inside a git repository"), "{error}");
}

#[test]
fn creates_branch_and_worktree_under_ralphterm_workspaces() {
    let repo = TestRepo::new();
    let base_commit = git(repo.path(), ["rev-parse", "HEAD"]);
    let manager = WorkspaceManager::discover(repo.path()).unwrap();

    let workspace = manager.create("task-4").unwrap();

    assert_eq!(workspace.base_commit, base_commit);
    assert_eq!(workspace.branch, "ralphterm/task-4");
    assert_eq!(
        workspace.path,
        repo.path()
            .join(".ralphterm")
            .join("workspaces")
            .join("task-4")
    );
    assert!(workspace.path.is_dir());
    assert_eq!(git(&workspace.path, ["rev-parse", "HEAD"]), base_commit);
    assert_eq!(
        git(&workspace.path, ["branch", "--show-current"]),
        workspace.branch
    );
    assert_eq!(git(repo.path(), ["branch", "--show-current"]), "main");
}

#[test]
fn cli_create_workspace_creates_worktree_and_prints_metadata() {
    let repo = TestRepo::new();
    let base_commit = git(repo.path(), ["rev-parse", "HEAD"]);

    let output = ralphterm(repo.path(), ["workspace", "create", "cli-task"]);

    let workspace_path = repo
        .path()
        .join(".ralphterm")
        .join("workspaces")
        .join("cli-task");
    assert!(output.contains("Workspace: cli-task"), "{output}");
    assert!(
        output.contains(&format!("Path: {}", workspace_path.display())),
        "{output}"
    );
    assert!(output.contains("Branch: ralphterm/cli-task"), "{output}");
    assert!(output.contains(&format!("Base: {base_commit}")), "{output}");
    assert!(workspace_path.is_dir());
    assert_eq!(git(&workspace_path, ["rev-parse", "HEAD"]), base_commit);
    assert_eq!(
        git(&workspace_path, ["branch", "--show-current"]),
        "ralphterm/cli-task"
    );
}

#[test]
fn cli_cleanup_workspace_removes_worktree_and_branch() {
    let repo = TestRepo::new();
    ralphterm(repo.path(), ["workspace", "create", "cli-cleanup"]);
    let workspace_path = repo
        .path()
        .join(".ralphterm")
        .join("workspaces")
        .join("cli-cleanup");
    assert!(workspace_path.exists());
    assert_eq!(
        git(
            repo.path(),
            [
                "branch",
                "--list",
                "ralphterm/cli-cleanup",
                "--format=%(refname:short)"
            ]
        ),
        "ralphterm/cli-cleanup"
    );

    let output = ralphterm(repo.path(), ["workspace", "cleanup", "cli-cleanup"]);

    assert!(
        output.contains("Cleaned workspace: cli-cleanup"),
        "{output}"
    );
    assert!(!workspace_path.exists());
    assert_eq!(
        git(
            repo.path(),
            [
                "branch",
                "--list",
                "ralphterm/cli-cleanup",
                "--format=%(refname:short)"
            ]
        ),
        ""
    );
}

#[test]
fn cli_cleanup_rejects_branch_without_managed_worktree() {
    let repo = TestRepo::new();
    git(repo.path(), ["branch", "ralphterm/not-managed"]);

    let output = ralphterm_failure(repo.path(), ["workspace", "cleanup", "not-managed"]);

    assert!(
        output.contains("no managed workspace") || output.contains("no managed worktree"),
        "{output}"
    );
    assert_eq!(
        git(
            repo.path(),
            [
                "branch",
                "--list",
                "ralphterm/not-managed",
                "--format=%(refname:short)"
            ]
        ),
        "ralphterm/not-managed"
    );
}

#[test]
fn cli_cleanup_rejects_worktree_registered_at_expected_path_on_different_branch() {
    let repo = TestRepo::new();
    let workspace_path = repo
        .path()
        .join(".ralphterm")
        .join("workspaces")
        .join("mismatch");
    fs::create_dir_all(workspace_path.parent().unwrap()).unwrap();
    git(repo.path(), ["branch", "ralphterm/mismatch"]);
    git(
        repo.path(),
        [
            "worktree",
            "add",
            "-b",
            "ralphterm/different",
            workspace_path.to_str().unwrap(),
            "HEAD",
        ],
    );

    let output = ralphterm_failure(repo.path(), ["workspace", "cleanup", "mismatch"]);

    assert!(
        output.contains("branch mismatch") || output.contains("not managed by expected branch"),
        "{output}"
    );
    assert!(workspace_path.exists());
    assert_eq!(
        git(
            repo.path(),
            [
                "branch",
                "--list",
                "ralphterm/mismatch",
                "--format=%(refname:short)"
            ]
        ),
        "ralphterm/mismatch"
    );
}

#[test]
fn cli_cleanup_rejects_dirty_workspace_without_forcing_removal() {
    let repo = TestRepo::new();
    ralphterm(repo.path(), ["workspace", "create", "dirty-workspace"]);
    let workspace_path = repo
        .path()
        .join(".ralphterm")
        .join("workspaces")
        .join("dirty-workspace");
    fs::write(workspace_path.join("uncommitted.txt"), "dirty\n").unwrap();

    let output = ralphterm_failure(repo.path(), ["workspace", "cleanup", "dirty-workspace"]);

    assert!(output.contains("git command failed"), "{output}");
    assert!(workspace_path.exists());
    assert_eq!(
        git(
            repo.path(),
            [
                "branch",
                "--list",
                "ralphterm/dirty-workspace",
                "--format=%(refname:short)"
            ]
        ),
        "ralphterm/dirty-workspace"
    );
}

#[test]
fn create_excludes_ralphterm_metadata_from_git_status() {
    let repo = TestRepo::new();
    let manager = WorkspaceManager::discover(repo.path()).unwrap();

    let workspace = manager.create("status-clean").unwrap();

    assert_eq!(git(repo.path(), ["status", "--short"]), "");
    assert!(
        fs::read_to_string(repo.path().join(".git").join("info").join("exclude"))
            .unwrap()
            .lines()
            .any(|line| line == ".ralphterm/")
    );

    manager.cleanup(&workspace).unwrap();
}

#[test]
fn creates_workspace_from_linked_worktree_and_excludes_metadata_there() {
    let repo = TestRepo::new();
    let linked = TestDir::new("ralphterm-linked-worktree");
    git(
        repo.path(),
        [
            "worktree",
            "add",
            "-b",
            "linked-start",
            linked.path().to_str().unwrap(),
            "HEAD",
        ],
    );
    assert!(linked.path().join(".git").is_file());

    let manager = WorkspaceManager::discover(linked.path()).unwrap();
    let workspace = manager.create("linked-status-clean").unwrap();

    assert_eq!(manager.repo_root(), linked.path());
    assert!(workspace.path.is_dir());
    assert_eq!(git(linked.path(), ["status", "--short"]), "");
    let exclude_path = git(linked.path(), ["rev-parse", "--git-path", "info/exclude"]);
    assert!(fs::read_to_string(linked.path().join(exclude_path))
        .unwrap()
        .lines()
        .any(|line| line == ".ralphterm/"));

    manager.cleanup(&workspace).unwrap();
    git(
        repo.path(),
        [
            "worktree",
            "remove",
            "--force",
            linked.path().to_str().unwrap(),
        ],
    );
}

#[test]
fn cleanup_is_explicit_and_removes_worktree_and_branch() {
    let repo = TestRepo::new();
    let manager = WorkspaceManager::discover(repo.path()).unwrap();
    let workspace = manager.create("needs-explicit-cleanup").unwrap();

    assert!(workspace.path.exists());
    assert_eq!(
        git(
            repo.path(),
            [
                "branch",
                "--list",
                &workspace.branch,
                "--format=%(refname:short)"
            ]
        ),
        workspace.branch
    );

    manager.cleanup(&workspace).unwrap();

    assert!(!workspace.path.exists());
    assert_eq!(
        git(
            repo.path(),
            [
                "branch",
                "--list",
                &workspace.branch,
                "--format=%(refname:short)"
            ]
        ),
        ""
    );
}

#[test]
fn cleanup_removes_git_worktree_metadata_when_directory_was_deleted() {
    let repo = TestRepo::new();
    let manager = WorkspaceManager::discover(repo.path()).unwrap();
    let workspace = manager.create("missing-dir-cleanup").unwrap();
    fs::remove_dir_all(&workspace.path).unwrap();

    manager.cleanup(&workspace).unwrap();

    assert!(!workspace.path.exists());
    assert!(!git(repo.path(), ["worktree", "list", "--porcelain"])
        .contains(workspace.path.to_str().unwrap()));
    assert_eq!(
        git(
            repo.path(),
            [
                "branch",
                "--list",
                &workspace.branch,
                "--format=%(refname:short)"
            ]
        ),
        ""
    );
}

#[test]
fn cleanup_rejects_workspace_values_that_do_not_match_manager_and_id() {
    let repo = TestRepo::new();
    let other_repo = TestRepo::new();
    let manager = WorkspaceManager::discover(repo.path()).unwrap();
    let workspace = manager.create("trusted-id").unwrap();

    let wrong_repo = Workspace {
        repo_root: other_repo.path().to_path_buf(),
        ..workspace.clone()
    };
    assert!(manager
        .cleanup(&wrong_repo)
        .unwrap_err()
        .to_string()
        .contains("repo root"));

    let wrong_path = Workspace {
        path: repo.path().join("outside-trusted-id"),
        ..workspace.clone()
    };
    assert!(manager
        .cleanup(&wrong_path)
        .unwrap_err()
        .to_string()
        .contains("workspace path"));

    let wrong_branch = Workspace {
        branch: "not-ralphterm/trusted-id".to_string(),
        ..workspace.clone()
    };
    assert!(manager
        .cleanup(&wrong_branch)
        .unwrap_err()
        .to_string()
        .contains("workspace branch"));

    manager.cleanup(&workspace).unwrap();
}

struct TestDir {
    path: std::path::PathBuf,
}

impl TestDir {
    fn new(prefix: &str) -> Self {
        let path = temp_dir(prefix);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct TestRepo {
    path: std::path::PathBuf,
}

impl TestRepo {
    fn new() -> Self {
        let path = temp_dir("ralphterm-workspace-repo");
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

fn temp_dir(prefix: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, Uuid::new_v4()))
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

fn ralphterm<I, S>(cwd: &Path, args: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "ralphterm command failed in {}\nstdout:\n{}\nstderr:\n{}",
        cwd.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn ralphterm_failure<I, S>(cwd: &Path, args: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "ralphterm command unexpectedly succeeded in {}\nstdout:\n{}\nstderr:\n{}",
        cwd.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim()
    .to_string()
}
