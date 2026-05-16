use std::{fs, path::Path, process::Command};

use ralphterm::workspace::WorkspaceManager;
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
    let dir = temp_dir("ralphterm-not-git");
    fs::create_dir_all(&dir).unwrap();

    let error = WorkspaceManager::discover(&dir).unwrap_err().to_string();

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
