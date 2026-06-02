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
    routing::{delete, get, post},
};
use clap::{Parser, Subcommand};
use fishtank_core::Engine;
use fishtank_protocol::{
    AuthenticatedCharacterRequest, Character, Command, CommandEnvelope, Event, EventId, EventKind,
    SCHEMA_VERSION, TokenCharacter, WorldDefinition, WorldSnapshot,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    convert::Infallible,
    env,
    net::SocketAddr,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};
use storage::{FileStorage, PgStorage, Storage};
use tokio::{fs, sync::watch};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{error, info};

const SINGLETON_STORAGE_KEY: &str = "singleton";
const REAL_SECONDS_PER_TICK: u64 = 5;
const DEFAULT_EVENT_HISTORY_LIMIT: usize = 2_000;
const DEFAULT_COMMAND_HISTORY_LIMIT: usize = 1_000;
const DEFAULT_STREAM_KEEPALIVE_SECONDS: u64 = 60;

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
        bind: Option<String>,
        #[arg(long, env = "PORT")]
        port: Option<String>,
        #[arg(long, env = "DATABASE_URL")]
        database_url: Option<String>,
        #[arg(long, env = "FISHTANK_GATEWAY_SECRET")]
        gateway_secret: Option<String>,
        #[arg(long, env = "FISHTANK_ADMIN_TOKEN")]
        admin_token: Option<String>,
        #[arg(
            long,
            env = "FISHTANK_EVENT_HISTORY_LIMIT",
            default_value_t = DEFAULT_EVENT_HISTORY_LIMIT
        )]
        event_history_limit: usize,
        #[arg(
            long,
            env = "FISHTANK_COMMAND_HISTORY_LIMIT",
            default_value_t = DEFAULT_COMMAND_HISTORY_LIMIT
        )]
        command_history_limit: usize,
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
    event_signal: watch::Sender<EventId>,
    gateway_secret: Option<String>,
    admin_token: Option<String>,
    legacy_world_id: String,
    history_limits: HistoryLimits,
}

#[derive(Clone, Copy)]
struct HistoryLimits {
    events: usize,
    commands: usize,
}

#[derive(Deserialize)]
struct EventsQuery {
    after: Option<u64>,
    compact: Option<String>,
    limit: Option<usize>,
    snapshot: Option<bool>,
}

#[derive(Deserialize)]
struct SnapshotQuery {
    compact: Option<String>,
}

#[derive(Serialize)]
struct AdminCharacterList {
    world_model: &'static str,
    tick: u64,
    characters: Vec<Character>,
}

#[derive(Serialize)]
struct AdminDeleteCharacterResponse {
    ok: bool,
    character: Character,
    token_bindings_deleted: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "fishtank_server=info,tower_http=warn".into()),
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
            admin_token,
            event_history_limit,
            command_history_limit,
        } => {
            let bind = resolve_bind(bind, port)?;
            serve(
                world,
                state,
                bind,
                database_url,
                gateway_secret,
                admin_token,
                HistoryLimits {
                    events: event_history_limit,
                    commands: command_history_limit,
                },
            )
            .await
        }
        Commands::Replay { world, commands } => replay(world, commands).await,
    }
}

fn resolve_bind(bind: Option<String>, port: Option<String>) -> Result<SocketAddr> {
    if let Some(bind) = bind.filter(|value| !value.trim().is_empty()) {
        return SocketAddr::from_str(&bind)
            .with_context(|| format!("invalid bind address `{bind}`"));
    }

    let port = port
        .filter(|value| !value.trim().is_empty())
        .as_deref()
        .unwrap_or("3838")
        .parse::<u16>()
        .context("invalid PORT")?;
    Ok(SocketAddr::from(([0, 0, 0, 0], port)))
}

