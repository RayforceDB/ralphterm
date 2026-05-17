//! Compatibility tests for the bundled provider wrapper scripts and the
//! ralphex `[agent] provider = ...` auto-detection logic.
//!
//! Each wrapper is exercised against a fake shim binary placed on `$PATH` via
//! `PROVIDER_OVERRIDE`, so the upstream CLIs (codex, gh, gemini, opencode)
//! are not required to run these tests.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn wrapper_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("wrappers")
        .join(name)
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ralphterm-wrappers-{label}-{unique}"));
        fs::create_dir(&path).expect("create temp dir");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_shim(dir: &Path, name: &str, body: &str) -> PathBuf {
    let shim = dir.join(name);
    fs::write(&shim, body).expect("write shim");
    let mut perms = fs::metadata(&shim).expect("stat shim").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&shim, perms).expect("chmod shim");
    shim
}

fn success_shim_body(argv_log: &Path, stdin_log: &Path) -> String {
    format!(
        "#!/usr/bin/env sh\nset -eu\nprintf '%s\\n' \"$@\" > {argv}\ncat > {stdin}\nprintf 'hello from shim\\n'\n",
        argv = shell_quote(argv_log),
        stdin = shell_quote(stdin_log)
    )
}

fn failure_shim_body(rc: i32) -> String {
    format!("#!/usr/bin/env sh\nset -eu\ncat > /dev/null\nprintf 'shim failed\\n' >&2\nexit {rc}\n")
}

fn shell_quote(path: &Path) -> String {
    let s = path.to_str().expect("utf8 path");
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

fn run_wrapper(
    wrapper: &Path,
    shim_dir: &Path,
    shim_name: &str,
    prompt: &str,
    extra_env: &[(&str, &str)],
) -> std::process::Output {
    let path = format!(
        "{}:{}",
        shim_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new("/usr/bin/env");
    command
        .arg("sh")
        .arg(wrapper)
        .env("PATH", path)
        .env("PROVIDER_OVERRIDE", shim_name)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let mut spawned = command.spawn().expect("spawn wrapper");
    {
        use std::io::Write;
        let stdin = spawned.stdin.as_mut().expect("wrapper stdin");
        stdin.write_all(prompt.as_bytes()).expect("write prompt");
    }
    spawned.wait_with_output().expect("collect wrapper output")
}

#[test]
fn codex_wrapper_emits_completed_on_success() {
    let tmp = TempDir::new("codex-ok");
    let argv_log = tmp.path.join("argv.txt");
    let stdin_log = tmp.path.join("stdin.txt");
    let shim_name = "codex-shim";
    write_shim(
        &tmp.path,
        shim_name,
        &success_shim_body(&argv_log, &stdin_log),
    );

    let output = run_wrapper(
        &wrapper_path("codex.sh"),
        &tmp.path,
        shim_name,
        "hello codex",
        &[],
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "codex wrapper failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("COMPLETED"),
        "expected COMPLETED in: {stdout}"
    );
    let stdin_contents = fs::read_to_string(&stdin_log).expect("read stdin log");
    assert!(
        stdin_contents.contains("hello codex"),
        "shim stdin should contain prompt: {stdin_contents}"
    );
}

#[test]
fn codex_wrapper_forwards_claude_model_as_model_flag() {
    let tmp = TempDir::new("codex-model");
    let argv_log = tmp.path.join("argv.txt");
    let stdin_log = tmp.path.join("stdin.txt");
    let shim_name = "codex-shim-model";
    write_shim(
        &tmp.path,
        shim_name,
        &success_shim_body(&argv_log, &stdin_log),
    );

    let output = run_wrapper(
        &wrapper_path("codex.sh"),
        &tmp.path,
        shim_name,
        "prompt",
        &[("CLAUDE_MODEL", "gpt-5-codex")],
    );

    assert!(output.status.success());
    let argv = fs::read_to_string(&argv_log).expect("read argv log");
    assert!(
        argv.contains("--model") && argv.contains("gpt-5-codex"),
        "expected --model gpt-5-codex in argv: {argv}"
    );
}

#[test]
fn codex_wrapper_propagates_shim_failure_exit_code() {
    let tmp = TempDir::new("codex-fail");
    let shim_name = "codex-shim-fail";
    write_shim(&tmp.path, shim_name, &failure_shim_body(7));

    let output = run_wrapper(
        &wrapper_path("codex.sh"),
        &tmp.path,
        shim_name,
        "prompt",
        &[],
    );

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(7));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("FAILED"),
        "expected FAILED line in: {stdout}"
    );
}

