use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use anyhow::{bail, Context};
use axum::{
    extract::{Path, State, WebSocketUpgrade},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use clap::{ArgMatches, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

use crate::pty_agent::{AgentKind, SessionConfig, SessionInput};
use crate::store::{ApprovalDecisionError, SessionRecord, SessionStore};
use crate::{
    plan::parse_plan,
    runner::{
        agent_commands_equivalent, run_plan, run_smoke, PlanRunEvent, RunOptions,
        DEFAULT_PLAN_AGENT_COMMAND,
    },
    runs::{
        CreatedRunRecord, RunPhase, RunProgressEvent, RunRecord, RunResultArtifacts, RunStatus,
        RunStore,
    },
    workspace::WorkspaceManager,
};

#[derive(Debug, Parser)]
#[command(name = "ralphterm")]
#[command(version)]
#[command(about = "Terminal-native Claude/Codex orchestration API", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(
        short = 't',
        long = "tasks-only",
        help = "ralphex-compatible alias: run only task phase"
    )]
    compat_tasks_only: bool,
    #[arg(
        long = "claude-command",
        help = "ralphex-compatible alias for run --agent-command"
    )]
    compat_claude_command: Option<String>,
    #[arg(
        long = "custom-review-script",
        help = "ralphex-compatible alias for run --review-command"
    )]
    compat_custom_review_script: Option<String>,
    #[arg(long = "external-review-tool", value_parser = ["custom", "none"], help = "ralphex-compatible external review selector")]
    compat_external_review_tool: Option<String>,
    #[arg(
        long = "no-commit",
        help = "RalphTerm compatibility extension: skip local checkpoint commit"
    )]
    compat_no_commit: bool,
    #[arg(
        short = 'r',
        long = "review",
        conflicts_with_all = ["compat_tasks_only", "compat_external_only", "compat_codex_only"],
        help = "ralphex-compatible: skip task phase and run reviewer once"
    )]
    compat_review: bool,
    #[arg(
        short = 'e',
        long = "external-only",
        conflicts_with_all = ["compat_tasks_only", "compat_codex_only"],
        help = "ralphex-compatible: run external review loop only"
    )]
    compat_external_only: bool,
    #[arg(
        short = 'c',
        long = "codex-only",
        conflicts_with = "compat_tasks_only",
        help = "ralphex-compatible alias for --external-only"
    )]
    compat_codex_only: bool,
    #[arg(
        short = 'm',
        long = "max-iterations",
        default_value_t = 50,
        help = "ralphex-compatible: ceiling on per-task implementer attempts"
    )]
    compat_max_iterations: usize,
    #[arg(
        long = "max-external-iterations",
        help = "ralphex-compatible: ceiling on external review-loop iterations"
    )]
    compat_max_external_iterations: Option<usize>,
    #[arg(
        long = "review-patience",
        default_value_t = 2,
        help = "ralphex-compatible: abort retry loop after N consecutive identical review failures"
    )]
    compat_review_patience: usize,
    #[arg(
        long = "task-model",
        help = "ralphex-compatible: forwarded to the agent as $CLAUDE_MODEL"
    )]
    compat_task_model: Option<String>,
    #[arg(
        long = "review-model",
        help = "ralphex-compatible: forwarded to the reviewer as $CLAUDE_REVIEW_MODEL"
    )]
    compat_review_model: Option<String>,
    #[arg(
        long = "claude-args",
        help = "ralphex-compatible: extra args appended to --claude-command (shell-split)"
    )]
    compat_claude_args: Option<String>,
    #[arg(
        short = 'b',
        long = "base-ref",
        help = "ralphex-compatible: git ref used as the review diff base"
    )]
    compat_base_ref: Option<String>,
    #[arg(
        long = "session-timeout",
        help = "ralphex-compatible: per-agent session timeout (e.g. 30s, 5m, 1h)"
    )]
    compat_session_timeout: Option<String>,
    #[arg(
        long = "idle-timeout",
        help = "ralphex-compatible: idle timeout (currently parsed but unused)"
    )]
    compat_idle_timeout: Option<String>,
    #[arg(
        long = "wait",
        help = "ralphex-compatible: wait between iterations (currently parsed but unused)"
    )]
    compat_wait: Option<String>,
    #[arg(
        short = 'd',
        long = "debug",
        help = "ralphex-compatible: enable debug logging (sets RUST_LOG=debug if unset)"
    )]
    compat_debug: bool,
    #[arg(
        long = "no-color",
        help = "ralphex-compatible: disable color output (sets NO_COLOR=1)"
    )]
    compat_no_color: bool,
    #[arg(
        long = "config-dir",
        env = "RALPHEX_CONFIG_DIR",
        help = "ralphex-compatible: directory containing the global config file"
    )]
    compat_config_dir: Option<PathBuf>,
    #[arg(
        long = "worktree",
        help = "ralphex-compatible: run the plan inside an isolated git worktree (workspace id derived from plan filename)"
    )]
    compat_worktree: bool,
    #[arg(
        long = "branch",
        requires = "compat_worktree",
        help = "ralphex-compatible: override the worktree branch name (requires --worktree)"
    )]
    compat_branch: Option<String>,
    #[arg(value_name = "plan-file")]
    compat_plan_file: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve {
        #[arg(long, default_value = "127.0.0.1:7878")]
        bind: SocketAddr,
    },
    Run {
        plan: PathBuf,
        #[arg(long, value_enum, conflicts_with = "agent_command")]
        agent: Option<RunAgentKind>,
        #[arg(long)]
        agent_command: Option<String>,
        #[arg(long, value_enum, conflicts_with = "review_command")]
        review_agent: Option<RunAgentKind>,
        #[arg(long)]
        review_command: Option<String>,
        #[arg(long)]
        require_review: bool,
        #[arg(long, value_parser = parse_positive_duration_ms)]
        agent_timeout_ms: Option<std::time::Duration>,
        #[arg(
            long,
            default_value_t = 1,
            help = "Maximum number of implementation retries after REVIEW_FAIL decisions"
        )]
        max_review_retries: usize,
        #[arg(long)]
        no_commit: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        workspace_id: Option<String>,
    },
    Smoke {
        #[arg(long, value_enum, conflicts_with = "agent_command")]
        agent: Option<RunAgentKind>,
        #[arg(long)]
        agent_command: Option<String>,
    },
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
}

