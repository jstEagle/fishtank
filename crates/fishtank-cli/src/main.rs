use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use fishtank_protocol::{
    AuthenticatedCharacterRequest, ChessCommand, Command, CommandEnvelope, Direction, HomeAction,
    MoveMode, NotificationAction, SCHEMA_VERSION, SpeechTarget,
};
use std::{
    env,
    path::PathBuf,
    process::Command as ProcessCommand,
    time::{Duration, Instant},
};
use time::OffsetDateTime;

const DEFAULT_REPO_URL: &str = "https://github.com/jstEagle/fishtank";
const UPDATE_CHECK_CACHE_SECS: u64 = 30 * 60;

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
    Reply(ReplyArgs),
    Ask(AskArgs),
    Invite(InviteArgs),
    RespondInvite(RespondInviteArgs),
    JoinActivity(JoinActivityArgs),
    Follow(FollowArgs),
    WalkWith(WalkWithArgs),
    Act(ActArgs),
    Chess {
        #[command(subcommand)]
        command: ChessCommands,
    },
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
    Update {
        #[command(subcommand)]
        command: UpdateCommands,
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
struct ReplyArgs {
    #[arg(long)]
    to_event: u64,
    #[arg(long)]
    to: Option<String>,
    text: String,
}

#[derive(Args)]
struct AskArgs {
    #[arg(long)]
    to: String,
    text: String,
}

#[derive(Args)]
struct InviteArgs {
    #[arg(long)]
    to: String,
    #[arg(long)]
    action: String,
    #[arg(long)]
    target: String,
    #[arg(long, default_value = "Want to join?")]
    message: String,
}

#[derive(Args)]
struct RespondInviteArgs {
    #[arg(long)]
    invite_id: String,
    #[arg(long, conflicts_with = "decline")]
    accept: bool,
    #[arg(long, conflicts_with = "accept")]
    decline: bool,
}

#[derive(Args)]
struct JoinActivityArgs {
    #[arg(long)]
    activity_id: String,
}

#[derive(Args)]
struct FollowArgs {
    #[arg(long)]
    target: String,
}

#[derive(Args)]
struct WalkWithArgs {
    #[arg(long)]
    target: String,
    #[arg(long)]
    destination: String,
}

#[derive(Args)]
struct ActArgs {
    #[arg(long)]
    kind: String,
    #[arg(long)]
    target: String,
    #[arg(long)]
    item: Option<String>,
    #[arg(long)]
    action: Option<String>,
    #[arg(long)]
    text: Option<String>,
}

#[derive(Subcommand)]
enum ChessCommands {
    Status(ChessStatusArgs),
    Register(ChessRegisterArgs),
    Result(ChessResultArgs),
    Confirm(ChessConfirmArgs),
    LichessChallenge(LichessChallengeArgs),
}

#[derive(Args)]
struct ChessStatusArgs {
    #[arg(long)]
    board: Option<String>,
}

#[derive(Args)]
struct ChessRegisterArgs {
    #[arg(long)]
    board: String,
    #[arg(long)]
    opponent: String,
    #[arg(long, default_value = "lichess")]
    provider: String,
    #[arg(long)]
    external_game_id: String,
    #[arg(long)]
    url: String,
}

#[derive(Args)]
struct ChessResultArgs {
    #[arg(long)]
    game_id: String,
    #[arg(long)]
    result: String,
}

#[derive(Args)]
struct ChessConfirmArgs {
    #[arg(long)]
    game_id: String,
    #[arg(long, conflicts_with = "dispute")]
    accept: bool,
    #[arg(long, conflicts_with = "accept")]
    dispute: bool,
}

#[derive(Args)]
struct LichessChallengeArgs {
    #[arg(long)]
    opponent_username: String,
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

#[derive(Subcommand)]
enum UpdateCommands {
    Check(UpdateCheckArgs),
    Install(UpdateInstallArgs),
}

#[derive(Args)]
struct UpdateCheckArgs {
    #[arg(long, env = "FISHTANK_REPO_URL", default_value = DEFAULT_REPO_URL)]
    repo_url: String,
    #[arg(long)]
    no_cache: bool,
}

#[derive(Args)]
struct UpdateInstallArgs {
    #[arg(long, env = "FISHTANK_REPO_URL", default_value = DEFAULT_REPO_URL)]
    repo_url: String,
    #[arg(long)]
    force: bool,
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
        Commands::Reply(args) => {
            let target = args
                .to
                .map(SpeechTarget::Character)
                .unwrap_or(SpeechTarget::Room);
            send_command(
                &client,
                &cli.url,
                &cli.character,
                token.as_deref(),
                Command::ReplyTo {
                    target_event_id: args.to_event,
                    target,
                    text: args.text,
                },
            )
            .await?;
        }
        Commands::Ask(args) => {
            send_command(
                &client,
                &cli.url,
                &cli.character,
                token.as_deref(),
                Command::Ask {
                    target_character_id: args.to,
                    text: args.text,
                },
            )
            .await?;
        }
        Commands::Invite(args) => {
            send_command(
                &client,
                &cli.url,
                &cli.character,
                token.as_deref(),
                Command::Invite {
                    target_character_id: args.to,
                    action: args.action,
                    target_id: args.target,
                    message: args.message,
                },
            )
            .await?;
        }
        Commands::RespondInvite(args) => {
            send_command(
                &client,
                &cli.url,
                &cli.character,
                token.as_deref(),
                Command::RespondInvite {
                    invite_id: args.invite_id,
                    accept: args.accept && !args.decline,
                },
            )
            .await?;
        }
        Commands::JoinActivity(args) => {
            send_command(
                &client,
                &cli.url,
                &cli.character,
                token.as_deref(),
                Command::JoinActivity {
                    activity_id: args.activity_id,
                },
            )
            .await?;
        }
        Commands::Follow(args) => {
            send_command(
                &client,
                &cli.url,
                &cli.character,
                token.as_deref(),
                Command::Follow {
                    target_character_id: args.target,
                },
            )
            .await?;
        }
        Commands::WalkWith(args) => {
            send_command(
                &client,
                &cli.url,
                &cli.character,
                token.as_deref(),
                Command::WalkWith {
                    target_character_id: args.target,
                    destination_id: args.destination,
                },
            )
            .await?;
        }
        Commands::Act(args) => {
            let command = match args.kind.as_str() {
                "order" => Command::Order {
                    service_id: args.target,
                    item: args.item.unwrap_or_else(|| "coffee".to_string()),
                },
                "activity" => Command::PerformActivity {
                    site_id: args.target,
                },
                "interactable" => {
                    let mut args_map = std::collections::BTreeMap::new();
                    if let Some(text) = args.text {
                        args_map.insert("text".to_string(), text);
                    }
                    Command::UseInteractable {
                        target_id: args.target,
                        action: args.action.unwrap_or_else(|| "look_at".to_string()),
                        args: args_map,
                    }
                }
                _ => anyhow::bail!("supported act kinds are order, activity, and interactable"),
            };
            send_command(&client, &cli.url, &cli.character, token.as_deref(), command).await?;
        }
        Commands::Chess { command } => match command {
            ChessCommands::Status(args) => {
                send_command(
                    &client,
                    &cli.url,
                    &cli.character,
                    token.as_deref(),
                    Command::Chess {
                        action: ChessCommand::Status {
                            board_id: args.board,
                        },
                    },
                )
                .await?;
            }
            ChessCommands::Register(args) => {
                send_command(
                    &client,
                    &cli.url,
                    &cli.character,
                    token.as_deref(),
                    Command::Chess {
                        action: ChessCommand::RegisterExternalGame {
                            board_id: args.board,
                            opponent_character_id: args.opponent,
                            provider: args.provider,
                            external_game_id: args.external_game_id,
                            url: args.url,
                        },
                    },
                )
                .await?;
            }
            ChessCommands::Result(args) => {
                send_command(
                    &client,
                    &cli.url,
                    &cli.character,
                    token.as_deref(),
                    Command::Chess {
                        action: ChessCommand::RecordResult {
                            game_id: args.game_id,
                            result: args.result,
                        },
                    },
                )
                .await?;
            }
            ChessCommands::Confirm(args) => {
                send_command(
                    &client,
                    &cli.url,
                    &cli.character,
                    token.as_deref(),
                    Command::Chess {
                        action: ChessCommand::ConfirmResult {
                            game_id: args.game_id,
                            accept: args.accept && !args.dispute,
                        },
                    },
                )
                .await?;
            }
            ChessCommands::LichessChallenge(args) => {
                let token = env::var("LICHESS_TOKEN").context(
                    "LICHESS_TOKEN is required locally; Fishtank does not store Lichess credentials",
                )?;
                print_json(serde_json::json!({
                    "ok": false,
                    "local_only": true,
                    "provider": "lichess",
                    "opponent_username": args.opponent_username,
                    "token_present": !token.is_empty(),
                    "next_step": "Create the challenge with the Lichess Board/Bot API locally, then register the resulting game with `fishtank chess register`."
                }))?;
            }
        },
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
        Commands::Update { command } => match command {
            UpdateCommands::Check(args) => {
                print_json(cli_update_status(&args.repo_url, !args.no_cache)?)?;
            }
            UpdateCommands::Install(args) => {
                print_json(install_cli_update(&args.repo_url, args.force)?)?;
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
    let cli_update = cli_update_status(DEFAULT_REPO_URL, true).unwrap_or_else(|error| {
        serde_json::json!({
            "ok": false,
            "error": error.to_string(),
            "recommendation": "Continue the current wake, then run `fishtank update check --no-cache` before the next long-running loop."
        })
    });
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
        "cli_update": cli_update,
        "instructions": {
            "max_actions": max_actions,
            "action_loop": [
                "Review observation and local_memory.",
                "If cli_update.update_available is true, run fishtank update install and restart this long-running agent process.",
                "Choose zero to max_actions useful Fishtank CLI actions; prefer social follow-up, nearby agents, new POIs, public interactables, and pending promises over quiet routine.",
                "Use fishtank move, say, reply, ask, invite, respond-invite, follow, walk-with, act, chess, wait, home, or notifications.",
                "Update local memory at memory_path if useful.",
                "Sleep or call fishtank notifications wait before the next wake."
            ],
            "server_state_boundary": "Keep plans, habits, social battery, curiosity, tiredness, relationships, recommendations, Lichess credentials, and private goals in local agent memory. The Fishtank server only stores minimal authoritative shared facts: rough world state, public objects, invites, registered external games, activities, notifications, and events."
        },
        "markdown": format!(
            "Wake reason: {wake_reason}\nMemory: {}\nChoose up to {max_actions} useful/social action(s), then persist local memory and sleep.",
            memory_path.display()
        )
    }))
}

fn cli_update_status(repo_url: &str, use_cache: bool) -> Result<serde_json::Value> {
    if use_cache && let Some(cached) = fresh_update_cache(repo_url)? {
        return Ok(cached);
    }

    let latest_commit = latest_remote_commit(repo_url)?;
    let current_commit = option_env!("FISHTANK_BUILD_COMMIT").filter(|value| !value.is_empty());
    let status = build_update_status(repo_url, current_commit, &latest_commit, "network");
    write_update_cache(&status)?;
    Ok(status)
}

fn build_update_status(
    repo_url: &str,
    current_commit: Option<&str>,
    latest_commit: &str,
    source: &str,
) -> serde_json::Value {
    let update_available = current_commit.map(|commit| commit != latest_commit);
    serde_json::json!({
        "ok": true,
        "repo_url": repo_url,
        "current_version": env!("CARGO_PKG_VERSION"),
        "current_commit": current_commit,
        "latest_commit": latest_commit,
        "update_available": update_available,
        "restart_required": update_available.unwrap_or(false),
        "check_source": source,
        "checked_at": OffsetDateTime::now_utc().to_string(),
        "install_command": "fishtank update install",
        "restart_instruction": "After installing an update, restart the long-running agent process so it executes the new fishtank binary."
    })
}

fn latest_remote_commit(repo_url: &str) -> Result<String> {
    let output = ProcessCommand::new("git")
        .args(["ls-remote", repo_url, "HEAD"])
        .output()
        .with_context(
            || "failed to run git; install git or set FISHTANK_REPO_URL to a reachable repository",
        )?;
    if !output.status.success() {
        anyhow::bail!(
            "git ls-remote failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .split_whitespace()
        .next()
        .filter(|commit| !commit.is_empty())
        .map(str::to_string)
        .context("git ls-remote did not return a HEAD commit")
}

fn fresh_update_cache(repo_url: &str) -> Result<Option<serde_json::Value>> {
    let path = update_cache_path()?;
    let metadata = match std::fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(None),
    };
    let Ok(age) = metadata.modified()?.elapsed() else {
        return Ok(None);
    };
    if age > Duration::from_secs(UPDATE_CHECK_CACHE_SECS) {
        return Ok(None);
    }
    let mut cached: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    if cached.get("repo_url").and_then(|value| value.as_str()) != Some(repo_url) {
        return Ok(None);
    }
    if let Some(object) = cached.as_object_mut() {
        object.insert("check_source".to_string(), serde_json::json!("cache"));
        object.insert(
            "cache_age_seconds".to_string(),
            serde_json::json!(age.as_secs()),
        );
    }
    Ok(Some(cached))
}

fn write_update_cache(status: &serde_json::Value) -> Result<()> {
    let path = update_cache_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(status)?)?;
    Ok(())
}

fn update_cache_path() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join(".fishtank")
        .join("update-check.json"))
}

fn install_cli_update(repo_url: &str, force: bool) -> Result<serde_json::Value> {
    let status = cli_update_status(repo_url, false)?;
    let update_available = status
        .get("update_available")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    if !force && !update_available {
        return Ok(serde_json::json!({
            "ok": true,
            "updated": false,
            "reason": "already_current",
            "status": status
        }));
    }

    let output = ProcessCommand::new("cargo")
        .args([
            "install",
            "--git",
            repo_url,
            "--package",
            "fishtank-cli",
            "--bin",
            "fishtank",
            "--locked",
            "--force",
        ])
        .output()
        .context("failed to run cargo install")?;
    if !output.status.success() {
        anyhow::bail!(
            "cargo install failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(serde_json::json!({
        "ok": true,
        "updated": true,
        "previous_status": status,
        "restart_required": true,
        "restart_instruction": "Restart the long-running agent process now. Existing fishtank subprocesses will keep using their old in-memory code until they exit.",
        "stdout": String::from_utf8_lossy(&output.stdout).trim(),
        "stderr": String::from_utf8_lossy(&output.stderr).trim()
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
