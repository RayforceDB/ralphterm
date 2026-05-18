use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::notify::NotifyOn;

/// Provider names recognised by the wrapper auto-detection logic. Each one
/// corresponds to a shipped `scripts/wrappers/<name>.sh` script (also
/// installed under `<exe_dir>/../share/ralphterm/wrappers/<name>.sh`).
const KNOWN_WRAPPER_PROVIDERS: &[&str] = &["codex", "copilot", "gemini", "opencode"];

/// Subset of ralphex configuration keys this CLI understands. Unknown keys are
/// ignored silently so forward-compatible config files do not break older binaries.
#[derive(Debug, Default, Clone)]
pub struct RalphexConfig {
    pub claude_command: Option<String>,
    pub claude_args: Option<String>,
    pub external_review_tool: Option<String>,
    pub custom_review_script: Option<String>,
    pub max_iterations: Option<usize>,
    pub max_external_iterations: Option<usize>,
    pub review_patience: Option<usize>,
    pub task_model: Option<String>,
    pub review_model: Option<String>,
    pub session_timeout: Option<String>,
    pub idle_timeout: Option<String>,
    pub wait: Option<String>,
    pub base_ref: Option<String>,
    pub move_plan_on_completion: Option<bool>,
    pub notify_telegram_token: Option<String>,
    pub notify_telegram_chat: Option<String>,
    pub notify_telegram_base: Option<String>,
    pub notify_slack_webhook: Option<String>,
    pub notify_webhook_url: Option<String>,
    pub notify_email_smtp_url: Option<String>,
    pub notify_email_from: Option<String>,
    pub notify_email_to: Option<String>,
    pub notify_on: Vec<NotifyOn>,
    /// `[agent] provider = ...` value when set. Drives wrapper auto-detection
    /// when `claude_command` is otherwise unset.
    pub agent_provider: Option<String>,
}

impl RalphexConfig {
    fn merge(&mut self, override_with: RalphexConfig) {
        if override_with.claude_command.is_some() {
            self.claude_command = override_with.claude_command;
        }
        if override_with.claude_args.is_some() {
            self.claude_args = override_with.claude_args;
        }
        if override_with.external_review_tool.is_some() {
            self.external_review_tool = override_with.external_review_tool;
        }
        if override_with.custom_review_script.is_some() {
            self.custom_review_script = override_with.custom_review_script;
        }
        if override_with.max_iterations.is_some() {
            self.max_iterations = override_with.max_iterations;
        }
        if override_with.max_external_iterations.is_some() {
            self.max_external_iterations = override_with.max_external_iterations;
        }
        if override_with.review_patience.is_some() {
            self.review_patience = override_with.review_patience;
        }
        if override_with.task_model.is_some() {
            self.task_model = override_with.task_model;
        }
        if override_with.review_model.is_some() {
            self.review_model = override_with.review_model;
        }
        if override_with.session_timeout.is_some() {
            self.session_timeout = override_with.session_timeout;
        }
        if override_with.idle_timeout.is_some() {
            self.idle_timeout = override_with.idle_timeout;
        }
        if override_with.wait.is_some() {
            self.wait = override_with.wait;
        }
        if override_with.base_ref.is_some() {
            self.base_ref = override_with.base_ref;
        }
        if override_with.move_plan_on_completion.is_some() {
            self.move_plan_on_completion = override_with.move_plan_on_completion;
        }
        if override_with.notify_telegram_token.is_some() {
            self.notify_telegram_token = override_with.notify_telegram_token;
        }
        if override_with.notify_telegram_chat.is_some() {
            self.notify_telegram_chat = override_with.notify_telegram_chat;
        }
        if override_with.notify_telegram_base.is_some() {
            self.notify_telegram_base = override_with.notify_telegram_base;
        }
        if override_with.notify_slack_webhook.is_some() {
            self.notify_slack_webhook = override_with.notify_slack_webhook;
        }
        if override_with.notify_webhook_url.is_some() {
            self.notify_webhook_url = override_with.notify_webhook_url;
        }
        if override_with.notify_email_smtp_url.is_some() {
            self.notify_email_smtp_url = override_with.notify_email_smtp_url;
        }
        if override_with.notify_email_from.is_some() {
            self.notify_email_from = override_with.notify_email_from;
        }
        if override_with.notify_email_to.is_some() {
            self.notify_email_to = override_with.notify_email_to;
        }
        if !override_with.notify_on.is_empty() {
            self.notify_on = override_with.notify_on;
        }
        if override_with.agent_provider.is_some() {
            self.agent_provider = override_with.agent_provider;
        }
    }