#[derive(Debug, Subcommand)]
enum WorkspaceCommand {
    Create { id: String },
    Cleanup { id: String },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RunAgentKind {
    Claude,
    Codex,
}

impl RunAgentKind {
    fn command(self) -> String {
        match self {
            RunAgentKind::Claude => "claude".to_string(),
            RunAgentKind::Codex => "codex".to_string(),
        }
    }
}

#[derive(Clone)]
struct AppState {
    store: Arc<SessionStore>,
    run_base_dir: Arc<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct CreateSessionRequest {
    agent: ApiAgentKind,
    prompt: String,
    cwd: Option<String>,
    command: Option<String>,
    args: Option<Vec<String>>,
    cols: Option<u16>,
    rows: Option<u16>,
}

#[derive(Debug, Clone, Copy, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
enum ApiAgentKind {
    Claude,
    Codex,
}

impl ApiAgentKind {
    fn command(self) -> String {
        match self {
            ApiAgentKind::Claude => "claude".to_string(),
            ApiAgentKind::Codex => "codex".to_string(),
        }
    }
}

impl From<ApiAgentKind> for AgentKind {
    fn from(value: ApiAgentKind) -> Self {
        match value {
            ApiAgentKind::Claude => AgentKind::Claude,
            ApiAgentKind::Codex => AgentKind::Codex,
        }
    }
}

#[derive(Debug, Deserialize)]
struct InputRequest {
    text: String,
    enter: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ApprovalRequest {
    approved: bool,
}

#[derive(Debug, Deserialize)]
struct ResizeRequest {
    cols: u16,
    rows: u16,
}

#[derive(Debug, Serialize)]
struct CreateSessionResponse {
    id: Uuid,
}

#[derive(Debug, Serialize)]
struct ApprovalResponse {
    id: Uuid,
    approved: bool,
}

#[derive(Debug, Serialize)]
struct ProgressArtifactIndexItem {
    name: String,
    kind: &'static str,
    url: String,
}

#[derive(Debug, Deserialize)]
struct CreateRunRequest {
    repo_path: Option<String>,
    plan_path: Option<String>,
    workspace_id: Option<String>,
    agent: Option<ApiAgentKind>,
    agent_command: Option<String>,
    review_agent: Option<ApiAgentKind>,
    review_command: Option<String>,
    require_review: Option<bool>,
    max_review_retries: Option<usize>,
    agent_timeout_ms: Option<u64>,
    no_commit: Option<bool>,
    dry_run: Option<bool>,
}

pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let matches = Cli::command().get_matches();
    let cli = Cli::from_arg_matches(&matches)
        .map_err(|err| anyhow::anyhow!("parse CLI arguments: {err}"))?;
    match cli.command {
        Some(Command::Serve { bind }) => serve(bind).await,
        Some(Command::Run {
            plan,
            agent,
            agent_command,
            review_agent,
            review_command,
            require_review,
            agent_timeout_ms,
            max_review_retries,
            no_commit,
            dry_run,
            workspace_id,
        }) => run_plan_cli(RunCliOptions {
            plan,
            agent,
            agent_command,
            review_agent,
            review_command,
            require_review,
            agent_timeout_ms,
            max_review_retries,
            no_commit,
            dry_run,
            workspace_id,
            review_patience: None,
            mode: crate::runner::RunMode::Full,
            max_external_iterations: None,
        }),
        Some(Command::Smoke {
            agent,
            agent_command,
        }) => {
            let agent_command =
                agent_command.unwrap_or_else(|| agent.unwrap_or(RunAgentKind::Claude).command());
            let output = run_smoke(&agent_command)?;
            print!("{output}");
            Ok(())
        }
        Some(Command::Workspace { command }) => run_workspace_command(command),
        None => run_compat_cli(cli, &matches),
    }
}

struct RunCliOptions {
    plan: PathBuf,
    agent: Option<RunAgentKind>,
    agent_command: Option<String>,
    review_agent: Option<RunAgentKind>,
    review_command: Option<String>,
    require_review: bool,
    agent_timeout_ms: Option<std::time::Duration>,
    max_review_retries: usize,
    no_commit: bool,
    dry_run: bool,
    workspace_id: Option<String>,
    review_patience: Option<usize>,
    mode: crate::runner::RunMode,
    max_external_iterations: Option<usize>,
}

fn run_plan_cli(options: RunCliOptions) -> anyhow::Result<()> {
    let RunCliOptions {
        plan,
        agent,
        agent_command,
        review_agent,
        review_command,
        require_review,
        agent_timeout_ms,
        max_review_retries,
        no_commit,
        dry_run,
        workspace_id,
        review_patience,
        mode,
        max_external_iterations,
    } = options;

    if let Some(id) = workspace_id {
        if plan.is_absolute() {
            bail!("--workspace-id requires a relative plan path");
        }

        let cwd = std::env::current_dir().context("read current directory")?;
        let manager = WorkspaceManager::discover(&cwd)?;
        let cwd_relative = cwd.strip_prefix(manager.repo_root()).with_context(|| {
            format!(
                "current directory {} is not inside repository {}",
                cwd.display(),
                manager.repo_root().display()
            )
        })?;
        validate_workspace_plan_path(cwd_relative, &plan)?;

        let candidate = manager.workspace(&id)?;
        if dry_run {
            println!("Workspace: {} (dry run)", candidate.path.display());
        } else {
            let workspace = if candidate.path.exists() {
                manager.validate_existing_workspace(&candidate)?;
                candidate
            } else {
                manager.create(id)?
            };
            println!("Workspace: {}", workspace.path.display());
            let workspace_cwd = workspace.path.join(cwd_relative);
            std::env::set_current_dir(&workspace_cwd).with_context(|| {
                format!("switch to workspace directory {}", workspace_cwd.display())
            })?;
        }
    }

    let output = run_plan(RunOptions {
        plan_path: plan,
        agent_command: agent_command.or_else(|| agent.map(RunAgentKind::command)),
        review_command: review_command.or_else(|| review_agent.map(RunAgentKind::command)),
        agent_timeout: agent_timeout_ms,
        require_review,
        max_review_retries,
        no_commit,
        dry_run,
        event_sink: None,
        cancellation_check: None,
        review_patience,
        mode,
        max_external_iterations,
    })?;
    print!("{output}");
    Ok(())
}

fn run_compat_cli(cli: Cli, matches: &ArgMatches) -> anyhow::Result<()> {
    let Some(plan) = cli.compat_plan_file.clone() else {
        bail!("plan file required for ralphex-compatible execution");
    };

    let project_root = std::env::current_dir().context("read current directory")?;
    let config = crate::config::load(cli.compat_config_dir.as_deref(), &project_root)?;

    let worktree_info = if cli.compat_worktree {
        Some(prepare_compat_worktree(
            &project_root,
            &plan,
            cli.compat_branch.as_deref(),
        )?)
    } else {
        None
    };

    // Helper: an arg counts as "CLI-provided" iff value_source is CommandLine.
    let cli_provided = |id: &str| -> bool {
        matches!(
            matches.value_source(id),
            Some(clap::parser::ValueSource::CommandLine)
        )
    };
    let cli_provided_or = |id: &str, cli_value: Option<String>, fallback: Option<String>| {
        if cli_provided(id) {
            cli_value
        } else {
            cli_value.or(fallback)
        }
    };

    let external_review_tool = cli_provided_or(
        "compat_external_review_tool",
        cli.compat_external_review_tool.clone(),
        config.external_review_tool.clone(),
    );
    if let Some(tool) = external_review_tool.as_deref() {
        if !matches!(tool, "custom" | "none") {
            bail!("unsupported external review tool: {tool}");
        }
    }
    let custom_review_script = cli_provided_or(
        "compat_custom_review_script",
        cli.compat_custom_review_script.clone(),
        config.custom_review_script.clone(),
    );
    let review_command = match external_review_tool.as_deref() {
        Some("custom") => custom_review_script.clone(),
        _ => None,
    };

    let mode = if cli.compat_tasks_only {
        crate::runner::RunMode::TasksOnly
    } else if cli.compat_review {
        crate::runner::RunMode::ReviewOnly
    } else if cli.compat_external_only || cli.compat_codex_only {
        crate::runner::RunMode::ExternalOnly
    } else {
        crate::runner::RunMode::Full
    };

    // Default full mode requires an independent reviewer unless --tasks-only is
    // set. --review and --external-only/--codex-only also require a reviewer.
    let mut require_review = false;
    match mode {
        crate::runner::RunMode::TasksOnly => {}
        crate::runner::RunMode::Full => {
            match (external_review_tool.as_deref(), review_command.as_deref()) {
                (Some("custom"), Some(_)) => {
                    require_review = true;
                }
                (Some("custom"), None) => {
                    bail!(
                        "ralphex-compatible full mode requires --external-review-tool=custom \
--custom-review-script <cmd>, or pass --tasks-only"
                    );
                }
                (Some("none"), _) => {
                    eprintln!(
                        "[warning] running without independent review because \
                         external-review-tool=none"
                    );
                }
                _ => {
                    bail!(
                        "ralphex-compatible full mode requires --external-review-tool=custom \
--custom-review-script <cmd>, or pass --tasks-only"
                    );
                }
            }
        }
        crate::runner::RunMode::ReviewOnly | crate::runner::RunMode::ExternalOnly => {
            if !matches!(external_review_tool.as_deref(), Some("custom"))
                || review_command.is_none()
            {
                bail!(
                    "--review and --external-only require --external-review-tool=custom \
--custom-review-script <cmd>"
                );
            }
        }
    }

    let claude_command = cli_provided_or(
        "compat_claude_command",
        cli.compat_claude_command.clone(),
        config.claude_command.clone(),
    );
    let claude_args = cli_provided_or(
        "compat_claude_args",
        cli.compat_claude_args.clone(),
        config.claude_args.clone(),
    );

    // Compose the implementer command with optional --claude-args appended.
    let agent_command = match (claude_command, claude_args.as_deref()) {
        (Some(command), Some(extra)) if !extra.trim().is_empty() => {
            if shlex::split(extra).is_none() {
                bail!("invalid --claude-args: shell-split failed");
            }
            Some(format!("{command} {extra}"))
        }
        (Some(command), _) => Some(command),
        (None, Some(extra)) if !extra.trim().is_empty() => {
            bail!("--claude-args requires --claude-command");
        }
        (None, _) => None,
    };

    let session_timeout_value = cli_provided_or(
        "compat_session_timeout",
        cli.compat_session_timeout.clone(),
        config.session_timeout.clone(),
    );
    let agent_timeout =
        if let Some(value) = session_timeout_value.as_deref() {
            Some(parse_duration(value).map_err(|err| {
                anyhow::anyhow!("invalid --session-timeout value '{value}': {err}")
            })?)
        } else {
            None
        };

    let idle_timeout_value = cli_provided_or(
        "compat_idle_timeout",
        cli.compat_idle_timeout.clone(),
        config.idle_timeout.clone(),
    );
    if let Some(value) = idle_timeout_value.as_deref() {
        parse_duration(value)
            .map_err(|err| anyhow::anyhow!("invalid --idle-timeout value '{value}': {err}"))?;
        eprintln!("[warning] --idle-timeout is accepted but not yet implemented; value ignored");
    }
    let wait_value = cli_provided_or("compat_wait", cli.compat_wait.clone(), config.wait.clone());
    if let Some(value) = wait_value.as_deref() {
        parse_duration(value)
            .map_err(|err| anyhow::anyhow!("invalid --wait value '{value}': {err}"))?;
        eprintln!("[warning] --wait is accepted but not yet implemented; value ignored");
    }

    if cli.compat_debug && std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "debug");
    }
    if cli.compat_no_color {
        std::env::set_var("NO_COLOR", "1");
    }

