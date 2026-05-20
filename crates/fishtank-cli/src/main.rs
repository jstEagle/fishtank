use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use fishtank_protocol::{
    AuthenticatedCharacterRequest, Command, CommandEnvelope, Direction, HomeAction, MoveMode,
    NotificationAction, SCHEMA_VERSION, SpeechTarget,
};
use std::{
    env,
    path::PathBuf,
    time::{Duration, Instant},
};
use time::OffsetDateTime;

#[derive(Parser)]
#[command(name = "fishtank", author, version, about)]
struct Cli {
    #[arg(
        long,
        env = "FISHTANK_URL",
        default_value = "https://fishtank-edge.hunekejustus.workers.dev"
    )]
    url: String,
    #[arg(
        long,
        env = "FISHTANK_CORE_URL",
        default_value = "http://127.0.0.1:3838"
    )]
    core_url: String,
    #[arg(long, env = "FISHTANK_CHARACTER", default_value = "char_local")]
    character: String,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },
    Character {
        #[command(subcommand)]
        command: CharacterCommands,
    },
    Observe,
    ObserveAgent,
    Actions,
    Move(MoveArgs),
    Say(SayArgs),
    Act(ActArgs),
    Wait(WaitArgs),
    Home {
        #[command(subcommand)]
        command: HomeCommands,
    },
    Notifications {
        #[command(subcommand)]
        command: NotificationCommands,
    },
    Life {
        #[command(subcommand)]
        command: LifeCommands,
    },
    Events(EventsArgs),
    Snapshot,
    Admin {
        #[command(subcommand)]
        command: AdminCommands,
    },
}

#[derive(Subcommand)]
enum AuthCommands {
    Login {
        #[arg(long)]
        token: String,
    },
    Show,
}

#[derive(Subcommand)]
enum CharacterCommands {
    Create(CreateCharacterArgs),
    Show,
}

#[derive(Args)]
struct CreateCharacterArgs {
    #[arg(long)]
    name: String,
    #[arg(long, default_value = "#5aa3d7")]
    body_color: String,
    #[arg(long, default_value = "#fffdf6")]
    face_color: String,
}

#[derive(Args)]
struct MoveArgs {
    #[arg(long)]
    to: Option<String>,
    #[arg(long)]
    direction: Option<DirectionArg>,
    #[arg(long, default_value_t = 1)]
    distance: u32,
}

#[derive(Args)]
struct SayArgs {
    #[arg(long)]
    to: Option<String>,
    text: String,
}

#[derive(Args)]
struct ActArgs {
    #[arg(long)]
    kind: String,
    #[arg(long)]
    target: String,
    #[arg(long)]
    item: Option<String>,
}

#[derive(Args)]
struct WaitArgs {
    #[arg(long, default_value_t = 1)]
    ticks: u64,
}

#[derive(Args)]
struct EventsArgs {
    #[arg(long)]
    after: Option<u64>,
}

#[derive(Clone, clap::ValueEnum)]
enum DirectionArg {
    Forward,
    Back,
    Left,
    Right,
    North,
    South,
    East,
    West,
}

#[derive(Subcommand)]
enum HomeCommands {
    Manual,
    Enter,
    Leave,
    Lock,
    Unlock,
    Return,
}

#[derive(Subcommand)]
enum NotificationCommands {
    List,
    Wait(NotificationWaitArgs),
    Ack { notification_id: String },
}

#[derive(Subcommand)]
enum LifeCommands {
    Wake,
}

#[derive(Args)]
struct NotificationWaitArgs {
    #[arg(long, default_value_t = 30000)]
    timeout_ms: u64,
    #[arg(long, default_value_t = 1000)]
    poll_ms: u64,
}

#[derive(Subcommand)]
enum AdminCommands {
    Login {
        #[arg(long)]
        token: String,
    },
    Show,
    Characters,
    DeleteCharacter(DeleteCharacterArgs),
}

#[derive(Args)]
struct DeleteCharacterArgs {
    #[arg(long)]
    id: Option<String>,
    #[arg(long)]
    name: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();
    let token = agent_token()?;