async fn serve(
    world_path: PathBuf,
    state_dir: PathBuf,
    bind: SocketAddr,
    database_url: Option<String>,
    gateway_secret: Option<String>,
    admin_token: Option<String>,
    history_limits: HistoryLimits,
) -> Result<()> {
    let storage: Arc<dyn Storage> = if let Some(database_url) = database_url {
        let legacy_keys = postgres_legacy_storage_keys();
        info!(
            storage_key = SINGLETON_STORAGE_KEY,
            ?legacy_keys,
            "using postgres storage"
        );
        Arc::new(
            PgStorage::connect(
                &database_url,
                SINGLETON_STORAGE_KEY.to_string(),
                legacy_keys,
            )
            .await?,
        )
    } else {
        info!(state_dir = %state_dir.display(), "using file storage");
        Arc::new(FileStorage::new(state_dir))
    };

    let (mut engine, seeded) = load_or_seed_engine(&world_path, storage.as_ref()).await?;
    let compacted = engine.compact_history(history_limits.events, history_limits.commands);
    if seeded || compacted {
        storage
            .save(engine.state(), engine.events(), engine.command_log())
            .await?;
    }

    let legacy_world_id = engine.state().world_id.clone();
    let (event_signal, _) = watch::channel(engine.state().next_event_id);
    let app_state = AppState {
        engine: Arc::new(Mutex::new(engine)),
        storage,
        event_signal,
        gateway_secret,
        admin_token,
        legacy_world_id,
        history_limits,
    };
    tokio::spawn(run_simulation_clock(app_state.clone()));
    let app = Router::new()
        .route("/health", get(health))
        .route("/snapshot", get(snapshot))
        .route("/events", get(events))
        .route("/characters/{character_id}/observe", get(observe))
        .route(
            "/characters/{character_id}/observe-agent",
            get(observe_agent),
        )
        .route("/command", post(command))
        .route("/v1/worlds/{world_id}/snapshot", get(v1_snapshot))
        .route("/v1/worlds/{world_id}/events", get(v1_events))
        .route("/v1/worlds/{world_id}/stream", get(v1_stream))
        .route("/v1/snapshot", get(v1_snapshot_default))
        .route("/v1/character", post(v1_character))
        .route("/v1/observe", get(v1_observe))
        .route("/v1/observe/agent", get(v1_observe_agent))
        .route("/v1/actions", get(v1_actions))
        .route("/v1/command", post(v1_command))
        .route("/v1/events", get(v1_events_default))
        .route("/v1/stream", get(v1_stream_default))
        .route("/v1/notifications", get(v1_notifications))
        .route("/admin/characters", get(admin_characters))
        .route(
            "/admin/characters/{character_id}",
            delete(admin_delete_character),
        )
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(app_state);

    info!(%bind, "starting fishtank server");
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn postgres_legacy_storage_keys() -> Vec<String> {
    let mut keys = vec!["village".to_string()];
    if let Ok(world_id) = env::var("FISHTANK_WORLD_ID") {
        let trimmed = world_id.trim();
        if !trimmed.is_empty() && trimmed != SINGLETON_STORAGE_KEY {
            keys.push(trimmed.to_string());
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

async fn run_simulation_clock(state: AppState) {
    let mut wake_rx = state.event_signal.subscribe();
    loop {
        if !has_active_activity(&state) {
            if wake_rx.changed().await.is_err() {
                break;
            }
            continue;
        }

        tokio::time::sleep(Duration::from_secs(REAL_SECONDS_PER_TICK)).await;
        let payload = {
            let mut engine = state.engine.lock().expect("engine mutex poisoned");
            let has_active_activity = engine
                .state()
                .characters
                .values()
                .any(|character| character.current_activity.is_some());
            if !has_active_activity {
                None
            } else {
                engine.advance_ticks(1);
                engine.compact_history(state.history_limits.events, state.history_limits.commands);
                Some((
                    snapshot_for_storage(engine.state()),
                    engine.events().to_vec(),
                    engine.command_log().to_vec(),
                ))
            }
        };

        if let Some((snapshot, events, commands)) = payload {
            notify_state_changed(&state, snapshot.next_event_id);
            if let Err(err) = state.storage.save(&snapshot, &events, &commands).await {
                error!(?err, "failed to persist simulation tick");
            }
        }
    }
}

fn has_active_activity(state: &AppState) -> bool {
    state
        .engine
        .lock()
        .expect("engine lock poisoned")
        .state()
        .characters
        .values()
        .any(|character| character.current_activity.is_some())
}

async fn load_or_seed_engine(
    world_path: &PathBuf,
    storage: &dyn Storage,
) -> Result<(Engine, bool)> {
    let world_json = fs::read_to_string(world_path)
        .await
        .with_context(|| format!("failed to read world file {}", world_path.display()))?;
    let world: WorldDefinition = serde_json::from_str(&world_json)?;
    if let Some(stored) = storage.load().await? {
        return Ok((
            Engine::from_snapshot_with_world_definition(stored.snapshot, stored.events, world)?,
            false,
        ));
    }
    Ok((Engine::new(world)?, true))
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
    Query(query): Query<SnapshotQuery>,
) -> Result<axum::response::Response, AppError> {
    authorize_gateway(&state, &headers)?;
    snapshot_response_for_query(&state, query.compact.as_deref())
}

async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<Event>>, AppError> {
    authorize_gateway(&state, &headers)?;
    Ok(Json(events_for_query(
        &state,
        query.after,
        query.limit,
        query.compact.as_deref(),
    )))
}

async fn observe(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(character_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    authorize_gateway(&state, &headers)?;
    Ok(observe_character(&state, &character_id).into_response())
}

async fn observe_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(character_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    authorize_gateway(&state, &headers)?;
    Ok(observe_agent_character(&state, &character_id).into_response())
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
    Query(query): Query<SnapshotQuery>,
) -> Result<axum::response::Response, AppError> {
    authorize_gateway(&state, &headers)?;
    ensure_world(&state, &world_id)?;
    snapshot_response_for_query(&state, query.compact.as_deref())
}

async fn v1_snapshot_default(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SnapshotQuery>,
) -> Result<axum::response::Response, AppError> {
    authorize_gateway(&state, &headers)?;
    snapshot_response_for_query(&state, query.compact.as_deref())
}

async fn v1_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(world_id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<Event>>, AppError> {
    authorize_gateway(&state, &headers)?;
    ensure_world(&state, &world_id)?;
    Ok(Json(events_for_query(
        &state,
        query.after,
        query.limit,
        query.compact.as_deref(),
    )))
}

async fn v1_events_default(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<Event>>, AppError> {
    authorize_gateway(&state, &headers)?;
    Ok(Json(events_for_query(
        &state,
        query.after,
        query.limit,
        query.compact.as_deref(),
    )))
}

async fn v1_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(world_id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>>, AppError> {
    authorize_gateway(&state, &headers)?;
    ensure_world(&state, &world_id)?;
    stream_events(
        state,
        query.after,
        query.compact,
        query.snapshot.unwrap_or(true),
    )
}

async fn v1_stream_default(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>>, AppError> {
    authorize_gateway(&state, &headers)?;
    stream_events(
        state,
        query.after,
        query.compact,
        query.snapshot.unwrap_or(true),
    )
}

fn stream_events(
    state: AppState,
    after: Option<EventId>,
    compact: Option<String>,
    send_snapshot: bool,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>>, AppError> {
    let mut last_event_id = after.unwrap_or(0);
    let stream_state = state.clone();
    let compact_viewer = compact.as_deref() == Some("viewer");
    let mut event_rx = state.event_signal.subscribe();
    let stream = async_stream::stream! {
        let compact_mode = compact_viewer.then_some("viewer");
        if compact_viewer && send_snapshot {
            last_event_id = last_event_id.max(snapshot_last_event_id(&stream_state));
        }
        if send_snapshot {
            let initial = snapshot_json_for_query(&stream_state, compact_mode)
                .unwrap_or_else(|_| "{}".to_string());
            yield Ok(SseEvent::default().event("snapshot").data(initial));
        }
        loop {
            let (events, latest_event_id) = if compact_viewer {
                let compact_events = compact_viewer_events_from_state(&stream_state, Some(last_event_id), None);
                (compact_events.events, compact_events.latest_event_id)
            } else {
                let events = events_from_state(&stream_state, Some(last_event_id), None);
                let latest_event_id = events.last().map(|event| event.id).unwrap_or(last_event_id);
                (events, latest_event_id)
            };
            last_event_id = latest_event_id;
            let mut sent_events = false;
            for event in events {
                sent_events = true;
                let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
                yield Ok(SseEvent::default().event("event").id(event.id.to_string()).data(data));
            }
            if sent_events {
                continue;
            }
            if event_rx.changed().await.is_err() {
                break;
            }
        }
    };
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(stream_keepalive_seconds()))))
}

async fn v1_character(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AuthenticatedCharacterRequest>,
) -> Result<impl IntoResponse, AppError> {
    authorize_gateway(&state, &headers)?;
    let issued_token = issue_agent_token_if_missing(&headers);
    let token_hash = match issued_token.as_deref() {
        Some(token) => hash_token(token),
        None => require_agent_token_hash(&headers)?,
    };
    if let Some(character_id) = state.storage.character_for_token(&token_hash).await? {
        return Ok(Json(TokenCharacter {
            token_id: token_hash[..12].to_string(),
            character_id,
            raw_token: None,
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
                raw_token: issued_token,
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

async fn v1_observe_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    authorize_gateway(&state, &headers)?;
    let character_id = character_for_headers(&state, &headers).await?;
    Ok(observe_agent_character(&state, &character_id).into_response())
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

async fn admin_characters(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AdminCharacterList>, AppError> {
    authorize_admin(&state, &headers)?;
    let snapshot = snapshot_from_state(&state);
    Ok(Json(AdminCharacterList {
        world_model: "single_shared_world",
        tick: snapshot.tick,
        characters: snapshot.characters.into_values().collect(),
    }))
}

async fn admin_delete_character(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(character_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    authorize_admin(&state, &headers)?;
    let (character, snapshot, events, commands) = {
        let mut engine = state.engine.lock().expect("engine lock poisoned");
        let Some(character) = engine.delete_character(&character_id) else {
            return Err(AppError::status(StatusCode::NOT_FOUND, "unknown character"));
        };
        engine.compact_history(state.history_limits.events, state.history_limits.commands);
        (
            character,
            snapshot_for_storage(engine.state()),
            engine.events().to_vec(),
            engine.command_log().to_vec(),
        )
    };
    state.storage.save(&snapshot, &events, &commands).await?;
    notify_state_changed(&state, snapshot.next_event_id);
    let token_bindings_deleted = state
        .storage
        .delete_tokens_for_character(&character.id)
        .await?;
    Ok(Json(AdminDeleteCharacterResponse {
        ok: true,
        character,
        token_bindings_deleted,
    }))
}

fn snapshot_from_state(state: &AppState) -> WorldSnapshot {
    state
        .engine
        .lock()
        .expect("engine lock poisoned")
        .state()
        .clone()
}

fn snapshot_response_for_query(
    state: &AppState,
    compact: Option<&str>,
) -> Result<axum::response::Response, AppError> {
    if matches!(compact, Some("viewer" | "viewer_state")) {
        let body = snapshot_json_for_query(state, compact)?;
        Ok(([(header::CONTENT_TYPE, "application/json")], body).into_response())
    } else {
        Ok(Json(snapshot_from_state(state)).into_response())
    }
}

fn snapshot_json_for_query(state: &AppState, compact: Option<&str>) -> Result<String> {
    match compact {
        Some("viewer") => viewer_snapshot_json_from_state(state),
        Some("viewer_state") => viewer_state_snapshot_json_from_state(state),
        _ => Ok(serde_json::to_string(&snapshot_from_state(state))?),
    }
}

fn snapshot_last_event_id(state: &AppState) -> EventId {
    state
        .engine
        .lock()
        .expect("engine lock poisoned")
        .state()
        .next_event_id
        .saturating_sub(1)
}

#[derive(Serialize)]
struct ViewerSnapshot<'a> {
    schema_version: &'a str,
    world_id: &'a str,
    tick: u64,
    next_event_id: EventId,
    next_command_seq: u64,
    next_conversation_seq: u64,
    world: &'a WorldDefinition,
    characters: &'a BTreeMap<String, Character>,
    home_locks: &'a BTreeMap<String, bool>,
    conversations: &'a BTreeMap<String, serde_json::Value>,
    notifications: &'a BTreeMap<String, serde_json::Value>,
    public_invites: &'a BTreeMap<String, serde_json::Value>,
    public_notices: &'a BTreeMap<String, serde_json::Value>,
    external_games: &'a BTreeMap<String, serde_json::Value>,
    command_log: &'a [CommandEnvelope],
}

#[derive(Serialize)]
struct ViewerStateSnapshot<'a> {
    schema_version: &'a str,
    world_id: &'a str,
    tick: u64,
    next_event_id: EventId,
    next_command_seq: u64,
    next_conversation_seq: u64,
    characters: &'a BTreeMap<String, Character>,
    home_locks: &'a BTreeMap<String, bool>,
}

fn viewer_snapshot_json_from_state(state: &AppState) -> Result<String> {
    let engine = state.engine.lock().expect("engine lock poisoned");
    let snapshot = engine.state();
    let empty_map = BTreeMap::<String, serde_json::Value>::new();
    let empty_command_log: &[CommandEnvelope] = &[];
    Ok(serde_json::to_string(&ViewerSnapshot {
        schema_version: &snapshot.schema_version,
        world_id: &snapshot.world_id,
        tick: snapshot.tick,
        next_event_id: snapshot.next_event_id,
        next_command_seq: snapshot.next_command_seq,
        next_conversation_seq: snapshot.next_conversation_seq,
        world: &snapshot.world,
        characters: &snapshot.characters,
        home_locks: &snapshot.home_locks,
        conversations: &empty_map,
        notifications: &empty_map,
        public_invites: &empty_map,
        public_notices: &empty_map,
        external_games: &empty_map,
        command_log: empty_command_log,
    })?)
}

fn viewer_state_snapshot_json_from_state(state: &AppState) -> Result<String> {
    let engine = state.engine.lock().expect("engine lock poisoned");
    let snapshot = engine.state();
    Ok(serde_json::to_string(&ViewerStateSnapshot {
        schema_version: &snapshot.schema_version,
        world_id: &snapshot.world_id,
        tick: snapshot.tick,
        next_event_id: snapshot.next_event_id,
        next_command_seq: snapshot.next_command_seq,
        next_conversation_seq: snapshot.next_conversation_seq,
        characters: &snapshot.characters,
        home_locks: &snapshot.home_locks,
    })?)
}

fn events_from_state(state: &AppState, after: Option<u64>, limit: Option<usize>) -> Vec<Event> {
    let mut events = state
        .engine
        .lock()
        .expect("engine lock poisoned")
        .events_after(after);
    if let Some(limit) = limit.filter(|limit| *limit > 0)
        && events.len() > limit
    {
        events.drain(0..events.len() - limit);
    }
    events
}

struct CompactEvents {
    events: Vec<Event>,
    latest_event_id: EventId,
}

fn compact_viewer_events_from_state(
    state: &AppState,
    after: Option<u64>,
    limit: Option<usize>,
) -> CompactEvents {
    let min_id = after.unwrap_or(0);
    let limit = limit.filter(|limit| *limit > 0);
    let engine = state.engine.lock().expect("engine lock poisoned");
    let latest_event_id = engine
        .events()
        .iter()
        .rev()
        .find(|event| event.id > min_id)
        .map(|event| event.id)
        .unwrap_or(min_id);
    let mut events: Vec<Event> = engine
        .events()
        .iter()
        .rev()
        .filter(|event| event.id > min_id)
        .filter(|event| !matches!(event.kind, EventKind::WorldTimeAdvanced { .. }))
        .take(limit.unwrap_or(usize::MAX))
        .cloned()
        .collect();
    events.reverse();
    CompactEvents {
        events,
        latest_event_id,
    }
}

fn events_for_query(
    state: &AppState,
    after: Option<u64>,
    limit: Option<usize>,
    compact: Option<&str>,
) -> Vec<Event> {
    if compact != Some("viewer") {
        return events_from_state(state, after, limit);
    }

    compact_viewer_events_from_state(state, after, limit).events
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

fn observe_agent_character(state: &AppState, character_id: &str) -> axum::response::Response {
    let response = state
        .engine
        .lock()
        .expect("engine lock poisoned")
        .observe_agent(character_id);
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
        engine.compact_history(state.history_limits.events, state.history_limits.commands);
        let snapshot = snapshot_for_storage(engine.state());
        let events = engine.events().to_vec();
        let commands = engine.command_log().to_vec();
        (response, snapshot, events, commands)
    };
    state.storage.save(&snapshot, &events, &commands).await?;
    notify_state_changed(state, snapshot.next_event_id);
    Ok(response)
}

fn notify_state_changed(state: &AppState, next_event_id: EventId) {
    let _ = state.event_signal.send(next_event_id);
}

fn stream_keepalive_seconds() -> u64 {
    env::var("FISHTANK_STREAM_KEEPALIVE_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value >= 15)
        .unwrap_or(DEFAULT_STREAM_KEEPALIVE_SECONDS)
}

fn snapshot_for_storage(snapshot: &WorldSnapshot) -> WorldSnapshot {
    let mut stored = snapshot.clone();
    stored.command_log.clear();
    stored
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

fn authorize_admin(state: &AppState, headers: &HeaderMap) -> Result<(), AppError> {
    let Some(admin_token) = &state.admin_token else {
        return Err(AppError::status(
            StatusCode::SERVICE_UNAVAILABLE,
            "admin token is not configured",
        ));
    };
    let provided = headers
        .get("x-fishtank-admin-token")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty());
    if provided == Some(admin_token.as_str()) {
        Ok(())
    } else {
        Err(AppError::status(
            StatusCode::UNAUTHORIZED,
            "invalid admin token",
        ))
    }
}

fn ensure_world(state: &AppState, world_id: &str) -> Result<(), AppError> {
    if state.legacy_world_id == world_id {
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

fn issue_agent_token_if_missing(headers: &HeaderMap) -> Option<String> {
    let has_token = headers
        .get("x-fishtank-agent-token")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| !value.trim().is_empty());
    if has_token {
        return None;
    }

    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    Some(format!(
        "ft_{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fishtank_protocol::{CommandResponse, EventKind};

    struct NoopStorage;

    #[async_trait]
    impl Storage for NoopStorage {
        async fn load(&self) -> Result<Option<storage::StoredState>> {
            Ok(None)
        }

        async fn save(
            &self,
            _snapshot: &WorldSnapshot,
            _events: &[Event],
            _commands: &[CommandEnvelope],
        ) -> Result<()> {
            Ok(())
        }

        async fn character_for_token(&self, _token_hash: &str) -> Result<Option<String>> {
            Ok(None)
        }

        async fn bind_token(&self, _token_hash: &str, _character_id: &str) -> Result<()> {
            Ok(())
        }

        async fn delete_tokens_for_character(&self, _character_id: &str) -> Result<u64> {
            Ok(0)
        }
    }

    fn test_state(engine: Engine) -> AppState {
        let legacy_world_id = engine.state().world_id.clone();
        let (event_signal, _) = watch::channel(engine.state().next_event_id);
        AppState {
            engine: Arc::new(Mutex::new(engine)),
            storage: Arc::new(NoopStorage),
            event_signal,
            gateway_secret: None,
            admin_token: None,
            legacy_world_id,
            history_limits: HistoryLimits {
                events: DEFAULT_EVENT_HISTORY_LIMIT,
                commands: DEFAULT_COMMAND_HISTORY_LIMIT,
            },
        }
    }

    fn world() -> WorldDefinition {
        serde_json::from_str(include_str!("../../../worlds/village.json")).unwrap()
    }

    fn create_character(engine: &mut Engine, id: &str) -> CommandResponse {
        engine.apply(CommandEnvelope {
            schema_version: SCHEMA_VERSION.to_string(),
            command_id: format!("cmd.{id}"),
            character_id: id.to_string(),
            submitted_at: time::OffsetDateTime::UNIX_EPOCH.to_string(),
            based_on_tick: None,
            valid_until_tick: None,
            local_state_hash: None,
            preconditions: Vec::new(),
            command: Command::CreateCharacter {
                name: id.to_string(),
                body_color: "#4ea1ff".to_string(),
                face_color: "#101820".to_string(),
            },
        })
    }

    #[test]
    fn compact_viewer_events_filter_tick_noise_before_limiting() {
        let mut engine = Engine::new(world()).unwrap();
        assert!(create_character(&mut engine, "char_one").ok);
        engine.advance_ticks(1);
        assert!(create_character(&mut engine, "char_two").ok);

        let state = test_state(engine);
        let events = events_for_query(&state, Some(0), Some(1), Some("viewer"));

        assert_eq!(events.len(), 1);
        assert!(!matches!(
            events[0].kind,
            EventKind::WorldTimeAdvanced { .. }
        ));
        assert!(matches!(events[0].kind, EventKind::CharacterCreated { .. }));
    }

    #[test]
    fn compact_viewer_event_scan_advances_past_filtered_tick_events() {
        let mut engine = Engine::new(world()).unwrap();
        assert!(create_character(&mut engine, "char_one").ok);
        let first_event_id = engine.events().last().unwrap().id;
        engine.advance_ticks(1);
        let latest_event_id = engine.events().last().unwrap().id;

        let state = test_state(engine);
        let compact = compact_viewer_events_from_state(&state, Some(first_event_id), None);

        assert!(compact.events.is_empty());
        assert_eq!(compact.latest_event_id, latest_event_id);
    }

    #[test]
    fn viewer_state_snapshot_excludes_static_world_payload() {
        let mut engine = Engine::new(world()).unwrap();
        assert!(create_character(&mut engine, "char_one").ok);

        let state = test_state(engine);
        let body = snapshot_json_for_query(&state, Some("viewer_state")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();

        assert!(value.get("world").is_none());
        assert!(value.get("characters").is_some());
        assert!(value.get("home_locks").is_some());
        assert_eq!(value["world_id"], "village");
    }
}