    let task_model_value = cli_provided_or(
        "compat_task_model",
        cli.compat_task_model.clone(),
        config.task_model.clone(),
    );
    let review_model_value = cli_provided_or(
        "compat_review_model",
        cli.compat_review_model.clone(),
        config.review_model.clone(),
    );
    if let Some(model) = task_model_value.as_deref() {
        eprintln!("[warning] --task-model is forwarded as $CLAUDE_MODEL");
        std::env::set_var("CLAUDE_MODEL", model);
    }
    if let Some(model) = review_model_value.as_deref() {
        eprintln!("[warning] --review-model is forwarded as $CLAUDE_REVIEW_MODEL");
        std::env::set_var("CLAUDE_REVIEW_MODEL", model);
    }

    let base_ref_value = cli_provided_or(
        "compat_base_ref",
        cli.compat_base_ref.clone(),
        config.base_ref.clone(),
    );
    if base_ref_value.is_some() {
        eprintln!("[warning] --base-ref is accepted but full diff-range support is pending");
    }
    let max_external_iterations_value = if cli_provided("compat_max_external_iterations") {
        cli.compat_max_external_iterations
    } else {
        cli.compat_max_external_iterations
            .or(config.max_external_iterations)
    };
    // max-iterations is stored on the future RunOptions; reading here keeps the
    // option live until later tasks wire it into the retry budget.
    let _ = if cli_provided("compat_max_iterations") {
        cli.compat_max_iterations
    } else {
        config.max_iterations.unwrap_or(cli.compat_max_iterations)
    };
    let review_patience_value = if cli_provided("compat_review_patience") {
        cli.compat_review_patience
    } else {
        config.review_patience.unwrap_or(cli.compat_review_patience)
    };

    let final_plan_path = worktree_info
        .as_ref()
        .map(|info| info.plan_in_worktree.clone())
        .unwrap_or(plan);

    let worktree_path_for_print = worktree_info
        .as_ref()
        .map(|info| info.worktree_path.clone());

    run_plan_cli(RunCliOptions {
        plan: final_plan_path,
        agent: None,
        agent_command,
        review_agent: None,
        review_command,
        require_review,
        agent_timeout_ms: agent_timeout,
        max_review_retries: 1,
        no_commit: cli.compat_no_commit,
        dry_run: false,
        workspace_id: None,
        review_patience: Some(review_patience_value),
        mode,
        max_external_iterations: max_external_iterations_value,
    })?;

    if let Some(path) = worktree_path_for_print {
        println!("Worktree: {}", path.display());
    }
    Ok(())
}

struct CompatWorktreeInfo {
    worktree_path: PathBuf,
    plan_in_worktree: PathBuf,
}

fn prepare_compat_worktree(
    project_root: &FsPath,
    plan: &FsPath,
    branch: Option<&str>,
) -> anyhow::Result<CompatWorktreeInfo> {
    let manager = WorkspaceManager::discover(project_root)?;
    let id = crate::workspace::workspace_id_from_plan_path(plan)?;
    let candidate = manager.workspace_with_branch(&id, branch)?;
    let workspace = if candidate.path.exists() {
        manager.validate_existing_workspace(&candidate)?;
        candidate
    } else {
        manager.create_with_branch(&id, branch)?
    };

    let plan_resolved = if plan.is_absolute() {
        plan.to_path_buf()
    } else {
        let cwd_relative = project_root
            .strip_prefix(manager.repo_root())
            .with_context(|| {
                format!(
                    "current directory {} is not inside repository {}",
                    project_root.display(),
                    manager.repo_root().display()
                )
            })?;
        validate_workspace_plan_path(cwd_relative, plan)?;
        workspace.path.join(cwd_relative).join(plan)
    };

    let workspace_cwd = if project_root == manager.repo_root() {
        workspace.path.clone()
    } else {
        let cwd_relative = project_root
            .strip_prefix(manager.repo_root())
            .unwrap_or(FsPath::new(""));
        workspace.path.join(cwd_relative)
    };
    std::env::set_current_dir(&workspace_cwd)
        .with_context(|| format!("switch to workspace directory {}", workspace_cwd.display()))?;

    Ok(CompatWorktreeInfo {
        worktree_path: workspace.path,
        plan_in_worktree: plan_resolved,
    })
}

fn parse_duration(value: &str) -> Result<std::time::Duration, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("duration is empty".to_string());
    }
    // Accept either a plain integer (treated as seconds) or a number + unit suffix.
    let (number_part, unit) = match trimmed.find(|ch: char| !ch.is_ascii_digit() && ch != '.') {
        Some(index) => (&trimmed[..index], trimmed[index..].trim()),
        None => (trimmed, ""),
    };
    if number_part.is_empty() {
        return Err(format!("could not parse number in '{value}'"));
    }
    let number: f64 = number_part
        .parse()
        .map_err(|_| format!("invalid number '{number_part}' in '{value}'"))?;
    if !number.is_finite() || number < 0.0 {
        return Err(format!("duration must be non-negative: '{value}'"));
    }
    let multiplier_ms: f64 = match unit {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => 1_000.0,
        "ms" => 1.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 60_000.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3_600_000.0,
        other => {
            return Err(format!("unknown duration unit '{other}' in '{value}'"));
        }
    };
    let total_ms = number * multiplier_ms;
    if total_ms <= 0.0 {
        return Err(format!("duration must be greater than zero: '{value}'"));
    }
    Ok(std::time::Duration::from_millis(total_ms.round() as u64))
}

