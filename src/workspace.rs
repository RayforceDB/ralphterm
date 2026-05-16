use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub id: String,
    pub repo_root: PathBuf,
    pub path: PathBuf,
    pub branch: String,
    pub base_commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceManager {
    repo_root: PathBuf,
}

impl WorkspaceManager {
    pub fn discover(start: impl AsRef<Path>) -> Result<Self> {
        let start = start.as_ref();
        let output = Command::new("git")
            .arg("-C")
            .arg(start)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .with_context(|| format!("detect git repository from {}", start.display()))?;

        if !output.status.success() {
            bail!("{} is not inside a git repository", start.display());
        }

        let repo_root = String::from_utf8(output.stdout)
            .context("git returned non-utf8 repository path")?
            .trim()
            .to_string();
        if repo_root.is_empty() {
            bail!("git returned an empty repository path");
        }

        Ok(Self {
            repo_root: PathBuf::from(repo_root),
        })
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    pub fn create(&self, id: impl AsRef<str>) -> Result<Workspace> {
        let id = sanitize_id(id.as_ref())?;
        let base_commit = self.git_output(["rev-parse", "HEAD"])?;
        let branch = format!("ralphterm/{id}");
        let workspaces_dir = self.repo_root.join(".ralphterm").join("workspaces");
        let path = workspaces_dir.join(&id);

        fs::create_dir_all(&workspaces_dir)
            .with_context(|| format!("create workspaces directory {}", workspaces_dir.display()))?;
        if path.exists() {
            bail!("workspace path already exists: {}", path.display());
        }

        self.git_checked([
            "worktree",
            "add",
            "-b",
            branch.as_str(),
            path.to_str().with_context(|| {
                format!("workspace path is not valid utf-8: {}", path.display())
            })?,
            base_commit.as_str(),
        ])?;

        Ok(Workspace {
            id,
            repo_root: self.repo_root.clone(),
            path,
            branch,
            base_commit,
        })
    }

    pub fn cleanup(&self, workspace: &Workspace) -> Result<()> {
        if workspace.path.exists() {
            self.git_checked([
                "worktree",
                "remove",
                "--force",
                workspace.path.to_str().with_context(|| {
                    format!(
                        "workspace path is not valid utf-8: {}",
                        workspace.path.display()
                    )
                })?,
            ])?;
        }

        let branch_exists = self.git_output(["branch", "--list", workspace.branch.as_str()])?;
        if !branch_exists.trim().is_empty() {
            self.git_checked(["branch", "-D", workspace.branch.as_str()])?;
        }

        Ok(())
    }

    fn git_output<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.repo_root)
            .args(args)
            .output()
            .with_context(|| format!("run git in {}", self.repo_root.display()))?;
        if !output.status.success() {
            bail!(
                "git command failed in {}: {}",
                self.repo_root.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8(output.stdout)
            .context("git returned non-utf8 output")?
            .trim()
            .to_string())
    }

    fn git_checked<I, S>(&self, args: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        self.git_output(args).map(|_| ())
    }
}

fn sanitize_id(input: &str) -> Result<String> {
    let id = input.trim();
    if id.is_empty() {
        bail!("workspace id cannot be empty");
    }
    if id == "." || id == ".." || id.contains('/') || id.contains('\\') {
        bail!("workspace id contains path separators");
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        bail!("workspace id may only contain ascii letters, numbers, '-' and '_'");
    }
    Ok(id.to_string())
}