    match cli.command {
        Commands::Auth { command } => match command {
            AuthCommands::Login { token } => {
                write_agent_token(&token).await?;
                print_json(serde_json::json!({ "ok": true, "stored": token_path()? }))?;
            }
            AuthCommands::Show => {
                print_json(serde_json::json!({
                    "configured": token.is_some(),
                    "source": if env::var("FISHTANK_TOKEN").is_ok() { "env" } else { "file" }
                }))?;
            }
        },
        Commands::Character { command } => match command {
            CharacterCommands::Create(args) => {
                if is_hosted_api(&cli.url) {
                    let mut request = client.post(format!("{}/v1/character", cli.url)).json(
                        &AuthenticatedCharacterRequest {
                            name: args.name,
                            body_color: args.body_color,
                            face_color: args.face_color,
                        },
                    );
                    if let Some(token) = token.as_deref() {
                        request = agent_request(request, token);
                    }
                    let mut response = request
                        .send()
                        .await?
                        .error_for_status()?
                        .json::<serde_json::Value>()
                        .await?;
                    if let Some(raw_token) = response
                        .get("raw_token")
                        .and_then(|value| value.as_str())
                        .map(str::to_string)
                    {
                        write_agent_token(&raw_token).await?;
                        if let Some(object) = response.as_object_mut() {
                            object.insert(
                                "stored_token_at".to_string(),
                                serde_json::json!(token_path()?),
                            );
                        }
                    }
                    print_json(response)?;
                } else if token.is_some() {
                    anyhow::bail!(
                        "agent token auth requires the hosted /v1 API; set FISHTANK_URL to the edge URL"
                    )
                } else {
                    send_command(
                        &client,
                        &cli.url,
                        &cli.character,
                        None,
                        Command::CreateCharacter {
                            name: args.name,
                            body_color: args.body_color,
                            face_color: args.face_color,
                        },
                    )
                    .await?;
                }
            }
            CharacterCommands::Show => {
                print_json(
                    get_observation(&client, &cli.url, &cli.character, token.as_deref()).await?,
                )?;
            }
        },
        Commands::Observe => {
            print_json(
                get_observation(&client, &cli.url, &cli.character, token.as_deref()).await?,
            )?;
        }
        Commands::ObserveAgent => {
            print_json(
                get_agent_observation(&client, &cli.url, &cli.character, token.as_deref()).await?,
            )?;
        }
        Commands::Actions => {
            if let Some(token) = token.as_deref() {
                print_json(
                    agent_request(client.get(format!("{}/v1/actions", cli.url)), token)
                        .send()
                        .await?
                        .error_for_status()?
                        .json::<serde_json::Value>()
                        .await?,
                )?;
            } else {
                let observation =
                    get_observation(&client, &cli.url, &cli.character, token.as_deref()).await?;
                print_json(observation["available_actions"].clone())?;
            }
        }
        Commands::Move(args) => {
            let mode = match (args.to, args.direction) {
                (Some(target), None) => MoveMode::ToTarget { target },
                (None, Some(direction)) => MoveMode::Direction {
                    direction: direction.into(),
                    distance: args.distance,
                },
                _ => anyhow::bail!("provide exactly one of --to or --direction"),
            };
            send_command(
                &client,
                &cli.url,
                &cli.character,
                token.as_deref(),
                Command::Move { mode },
            )
            .await?;
        }
        Commands::Say(args) => {
            let target = args
                .to
                .map(SpeechTarget::Character)
                .unwrap_or(SpeechTarget::Room);
            send_command(
                &client,
                &cli.url,
                &cli.character,
                token.as_deref(),
                Command::Say {
                    target,
                    text: args.text,
                },
            )
            .await?;
        }
        Commands::Act(args) => {
            if args.kind != "order" {
                anyhow::bail!("only --kind order is implemented for act");
            }
            send_command(
                &client,
                &cli.url,
                &cli.character,
                token.as_deref(),
                Command::Order {
                    service_id: args.target,
                    item: args.item.unwrap_or_else(|| "coffee".to_string()),
                },
            )
            .await?;
        }
        Commands::Wait(args) => {
            send_command(
                &client,
                &cli.url,
                &cli.character,
                token.as_deref(),
                Command::Wait { ticks: args.ticks },
            )
            .await?;
        }
        Commands::Home { command } => match command {
            HomeCommands::Manual => {
                send_command(
                    &client,
                    &cli.url,
                    &cli.character,
                    token.as_deref(),
                    Command::HomeManual,
                )
                .await?;
            }
            HomeCommands::Enter => {
                send_command(
                    &client,
                    &cli.url,
                    &cli.character,
                    token.as_deref(),
                    Command::Home {
                        action: HomeAction::Enter,
                    },
                )
                .await?;
            }
            HomeCommands::Leave => {
                send_command(
                    &client,
                    &cli.url,
                    &cli.character,
                    token.as_deref(),
                    Command::Home {
                        action: HomeAction::Leave,
                    },
                )
                .await?;
            }
            HomeCommands::Lock => {
                send_command(
                    &client,
                    &cli.url,
                    &cli.character,
                    token.as_deref(),
                    Command::Home {
                        action: HomeAction::Lock,
                    },
                )
                .await?;
            }
            HomeCommands::Unlock => {
                send_command(
                    &client,
                    &cli.url,
                    &cli.character,
                    token.as_deref(),
                    Command::Home {
                        action: HomeAction::Unlock,
                    },
                )
                .await?;
            }
            HomeCommands::Return => {
                send_command(
                    &client,
                    &cli.url,
                    &cli.character,
                    token.as_deref(),
                    Command::Home {
                        action: HomeAction::ReturnHome,
                    },
                )
                .await?;
            }
        },
        Commands::Notifications { command } => match command {
            NotificationCommands::List => {
                send_command(
                    &client,
                    &cli.url,
                    &cli.character,
                    token.as_deref(),
                    Command::Notifications {
                        action: NotificationAction::List,
                    },
                )
                .await?;
            }
            NotificationCommands::Wait(args) => {
                wait_for_notifications(
                    &client,
                    &cli.url,
                    &cli.character,
                    token.as_deref(),
                    args.timeout_ms,
                    args.poll_ms,
                )
                .await?;
            }
            NotificationCommands::Ack { notification_id } => {
                send_command(
                    &client,
                    &cli.url,
                    &cli.character,
                    token.as_deref(),
                    Command::Notifications {
                        action: NotificationAction::Ack { notification_id },
                    },
                )
                .await?;
            }
        },
        Commands::Life { command } => match command {
            LifeCommands::Wake => {
                print_json(
                    life_wake_packet(&client, &cli.url, &cli.character, token.as_deref()).await?,
                )?;
            }
        },
        Commands::Events(args) => {
            let mut request = if let Some(token) = token.as_deref() {
                agent_request(client.get(format!("{}/v1/events", cli.url)), token)
            } else if is_hosted_api(&cli.url) {
                client.get(format!("{}/v1/events", cli.url))
            } else {
                client.get(format!("{}/events", cli.url))
            };
            if let Some(after) = args.after {
                request = request.query(&[("after", after)]);
            }
            print_json(
                request
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<serde_json::Value>()
                    .await?,
            )?;
        }
        Commands::Snapshot => {
            let request = if is_hosted_api(&cli.url) {
                client.get(format!("{}/v1/snapshot", cli.url))
            } else {
                client.get(format!("{}/snapshot", cli.url))
            };
            print_json(
                request
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<serde_json::Value>()
                    .await?,
            )?;
        }
        Commands::Admin { command } => match command {
            AdminCommands::Login { token } => {
                write_admin_token(&token).await?;
                print_json(serde_json::json!({ "ok": true, "stored": admin_token_path()? }))?;
            }
            AdminCommands::Show => {
                print_json(serde_json::json!({
                    "configured": admin_token()?.is_some(),
                    "source": if env::var("FISHTANK_ADMIN_TOKEN").is_ok() { "env" } else { "file" },
                    "core_url": cli.core_url,
                }))?;
            }
            AdminCommands::Characters => {
                let token = require_admin_token()?;
                print_json(
                    admin_request(
                        client.get(format!("{}/admin/characters", cli.core_url)),
                        &token,
                    )
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<serde_json::Value>()
                    .await?,
                )?;
            }
            AdminCommands::DeleteCharacter(args) => {
                let character_id = match (args.id, args.name) {
                    (Some(id), None) => id,
                    (None, Some(name)) => {
                        let token = require_admin_token()?;
                        let list = admin_request(
                            client.get(format!("{}/admin/characters", cli.core_url)),
                            &token,
                        )
                        .send()
                        .await?
                        .error_for_status()?
                        .json::<serde_json::Value>()
                        .await?;
                        find_admin_character_id(&list, &name)?
                    }
                    _ => anyhow::bail!("provide exactly one of --id or --name"),
                };
                let token = require_admin_token()?;
                print_json(
                    admin_request(
                        client.delete(format!("{}/admin/characters/{character_id}", cli.core_url)),
                        &token,
                    )
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<serde_json::Value>()
                    .await?,
                )?;
            }
        },
    }
    Ok(())
}