fn validate_workspace_plan_path(cwd_relative: &FsPath, plan: &FsPath) -> anyhow::Result<()> {
    let mut normalized = PathBuf::new();
    for component in cwd_relative.components().chain(plan.components()) {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => normalized.push(part),
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    bail!("plan path must stay inside the repository");
                }
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                bail!("plan path must stay inside the repository");
            }
        }
    }

    Ok(())
}

fn parse_positive_duration_ms(value: &str) -> Result<std::time::Duration, String> {
    let millis = value
        .parse::<u64>()
        .map_err(|_| "timeout must be a positive integer number of milliseconds".to_string())?;
    if millis == 0 {
        return Err("timeout must be greater than 0 milliseconds".to_string());
    }
    Ok(std::time::Duration::from_millis(millis))
}

fn run_workspace_command(command: WorkspaceCommand) -> anyhow::Result<()> {
    let cwd = std::env::current_dir().context("read current directory")?;
    let manager = WorkspaceManager::discover(cwd)?;

    match command {
        WorkspaceCommand::Create { id } => {
            let workspace = manager.create(id)?;
            println!("Workspace: {}", workspace.id);
            println!("Path: {}", workspace.path.display());
            println!("Branch: {}", workspace.branch);
            println!("Base: {}", workspace.base_commit);
        }
        WorkspaceCommand::Cleanup { id } => {
            let workspace = manager.workspace(&id)?;
            manager.cleanup(&workspace)?;
            println!("Cleaned workspace: {id}");
        }
    }

    Ok(())
}

async fn serve(bind: SocketAddr) -> anyhow::Result<()> {
    let state = AppState {
        store: Arc::new(SessionStore::default()),
        run_base_dir: Arc::new(std::env::current_dir().context("read current directory")?),
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/dashboard", get(dashboard_index))
        .route("/dashboard/app.js", get(dashboard_app_js))
        .route("/dashboard/styles.css", get(dashboard_styles_css))
        .route("/v1/runs", post(create_run).get(list_runs))
        .route("/v1/runs/:id", get(get_run))
        .route("/v1/runs/:id/summary", get(get_run_summary))
        .route("/v1/runs/:id/summary.json", get(get_run_summary_json))
        .route("/v1/runs/:id/diff", get(get_run_diff))
        .route("/v1/runs/:id/progress", get(list_run_progress))
        .route("/v1/runs/:id/progress/:artifact", get(get_run_progress))
        .route("/v1/runs/:id/events", get(get_run_events))
        .route("/v1/runs/:id/cancel", post(cancel_run))
        .route("/v1/sessions", post(create_session).get(list_sessions))
        .route("/v1/sessions/:id", get(get_session))
        .route("/v1/sessions/:id/input", post(send_input))
        .route("/v1/sessions/:id/approval", post(approval_decision))
        .route("/v1/sessions/:id/resize", post(resize_session))
        .route("/v1/sessions/:id/cancel", post(cancel_session))
        .route("/v1/sessions/:id/transcript", get(get_transcript))
        .route("/v1/sessions/:id/events", get(ws_events))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("bind {bind}"))?;
    tracing::info!(%bind, "serving ralphterm");
    axum::serve(listener, app).await.context("serve")
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"ok": true}))
}

fn api_plan_execution_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

async fn dashboard_index() -> Html<&'static str> {
    Html(include_str!("../dashboard/index.html"))
}

async fn dashboard_app_js() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../dashboard/app.js"),
    )
}

async fn dashboard_styles_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../dashboard/styles.css"),
    )
}