    fn set_known_key_in_section(&mut self, section: Option<&str>, key: &str, value: String) {
        // Inside `[agent]`, surface the `provider` shorthand as
        // `agent_provider` so wrapper auto-detection can resolve it later.
        if section == Some("agent") && key == "provider" {
            self.agent_provider = Some(value);
            return;
        }
        // Otherwise treat keys as belonging to a flat namespace for ralphex
        // compatibility (matching the legacy parser).
        self.set_known_key(key, value);
    }

    fn set_known_key(&mut self, key: &str, value: String) {
        match key {
            "agent_provider" => self.agent_provider = Some(value),
            "claude_command" => self.claude_command = Some(value),
            "claude_args" => self.claude_args = Some(value),
            "external_review_tool" => self.external_review_tool = Some(value),
            "custom_review_script" => self.custom_review_script = Some(value),
            "max_iterations" => {
                if let Ok(parsed) = value.parse::<usize>() {
                    self.max_iterations = Some(parsed);
                }
            }
            "max_external_iterations" => {
                if let Ok(parsed) = value.parse::<usize>() {
                    self.max_external_iterations = Some(parsed);
                }
            }
            "review_patience" => {
                if let Ok(parsed) = value.parse::<usize>() {
                    self.review_patience = Some(parsed);
                }
            }
            "task_model" => self.task_model = Some(value),
            "review_model" => self.review_model = Some(value),
            "session_timeout" => self.session_timeout = Some(value),
            "idle_timeout" => self.idle_timeout = Some(value),
            "wait" => self.wait = Some(value),
            "base_ref" => self.base_ref = Some(value),
            "move_plan_on_completion" => {
                if let Some(parsed) = parse_bool(&value) {
                    self.move_plan_on_completion = Some(parsed);
                }
            }
            "notify_telegram_token" => self.notify_telegram_token = Some(value),
            "notify_telegram_chat" | "notify_telegram_chat_id" => {
                self.notify_telegram_chat = Some(value)
            }
            "notify_telegram_base" => self.notify_telegram_base = Some(value),
            "notify_slack" | "notify_slack_webhook" => self.notify_slack_webhook = Some(value),
            "notify_webhook" | "notify_webhook_url" => self.notify_webhook_url = Some(value),
            "notify_email_smtp_url" => self.notify_email_smtp_url = Some(value),
            "notify_email_from" => self.notify_email_from = Some(value),
            "notify_email_to" => self.notify_email_to = Some(value),
            "notify_on" => {
                let mut parsed = Vec::new();
                for entry in value.split(',') {
                    if let Some(notify_on) = NotifyOn::parse(entry) {
                        parsed.push(notify_on);
                    }
                }
                if !parsed.is_empty() {
                    self.notify_on = parsed;
                }
            }
            _ => {}
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct ProjectConfigDocument {
    #[serde(default)]
    claude_command: Option<String>,
    #[serde(default)]
    claude_args: Option<String>,
    #[serde(default)]
    external_review_tool: Option<String>,
    #[serde(default)]
    custom_review_script: Option<String>,
    #[serde(default)]
    max_iterations: Option<usize>,
    #[serde(default)]
    max_external_iterations: Option<usize>,
    #[serde(default)]
    review_patience: Option<usize>,
    #[serde(default)]
    task_model: Option<String>,
    #[serde(default)]
    review_model: Option<String>,
    #[serde(default)]
    session_timeout: Option<String>,
    #[serde(default)]
    idle_timeout: Option<String>,
    #[serde(default)]
    wait: Option<String>,
    #[serde(default)]
    base_ref: Option<String>,
    #[serde(
        default,
        alias = "movePlanOnCompletion",
        alias = "move_plan_on_completion"
    )]
    move_plan_on_completion: Option<bool>,
    #[serde(default)]
    notify_telegram_token: Option<String>,
    #[serde(default, alias = "notify_telegram_chat_id")]
    notify_telegram_chat: Option<String>,
    #[serde(default)]
    notify_telegram_base: Option<String>,
    #[serde(default, alias = "notify_slack")]
    notify_slack_webhook: Option<String>,
    #[serde(default, alias = "notify_webhook")]
    notify_webhook_url: Option<String>,
    #[serde(default)]
    notify_email_smtp_url: Option<String>,
    #[serde(default)]
    notify_email_from: Option<String>,
    #[serde(default)]
    notify_email_to: Option<String>,
    #[serde(default)]
    notify_on: Vec<String>,
    #[serde(default, alias = "provider")]
    agent_provider: Option<String>,
}

impl From<ProjectConfigDocument> for RalphexConfig {
    fn from(value: ProjectConfigDocument) -> Self {
        Self {
            claude_command: value.claude_command,
            claude_args: value.claude_args,
            external_review_tool: value.external_review_tool,
            custom_review_script: value.custom_review_script,
            max_iterations: value.max_iterations,
            max_external_iterations: value.max_external_iterations,
            review_patience: value.review_patience,
            task_model: value.task_model,
            review_model: value.review_model,
            session_timeout: value.session_timeout,
            idle_timeout: value.idle_timeout,
            wait: value.wait,
            base_ref: value.base_ref,
            move_plan_on_completion: value.move_plan_on_completion,
            notify_telegram_token: value.notify_telegram_token,
            notify_telegram_chat: value.notify_telegram_chat,
            notify_telegram_base: value.notify_telegram_base,
            notify_slack_webhook: value.notify_slack_webhook,
            notify_webhook_url: value.notify_webhook_url,
            notify_email_smtp_url: value.notify_email_smtp_url,
            notify_email_from: value.notify_email_from,
            notify_email_to: value.notify_email_to,
            notify_on: value
                .notify_on
                .iter()
                .filter_map(|s| NotifyOn::parse(s))
                .collect(),
            agent_provider: value.agent_provider,
        }
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Load configuration from the global ralphex directory and the project-local
/// `.ralphterm/config.json` (or `.ralphterm/config`) file. Project values override
/// global values per field. If neither file exists, returns the default
/// configuration.
pub fn load(config_dir: Option<&Path>, project_root: &Path) -> Result<RalphexConfig> {
    let mut config = RalphexConfig::default();
    if let Some(global) = load_global(config_dir)? {
        config.merge(global);
    }
    if let Some(local) = load_project(project_root)? {
        config.merge(local);
    }
    apply_provider_wrapper(&mut config);
    Ok(config)
}

/// When `claude_command` is unset but `[agent] provider = ...` is set in the
/// loaded ralphex-compatible config, resolve the bundled wrapper script for
/// the requested provider and populate `claude_command` with its absolute
/// path. Unknown providers or missing wrapper files are logged with
/// `tracing::warn!` and leave `claude_command` untouched.
fn apply_provider_wrapper(config: &mut RalphexConfig) {
    if config.claude_command.is_some() {
        return;
    }
    let Some(provider_raw) = config.agent_provider.as_deref() else {
        return;
    };
    let provider = provider_raw.trim().to_ascii_lowercase();
    if provider.is_empty() {
        return;
    }
    if !KNOWN_WRAPPER_PROVIDERS.contains(&provider.as_str()) {
        tracing::warn!(
            provider = %provider_raw,
            "unknown agent provider; leaving claude_command unset"
        );
        return;
    }
    match locate_wrapper_script(&provider) {
        Some(path) => {
            config.claude_command = Some(path.to_string_lossy().into_owned());
        }
        None => {
            tracing::warn!(
                provider = %provider,
                "no wrapper script found for provider; leaving claude_command unset"
            );
        }
    }
}

/// Walk outward from the current executable and a few common ancestor
/// locations looking for a wrapper script. Returns the first path that
/// exists. Both the installed layout (`<exe_dir>/../share/ralphterm/wrappers/`)
/// and the development layout (`<repo_root>/scripts/wrappers/`) are
/// supported.
pub(crate) fn locate_wrapper_script(provider: &str) -> Option<PathBuf> {
    let filename = format!("{provider}.sh");

    let exe_path = std::env::current_exe().ok();

    if let Some(exe) = exe_path.as_ref() {
        let exe_dir = exe.parent();
        if let Some(dir) = exe_dir {
            // Installed layout: <exe_dir>/../share/ralphterm/wrappers/<name>.sh
            let installed = dir
                .join("..")
                .join("share")
                .join("ralphterm")
                .join("wrappers")
                .join(&filename);
            if installed.is_file() {
                return Some(canonicalize_or_keep(installed));
            }
        }

        // Dev fallback: walk up from the executable looking for a
        // `scripts/wrappers/<name>.sh` sibling. Cargo builds put the binary
        // in `target/<profile>/<binary>`, so we may need to climb several
        // levels before finding the repo root.
        if let Some(found) = walk_up_for_wrapper(exe, &filename) {
            return Some(canonicalize_or_keep(found));
        }
    }

    // Last-ditch: walk up from the current working directory. This makes the
    // dev fallback useful for cargo-run-from-checkout scenarios where
    // current_exe() resolves into a tmp dir without nearby wrappers.
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(found) = walk_up_for_wrapper(&cwd, &filename) {
            return Some(canonicalize_or_keep(found));
        }
    }

    // Final fallback: extract the wrapper from the binary itself. `cargo install`
    // only deposits the executable, so the on-disk scripts/ directory does not
    // exist for crates.io installs. The wrapper sources are embedded via
    // `include_str!` at compile time and written to an XDG cache directory on
    // first use.
    extract_embedded_wrapper(provider)
}

fn embedded_wrapper(provider: &str) -> Option<&'static str> {
    match provider {
        "codex" => Some(include_str!("../scripts/wrappers/codex.sh")),
        "copilot" => Some(include_str!("../scripts/wrappers/copilot.sh")),
        "gemini" => Some(include_str!("../scripts/wrappers/gemini.sh")),
        "opencode" => Some(include_str!("../scripts/wrappers/opencode.sh")),
        _ => None,
    }
}

fn extract_embedded_wrapper(provider: &str) -> Option<PathBuf> {
    let content = embedded_wrapper(provider)?;

    let cache_root = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;

    // Version-suffix the directory so a binary upgrade automatically refreshes
    // the cached wrappers instead of running a stale copy.
    let wrapper_dir = cache_root
        .join("ralphterm")
        .join(format!("wrappers-{}", env!("CARGO_PKG_VERSION")));
    fs::create_dir_all(&wrapper_dir).ok()?;
    let path = wrapper_dir.join(format!("{provider}.sh"));

    // Idempotent: skip rewriting if the cached content already matches.
    let needs_write = match fs::read_to_string(&path) {
        Ok(existing) => existing != content,
        Err(_) => true,
    };
    if needs_write {
        fs::write(&path, content).ok()?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path).ok()?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).ok()?;
    }

    Some(path)
}