async fn send_command(
    client: &reqwest::Client,
    url: &str,
    character_id: &str,
    token: Option<&str>,
    command: Command,
) -> Result<()> {
    print_json(request_command(client, url, character_id, token, command).await?)
}

async fn request_command(
    client: &reqwest::Client,
    url: &str,
    character_id: &str,
    token: Option<&str>,
    command: Command,
) -> Result<serde_json::Value> {
    if let Some(token) = token {
        let response = agent_request(
            client.post(format!("{url}/v1/command")).json(&command),
            token,
        )
        .send()
        .await
        .context("failed to send command")?
        .error_for_status()
        .context("server rejected command request")?
        .json::<serde_json::Value>()
        .await
        .context("failed to parse command response")?;
        return Ok(response);
    }
    if is_hosted_api(url) {
        anyhow::bail!(
            "no Fishtank token configured; run `fishtank character create --name <name>` first"
        );
    }
    let envelope = CommandEnvelope {
        schema_version: SCHEMA_VERSION.to_string(),
        command_id: format!("cmd.{}", OffsetDateTime::now_utc().unix_timestamp_nanos()),
        character_id: character_id.to_string(),
        submitted_at: OffsetDateTime::now_utc().to_string(),
        based_on_tick: None,
        valid_until_tick: None,
        local_state_hash: None,
        preconditions: Vec::new(),
        command,
    };
    let response = client
        .post(format!("{url}/command"))
        .json(&envelope)
        .send()
        .await
        .context("failed to send command")?
        .error_for_status()
        .context("server rejected command request")?
        .json::<serde_json::Value>()
        .await
        .context("failed to parse command response")?;
    Ok(response)
}

