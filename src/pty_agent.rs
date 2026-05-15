use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    Claude,
    Codex,
}

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub agent: AgentKind,
    pub prompt: String,
    pub cwd: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone)]
pub struct SessionInput {
    pub text: String,
    pub enter: bool,
}

pub fn default_command(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => "claude",
        AgentKind::Codex => "codex",
    }
}

pub fn default_args(agent: AgentKind) -> Vec<String> {
    match agent {
        // Important: no `-p`. The mux starts the official CLI like a user and pastes input.
        AgentKind::Claude => vec![],
        AgentKind::Codex => vec![],
    }
}