fn walk_up_for_wrapper(start: &Path, filename: &str) -> Option<PathBuf> {
    let mut current: Option<&Path> = Some(start);
    while let Some(dir) = current {
        let candidate = dir.join("scripts").join("wrappers").join(filename);
        if candidate.is_file() {
            return Some(candidate);
        }
        current = dir.parent();
    }
    None
}

fn canonicalize_or_keep(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

fn load_global(config_dir: Option<&Path>) -> Result<Option<RalphexConfig>> {
    let dir = match config_dir {
        Some(dir) => Some(dir.to_path_buf()),
        None => resolve_default_global_dir(),
    };
    let Some(dir) = dir else {
        return Ok(None);
    };
    let path = dir.join("config");
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("read ralphex global config {}", path.display()))?;
    Ok(Some(parse_ini(&raw)))
}

fn resolve_default_global_dir() -> Option<PathBuf> {
    if let Some(env_dir) = std::env::var_os("RALPHTERM_CONFIG_DIR") {
        return Some(PathBuf::from(env_dir));
    }
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let base =
        xdg.or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("ralphex"))
}

fn load_project(project_root: &Path) -> Result<Option<RalphexConfig>> {
    let candidates = [
        project_root.join(".ralphterm").join("config.json"),
        project_root.join(".ralphterm").join("config"),
    ];
    for candidate in candidates {
        if !candidate.exists() {
            continue;
        }
        let raw = fs::read_to_string(&candidate)
            .with_context(|| format!("read project ralphex config {}", candidate.display()))?;
        let parsed = if candidate.extension().and_then(|ext| ext.to_str()) == Some("json") {
            let doc: ProjectConfigDocument = serde_json::from_str(&raw)
                .with_context(|| format!("parse project ralphex config {}", candidate.display()))?;
            doc.into()
        } else {
            parse_ini(&raw)
        };
        return Ok(Some(parsed));
    }
    Ok(None)
}

