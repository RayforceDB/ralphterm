use std::{
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
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

mod pty_agent;
mod signals;
mod store;

use pty_agent::{AgentKind, SessionConfig, SessionInput};
use ralphterm::{
    runner::{agent_commands_equivalent, run_plan, run_smoke, PlanRunEvent, RunOptions},
    runs::{
        CreatedRunRecord, RunPhase, RunProgressEvent, RunRecord, RunResultArtifacts, RunStatus,
        RunStore,
    },
    workspace::WorkspaceManager,
};
use store::{SessionRecord, SessionStore};

#[derive(Debug, Parser)]
#[command(name = "ralphterm")]
#[command(about = "Terminal-native Claude/Codex orchestration API", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
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

#[derive(Debug, Deserialize)]
struct CreateRunRequest {
    plan_path: Option<String>,
    workspace_id: Option<String>,
    agent: Option<ApiAgentKind>,
    agent_command: Option<String>,
    review_agent: Option<ApiAgentKind>,
    review_command: Option<String>,
    require_review: Option<bool>,
    max_review_retries: Option<usize>,
    no_commit: Option<bool>,
    dry_run: Option<bool>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Serve { bind } => serve(bind).await,
        Command::Run {
            plan,
            agent,
            agent_command,
            review_agent,
            review_command,
            require_review,
            max_review_retries,
            no_commit,
            dry_run,
            workspace_id,
        } => {
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

                if dry_run {
                    let workspace = manager.workspace(id)?;
                    println!("Workspace: {} (dry run)", workspace.path.display());
                } else {
                    let workspace = manager.create(id)?;
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
                require_review,
                max_review_retries,
                no_commit,
                dry_run,
                event_sink: None,
                cancellation_check: None,
            })?;
            print!("{output}");
            Ok(())
        }
        Command::Smoke {
            agent,
            agent_command,
        } => {
            let agent_command =
                agent_command.unwrap_or_else(|| agent.unwrap_or(RunAgentKind::Claude).command());
            let output = run_smoke(&agent_command)?;
            print!("{output}");
            Ok(())
        }
        Command::Workspace { command } => run_workspace_command(command),
    }
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
        .route("/v1/runs/:id/diff", get(get_run_diff))
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
    if req.require_review.unwrap_or(false) && review_command.is_none() {
        return Err(ApiError::bad_request(
            "review_command or review_agent is required when require_review is true",
        ));
    }
    if let (Some(agent_command), Some(review_command)) =
        (agent_command.as_deref(), review_command.as_deref())
    {
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

        if agent_command.is_some() && !dry_run {
            let candidate = manager
                .workspace(workspace_id)
                .map_err(|err| ApiError::bad_request(err.to_string()))?;
            let workspace = if candidate.path.exists() {
                manager
                    .validate_existing_workspace(&candidate)
                    .map_err(|err| ApiError::bad_request(err.to_string()))?;
                candidate
            } else {
                manager.create(workspace_id)?
            };
            workspace_execution_dir = Some(workspace.path.join(cwd_relative));
            workspace_path = Some(workspace.path.to_string_lossy().to_string());
        }
    }
    let record = RunStore::create(
        state.run_base_dir.as_ref(),
        RunRecord {
            phase: RunPhase::Planning,
            status: RunStatus::Created,
            plan_path: plan_path.clone(),
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
    let execution_dir = workspace_execution_dir.unwrap_or_else(|| executor_base_dir.clone());
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
            let diff_path = progress_dir.join(format!("{slug}-diff.patch"));
            let _cwd_guard = CurrentDirGuard::change_to(&execution_dir).map_err(|err| {
                anyhow::Error::new(err)
                    .context(format!("switch to execution directory {}", execution_dir.display()))
            });
            let Ok(_cwd_guard) = _cwd_guard else {
                let _ = RunStore::write_failure(&base_dir, run_id, None, None);
                tracing::error!(%run_id, "background plan run failed to switch execution directory");
                return;
            };
            let event_base_dir = base_dir.clone();
            let event_sink = Arc::new(move |event: PlanRunEvent| {
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
                plan_path,
                agent_command,
                review_command,
                require_review,
                max_review_retries,
                no_commit,
                dry_run,
                event_sink: Some(event_sink),
                cancellation_check: Some(cancellation_check),
            }) {
                Ok(output) => output,
                Err(err) => {
                    let summary_markdown = fs::read_to_string(&summary_path).ok();
                    let diff_patch = fs::read_to_string(&diff_path).ok();
                    match RunStore::write_failure(&base_dir, run_id, summary_markdown, diff_patch) {
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
                match RunStore::write_result(
                    &base_dir,
                    run_id,
                    RunResultArtifacts {
                        summary_markdown: run_output,
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
                    let _ = RunStore::write_failure(&base_dir, run_id, None, None);
                    tracing::error!(%run_id, error = %error, "background plan run failed");
                    return;
                }
            };
            let diff_patch = match fs::read_to_string(&diff_path) {
                Ok(diff_patch) => diff_patch,
                Err(err) => {
                    let error = anyhow::Error::new(err).context("read run diff artifact");
                    let _ = RunStore::write_failure(&base_dir, run_id, Some(summary_markdown), None);
                    tracing::error!(%run_id, error = %error, "background plan run failed");
                    return;
                }
            };
            match RunStore::write_result(
                &base_dir,
                run_id,
                RunResultArtifacts {
                    summary_markdown,
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
            let _ = RunStore::write_failure(&supervisor_base_dir, run_id, None, None);
            tracing::error!(%run_id, error = %err, "background plan worker failed to join");
        }
    });

    Ok(Json(started))
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

async fn get_run_diff(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<String, ApiError> {
    let path =
        RunStore::diff_path(state.run_base_dir.as_ref(), id)?.ok_or(ApiError::run_not_found())?;
    read_run_artifact(path, "diff").await
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
) -> Result<Json<Vec<ralphterm::runs::RunEvent>>, ApiError> {
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
    state.store.approval_decision(id, req.approved).await?;
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
