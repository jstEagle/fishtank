mod storage;

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{
        IntoResponse,
        sse::{Event as SseEvent, KeepAlive, Sse},
    },
    routing::{get, post},
};
use clap::{Parser, Subcommand};
use fishtank_core::Engine;
use fishtank_protocol::{
    AuthenticatedCharacterRequest, Command, CommandEnvelope, Event, SCHEMA_VERSION, TokenCharacter,
    WorldDefinition, WorldSnapshot,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    convert::Infallible,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use storage::{FileStorage, PgStorage, Storage};
use tokio::fs;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Serve {
        #[arg(long, default_value = "worlds/village.json")]
        world: PathBuf,
        #[arg(long, default_value = ".fishtank/dev")]
        state: PathBuf,
        #[arg(long, env = "FISHTANK_BIND")]
        bind: Option<SocketAddr>,
        #[arg(long, env = "PORT")]
        port: Option<u16>,
        #[arg(long, env = "DATABASE_URL")]
        database_url: Option<String>,
        #[arg(long, env = "FISHTANK_GATEWAY_SECRET")]
        gateway_secret: Option<String>,
        #[arg(long, env = "FISHTANK_WORLD_ID", default_value = "village")]
        world_id: String,
    },
    Replay {
        #[arg(long, default_value = "worlds/village.json")]
        world: PathBuf,
        #[arg(long)]
        commands: Option<PathBuf>,
    },
}

#[derive(Clone)]
struct AppState {
    engine: Arc<Mutex<Engine>>,
    storage: Arc<dyn Storage>,
    gateway_secret: Option<String>,
    world_id: String,
}

#[derive(Deserialize)]
struct EventsQuery {
    after: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "fishtank_server=info,tower_http=info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Serve {
            world,
            state,
            bind,
            port,
            database_url,
            gateway_secret,
            world_id,
        } => {
            let bind =
                bind.unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], port.unwrap_or(3838))));
            serve(world, state, bind, database_url, gateway_secret, world_id).await
        }
        Commands::Replay { world, commands } => replay(world, commands).await,
    }
}

