use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use fishtank_protocol::{
    AuthenticatedCharacterRequest, Command, CommandEnvelope, Direction, HomeAction, MoveMode,
    NotificationAction, SCHEMA_VERSION, SpeechTarget,
};
use std::{env, path::PathBuf};
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
    Events(EventsArgs),
    Snapshot,
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
    Ack { notification_id: String },
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
                if let Some(token) = token.as_deref() {
                    let response = agent_request(
                        client.post(format!("{}/v1/character", cli.url)).json(
                            &AuthenticatedCharacterRequest {
                                name: args.name,
                                body_color: args.body_color,
                                face_color: args.face_color,
                            },
                        ),
                        token,
                    )
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<serde_json::Value>()
                    .await?;
                    print_json(response)?;
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
        Commands::Events(args) => {
            let mut request = if let Some(token) = token.as_deref() {
                agent_request(client.get(format!("{}/v1/events", cli.url)), token)
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
            let request = if let Some(token) = token.as_deref() {
                agent_request(
                    client.get(format!("{}/v1/worlds/village/snapshot", cli.url)),
                    token,
                )
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
        return print_json(response);
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
    print_json(response)
}

async fn get_observation(
    client: &reqwest::Client,
    url: &str,
    character_id: &str,
    token: Option<&str>,
) -> Result<serde_json::Value> {
    let request = if let Some(token) = token {
        agent_request(client.get(format!("{url}/v1/observe")), token)
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

fn agent_request(request: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    request.header("x-fishtank-agent-token", token)
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
