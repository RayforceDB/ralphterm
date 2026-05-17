use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::Deserialize;

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
    }

    fn set_known_key(&mut self, key: &str, value: String) {
        match key {
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
        }
    }
}

/// Load configuration from the global ralphex directory and the project-local
/// `.ralphex/config.json` (or `.ralphex/config`) file. Project values override
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
    Ok(config)
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
    if let Some(env_dir) = std::env::var_os("RALPHEX_CONFIG_DIR") {
        return Some(PathBuf::from(env_dir));
    }
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let base =
        xdg.or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("ralphex"))
}

fn load_project(project_root: &Path) -> Result<Option<RalphexConfig>> {
    let candidates = [
        project_root.join(".ralphex").join("config.json"),
        project_root.join(".ralphex").join("config"),
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
    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            // Section headers are tolerated but not currently used. We treat all
            // keys as living in a flat namespace for ralphex compatibility.
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
        config.set_known_key(key, value);
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