#[test]
fn copilot_wrapper_emits_completed_and_invokes_provider() {
    let tmp = TempDir::new("copilot-ok");
    let argv_log = tmp.path.join("argv.txt");
    let stdin_log = tmp.path.join("stdin.txt");
    let shim_name = "copilot-shim";
    write_shim(
        &tmp.path,
        shim_name,
        &success_shim_body(&argv_log, &stdin_log),
    );

    let output = run_wrapper(
        &wrapper_path("copilot.sh"),
        &tmp.path,
        shim_name,
        "what does this command do?",
        &[],
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "copilot wrapper failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("COMPLETED"));
    let stdin_contents = fs::read_to_string(&stdin_log).expect("read stdin log");
    assert!(stdin_contents.contains("what does this command do?"));
}

#[test]
fn copilot_wrapper_forwards_model_flag() {
    let tmp = TempDir::new("copilot-model");
    let argv_log = tmp.path.join("argv.txt");
    let stdin_log = tmp.path.join("stdin.txt");
    let shim_name = "copilot-shim-model";
    write_shim(
        &tmp.path,
        shim_name,
        &success_shim_body(&argv_log, &stdin_log),
    );

    let output = run_wrapper(
        &wrapper_path("copilot.sh"),
        &tmp.path,
        shim_name,
        "prompt",
        &[("CLAUDE_MODEL", "copilot-default")],
    );

    assert!(output.status.success());
    let argv = fs::read_to_string(&argv_log).expect("read argv log");
    assert!(
        argv.contains("--model") && argv.contains("copilot-default"),
        "expected --model copilot-default in argv: {argv}"
    );
}

#[test]
fn copilot_wrapper_propagates_shim_failure() {
    let tmp = TempDir::new("copilot-fail");
    let shim_name = "copilot-shim-fail";
    write_shim(&tmp.path, shim_name, &failure_shim_body(3));

    let output = run_wrapper(
        &wrapper_path("copilot.sh"),
        &tmp.path,
        shim_name,
        "prompt",
        &[],
    );

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stdout).contains("FAILED"));
}

#[test]
fn gemini_wrapper_emits_completed_on_success() {
    let tmp = TempDir::new("gemini-ok");
    let argv_log = tmp.path.join("argv.txt");
    let stdin_log = tmp.path.join("stdin.txt");
    let shim_name = "gemini-shim";
    write_shim(
        &tmp.path,
        shim_name,
        &success_shim_body(&argv_log, &stdin_log),
    );

    let output = run_wrapper(
        &wrapper_path("gemini.sh"),
        &tmp.path,
        shim_name,
        "hello gemini",
        &[],
    );

    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("COMPLETED"));
}

#[test]
fn gemini_wrapper_forwards_model_flag() {
    let tmp = TempDir::new("gemini-model");
    let argv_log = tmp.path.join("argv.txt");
    let stdin_log = tmp.path.join("stdin.txt");
    let shim_name = "gemini-shim-model";
    write_shim(
        &tmp.path,
        shim_name,
        &success_shim_body(&argv_log, &stdin_log),
    );

    let output = run_wrapper(
        &wrapper_path("gemini.sh"),
        &tmp.path,
        shim_name,
        "prompt",
        &[("CLAUDE_MODEL", "gemini-2.0-pro")],
    );

    assert!(output.status.success());
    let argv = fs::read_to_string(&argv_log).expect("read argv log");
    assert!(argv.contains("--model") && argv.contains("gemini-2.0-pro"));
}

#[test]
fn gemini_wrapper_propagates_shim_failure() {
    let tmp = TempDir::new("gemini-fail");
    let shim_name = "gemini-shim-fail";
    write_shim(&tmp.path, shim_name, &failure_shim_body(9));

    let output = run_wrapper(
        &wrapper_path("gemini.sh"),
        &tmp.path,
        shim_name,
        "prompt",
        &[],
    );

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(9));
    assert!(String::from_utf8_lossy(&output.stdout).contains("FAILED"));
}

#[test]
fn opencode_wrapper_emits_completed_on_success() {
    let tmp = TempDir::new("opencode-ok");
    let argv_log = tmp.path.join("argv.txt");
    let stdin_log = tmp.path.join("stdin.txt");
    let shim_name = "opencode-shim";
    write_shim(
        &tmp.path,
        shim_name,
        &success_shim_body(&argv_log, &stdin_log),
    );

    let output = run_wrapper(
        &wrapper_path("opencode.sh"),
        &tmp.path,
        shim_name,
        "hello opencode",
        &[],
    );

    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("COMPLETED"));
}

#[test]
fn opencode_wrapper_forwards_model_flag() {
    let tmp = TempDir::new("opencode-model");
    let argv_log = tmp.path.join("argv.txt");
    let stdin_log = tmp.path.join("stdin.txt");
    let shim_name = "opencode-shim-model";
    write_shim(
        &tmp.path,
        shim_name,
        &success_shim_body(&argv_log, &stdin_log),
    );

    let output = run_wrapper(
        &wrapper_path("opencode.sh"),
        &tmp.path,
        shim_name,
        "prompt",
        &[("CLAUDE_MODEL", "opus")],
    );

    assert!(output.status.success());
    let argv = fs::read_to_string(&argv_log).expect("read argv log");
    assert!(argv.contains("--model") && argv.contains("opus"));
}

