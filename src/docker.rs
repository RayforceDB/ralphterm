//! Docker-isolation helpers for plan runs.
//!
//! `docker_wrap_command` translates an existing implementer/reviewer command
//! into a `docker run` invocation that mounts the working tree and forwards
//! the requested environment. The runner can pass the wrapped command to the
//! existing PTY-based execution path unchanged.
//!
//! Volume passthrough (`RALPHEX_EXTRA_VOLUMES`) and env passthrough
//! (`RALPHEX_EXTRA_ENV`) follow ralphex semantics:
//!
//!   * Extra volumes are colon-separated `host:container[:ro]` triples. Pairs
//!     are read greedily so the parser tolerates colon-separated lists with
//!     mixed `:ro` markers.
//!   * Extra env entries are comma-separated. A bare `KEY` forwards the
//!     current value of the env var; `KEY=VAL` sets a literal value inside
//!     the container.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeSpec {
    pub host: PathBuf,
    pub container: PathBuf,
    pub read_only: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DockerConfig {
    pub enabled: bool,
    pub image: String,
    pub preserve_anthropic_api_key: bool,
    pub extra_volumes: Vec<VolumeSpec>,
    pub extra_env: Vec<(String, Option<String>)>,
    pub tz: Option<String>,
    pub aws_profile: Option<String>,
    pub aws_region: Option<String>,
}

impl DockerConfig {
    pub const DEFAULT_IMAGE: &'static str = "ralphterm:latest";
}

/// Build a (command, args) pair that runs the given `command`/`args` inside
/// a docker container, mounting `working_dir` and forwarding the configured
/// environment.
pub fn docker_wrap_command(
    cfg: &DockerConfig,
    working_dir: &Path,
    command: &str,
    args: &[String],
) -> (String, Vec<String>) {
    let mut wrapped: Vec<String> = vec!["run".to_string(), "--rm".to_string(), "-i".to_string()];
    // Allocate a TTY so interactive PTY clients (claude, codex) behave the
    // same inside the container as on the host.
    wrapped.push("--tty".to_string());

    // Mount the working tree at the same path so any tooling that uses
    // absolute paths sees a familiar layout.
    let working_dir_str = working_dir.to_string_lossy().to_string();
    wrapped.push("-v".to_string());
    wrapped.push(format!("{}:{}", working_dir_str, working_dir_str));
    for vol in &cfg.extra_volumes {
        let host = vol.host.to_string_lossy();
        let container = vol.container.to_string_lossy();
        let mount = if vol.read_only {
            format!("{host}:{container}:ro")
        } else {
            format!("{host}:{container}")
        };
        wrapped.push("-v".to_string());
        wrapped.push(mount);
    }
    wrapped.push("-w".to_string());
    wrapped.push(working_dir_str);

    if cfg.preserve_anthropic_api_key {
        wrapped.push("-e".to_string());
        wrapped.push("ANTHROPIC_API_KEY".to_string());
    }
    if let Some(tz) = &cfg.tz {
        wrapped.push("-e".to_string());
        wrapped.push(format!("TZ={tz}"));
    }
    if let Some(profile) = &cfg.aws_profile {
        wrapped.push("-e".to_string());
        wrapped.push(format!("AWS_PROFILE={profile}"));
    }
    if let Some(region) = &cfg.aws_region {
        wrapped.push("-e".to_string());
        wrapped.push(format!("AWS_REGION={region}"));
    }
    for (key, value) in &cfg.extra_env {
        wrapped.push("-e".to_string());
        match value {
            Some(value) => wrapped.push(format!("{key}={value}")),
            None => wrapped.push(key.clone()),
        }
    }

    wrapped.push(cfg.image.clone());
    wrapped.push(command.to_string());
    for arg in args {
        wrapped.push(arg.clone());
    }
    ("docker".to_string(), wrapped)
}

/// Returns true when a `docker` binary is on PATH and reports a version. Used
/// to gate Docker-dependent tests at runtime so the suite remains useful in
/// environments without Docker installed.
pub fn docker_available() -> bool {
    Command::new("docker")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Parse `RALPHEX_EXTRA_VOLUMES` colon-separated entries. The format is a
/// greedy `host:container[:ro]` list; pairs are picked off from the left so
/// multiple volumes can be packed into a single env var.
pub fn parse_extra_volumes(raw: &str) -> Result<Vec<VolumeSpec>, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let mut parts: Vec<&str> = raw.split(':').collect();
    let mut out = Vec::new();
    while !parts.is_empty() {
        if parts.len() < 2 {
            return Err(format!(
                "invalid volume spec (missing container path): {raw}"
            ));
        }
        let host = parts.remove(0);
        let container = parts.remove(0);
        let read_only = match parts.first().copied() {
            Some("ro") => {
                parts.remove(0);
                true
            }
            Some("rw") => {
                parts.remove(0);
                false
            }
            Some(other) if other.starts_with('/') => false,
            Some(other) => {
                return Err(format!("unexpected volume modifier {other:?} in {raw}"));
            }
            None => false,
        };
        if host.is_empty() || container.is_empty() {
            return Err(format!("empty volume path component in {raw}"));
        }
        out.push(VolumeSpec {
            host: PathBuf::from(host),
            container: PathBuf::from(container),
            read_only,
        });
    }
    Ok(out)
}

/// Parse `RALPHEX_EXTRA_ENV` comma-separated env-passthrough entries. Each
/// entry is either `KEY=VALUE` (forwarded with the given literal value) or
/// `KEY` (forwarded with whatever value the env var currently holds).
pub fn parse_extra_env(raw: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        match entry.split_once('=') {
            Some((key, value)) => out.push((key.trim().to_string(), Some(value.to_string()))),
            None => out.push((entry.to_string(), None)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extra_env_handles_mixed_entries() {
        let parsed = parse_extra_env("FOO=bar, PLAIN ,EMPTY=,LAST=v");
        assert_eq!(
            parsed,
            vec![
                ("FOO".to_string(), Some("bar".to_string())),
                ("PLAIN".to_string(), None),
                ("EMPTY".to_string(), Some("".to_string())),
                ("LAST".to_string(), Some("v".to_string())),
            ]
        );
    }
}