async fn serve(
    world_path: PathBuf,
    state_dir: PathBuf,
    bind: SocketAddr,
    database_url: Option<String>,
    gateway_secret: Option<String>,
    world_id: String,
) -> Result<()> {
    let storage: Arc<dyn Storage> = if let Some(database_url) = database_url {
        Arc::new(PgStorage::connect(&database_url, world_id.clone()).await?)
    } else {
        Arc::new(FileStorage::new(state_dir))
    };

    let (engine, seeded) = load_or_seed_engine(&world_path, storage.as_ref()).await?;
    if seeded {
        storage
            .save(engine.state(), engine.events(), engine.command_log())
            .await?;
    }

    let app_state = AppState {
        engine: Arc::new(Mutex::new(engine)),
        storage,
        gateway_secret,
        world_id,
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/snapshot", get(snapshot))
        .route("/events", get(events))
        .route("/characters/{character_id}/observe", get(observe))
        .route("/command", post(command))
        .route("/v1/worlds/{world_id}/snapshot", get(v1_snapshot))
        .route("/v1/worlds/{world_id}/events", get(v1_events))
        .route("/v1/worlds/{world_id}/stream", get(v1_stream))
        .route("/v1/character", post(v1_character))
        .route("/v1/observe", get(v1_observe))
        .route("/v1/actions", get(v1_actions))
        .route("/v1/command", post(v1_command))
        .route("/v1/events", get(v1_events_default))
        .route("/v1/notifications", get(v1_notifications))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(app_state);

    info!(%bind, "starting fishtank server");
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn load_or_seed_engine(
    world_path: &PathBuf,
    storage: &dyn Storage,
) -> Result<(Engine, bool)> {
    if let Some(stored) = storage.load().await? {
        return Ok((Engine::from_snapshot(stored.snapshot, stored.events), false));
    }
    let world_json = fs::read_to_string(world_path)
        .await
        .with_context(|| format!("failed to read world file {}", world_path.display()))?;
    Ok((Engine::from_world_json(&world_json)?, true))
}

async fn replay(world_path: PathBuf, commands_path: Option<PathBuf>) -> Result<()> {
    let world_json = fs::read_to_string(&world_path)
        .await
        .with_context(|| format!("failed to read world file {}", world_path.display()))?;
    let world: WorldDefinition = serde_json::from_str(&world_json)?;
    let commands = if let Some(commands_path) = commands_path {
        let command_log = fs::read_to_string(&commands_path)
            .await
            .with_context(|| format!("failed to read command log {}", commands_path.display()))?;
        command_log
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str::<CommandEnvelope>)
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    let engine = Engine::replay(world, &commands)?;
    println!("{}", serde_json::to_string_pretty(engine.state())?);
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

async fn snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WorldSnapshot>, AppError> {
    authorize_gateway(&state, &headers)?;
    Ok(Json(snapshot_from_state(&state)))
}

async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<Event>>, AppError> {
    authorize_gateway(&state, &headers)?;
    Ok(Json(events_from_state(&state, query.after)))
}

async fn observe(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(character_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    authorize_gateway(&state, &headers)?;
    Ok(observe_character(&state, &character_id).into_response())
}

async fn command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(envelope): Json<CommandEnvelope>,
) -> Result<Json<fishtank_protocol::CommandResponse>, AppError> {
    authorize_gateway(&state, &headers)?;
    Ok(Json(apply_envelope(&state, envelope).await?))
}

async fn v1_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(world_id): Path<String>,
) -> Result<Json<WorldSnapshot>, AppError> {
    authorize_gateway(&state, &headers)?;
    ensure_world(&state, &world_id)?;
    Ok(Json(snapshot_from_state(&state)))
}

async fn v1_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(world_id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<Event>>, AppError> {
    authorize_gateway(&state, &headers)?;
    ensure_world(&state, &world_id)?;
    Ok(Json(events_from_state(&state, query.after)))
}

async fn v1_events_default(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<Event>>, AppError> {
    authorize_gateway(&state, &headers)?;
    Ok(Json(events_from_state(&state, query.after)))
}

async fn v1_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(world_id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>>, AppError> {
    authorize_gateway(&state, &headers)?;
    ensure_world(&state, &world_id)?;
    let mut last_event_id = query.after.unwrap_or(0);
    let stream_state = state.clone();
    let stream = async_stream::stream! {
        let initial = serde_json::to_string(&snapshot_from_state(&stream_state))
            .unwrap_or_else(|_| "{}".to_string());
        yield Ok(SseEvent::default().event("snapshot").data(initial));
        loop {
            let events = events_from_state(&stream_state, Some(last_event_id));
            for event in events {
                last_event_id = event.id;
                let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
                yield Ok(SseEvent::default().event("event").id(event.id.to_string()).data(data));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn v1_character(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AuthenticatedCharacterRequest>,
) -> Result<impl IntoResponse, AppError> {
    authorize_gateway(&state, &headers)?;
    let token_hash = require_agent_token_hash(&headers)?;
    if let Some(character_id) = state.storage.character_for_token(&token_hash).await? {
        return Ok(Json(TokenCharacter {
            token_id: token_hash[..12].to_string(),
            character_id,
        })
        .into_response());
    }

    let character_id = format!("char_{}", &token_hash[..16]);
    let envelope = server_envelope(
        &character_id,
        Command::CreateCharacter {
            name: request.name,
            body_color: request.body_color,
            face_color: request.face_color,
        },
    );
    let response = apply_envelope(&state, envelope).await?;
    if response.ok {
        state.storage.bind_token(&token_hash, &character_id).await?;
        Ok((
            StatusCode::CREATED,
            Json(TokenCharacter {
                token_id: token_hash[..12].to_string(),
                character_id,
            }),
        )
            .into_response())
    } else {
        Ok((StatusCode::BAD_REQUEST, Json(response)).into_response())
    }
}

async fn v1_observe(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    authorize_gateway(&state, &headers)?;
    let character_id = character_for_headers(&state, &headers).await?;
    Ok(observe_character(&state, &character_id).into_response())
}

async fn v1_actions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    authorize_gateway(&state, &headers)?;
    let character_id = character_for_headers(&state, &headers).await?;
    let body = state
        .engine
        .lock()
        .expect("engine lock poisoned")
        .observe(&character_id)
        .map(|observation| observation.available_actions);
    match body {
        Ok(actions) => Ok(Json(actions).into_response()),
        Err(error) => Ok((StatusCode::BAD_REQUEST, Json(error)).into_response()),
    }
}

async fn v1_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(command): Json<Command>,
) -> Result<Json<fishtank_protocol::CommandResponse>, AppError> {
    authorize_gateway(&state, &headers)?;
    let character_id = character_for_headers(&state, &headers).await?;
    Ok(Json(
        apply_envelope(&state, server_envelope(&character_id, command)).await?,
    ))
}

async fn v1_notifications(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<fishtank_protocol::CommandResponse>, AppError> {
    authorize_gateway(&state, &headers)?;
    let character_id = character_for_headers(&state, &headers).await?;
    Ok(Json(
        apply_envelope(
            &state,
            server_envelope(
                &character_id,
                Command::Notifications {
                    action: fishtank_protocol::NotificationAction::List,
                },
            ),
        )
        .await?,
    ))
}

fn snapshot_from_state(state: &AppState) -> WorldSnapshot {
    state
        .engine
        .lock()
        .expect("engine lock poisoned")
        .state()
        .clone()
}

fn events_from_state(state: &AppState, after: Option<u64>) -> Vec<Event> {
    state
        .engine
        .lock()
        .expect("engine lock poisoned")
        .events_after(after)
}

fn observe_character(state: &AppState, character_id: &str) -> axum::response::Response {
    let response = state
        .engine
        .lock()
        .expect("engine lock poisoned")
        .observe(character_id);
    match response {
        Ok(observation) => Json(observation).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
    }
}

async fn apply_envelope(
    state: &AppState,
    envelope: CommandEnvelope,
) -> Result<fishtank_protocol::CommandResponse> {
    let (response, snapshot, events, commands) = {
        let mut engine = state.engine.lock().expect("engine lock poisoned");
        let response = engine.apply(envelope);
        let snapshot = engine.state().clone();
        let events = engine.events().to_vec();
        let commands = engine.command_log().to_vec();
        (response, snapshot, events, commands)
    };
    state.storage.save(&snapshot, &events, &commands).await?;
    Ok(response)
}

fn authorize_gateway(state: &AppState, headers: &HeaderMap) -> Result<(), AppError> {
    let Some(secret) = &state.gateway_secret else {
        return Ok(());
    };
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Err(AppError::status(
            StatusCode::UNAUTHORIZED,
            "missing gateway authorization",
        ));
    };
    let expected = format!("Bearer {secret}");
    if value.to_str().ok() == Some(expected.as_str()) {
        Ok(())
    } else {
        Err(AppError::status(
            StatusCode::FORBIDDEN,
            "invalid gateway authorization",
        ))
    }
}

fn ensure_world(state: &AppState, world_id: &str) -> Result<(), AppError> {
    if state.world_id == world_id {
        Ok(())
    } else {
        Err(AppError::status(StatusCode::NOT_FOUND, "unknown world"))
    }
}

async fn character_for_headers(state: &AppState, headers: &HeaderMap) -> Result<String, AppError> {
    let token_hash = require_agent_token_hash(headers)?;
    state
        .storage
        .character_for_token(&token_hash)
        .await?
        .ok_or_else(|| {
            AppError::status(
                StatusCode::UNAUTHORIZED,
                "agent token is not bound to a character",
            )
        })
}

fn require_agent_token_hash(headers: &HeaderMap) -> Result<String, AppError> {
    let token = headers
        .get("x-fishtank-agent-token")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::status(StatusCode::UNAUTHORIZED, "missing agent token"))?;
    Ok(hash_token(token))
}

fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn server_envelope(character_id: &str, command: Command) -> CommandEnvelope {
    CommandEnvelope {
        schema_version: SCHEMA_VERSION.to_string(),
        command_id: format!(
            "server.{}",
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ),
        character_id: character_id.to_string(),
        submitted_at: time::OffsetDateTime::now_utc().to_string(),
        based_on_tick: None,
        valid_until_tick: None,
        local_state_hash: None,
        preconditions: Vec::new(),
        command,
    }
}

#[derive(Debug)]
struct AppError {
    status: StatusCode,
    message: String,
}

impl AppError {
    fn status(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(error: E) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.into().to_string(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let body = Json(serde_json::json!({
            "ok": false,
            "error": self.message,
        }));
        (self.status, body).into_response()
    }
}