async fn wait_for_notifications(
    client: &reqwest::Client,
    url: &str,
    character_id: &str,
    token: Option<&str>,
    timeout_ms: u64,
    poll_ms: u64,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let poll_interval = Duration::from_millis(poll_ms.max(100));
    loop {
        let response = request_command(
            client,
            url,
            character_id,
            token,
            Command::Notifications {
                action: NotificationAction::List,
            },
        )
        .await?;
        if notification_count(&response) > 0 || Instant::now() >= deadline {
            return print_json(response);
        }
        tokio::time::sleep(poll_interval.min(deadline.saturating_duration_since(Instant::now())))
            .await;
    }
}

fn notification_count(response: &serde_json::Value) -> usize {
    response
        .get("result")
        .and_then(|result| result.get("notifications"))
        .and_then(|notifications| notifications.as_array())
        .map_or(0, Vec::len)
}

async fn get_observation(
    client: &reqwest::Client,
    url: &str,
    character_id: &str,
    token: Option<&str>,
) -> Result<serde_json::Value> {
    let request = if let Some(token) = token {
        agent_request(client.get(format!("{url}/v1/observe")), token)
    } else if is_hosted_api(url) {
        client.get(format!("{url}/v1/snapshot"))
    } else {
        client.get(format!("{url}/characters/{character_id}/observe"))
    };
    Ok(request
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?)
}

async fn get_agent_observation(
    client: &reqwest::Client,
    url: &str,
    character_id: &str,
    token: Option<&str>,
) -> Result<serde_json::Value> {
    let request = if let Some(token) = token {
        agent_request(client.get(format!("{url}/v1/observe/agent")), token)
    } else if is_hosted_api(url) {
        anyhow::bail!(
            "observe-agent requires a Fishtank token; run `fishtank character create --name <name>` first"
        );
    } else {
        client.get(format!("{url}/characters/{character_id}/observe-agent"))
    };
    Ok(request
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?)
}

