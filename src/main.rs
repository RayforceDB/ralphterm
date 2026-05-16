use std::{
    fs,
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
};

use anyhow::Context;
use axum::{
    extract::{Path, State, WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Json},
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
    runner::{run_plan, run_smoke, RunOptions},
    runs::{CreatedRunRecord, RunPhase, RunRecord, RunResultArtifacts, RunStatus, RunStore},
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
struct ResizeRequest {
    cols: u16,
    rows: u16,
}

#[derive(Debug, Serialize)]
struct CreateSessionResponse {
    id: Uuid,
}

#[derive(Debug, Deserialize)]
struct CreateRunRequest {
    plan_path: Option<String>,
    agent_command: Option<String>,
    review_command: Option<String>,
    require_review: Option<bool>,
    max_review_retries: Option<usize>,
    no_commit: Option<bool>,
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
        } => {
            let output = run_plan(RunOptions {
                plan_path: plan,
                agent_command: agent_command.or_else(|| agent.map(RunAgentKind::command)),
                review_command: review_command.or_else(|| review_agent.map(RunAgentKind::command)),
                require_review,
                max_review_retries,
                no_commit,
                dry_run,
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
        .route("/v1/runs", post(create_run).get(list_runs))
        .route("/v1/runs/:id", get(get_run))
        .route("/v1/runs/:id/events", get(get_run_events))
        .route("/v1/runs/:id/cancel", post(cancel_run))
        .route("/v1/sessions", post(create_session))
        .route("/v1/sessions/:id", get(get_session))
        .route("/v1/sessions/:id/input", post(send_input))
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

async fn create_run(
    State(state): State<AppState>,
    Json(req): Json<CreateRunRequest>,
) -> Result<Json<CreatedRunRecord>, ApiError> {
    let plan_path = req.plan_path.clone();
    if req.agent_command.is_some() && plan_path.is_none() {
        return Err(ApiError::bad_request(
            "plan_path is required when agent_command is set",
        ));
    }
    let record = RunStore::create(
        state.run_base_dir.as_ref(),
        RunRecord {
            phase: RunPhase::Planning,
            status: RunStatus::Created,
            plan_path: plan_path.clone(),
        },
    )?;

    let Some(agent_command) = req.agent_command else {
        return Ok(Json(record));
    };

    let plan_path = plan_path
        .map(PathBuf::from)
        .ok_or_else(|| ApiError::bad_request("plan_path is required when agent_command is set"))?;
    let base_dir = state.run_base_dir.as_ref().clone();
    let run_id = record.id;
    let review_command = req.review_command;
    let require_review = req.require_review.unwrap_or(false);
    let max_review_retries = req.max_review_retries.unwrap_or(1);
    let no_commit = req.no_commit.unwrap_or(false);
    let slug = plan_slug_for_artifacts(&plan_path);

    tokio::task::spawn_blocking(move || {
        let progress_dir = base_dir.join(".ralphterm").join("progress");
        let summary_path = progress_dir.join(format!("{slug}-summary.md"));
        let diff_path = progress_dir.join(format!("{slug}-diff.patch"));
        if let Err(err) = run_plan(RunOptions {
            plan_path,
            agent_command: Some(agent_command),
            review_command,
            require_review,
            max_review_retries,
            no_commit,
            dry_run: false,
        }) {
            let summary_markdown = fs::read_to_string(&summary_path).ok();
            let diff_patch = fs::read_to_string(&diff_path).ok();
            RunStore::write_failure(&base_dir, run_id, summary_markdown, diff_patch)?
                .context("run disappeared before failure could be written")?;
            return Err(err);
        }

        let summary_markdown =
            fs::read_to_string(&summary_path).context("read run summary artifact")?;
        let diff_patch = fs::read_to_string(&diff_path).context("read run diff artifact")?;
        RunStore::write_result(
            &base_dir,
            run_id,
            RunResultArtifacts {
                summary_markdown,
                diff_patch,
            },
        )?
        .context("run disappeared before result could be written")
    })
    .await
    .context("join run executor")?
    .map(Json)
    .map_err(ApiError::from)
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
    let id = state
        .store
        .spawn(SessionConfig {
            agent: req.agent.into(),
            prompt: req.prompt,
            cwd: req.cwd,
            command: req.command,
            args: req.args.unwrap_or_default(),
            cols: req.cols.unwrap_or(120),
            rows: req.rows.unwrap_or(40),
        })
        .await?;
    Ok(Json(CreateSessionResponse { id }))
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
