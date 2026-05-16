use std::{net::SocketAddr, path::PathBuf, sync::Arc};

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
use ralphterm::runner::{run_plan, RunOptions};
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
        #[arg(long)]
        agent_command: Option<String>,
    },
}

#[derive(Clone)]
struct AppState {
    store: Arc<SessionStore>,
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
            agent_command,
        } => {
            let output = run_plan(RunOptions {
                plan_path: plan,
                agent_command,
            })?;
            print!("{output}");
            Ok(())
        }
    }
}

async fn serve(bind: SocketAddr) -> anyhow::Result<()> {
    let state = AppState {
        store: Arc::new(SessionStore::default()),
    };
    let app = Router::new()
        .route("/health", get(health))
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
