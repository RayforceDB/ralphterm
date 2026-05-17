use std::{
    ffi::OsString,
    fs,
    io::Write,
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

    pub fn workspace(&self, id: impl AsRef<str>) -> Result<Workspace> {
        self.workspace_with_branch(id, None)
    }

    pub fn workspace_with_branch(
        &self,
        id: impl AsRef<str>,
        branch: Option<&str>,
    ) -> Result<Workspace> {
        let id = sanitize_id(id.as_ref())?;
        let base_commit = self.git_output(["rev-parse", "HEAD"])?;
        let branch = match branch {
            Some(name) => {
                validate_branch_name(name)?;
                name.to_string()
            }
            None => format!("ralphterm/{id}"),
        };
        let path = self
            .repo_root
            .join(".ralphterm")
            .join("workspaces")
            .join(&id);

        Ok(Workspace {
            id,
            repo_root: self.repo_root.clone(),
            path,
            branch,
            base_commit,
        })
    }

    pub fn create(&self, id: impl AsRef<str>) -> Result<Workspace> {
        self.create_with_branch(id, None)
    }

    pub fn create_with_branch(
        &self,
        id: impl AsRef<str>,
        branch: Option<&str>,
    ) -> Result<Workspace> {
        let workspace = self.workspace_with_branch(id, branch)?;
        let id = workspace.id.clone();
        let base_commit = workspace.base_commit.clone();
        let branch = workspace.branch.clone();
        let workspaces_dir = self.repo_root.join(".ralphterm").join("workspaces");
        let path = workspace.path.clone();

        self.exclude_ralphterm_metadata()?;
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
        self.validate_existing_workspace(workspace)?;

        self.git_checked([
            "worktree",
            "remove",
            workspace.path.to_str().with_context(|| {
                format!(
                    "workspace path is not valid utf-8: {}",
                    workspace.path.display()
                )
            })?,
        ])?;

        let branch_exists = self.git_output(["branch", "--list", workspace.branch.as_str()])?;
        if !branch_exists.trim().is_empty() {
            self.git_checked(["branch", "-d", workspace.branch.as_str()])?;
        }

        Ok(())
    }

    pub fn validate_existing_workspace(&self, workspace: &Workspace) -> Result<()> {
        self.validate_workspace_metadata(workspace)?;

        let expected_ref = format!("refs/heads/{}", workspace.branch);
        match self.worktree_branch(&workspace.path)? {
            Some(branch) if branch == expected_ref => {}
            Some(branch) => bail!(
                "workspace worktree at {} has branch mismatch: expected {}, found {}",
                workspace.path.display(),
                expected_ref,
                if branch.is_empty() {
                    "<none>".to_string()
                } else {
                    branch
                }
            ),
            None => bail!(
                "no managed workspace worktree found at {}",
                workspace.path.display()
            ),
        }

        Ok(())
    }

    fn validate_workspace_metadata(&self, workspace: &Workspace) -> Result<()> {
        let id = sanitize_id(&workspace.id)?;
        if workspace.repo_root != self.repo_root {
            bail!(
                "workspace repo root {} does not match manager repo root {}",
                workspace.repo_root.display(),
                self.repo_root.display()
            );
        }

        let expected_path = self
            .repo_root
            .join(".ralphterm")
            .join("workspaces")
            .join(&id);
        if workspace.path != expected_path {
            bail!(
                "workspace path {} does not match expected path {}",
                workspace.path.display(),
                expected_path.display()
            );
        }

        let expected_branch = format!("ralphterm/{id}");
        if workspace.branch != expected_branch {
            // Allow custom branch overrides (validated separately).
            validate_branch_name(&workspace.branch)?;
        }

        Ok(())
    }

    fn exclude_ralphterm_metadata(&self) -> Result<()> {
        let exclude_path = self.git_path("info/exclude")?;
        let existing = fs::read_to_string(&exclude_path).unwrap_or_default();
        if existing.lines().any(|line| line == ".ralphterm/") {
            return Ok(());
        }

        if let Some(parent) = exclude_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create git exclude directory {}", parent.display()))?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&exclude_path)
            .with_context(|| format!("open git exclude file {}", exclude_path.display()))?;
        if !existing.is_empty() && !existing.ends_with('\n') {
            writeln!(file).context("terminate existing git exclude entry")?;
        }
        writeln!(file, ".ralphterm/").context("write .ralphterm/ to git exclude")?;
        Ok(())
    }

    fn worktree_branch(&self, path: &Path) -> Result<Option<String>> {
        let path = path
            .to_str()
            .with_context(|| format!("workspace path is not valid utf-8: {}", path.display()))?;
        let output = self.git_output(["worktree", "list", "--porcelain"])?;
        let mut record_path: Option<&str> = None;
        let mut record_branch: Option<&str> = None;

        for line in output.lines().chain(std::iter::once("")) {
            if line.is_empty() {
                if record_path == Some(path) {
                    return Ok(Some(record_branch.unwrap_or_default().to_string()));
                }
                record_path = None;
                record_branch = None;
                continue;
            }

            if let Some(worktree_path) = line.strip_prefix("worktree ") {
                record_path = Some(worktree_path);
            } else if let Some(branch) = line.strip_prefix("branch ") {
                record_branch = Some(branch);
            }
        }

        Ok(None)
    }

    fn git_path(&self, path: &str) -> Result<PathBuf> {
        let output = self.git_output(["rev-parse", "--git-path", path])?;
        let path = PathBuf::from(output);
        if path.is_absolute() {
            Ok(path)
        } else {
            Ok(self.repo_root.join(path))
        }
    }

    fn git_output<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let args = args
            .into_iter()
            .map(|arg| arg.as_ref().to_os_string())
            .collect::<Vec<OsString>>();
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.repo_root)
            .args(&args)
            .output()
            .with_context(|| format!("run git in {}", self.repo_root.display()))?;
        if !output.status.success() {
            let args = args
                .iter()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            bail!(
                "git command failed in {} (git {}): {}",
                self.repo_root.display(),
                args,
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

fn validate_branch_name(branch: &str) -> Result<()> {
    let trimmed = branch.trim();
    if trimmed.is_empty() {
        bail!("branch name cannot be empty");
    }
    if trimmed != branch {
        bail!("branch name must not have leading or trailing whitespace");
    }
    if trimmed.starts_with('-')
        || trimmed.starts_with('/')
        || trimmed.ends_with('/')
        || trimmed.contains("..")
        || trimmed.contains("//")
        || trimmed.contains('\\')
        || trimmed.contains('\0')
    {
        bail!("invalid branch name: {trimmed}");
    }
    if !trimmed.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | '+' | '#' | '@')
    }) {
        bail!("branch name may only contain ascii letters, numbers, '-', '_', '.', '/', '+', '#', '@'");
    }
    Ok(())
}

/// Derive a kebab-case ASCII workspace id from a plan file path. Uses the
/// file stem, lower-cases ascii letters, replaces non-alphanumeric runs with
/// a single '-', and trims leading/trailing dashes. Errors if the result is
/// empty (e.g., the plan filename has no ascii characters).
pub fn workspace_id_from_plan_path(plan: &Path) -> Result<String> {
    let stem = plan
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("plan filename is not valid utf-8: {}", plan.display()))?;
    let mut out = String::with_capacity(stem.len());
    let mut last_dash = false;
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            last_dash = false;
        } else if (matches!(ch, '-' | '_') || ch.is_ascii_whitespace())
            && !out.is_empty()
            && !last_dash
        {
            out.push('-');
            last_dash = true;
        }
        // Drop other characters silently (e.g., dots, plus signs).
    }
    let id = out.trim_matches('-').to_string();
    if id.is_empty() {
        bail!(
            "cannot derive workspace id from plan filename: {}",
            plan.display()
        );
    }
    sanitize_id(&id)
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