async fn create_run(
    State(state): State<AppState>,
    Json(req): Json<CreateRunRequest>,
) -> Result<Json<CreatedRunRecord>, ApiError> {
    let plan_path = req.plan_path.clone();
    let base_dir = state.run_base_dir.as_ref().clone();
    let mut repo_path = req.repo_path.clone();
    let requested_repo_dir = repo_path.as_deref().map(PathBuf::from);
    if req.repo_path.is_some() && req.workspace_id.is_some() {
        return Err(ApiError::bad_request(
            "repo_path cannot be combined with workspace_id",
        ));
    }
    let mut canonical_requested_repo_dir = None;
    if let Some(repo_dir) = requested_repo_dir.as_deref() {
        if !repo_dir.is_absolute() {
            return Err(ApiError::bad_request("repo_path must be absolute"));
        }
        if !repo_dir.is_dir() {
            return Err(ApiError::bad_request(format!(
                "repo_path is not a directory: {}",
                repo_dir.display()
            )));
        }
        let canonical_repo_dir = repo_dir.canonicalize().map_err(|err| {
            ApiError::bad_request(format!(
                "unable to resolve repo_path {}: {err}",
                repo_dir.display()
            ))
        })?;
        repo_path = Some(canonical_repo_dir.to_string_lossy().to_string());
        canonical_requested_repo_dir = Some(canonical_repo_dir);
    }
    if req.agent.is_some() && req.agent_command.is_some() {
        return Err(ApiError::bad_request("agent conflicts with agent_command"));
    }
    if req.review_agent.is_some() && req.review_command.is_some() {
        return Err(ApiError::bad_request(
            "review_agent conflicts with review_command",
        ));
    }
    let agent_command = req
        .agent_command
        .clone()
        .or_else(|| req.agent.map(ApiAgentKind::command));
    let review_command = req
        .review_command
        .clone()
        .or_else(|| req.review_agent.map(ApiAgentKind::command));
    let dry_run = req.dry_run.unwrap_or(false);
    if (agent_command.is_some() || dry_run) && plan_path.is_none() {
        return Err(ApiError::bad_request(
            "plan_path is required when agent, agent_command, or dry_run is set",
        ));
    }
    if requested_repo_dir.is_some() {
        if !dry_run {
            return Err(ApiError::bad_request(
                "repo_path currently supports dry_run only",
            ));
        }
        let plan_path = plan_path
            .as_deref()
            .ok_or_else(|| ApiError::bad_request("plan_path is required when repo_path is set"))?;
        let plan = PathBuf::from(plan_path);
        if plan.is_absolute() {
            return Err(ApiError::bad_request(
                "repo_path requires a relative plan_path",
            ));
        }
        validate_workspace_plan_path(FsPath::new(""), &plan)
            .map_err(|_| ApiError::bad_request("plan_path must stay inside repo_path"))?;
        let canonical_repo_dir = canonical_requested_repo_dir.as_deref().ok_or_else(|| {
            ApiError::bad_request("unable to resolve repo_path for plan_path validation")
        })?;
        let canonical_plan_path = canonical_repo_dir
            .join(&plan)
            .canonicalize()
            .map_err(|_| ApiError::bad_request("unable to resolve plan_path inside repo_path"))?;
        if !canonical_plan_path.starts_with(canonical_repo_dir) {
            return Err(ApiError::bad_request(
                "plan_path must stay inside repo_path",
            ));
        }
    }
    if req.require_review.unwrap_or(false) && review_command.is_none() {
        return Err(ApiError::bad_request(
            "review_command or review_agent is required when require_review is true",
        ));
    }
    let agent_timeout = match req.agent_timeout_ms {
        Some(0) => {
            return Err(ApiError::bad_request(
                "agent_timeout_ms must be greater than 0 milliseconds",
            ))
        }
        Some(millis) => Some(std::time::Duration::from_millis(millis)),
        None => None,
    };
    let effective_agent_command_for_validation = agent_command
        .clone()
        .or_else(|| dry_run.then(|| DEFAULT_PLAN_AGENT_COMMAND.to_string()));
    if let (Some(agent_command), Some(review_command)) = (
        effective_agent_command_for_validation.as_deref(),
        review_command.as_deref(),
    ) {
        let commands_equivalent = agent_commands_equivalent(agent_command, review_command)
            .map_err(|err| ApiError::bad_request(err.to_string()))?;
        if commands_equivalent {
            return Err(ApiError::bad_request(
                "implementation agent/command and review agent/command must be different",
            ));
        }
    }
    let mut workspace_path = None;
    let mut workspace_execution_dir = None;
    if let Some(workspace_id) = req.workspace_id.as_deref() {
        let plan_path = plan_path.as_deref().ok_or_else(|| {
            ApiError::bad_request("plan_path is required when workspace_id is set")
        })?;
        let plan = PathBuf::from(plan_path);
        if plan.is_absolute() {
            return Err(ApiError::bad_request(
                "workspace_id requires a relative plan_path",
            ));
        }

        let manager = WorkspaceManager::discover(&base_dir)?;
        let cwd_relative = base_dir.strip_prefix(manager.repo_root()).map_err(|_| {
            ApiError::bad_request(format!(
                "current directory {} is not inside repository {}",
                base_dir.display(),
                manager.repo_root().display()
            ))
        })?;
        validate_workspace_plan_path(cwd_relative, &plan)
            .map_err(|err| ApiError::bad_request(err.to_string()))?;

        if agent_command.is_some() || dry_run {
            let candidate = manager
                .workspace(workspace_id)
                .map_err(|err| ApiError::bad_request(err.to_string()))?;
            workspace_path = Some(candidate.path.to_string_lossy().to_string());

            if !dry_run {
                let workspace = if candidate.path.exists() {
                    manager
                        .validate_existing_workspace(&candidate)
                        .map_err(|err| ApiError::bad_request(err.to_string()))?;
                    candidate
                } else {
                    manager.create(workspace_id)?
                };
                workspace_execution_dir = Some(workspace.path.join(cwd_relative));
            }
        }
    }
    let record = RunStore::create(
        state.run_base_dir.as_ref(),
        RunRecord {
            phase: RunPhase::Planning,
            status: RunStatus::Created,
            plan_path: plan_path.clone(),
            repo_path: repo_path.clone(),
            workspace_path: workspace_path.clone(),
        },
    )?;

    if agent_command.is_none() && !dry_run {
        return Ok(Json(record));
    }

    let plan_path = plan_path.map(PathBuf::from).ok_or_else(|| {
        ApiError::bad_request("plan_path is required when agent, agent_command, or dry_run is set")
    })?;
    let run_id = record.id;
    let require_review = req.require_review.unwrap_or(false);
    let max_review_retries = req.max_review_retries.unwrap_or(1);
    let no_commit = req.no_commit.unwrap_or(false);
    let slug = plan_slug_for_artifacts(&plan_path);

    let started = RunStore::start(&base_dir, run_id)?.context("run disappeared before start")?;

    let executor_base_dir = base_dir.clone();
    let execution_dir = workspace_execution_dir
        .or(canonical_requested_repo_dir)
        .unwrap_or_else(|| executor_base_dir.clone());
    tokio::spawn(async move {
        let supervisor_base_dir = base_dir;
        let result = tokio::task::spawn_blocking(move || {
            let _execution_guard = api_plan_execution_lock().lock().unwrap_or_else(|poisoned| {
                tracing::error!("API plan execution lock was poisoned; continuing with inner lock");
                poisoned.into_inner()
            });
            let base_dir = executor_base_dir;
            let progress_dir = execution_dir.join(".ralphterm").join("progress");
            let summary_path = progress_dir.join(format!("{slug}-summary.md"));
            let summary_json_path = progress_dir.join(format!("{slug}-summary.json"));
            let diff_path = progress_dir.join(format!("{slug}-diff.patch"));
            let _cwd_guard = CurrentDirGuard::change_to(&execution_dir).map_err(|err| {
                anyhow::Error::new(err)
                    .context(format!("switch to execution directory {}", execution_dir.display()))
            });
            let Ok(_cwd_guard) = _cwd_guard else {
                let _ = RunStore::write_failure(&base_dir, run_id, None, None, None);
                tracing::error!(%run_id, "background plan run failed to switch execution directory");
                return;
            };
            let event_base_dir = base_dir.clone();
            let event_progress_dir = progress_dir.clone();
            let event_slug = slug.clone();
            let event_sink = Arc::new(move |event: PlanRunEvent| {
                match RunStore::get(&event_base_dir, run_id)? {
                    Some(record) if record.status == RunStatus::Running => {}
                    Some(_) => anyhow::bail!("run was cancelled"),
                    None => anyhow::bail!("run disappeared"),
                }
                let artifact_path = event.artifact_path.clone();
                if let Err(copy_err) = copy_progress_artifacts(
                    &event_base_dir,
                    run_id,
                    &event_progress_dir,
                    &event_slug,
                    None,
                ) {
                    tracing::error!(%run_id, error = %copy_err, "failed to copy live run progress artifacts");
                }
                if let Some(artifact_path) = artifact_path.as_deref() {
                    if let Err(copy_err) = copy_progress_artifact(
                        &event_base_dir,
                        run_id,
                        &event_progress_dir,
                        artifact_path,
                    ) {
                        tracing::error!(%run_id, artifact_path, error = %copy_err, "failed to copy event progress artifact");
                    }
                }
                let appended = RunStore::append_progress_event(
                    &event_base_dir,
                    run_id,
                    RunProgressEvent {
                        event_type: event.event_type.to_string(),
                        task_number: event.task_number,
                        task_title: event.task_title,
                        attempt: event.attempt,
                        artifact_path: event.artifact_path,
                        message: event.message,
                    },
                )?;
                if appended.is_none() {
                    anyhow::bail!("run was cancelled");
                }
                Ok(())
            });
            let cancellation_base_dir = base_dir.clone();
            let cancellation_check = Arc::new(move || {
                match RunStore::get(&cancellation_base_dir, run_id)? {
                    Some(record) if record.status == RunStatus::Running => Ok(()),
                    Some(_) => anyhow::bail!("run was cancelled"),
                    None => anyhow::bail!("run disappeared"),
                }
            });
            let run_output = match run_plan(RunOptions {
                plan_path: plan_path.clone(),
                agent_command,
                review_command: review_command.clone(),
                agent_timeout,
                require_review,
                max_review_retries,
                no_commit,
                dry_run,
                event_sink: Some(event_sink),
                cancellation_check: Some(cancellation_check),
                review_patience: None,
                mode: crate::runner::RunMode::Full,
                max_external_iterations: None,
            }) {
                Ok(output) => output,
                Err(err) => {
                    let summary_markdown = fs::read_to_string(&summary_path).ok();
                    let summary_json = fs::read_to_string(&summary_json_path).ok();
                    let diff_patch = fs::read_to_string(&diff_path).ok();
                    if let Err(copy_err) = copy_progress_artifacts(
                        &base_dir,
                        run_id,
                        &progress_dir,
                        &slug,
                        summary_json.as_deref(),
                    ) {
                        tracing::error!(%run_id, error = %copy_err, "failed to copy run progress artifacts");
                    }
                    match RunStore::write_failure(
                        &base_dir,
                        run_id,
                        summary_markdown,
                        summary_json,
                        diff_patch,
                    ) {
                        Ok(Some(_)) => {}
                        Ok(None) => {
                            tracing::error!(%run_id, "run disappeared before failure could be written")
                        }
                        Err(write_err) => {
                            tracing::error!(%run_id, error = %write_err, "failed to write failed run record")
                        }
                    }
                    tracing::error!(%run_id, error = %err, "background plan run failed");
                    return;
                }
            };

            if dry_run {
                let summary_json = match dry_run_summary_json(
                    &plan_path,
                    review_command.as_deref(),
                    max_review_retries,
                ) {
                    Ok(summary_json) => summary_json,
                    Err(err) => {
                        let error = err.context("build dry-run summary json");
                        let _ = RunStore::write_failure(&base_dir, run_id, Some(run_output), None, None);
                        tracing::error!(%run_id, error = %error, "background plan run failed");
                        return;
                    }
                };
                match RunStore::write_result(
                    &base_dir,
                    run_id,
                    RunResultArtifacts {
                        summary_markdown: run_output,
                        summary_json: Some(summary_json),
                        diff_patch: String::new(),
                    },
                ) {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        tracing::error!(%run_id, "run disappeared before result could be written")
                    }
                    Err(err) => {
                        tracing::error!(%run_id, error = %err, "failed to write successful run record")
                    }
                }
                return;
            }

            let summary_markdown = match fs::read_to_string(&summary_path) {
                Ok(summary_markdown) => summary_markdown,
                Err(err) => {
                    let error = anyhow::Error::new(err).context("read run summary artifact");
                    if let Err(copy_err) =
                        copy_progress_artifacts(&base_dir, run_id, &progress_dir, &slug, None)
                    {
                        tracing::error!(%run_id, error = %copy_err, "failed to copy run progress artifacts");
                    }
                    let _ = RunStore::write_failure(&base_dir, run_id, None, None, None);
                    tracing::error!(%run_id, error = %error, "background plan run failed");
                    return;
                }
            };
            let diff_patch = match fs::read_to_string(&diff_path) {
                Ok(diff_patch) => diff_patch,
                Err(err) => {
                    let error = anyhow::Error::new(err).context("read run diff artifact");
                    if let Err(copy_err) =
                        copy_progress_artifacts(&base_dir, run_id, &progress_dir, &slug, None)
                    {
                        tracing::error!(%run_id, error = %copy_err, "failed to copy run progress artifacts");
                    }
                    let _ = RunStore::write_failure(
                        &base_dir,
                        run_id,
                        Some(summary_markdown),
                        None,
                        None,
                    );
                    tracing::error!(%run_id, error = %error, "background plan run failed");
                    return;
                }
            };
            let summary_json = match fs::read_to_string(&summary_json_path) {
                Ok(summary_json) => summary_json,
                Err(err) => {
                    let error = anyhow::Error::new(err).context("read run summary json artifact");
                    if let Err(copy_err) =
                        copy_progress_artifacts(&base_dir, run_id, &progress_dir, &slug, None)
                    {
                        tracing::error!(%run_id, error = %copy_err, "failed to copy run progress artifacts");
                    }
                    let _ = RunStore::write_failure(
                        &base_dir,
                        run_id,
                        Some(summary_markdown),
                        None,
                        Some(diff_patch),
                    );
                    tracing::error!(%run_id, error = %error, "background plan run failed");
                    return;
                }
            };
            if let Err(err) =
                copy_progress_artifacts(&base_dir, run_id, &progress_dir, &slug, Some(&summary_json))
            {
                let error = err.context("copy run progress artifacts");
                let _ = RunStore::write_failure(
                    &base_dir,
                    run_id,
                    Some(summary_markdown),
                    Some(summary_json),
                    Some(diff_patch),
                );
                tracing::error!(%run_id, error = %error, "background plan run failed");
                return;
            }
            match RunStore::write_result(
                &base_dir,
                run_id,
                RunResultArtifacts {
                    summary_markdown,
                    summary_json: Some(summary_json),
                    diff_patch,
                },
            ) {
                Ok(Some(_)) => {}
                Ok(None) => tracing::error!(%run_id, "run disappeared before result could be written"),
                Err(err) => {
                    tracing::error!(%run_id, error = %err, "failed to write successful run record")
                }
            }
        })
        .await;
        if let Err(err) = result {
            let _ = RunStore::write_failure(&supervisor_base_dir, run_id, None, None, None);
            tracing::error!(%run_id, error = %err, "background plan worker failed to join");
        }
    });

    Ok(Json(started))
}