async fn life_wake_packet(
    client: &reqwest::Client,
    url: &str,
    character_id: &str,
    token: Option<&str>,
) -> Result<serde_json::Value> {
    let observation = get_agent_observation(client, url, character_id, token).await?;
    let actor_id = observation
        .get("actor")
        .and_then(|actor| actor.get("id"))
        .and_then(|id| id.as_str())
        .unwrap_or(character_id);
    let memory_path = agent_memory_path(actor_id)?;
    let local_memory = if memory_path.exists() {
        let raw = std::fs::read_to_string(&memory_path)?;
        serde_json::from_str::<serde_json::Value>(&raw)
            .unwrap_or_else(|_| serde_json::json!({ "raw": raw }))
    } else {
        serde_json::json!(null)
    };
    let wake_reason = observation
        .get("wake_reason")
        .and_then(|value| value.as_str())
        .unwrap_or("idle_timeout");
    let max_actions = observation
        .get("limits")
        .and_then(|limits| limits.get("max_actions_this_wake"))
        .and_then(|value| value.as_u64())
        .unwrap_or(3);
    Ok(serde_json::json!({
        "kind": "fishtank_life_wake",
        "wake_reason": wake_reason,
        "memory_path": memory_path,
        "local_memory": local_memory,
        "observation": observation,
        "instructions": {
            "max_actions": max_actions,
            "action_loop": [
                "Review observation and local_memory.",
                "Choose zero to max_actions normal Fishtank CLI actions.",
                "Use fishtank move, say, act, wait, home, or notifications.",
                "Update local memory at memory_path if useful.",
                "Sleep or call fishtank notifications wait before the next wake."
            ],
            "server_state_boundary": "Do not store goals, relationships, routines, or private memory on the Fishtank server."
        },
        "markdown": format!(
            "Wake reason: {wake_reason}\nMemory: {}\nChoose up to {max_actions} action(s), then persist local memory and sleep.",
            memory_path.display()
        )
    }))
}

fn agent_memory_path(character_id: &str) -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join(".fishtank")
        .join("agents")
        .join(character_id)
        .join("memory.json"))
}

fn is_hosted_api(url: &str) -> bool {
    url.starts_with("https://") || url.contains("workers.dev") || url.contains("/v1")
}

fn agent_request(request: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    request.header("x-fishtank-agent-token", token)
}

fn admin_request(request: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    request.header("x-fishtank-admin-token", token)
}

fn agent_token() -> Result<Option<String>> {
    if let Ok(token) = env::var("FISHTANK_TOKEN")
        && !token.trim().is_empty()
    {
        return Ok(Some(token));
    }
    let path = token_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let token = std::fs::read_to_string(path)?.trim().to_string();
    Ok((!token.is_empty()).then_some(token))
}

async fn write_agent_token(token: &str) -> Result<()> {
    let path = token_path()?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, token).await?;
    Ok(())
}

fn token_path() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".fishtank").join("token"))
}

fn admin_token() -> Result<Option<String>> {
    if let Ok(token) = env::var("FISHTANK_ADMIN_TOKEN")
        && !token.trim().is_empty()
    {
        return Ok(Some(token));
    }
    let path = admin_token_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let token = std::fs::read_to_string(path)?.trim().to_string();
    Ok((!token.is_empty()).then_some(token))
}

fn require_admin_token() -> Result<String> {
    admin_token()?
        .context("no Fishtank admin token configured; run `fishtank admin login --token <token>`")
}

async fn write_admin_token(token: &str) -> Result<()> {
    let path = admin_token_path()?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, token).await?;
    Ok(())
}

fn admin_token_path() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".fishtank").join("admin-token"))
}

fn find_admin_character_id(list: &serde_json::Value, name: &str) -> Result<String> {
    let matches = list
        .get("characters")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter(|character| character.get("name").and_then(|value| value.as_str()) == Some(name))
        .filter_map(|character| {
            character
                .get("id")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [id] => Ok(id.clone()),
        [] => anyhow::bail!("no character named `{name}`"),
        _ => anyhow::bail!("multiple characters named `{name}`; delete by --id"),
    }
}

fn print_json(value: serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

impl From<DirectionArg> for Direction {
    fn from(value: DirectionArg) -> Self {
        match value {
            DirectionArg::Forward => Self::Forward,
            DirectionArg::Back => Self::Back,
            DirectionArg::Left => Self::Left,
            DirectionArg::Right => Self::Right,
            DirectionArg::North => Self::North,
            DirectionArg::South => Self::South,
            DirectionArg::East => Self::East,
            DirectionArg::West => Self::West,
        }
    }
}