#[test]
fn opencode_wrapper_propagates_shim_failure() {
    let tmp = TempDir::new("opencode-fail");
    let shim_name = "opencode-shim-fail";
    write_shim(&tmp.path, shim_name, &failure_shim_body(5));

    let output = run_wrapper(
        &wrapper_path("opencode.sh"),
        &tmp.path,
        shim_name,
        "prompt",
        &[],
    );

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(5));
}

#[test]
fn wrappers_fail_when_stdin_is_empty() {
    let tmp = TempDir::new("empty-stdin");
    let argv_log = tmp.path.join("argv.txt");
    let stdin_log = tmp.path.join("stdin.txt");
    let shim_name = "noop-shim";
    write_shim(
        &tmp.path,
        shim_name,
        &success_shim_body(&argv_log, &stdin_log),
    );

    let output = run_wrapper(&wrapper_path("codex.sh"), &tmp.path, shim_name, "", &[]);

    assert!(!output.status.success(), "empty stdin must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("FAILED"),
        "expected FAILED on empty stdin: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Rust-side auto-detection tests for `[agent] provider = ...`.
// ---------------------------------------------------------------------------

struct CfgTempDir {
    path: PathBuf,
}

impl CfgTempDir {
    fn new(label: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ralphterm-cfg-{label}-{unique}"));
        fs::create_dir(&path).expect("create temp cfg dir");
        Self { path }
    }
}

impl Drop for CfgTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn agent_provider_codex_resolves_bundled_wrapper() {
    let cfg = CfgTempDir::new("codex-detect");
    let cfg_dir = cfg.path.join("ralphex");
    fs::create_dir(&cfg_dir).expect("create cfg dir");
    fs::write(cfg_dir.join("config"), "[agent]\nprovider = codex\n").expect("write cfg");

    let project = CfgTempDir::new("codex-detect-project");

    let loaded = ralphterm::config::load(Some(&cfg_dir), &project.path).expect("load config");

    let resolved = loaded
        .claude_command
        .as_deref()
        .expect("claude_command should be auto-populated for provider = codex");
    assert!(
        resolved.ends_with("scripts/wrappers/codex.sh"),
        "expected wrapper path to end with scripts/wrappers/codex.sh, got {resolved}"
    );
    let p = Path::new(resolved);
    assert!(
        p.is_absolute(),
        "wrapper path should be absolute: {resolved}"
    );
    assert!(p.exists(), "wrapper path should exist on disk: {resolved}");
}

#[test]
fn agent_provider_for_each_known_provider_resolves() {
    for provider in ["codex", "copilot", "gemini", "opencode"] {
        let cfg = CfgTempDir::new(&format!("each-{provider}"));
        let cfg_dir = cfg.path.join("ralphex");
        fs::create_dir(&cfg_dir).expect("create cfg dir");
        fs::write(
            cfg_dir.join("config"),
            format!("[agent]\nprovider = {provider}\n"),
        )
        .expect("write cfg");

        let project = CfgTempDir::new(&format!("each-{provider}-project"));
        let loaded = ralphterm::config::load(Some(&cfg_dir), &project.path).expect("load config");
        let resolved = loaded
            .claude_command
            .as_deref()
            .unwrap_or_else(|| panic!("expected wrapper auto-detection for provider {provider}"));
        assert!(
            resolved.ends_with(&format!("scripts/wrappers/{provider}.sh")),
            "wrong wrapper for {provider}: {resolved}"
        );
    }
}

#[test]
fn agent_provider_unknown_leaves_claude_command_unset() {
    let cfg = CfgTempDir::new("unknown");
    let cfg_dir = cfg.path.join("ralphex");
    fs::create_dir(&cfg_dir).expect("create cfg dir");
    fs::write(
        cfg_dir.join("config"),
        "[agent]\nprovider = does-not-exist\n",
    )
    .expect("write cfg");
    let project = CfgTempDir::new("unknown-project");

    let loaded = ralphterm::config::load(Some(&cfg_dir), &project.path).expect("load config");
    assert!(
        loaded.claude_command.is_none(),
        "unknown provider should leave claude_command unset, got: {:?}",
        loaded.claude_command
    );
    // The provider value should still be preserved for diagnostics.
    assert_eq!(loaded.agent_provider.as_deref(), Some("does-not-exist"));
}

#[test]
fn agent_provider_does_not_override_explicit_claude_command() {
    let cfg = CfgTempDir::new("explicit-wins");
    let cfg_dir = cfg.path.join("ralphex");
    fs::create_dir(&cfg_dir).expect("create cfg dir");
    fs::write(
        cfg_dir.join("config"),
        "[default]\nclaude_command = /usr/bin/true\n[agent]\nprovider = codex\n",
    )
    .expect("write cfg");
    let project = CfgTempDir::new("explicit-wins-project");

    let loaded = ralphterm::config::load(Some(&cfg_dir), &project.path).expect("load config");
    assert_eq!(loaded.claude_command.as_deref(), Some("/usr/bin/true"));
    assert_eq!(loaded.agent_provider.as_deref(), Some("codex"));
}