fn dry_run_summary_json(
    plan_path: &FsPath,
    review_command: Option<&str>,
    max_review_retries: usize,
) -> anyhow::Result<String> {
    let input = fs::read_to_string(plan_path)
        .with_context(|| format!("read plan {}", plan_path.display()))?;
    let plan = parse_plan(&input).context("parse plan")?;
    let plan_name = plan_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("plan");
    let tasks: Vec<_> = plan
        .pending_tasks()
        .into_iter()
        .map(|task| {
            serde_json::json!({
                "number": task.number,
                "title": task.title,
            })
        })
        .collect();

    let summary_json = serde_json::json!({
        "result": "passed",
        "dry_run": true,
        "plan": plan_name,
        "review": review_command.unwrap_or("skipped"),
        "review_retries": max_review_retries,
        "validation": plan.validation_commands,
        "tasks": tasks,
    });
    Ok(
        serde_json::to_string_pretty(&summary_json).context("serialize dry-run summary json")?
            + "\n",
    )
}

fn copy_progress_artifacts(
    base_dir: &FsPath,
    run_id: Uuid,
    progress_dir: &FsPath,
    plan_slug: &str,
    summary_json: Option<&str>,
) -> anyhow::Result<()> {
    if !progress_dir.exists() {
        return Ok(());
    }

    let artifact_names = progress_artifact_names(progress_dir, plan_slug, summary_json)?;
    if artifact_names.is_empty() {
        return Ok(());
    }

    let artifact_dir = base_dir
        .join(".ralphterm")
        .join("runs")
        .join(run_id.to_string())
        .join("progress");
    fs::create_dir_all(&artifact_dir).with_context(|| {
        format!(
            "create run progress artifact directory {}",
            artifact_dir.display()
        )
    })?;

    for entry in fs::read_dir(progress_dir)
        .with_context(|| format!("read progress directory {}", progress_dir.display()))?
    {
        let entry = entry.with_context(|| format!("read entry in {}", progress_dir.display()))?;
        let artifact_name = entry.file_name().to_string_lossy().into_owned();
        if !artifact_names.contains(&artifact_name) {
            continue;
        }
        let source = entry.path();
        let metadata = fs::symlink_metadata(&source)
            .with_context(|| format!("read metadata for {}", source.display()))?;
        if !metadata.is_file() {
            continue;
        }
        let destination = artifact_dir.join(&artifact_name);
        atomic_copy_file(&source, &destination).with_context(|| {
            format!(
                "copy progress artifact {} to {}",
                source.display(),
                destination.display()
            )
        })?;
    }

    Ok(())
}

fn copy_progress_artifact(
    base_dir: &FsPath,
    run_id: Uuid,
    progress_dir: &FsPath,
    artifact_path: &str,
) -> anyhow::Result<()> {
    let Some(artifact_name) = FsPath::new(artifact_path)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        anyhow::bail!("event progress artifact path has no file name: {artifact_path}");
    };
    let source = progress_dir.join(artifact_name);
    let metadata = fs::symlink_metadata(&source)
        .with_context(|| format!("read metadata for event artifact {}", source.display()))?;
    if !metadata.is_file() {
        anyhow::bail!(
            "event progress artifact is not a file: {}",
            source.display()
        );
    }
    let artifact_dir = base_dir
        .join(".ralphterm")
        .join("runs")
        .join(run_id.to_string())
        .join("progress");
    fs::create_dir_all(&artifact_dir).with_context(|| {
        format!(
            "create run progress artifact directory {}",
            artifact_dir.display()
        )
    })?;
    let destination = artifact_dir.join(artifact_name);
    atomic_copy_file(&source, &destination).with_context(|| {
        format!(
            "copy event progress artifact {} to {}",
            source.display(),
            destination.display()
        )
    })
}

fn atomic_copy_file(source: &FsPath, destination: &FsPath) -> anyhow::Result<()> {
    let parent = destination.parent().with_context(|| {
        format!(
            "determine parent directory for destination {}",
            destination.display()
        )
    })?;
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| {
            format!(
                "determine file name for destination {}",
                destination.display()
            )
        })?;
    let temp_path = parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()));

    let copy_result = fs::copy(source, &temp_path).with_context(|| {
        format!(
            "copy {} to temporary file {}",
            source.display(),
            temp_path.display()
        )
    });
    if let Err(err) = copy_result {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }

    let rename_result = fs::rename(&temp_path, destination).with_context(|| {
        format!(
            "rename temporary progress artifact {} to {}",
            temp_path.display(),
            destination.display()
        )
    });
    if let Err(err) = rename_result {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }

    Ok(())
}