fn parse_ini(text: &str) -> RalphexConfig {
    let mut config = RalphexConfig::default();
    let mut current_section: Option<String> = None;
    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            // Section headers (other than [agent]) are tolerated for ralphex
            // compatibility: keys outside [agent] still live in a flat
            // namespace, so we only need to track whether we are inside the
            // [agent] block.
            let header = trimmed
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim()
                .to_ascii_lowercase();
            current_section = Some(header);
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = strip_inline_comment(value.trim());
        let value = strip_quotes(value).to_string();
        if key.is_empty() {
            continue;
        }
        config.set_known_key_in_section(current_section.as_deref(), key, value);
    }
    config
}

fn strip_inline_comment(value: &str) -> &str {
    // INI comments started with `#` or `;` are stripped, but only if they appear
    // outside of quoted strings. The values we care about (shell commands, file
    // paths) typically do not contain comment markers. Be conservative: only
    // strip if there is whitespace before the marker, which protects values that
    // legitimately contain `#` (such as URL fragments).
    let mut last_value_end = value.len();
    let mut prev_char: Option<char> = None;
    for (idx, ch) in value.char_indices() {
        if (ch == '#' || ch == ';') && prev_char.map(|c| c.is_whitespace()).unwrap_or(false) {
            last_value_end = idx;
            break;
        }
        prev_char = Some(ch);
    }
    value[..last_value_end].trim_end()
}

