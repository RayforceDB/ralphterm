use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use ralphterm::docker::{docker_available, docker_wrap_command, DockerConfig, VolumeSpec};
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
        let path = std::env::temp_dir().join(format!("ralphterm-docker-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        let _ = Command::new("git")
            .current_dir(&path)
            .args(["init", "--initial-branch", "main"])
            .output();
        let _ = Command::new("git")
            .current_dir(&path)
            .args(["config", "user.email", "test@example.com"])
            .output();
        let _ = Command::new("git")
            .current_dir(&path)
            .args(["config", "user.name", "Test User"])
            .output();
        fs::write(path.join("README.md"), "hello\n").unwrap();
        let _ = Command::new("git")
            .current_dir(&path)
            .args(["add", "README.md"])
            .output();
        let _ = Command::new("git")
            .current_dir(&path)
            .args(["commit", "-m", "initial"])
            .output();
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

#[test]
fn docker_wrap_command_builds_expected_invocation() {
    let cfg = DockerConfig {
        enabled: true,
        image: "ralphterm:latest".to_string(),
        preserve_anthropic_api_key: true,
        extra_volumes: vec![VolumeSpec {
            host: PathBuf::from("/host/data"),
            container: PathBuf::from("/data"),
            read_only: true,
        }],
        extra_env: vec![
            ("FOO".to_string(), Some("bar".to_string())),
            ("UNSET_VAR".to_string(), None),
        ],
        tz: Some("UTC".to_string()),
        aws_profile: Some("default".to_string()),
        aws_region: Some("us-east-1".to_string()),
    };
    let working_dir = PathBuf::from("/work/repo");
    let (cmd, args) = docker_wrap_command(&cfg, &working_dir, "claude", &["arg1".to_string()]);
    assert_eq!(cmd, "docker");
    // Required flags
    assert!(args.iter().any(|a| a == "run"));
    assert!(args.iter().any(|a| a == "--rm"));
    // Workdir mount and value
    let workdir_idx = args
        .iter()
        .position(|a| a == "-w")
        .expect("expected -w in args");
    assert_eq!(args[workdir_idx + 1], "/work/repo");
    // Volume mount for working dir
    let mut found_workdir_vol = false;
    let mut found_extra_vol = false;
    for window in args.windows(2) {
        if window[0] == "-v" {
            if window[1] == "/work/repo:/work/repo" {
                found_workdir_vol = true;
            }
            if window[1] == "/host/data:/data:ro" {
                found_extra_vol = true;
            }
        }
    }
    assert!(found_workdir_vol, "expected workdir volume mount: {args:?}");
    assert!(found_extra_vol, "expected extra volume mount: {args:?}");
    // Env passthrough: explicit value via -e KEY=value
    let mut found_foo = false;
    let mut found_unset = false;
    let mut found_anthropic = false;
    let mut found_tz = false;
    let mut found_aws_profile = false;
    let mut found_aws_region = false;
    for window in args.windows(2) {
        if window[0] == "-e" {
            if window[1] == "FOO=bar" {
                found_foo = true;
            }
            if window[1] == "UNSET_VAR" {
                found_unset = true;
            }
            if window[1] == "ANTHROPIC_API_KEY" {
                found_anthropic = true;
            }
            if window[1] == "TZ=UTC" {
                found_tz = true;
            }
            if window[1] == "AWS_PROFILE=default" {
                found_aws_profile = true;
            }
            if window[1] == "AWS_REGION=us-east-1" {
                found_aws_region = true;
            }
        }
    }
    assert!(found_foo, "expected FOO=bar env: {args:?}");
    assert!(found_unset, "expected UNSET_VAR env passthrough: {args:?}");
    assert!(found_anthropic, "expected ANTHROPIC_API_KEY env: {args:?}");
    assert!(found_tz, "expected TZ env: {args:?}");
    assert!(found_aws_profile, "expected AWS_PROFILE env: {args:?}");
    assert!(found_aws_region, "expected AWS_REGION env: {args:?}");
    // Image and trailing command
    let image_idx = args
        .iter()
        .position(|a| a == "ralphterm:latest")
        .expect("expected image name");
    assert_eq!(args[image_idx + 1], "claude");
    assert_eq!(args[image_idx + 2], "arg1");
}

#[test]
fn docker_wrap_command_skips_preserve_anthropic_when_disabled() {
    let cfg = DockerConfig {
        enabled: true,
        image: "ralphterm:latest".to_string(),
        preserve_anthropic_api_key: false,
        ..Default::default()
    };
    let (_, args) = docker_wrap_command(&cfg, &PathBuf::from("/work"), "claude", &[]);
    let mut found_anthropic = false;
    for window in args.windows(2) {
        if window[0] == "-e" && window[1] == "ANTHROPIC_API_KEY" {
            found_anthropic = true;
        }
    }
    assert!(
        !found_anthropic,
        "ANTHROPIC_API_KEY should not be forwarded when preserve flag is off: {args:?}"
    );
}

#[test]
fn extra_volumes_parser_rejects_malformed_input() {
    use ralphterm::docker::parse_extra_volumes;
    assert!(parse_extra_volumes("").unwrap().is_empty());
    let parsed = parse_extra_volumes("/host:/container:/host2:/container2:ro").unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].host, PathBuf::from("/host"));
    assert_eq!(parsed[0].container, PathBuf::from("/container"));
    assert!(!parsed[0].read_only);
    assert_eq!(parsed[1].host, PathBuf::from("/host2"));
    assert_eq!(parsed[1].container, PathBuf::from("/container2"));
    assert!(parsed[1].read_only);

    assert!(parse_extra_volumes("/missingcolon").is_err());
    assert!(parse_extra_volumes("/host:/container:weird").is_err());
}

#[test]
fn docker_smoke_runs_plan_against_in_test_image() {
    if !docker_available() {
        return;
    }
    let repo = TestRepo::new();
    let plan_path = repo.path().join("plan.md");
    write_minimal_plan(&plan_path);

    // Build a tiny image whose entrypoint prints COMPLETED and creates first.txt.
    let dockerfile = repo.path().join("Dockerfile.smoke");
    fs::write(
        &dockerfile,
        "FROM debian:stable-slim\nCMD [\"sh\",\"-c\",\"echo created > first.txt; echo RALPHEX:ALL_TASKS_DONE; echo COMPLETED\"]\n",
    )
    .unwrap();

    // Commit the plan + Dockerfile so preflight's dirty-worktree check
    // (added in 9ab2cbc) doesn't refuse to create the feature branch
    // for the run.
    let _ = Command::new("git")
        .current_dir(repo.path())
        .args(["add", "plan.md", "Dockerfile.smoke"])
        .output();
    let _ = Command::new("git")
        .current_dir(repo.path())
        .args(["commit", "-m", "fixtures"])
        .output();
    let image_tag = format!("ralphterm-test-smoke-{}", Uuid::new_v4().simple());
    let build = Command::new("docker")
        .current_dir(repo.path())
        .args(["build", "-f", "Dockerfile.smoke", "-t", &image_tag, "."])
        .output()
        .expect("build docker image");
    if !build.status.success() {
        eprintln!(
            "docker build failed; skipping. stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
        return;
    }

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path())
        .args([
            "--tasks-only",
            "--claude-command",
            fixture_path("fake-agent.sh").to_str().unwrap(),
            "--no-commit",
            "--docker",
            "--docker-image",
            &image_tag,
            "plan.md",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");
    assert!(
        output.status.success(),
        "ralphterm should succeed with docker image: stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = Command::new("docker")
        .args(["rmi", "-f", &image_tag])
        .output();
}