fn progress_artifact_names(
    progress_dir: &FsPath,
    plan_slug: &str,
    summary_json: Option<&str>,
) -> anyhow::Result<BTreeSet<String>> {
    let mut names = BTreeSet::from([format!("{plan_slug}.log")]);
    let Some(summary_json) = summary_json else {
        return Ok(names);
    };

    let summary: serde_json::Value = match serde_json::from_str(summary_json) {
        Ok(summary) => summary,
        Err(_) => return Ok(names),
    };
    let mut task_numbers = BTreeSet::new();
    if let Some(tasks) = summary.get("tasks").and_then(|tasks| tasks.as_array()) {
        for task in tasks {
            collect_progress_artifact_names(task, &mut names);
            collect_summary_task_number(task, &mut task_numbers);
        }
    }
    if let Some(failed_task) = summary.get("failed_task") {
        collect_progress_artifact_names(failed_task, &mut names);
        collect_summary_task_number(failed_task, &mut task_numbers);
    }
    collect_progress_log_artifact_names(progress_dir, plan_slug, &task_numbers, &mut names)?;

    Ok(names)
}

fn collect_summary_task_number(task: &serde_json::Value, task_numbers: &mut BTreeSet<usize>) {
    if let Some(number) = task.get("number").and_then(|value| value.as_u64()) {
        if let Ok(number) = usize::try_from(number) {
            task_numbers.insert(number);
        }
    }
}

fn collect_progress_artifact_names(task: &serde_json::Value, names: &mut BTreeSet<String>) {
    for field in ["transcript", "validation", "review_transcript"] {
        let Some(path) = task.get(field).and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(name) = FsPath::new(path).file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        names.insert(name.to_string());
        if field == "transcript" {
            if let Some(attempt_name) = first_attempt_transcript_name(name) {
                names.insert(attempt_name);
            }
        }
    }
}

fn first_attempt_transcript_name(name: &str) -> Option<String> {
    let stem = name.strip_suffix(".transcript")?;
    if stem.contains("-attempt-") {
        return None;
    }
    Some(format!("{stem}-attempt-1.transcript"))
}

fn collect_progress_log_artifact_names(
    progress_dir: &FsPath,
    plan_slug: &str,
    task_numbers: &BTreeSet<usize>,
    names: &mut BTreeSet<String>,
) -> anyhow::Result<()> {
    if task_numbers.is_empty() {
        return Ok(());
    }
    let log_path = progress_dir.join(format!("{plan_slug}.log"));
    let Ok(log) = fs::read_to_string(&log_path) else {
        return Ok(());
    };
    let mut latest_task_artifacts: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    let mut current_task: Option<usize> = None;
    let mut current_artifacts = BTreeSet::new();
    for line in log.lines() {
        let Some((_, event)) = line
            .strip_prefix("timestamp=")
            .and_then(|rest| rest.split_once(' '))
        else {
            continue;
        };
        if let Some(task_number) = task_event_number(event, "task_start number=") {
            if let Some(previous_task) = current_task.take() {
                latest_task_artifacts.insert(previous_task, std::mem::take(&mut current_artifacts));
            }
            if task_numbers.contains(&task_number) {
                current_task = Some(task_number);
            }
            continue;
        }
        if let Some(task_number) = current_task {
            collect_progress_log_event_artifact_name(event, &mut current_artifacts);
            if let Some(retry_attempt) = agent_retry_attempt(event) {
                if retry_attempt > 1 {
                    current_artifacts.insert(format!(
                        "{plan_slug}-task-{task_number}-attempt-{}.transcript",
                        retry_attempt - 1
                    ));
                    current_artifacts.insert(format!(
                        "{plan_slug}-task-{task_number}-attempt-{}-review.transcript",
                        retry_attempt - 1
                    ));
                }
            }
            if task_event_number(event, "task_end number=") == Some(task_number) {
                latest_task_artifacts.insert(task_number, std::mem::take(&mut current_artifacts));
                current_task = None;
            }
        }
    }
    if let Some(task_number) = current_task {
        latest_task_artifacts.insert(task_number, current_artifacts);
    }
    for task_artifacts in latest_task_artifacts.values() {
        names.extend(task_artifacts.iter().cloned());
    }
    Ok(())
}

fn collect_progress_log_event_artifact_name(event: &str, names: &mut BTreeSet<String>) {
    let artifact_path = if let Some((_, path)) = event.split_once("transcript path=") {
        Some(path)
    } else {
        event.strip_prefix("review transcript path=")
    };
    let Some(path) = artifact_path else {
        return;
    };
    if let Some(name) = FsPath::new(path.trim())
        .file_name()
        .and_then(|name| name.to_str())
    {
        names.insert(name.to_string());
    }
}

fn task_event_number(event: &str, prefix: &str) -> Option<usize> {
    let rest = event.strip_prefix(prefix)?;
    let number = rest.split_whitespace().next()?;
    number.parse().ok()
}

fn agent_retry_attempt(event: &str) -> Option<usize> {
    event
        .strip_prefix("agent_retry attempt=")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

struct CurrentDirGuard {
    previous: PathBuf,
}

impl CurrentDirGuard {
    fn change_to(path: &FsPath) -> std::io::Result<Self> {
        let previous = std::env::current_dir()?;
        std::env::set_current_dir(path)?;
        Ok(Self { previous })
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        if let Err(err) = std::env::set_current_dir(&self.previous) {
            tracing::error!(
                previous = %self.previous.display(),
                error = %err,
                "failed to restore current directory"
            );
        }
    }
}

fn plan_slug_for_artifacts(plan_path: &FsPath) -> String {
    let raw = plan_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("plan");
    let slug: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "plan".to_string()
    } else {
        slug
    }
}

async fn list_runs(State(state): State<AppState>) -> Result<Json<Vec<CreatedRunRecord>>, ApiError> {
    Ok(Json(RunStore::list(state.run_base_dir.as_ref())?))
}