fn strip_quotes(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
        {
            return &value[1..value.len() - 1];
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_wrapper_returns_script_for_each_known_provider() {
        for provider in &["codex", "copilot", "gemini", "opencode"] {
            let content = embedded_wrapper(provider)
                .unwrap_or_else(|| panic!("missing embedded wrapper for {provider}"));
            assert!(
                content.starts_with("#!/usr/bin/env sh"),
                "wrapper {provider} should start with a POSIX sh shebang"
            );
        }
        assert!(embedded_wrapper("unknown-provider").is_none());
    }

    #[test]
    fn extract_embedded_wrapper_writes_executable_script_into_cache() {
        let tmp = std::env::temp_dir().join(format!(
            "ralphterm-wrapper-extract-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let prev_xdg = std::env::var_os("XDG_CACHE_HOME");
        // SAFETY: tests in this binary are single-threaded by default in
        // cargo nextest's per-test process model, and the integration tests
        // do not read XDG_CACHE_HOME concurrently. We restore the prior
        // value before returning.
        unsafe {
            std::env::set_var("XDG_CACHE_HOME", &tmp);
        }

        let path = extract_embedded_wrapper("codex").expect("codex wrapper should extract");

        assert!(path.is_file(), "extracted wrapper should exist: {path:?}");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("COMPLETED"),
            "wrapper should contain the COMPLETED marker"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755, "wrapper should be executable: mode={mode:o}");
        }
        assert!(
            path.starts_with(&tmp),
            "wrapper should land under XDG_CACHE_HOME: {path:?}"
        );

        // restore
        // SAFETY: see note above.
        unsafe {
            match prev_xdg {
                Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
                None => std::env::remove_var("XDG_CACHE_HOME"),
            }
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn parse_ini_extracts_known_keys_and_ignores_unknown() {
        let raw = r#"
            [default]
            claude_command = /usr/local/bin/claude
            external_review_tool = none
            unknown_key = something
            # comment line
            ; another comment
        "#;
        let config = parse_ini(raw);
        assert_eq!(
            config.claude_command.as_deref(),
            Some("/usr/local/bin/claude")
        );
        assert_eq!(config.external_review_tool.as_deref(), Some("none"));
    }

    #[test]
    fn parse_ini_strips_quotes_and_inline_comments() {
        let raw = "claude_command = \"/path with space/claude\" ; trailing\n";
        let config = parse_ini(raw);
        assert_eq!(
            config.claude_command.as_deref(),
            Some("/path with space/claude")
        );
    }

    #[test]
    fn merge_overrides_only_set_fields() {
        let mut base = RalphexConfig {
            claude_command: Some("base-cmd".to_string()),
            external_review_tool: Some("none".to_string()),
            ..RalphexConfig::default()
        };
        let override_with = RalphexConfig {
            claude_command: Some("over-cmd".to_string()),
            ..RalphexConfig::default()
        };
        base.merge(override_with);
        assert_eq!(base.claude_command.as_deref(), Some("over-cmd"));
        assert_eq!(base.external_review_tool.as_deref(), Some("none"));
    }
}
