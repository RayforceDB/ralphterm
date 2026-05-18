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
        let path = std::env::temp_dir().join(format!("ralphterm-config-{label}-{unique}"));
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

fn write_minimal_plan(path: &std::path::Path) {
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

#[test]
fn global_ini_config_supplies_claude_command_and_no_review_tool() {
    let repo = TempRepo::new("global");
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add plan"]);

    let config_dir = repo.path.join("global-config");
    fs::create_dir(&config_dir).expect("create config dir");
    let config_body = format!(
        "[default]\nclaude_command = {}\nexternal_review_tool = none\n",
        fixture_path("fake-agent.sh")
            .to_str()
            .expect("utf8 fixture")
    );
    fs::write(config_dir.join("config"), config_body).expect("write global config");
    repo.git(["add", "global-config"]);
    repo.git(["commit", "-m", "wip: config"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "--tasks-only",
            "--config-dir",
            config_dir.to_str().expect("utf8 config dir"),
            "--no-commit",
            plan_path.to_str().expect("utf8 plan path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm with global config failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        repo.path.join("first.txt").exists(),
        "global config should provide --claude-command default"
    );
}

#[test]
fn project_local_config_overrides_global() {
    let repo = TempRepo::new("local");
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);

    let config_dir = repo.path.join("global-config");
    fs::create_dir(&config_dir).expect("create config dir");
    // Global points at a script that would NOT create first.txt; local must override.
    let bad_script = repo.path.join("bad-agent.sh");
    fs::write(&bad_script, "#!/usr/bin/env sh\nexit 1\n").expect("write bad script");
    let mut perms = fs::metadata(&bad_script).expect("stat bad").permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    fs::set_permissions(&bad_script, perms).expect("chmod bad");

    let global_body = format!(
        "[default]\nclaude_command = {}\nexternal_review_tool = none\n",
        bad_script.to_str().expect("utf8 path")
    );
    fs::write(config_dir.join("config"), global_body).expect("write global config");

    let project_dir = repo.path.join(".ralphterm");
    fs::create_dir(&project_dir).expect("create .ralphterm dir");
    let project_body = serde_json::json!({
        "claude_command": fixture_path("fake-agent.sh").to_str().expect("utf8 fixture"),
        "external_review_tool": "none"
    })
    .to_string();
    fs::write(project_dir.join("config.json"), project_body).expect("write project config");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "wip"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "--tasks-only",
            "--config-dir",
            config_dir.to_str().expect("utf8 config dir"),
            "--no-commit",
            plan_path.to_str().expect("utf8 plan path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm should run with project-local override\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        repo.path.join("first.txt").exists(),
        "project-local config should override global claude_command"
    );
}

#[test]
fn cli_flag_overrides_config_files() {
    let repo = TempRepo::new("cli");
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    write_minimal_plan(&plan_path);

    let config_dir = repo.path.join("global-config");
    fs::create_dir(&config_dir).expect("create config dir");
    let bad_script = repo.path.join("bad-agent.sh");
    fs::write(&bad_script, "#!/usr/bin/env sh\nexit 1\n").expect("write bad script");
    let mut perms = fs::metadata(&bad_script).expect("stat bad").permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    fs::set_permissions(&bad_script, perms).expect("chmod bad");

    let global_body = format!(
        "[default]\nclaude_command = {}\nexternal_review_tool = none\n",
        bad_script.to_str().expect("utf8 path")
    );
    fs::write(config_dir.join("config"), global_body).expect("write global config");

    let project_dir = repo.path.join(".ralphterm");
    fs::create_dir(&project_dir).expect("create .ralphterm dir");
    let project_body = serde_json::json!({
        "claude_command": bad_script.to_str().expect("utf8 path"),
        "external_review_tool": "none"
    })
    .to_string();
    fs::write(project_dir.join("config.json"), project_body).expect("write project config");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "wip"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "--tasks-only",
            "--config-dir",
            config_dir.to_str().expect("utf8 config dir"),
            "--claude-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--external-review-tool",
            "none",
            "--no-commit",
            plan_path.to_str().expect("utf8 plan path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm CLI override failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        repo.path.join("first.txt").exists(),
        "CLI --claude-command should take precedence over config files"
    );
}