async fn get_run(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CreatedRunRecord>, ApiError> {
    Ok(Json(
        RunStore::get(state.run_base_dir.as_ref(), id)?.ok_or(ApiError::run_not_found())?,
    ))
}

async fn get_run_summary(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<String, ApiError> {
    let path =
        RunStore::summary_path(state.run_base_dir.as_ref(), id)?.ok_or(ApiError::run_not_found())?;
    read_run_artifact(path, "summary").await
}

async fn get_run_summary_json(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let path = RunStore::summary_json_path(state.run_base_dir.as_ref(), id)?
        .ok_or(ApiError::run_not_found())?;
    let content = read_run_artifact(path, "summary json").await?;
    Ok(([(header::CONTENT_TYPE, "application/json")], content))
}

async fn get_run_diff(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<String, ApiError> {
    let path =
        RunStore::diff_path(state.run_base_dir.as_ref(), id)?.ok_or(ApiError::run_not_found())?;
    read_run_artifact(path, "diff").await
}

async fn list_run_progress(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<ProgressArtifactIndexItem>>, ApiError> {
    let record = RunStore::get(state.run_base_dir.as_ref(), id)?.ok_or(ApiError::run_not_found())?;
    let plan_slug = record
        .plan_path
        .as_deref()
        .map(FsPath::new)
        .map(plan_slug_for_artifacts)
        .unwrap_or_else(|| "plan".to_string());
    let summary_json = RunStore::summary_json_path(state.run_base_dir.as_ref(), id)?
        .and_then(|path| fs::read_to_string(path).ok());
    let progress_dir = state
        .run_base_dir
        .join(".ralphterm")
        .join("runs")
        .join(id.to_string())
        .join("progress");
    let mut allowed = progress_artifact_names(&progress_dir, &plan_slug, summary_json.as_deref())?;
    collect_event_progress_artifact_names(state.run_base_dir.as_ref(), id, &mut allowed)?;

    let mut artifacts = Vec::new();
    match fs::read_dir(&progress_dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.with_context(|| {
                    format!(
                        "read entry in progress directory {}",
                        progress_dir.display()
                    )
                })?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if !allowed.contains(&name) {
                    continue;
                }
                let metadata = fs::symlink_metadata(entry.path()).with_context(|| {
                    format!(
                        "read metadata for progress artifact {}",
                        entry.path().display()
                    )
                })?;
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    continue;
                }
                artifacts.push(ProgressArtifactIndexItem {
                    kind: progress_artifact_kind(&name),
                    url: format!(
                        "/v1/runs/{id}/progress/{}",
                        percent_encode_path_segment(&name)
                    ),
                    name,
                });
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(anyhow::Error::new(error)
                .context(format!(
                    "read progress directory {}",
                    progress_dir.display()
                ))
                .into())
        }
    }
    artifacts.sort_by(|left, right| left.name.cmp(&right.name));

    Ok(Json(artifacts))
}

fn collect_event_progress_artifact_names(
    base_dir: &FsPath,
    run_id: Uuid,
    names: &mut BTreeSet<String>,
) -> anyhow::Result<()> {
    let Some(events) = RunStore::events(base_dir, run_id)? else {
        return Ok(());
    };
    for event in events {
        let Some(path) = event.artifact_path.as_deref() else {
            continue;
        };
        let Some(name) = FsPath::new(path).file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        names.insert(name.to_string());
    }
    Ok(())
}

fn progress_artifact_kind(name: &str) -> &'static str {
    if name.ends_with("-validation.txt") {
        "validation"
    } else if name.ends_with("-review.transcript") {
        "review"
    } else if name.ends_with(".transcript") {
        "transcript"
    } else if name.ends_with(".log") {
        "log"
    } else {
        "artifact"
    }
}

fn percent_encode_path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

async fn get_run_progress(
    State(state): State<AppState>,
    Path((id, artifact)): Path<(Uuid, String)>,
) -> Result<String, ApiError> {
    if FsPath::new(&artifact)
        .file_name()
        .and_then(|name| name.to_str())
        != Some(artifact.as_str())
    {
        return Err(ApiError::artifact_not_found("progress"));
    }

    let record = RunStore::get(state.run_base_dir.as_ref(), id)?.ok_or(ApiError::run_not_found())?;
    let plan_slug = record
        .plan_path
        .as_deref()
        .map(FsPath::new)
        .map(plan_slug_for_artifacts)
        .unwrap_or_else(|| "plan".to_string());
    let summary_json = RunStore::summary_json_path(state.run_base_dir.as_ref(), id)?
        .and_then(|path| fs::read_to_string(path).ok());
    let progress_dir = state
        .run_base_dir
        .join(".ralphterm")
        .join("runs")
        .join(id.to_string())
        .join("progress");
    let mut allowed = progress_artifact_names(&progress_dir, &plan_slug, summary_json.as_deref())?;
    collect_event_progress_artifact_names(state.run_base_dir.as_ref(), id, &mut allowed)?;
    if !allowed.contains(&artifact) {
        return Err(ApiError::artifact_not_found("progress"));
    }

    let path = progress_dir.join(&artifact);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err(ApiError::artifact_not_found("progress")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ApiError::artifact_not_found("progress"));
        }
        Err(error) => {
            return Err(anyhow::Error::new(error)
                .context(format!(
                    "read progress artifact metadata {}",
                    path.display()
                ))
                .into());
        }
    }
    read_run_artifact(path, "progress").await
}

async fn read_run_artifact(path: PathBuf, name: &'static str) -> Result<String, ApiError> {
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(ApiError::artifact_not_found(name))
        }
        Err(error) => Err(anyhow::Error::new(error)
            .context(format!("read {}", path.display()))
            .into()),
    }
}

async fn get_run_events(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<crate::runs::RunEvent>>, ApiError> {
    Ok(Json(
        RunStore::events(state.run_base_dir.as_ref(), id)?.ok_or(ApiError::run_not_found())?,
    ))
}

async fn cancel_run(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    RunStore::cancel(state.run_base_dir.as_ref(), id)?.ok_or(ApiError::run_not_found())?;
    Ok(StatusCode::ACCEPTED)
}

async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<CreateSessionResponse>, ApiError> {
    let cwd = req
        .cwd
        .unwrap_or_else(|| state.run_base_dir.to_string_lossy().to_string());
    let id = state
        .store
        .spawn(SessionConfig {
            agent: req.agent.into(),
            prompt: req.prompt,
            cwd: Some(cwd),
            command: req.command,
            args: req.args.unwrap_or_default(),
            cols: req.cols.unwrap_or(120),
            rows: req.rows.unwrap_or(40),
        })
        .await?;
    Ok(Json(CreateSessionResponse { id }))
}

async fn list_sessions(State(state): State<AppState>) -> Json<Vec<SessionRecord>> {
    Json(state.store.list())
}

async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<SessionRecord>, ApiError> {
    Ok(Json(state.store.get(id).ok_or(ApiError::not_found())?))
}

async fn send_input(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<InputRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .send(
            id,
            SessionInput {
                text: req.text,
                enter: req.enter.unwrap_or(false),
            },
        )
        .await?;
    Ok(StatusCode::ACCEPTED)
}

async fn approval_decision(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<ApprovalRequest>,
) -> Result<(StatusCode, Json<ApprovalResponse>), ApiError> {
    state
        .store
        .approval_decision(id, req.approved)
        .await
        .map_err(|err| match err {
            ApprovalDecisionError::NotFound => ApiError::not_found(),
            ApprovalDecisionError::NoPending => ApiError::conflict("no approval pending"),
            ApprovalDecisionError::Send(err) => err.into(),
        })?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ApprovalResponse {
            id,
            approved: req.approved,
        }),
    ))
}

async fn resize_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<ResizeRequest>,
) -> Result<StatusCode, ApiError> {
    state.store.resize(id, req.cols, req.rows).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn cancel_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.store.cancel(id).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn get_transcript(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<String, ApiError> {
    state.store.transcript(id).ok_or(ApiError::not_found())
}

async fn ws_events(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, ApiError> {
    let mut rx = state.store.subscribe(id).ok_or(ApiError::not_found())?;
    Ok(ws.on_upgrade(move |mut socket| async move {
        while let Ok(event) = rx.recv().await {
            let Ok(text) = serde_json::to_string(&event) else {
                continue;
            };
            if socket
                .send(axum::extract::ws::Message::Text(text))
                .await
                .is_err()
            {
                break;
            }
        }
    }))
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "session not found".into(),
        }
    }

    fn run_not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "run not found".into(),
        }
    }

    fn artifact_not_found(name: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: format!("{name} artifact not found"),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(value: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: value.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(serde_json::json!({"error": self.message})),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::atomic_copy_file;
    use std::fs;

    #[test]
    fn atomic_copy_file_overwrites_destination_without_leaving_temps() {
        let temp = std::env::temp_dir().join(format!(
            "ralphterm-atomic-copy-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&temp).unwrap();

        let source = temp.join("source.log");
        let destination = temp.join("destination.log");
        fs::write(&source, "new progress").unwrap();
        fs::write(&destination, "old progress").unwrap();

        atomic_copy_file(&source, &destination).unwrap();

        assert_eq!(fs::read_to_string(&destination).unwrap(), "new progress");
        let leftover_temps: Vec<_> = fs::read_dir(&temp)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp"))
            .collect();
        assert!(
            leftover_temps.is_empty(),
            "leftover temp files: {leftover_temps:?}"
        );

        fs::remove_dir_all(temp).unwrap();
    }
}
