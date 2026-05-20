use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: &str = "fishtank.v1";
pub const TICKS_PER_REAL_SECOND: Tick = 5;
pub const TICKS_PER_INGAME_DAY: Tick = 6 * 60 * 60 * TICKS_PER_REAL_SECOND;
pub const OFFLINE_RETURN_HOME_TICKS: Tick = TICKS_PER_INGAME_DAY;

pub type CharacterId = String;
pub type CommandId = String;
pub type ConversationId = String;
pub type EntityId = String;
pub type EventId = u64;
pub type LocationId = String;
pub type NotificationId = String;
pub type PromiseId = String;
pub type Tick = u64;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct WorldDefinition {
    pub schema_version: String,
    pub id: String,
    pub name: String,
    pub seed: u64,
    pub grid: WorldGrid,
    pub starting_coins: u32,
    pub allowance_coins: u32,
    pub max_coins: u32,
    pub locations: Vec<LocationDefinition>,
    pub homes: Vec<HomeDefinition>,
    pub services: Vec<ServiceDefinition>,
    pub spawn_location_id: LocationId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct WorldGrid {
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_cell_size")]
    pub cell_size: u32,
    pub terrain: Vec<Vec<GroundType>>,
}

fn default_cell_size() -> u32 {
    1
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GroundType {
    Ground,
    Grass,
    Path,
    Water,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FacingDirection {
    North,
    South,
    East,
    West,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct GridPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct GridSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct LocationDefinition {
    pub id: LocationId,
    pub name: String,
    pub description: String,
    pub grid_position: GridPosition,
    pub grid_size: GridSize,
    pub facing: FacingDirection,
    pub exits: Vec<LocationId>,
    #[serde(default)]
    pub directional_exits: BTreeMap<Direction, LocationId>,
    #[serde(default)]
    pub poi_ids: Vec<EntityId>,
    #[serde(default)]
    pub private_home: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HomeDefinition {
    pub id: LocationId,
    pub name: String,
    pub owner_character_id: Option<CharacterId>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ServiceDefinition {
    pub id: EntityId,
    pub name: String,
    pub location_id: LocationId,
    pub item: String,
    pub price_coins: u32,
    pub duration_ticks: Tick,
    pub capacity: u32,
    #[serde(default = "default_queue_overflow")]
    pub overflow_behavior: String,
}

fn default_queue_overflow() -> String {
    "queue_nearby".to_string()
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Character {
    pub id: CharacterId,
    pub name: String,
    pub body_color: String,
    pub face_color: String,
    pub location_id: LocationId,
    pub home_id: LocationId,
    pub coins: u32,
    pub reserved_coins: u32,
    pub current_activity: Option<Activity>,
    #[serde(default)]
    pub queued_commands: Vec<QueuedCommand>,
    pub last_agent_action_tick: Tick,
    pub status: CharacterStatus,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacterStatus {
    Idle,
    Moving,
    Ordering,
    Waiting,
    InsideHome,
    OfflineReturningHome,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Activity {
    pub id: String,
    pub kind: ActivityKind,
    pub status: ActivityStatus,
    pub target_id: Option<EntityId>,
    #[serde(default)]
    pub movement_path: Vec<GridPosition>,
    pub started_at_tick: Tick,
    pub completes_at_tick: Tick,
    pub description: String,
    pub promise_id: Option<PromiseId>,
    #[serde(default)]
    pub reserved_coins: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    Moving,
    Ordering,
    Waiting,
    ReturningHome,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityStatus {
    Active,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Conversation {
    pub id: ConversationId,
    pub participant_ids: Vec<CharacterId>,
    pub recent_messages: Vec<SpeechMessage>,
    pub open: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SpeechMessage {
    pub speaker_id: CharacterId,
    pub target: SpeechTarget,
    pub text: String,
    pub tick: Tick,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "mode", content = "value")]
pub enum SpeechTarget {
    Room,
    Character(CharacterId),
    Shout,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct CommandEnvelope {
    pub schema_version: String,
    pub command_id: CommandId,
    pub character_id: CharacterId,
    pub submitted_at: String,
    pub based_on_tick: Option<Tick>,
    pub valid_until_tick: Option<Tick>,
    pub local_state_hash: Option<String>,
    #[serde(default)]
    pub preconditions: Vec<Precondition>,
    pub command: Command,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Precondition {
    pub entity: EntityId,
    pub condition: PreconditionKind,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreconditionKind {
    NearbyOrVisible,
    ActorAtLocation,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Command {
    CreateCharacter {
        name: String,
        body_color: String,
        face_color: String,
    },
    Observe,
    LookAt {
        target: EntityId,
    },
    Move {
        mode: MoveMode,
    },
    Say {
        target: SpeechTarget,
        text: String,
    },
    Order {
        service_id: EntityId,
        item: String,
    },
    Wait {
        ticks: Tick,
    },
    Queue {
        actions: Vec<QueuedCommand>,
    },
    HomeManual,
    Home {
        action: HomeAction,
    },
    Notifications {
        action: NotificationAction,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct QueuedCommand {
    pub command: QueueableCommand,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum QueueableCommand {
    Move { mode: MoveMode },
    Say { target: SpeechTarget, text: String },
    Order { service_id: EntityId, item: String },
    Wait { ticks: Tick },
    Home { action: HomeAction },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum MoveMode {
    ToTarget { target: EntityId },
    Direction { direction: Direction, distance: u32 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Forward,
    Back,
    Left,
    Right,
    North,
    South,
    East,
    West,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeAction {
    Enter,
    Leave,
    Lock,
    Unlock,
    ReturnHome,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum NotificationAction {
    List,
    Ack { notification_id: NotificationId },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct CommandResponse {
    pub ok: bool,
    pub accepted: bool,
    pub command_id: CommandId,
    pub tick: Tick,
    pub result: Option<CommandResult>,
    pub observation: Option<Observation>,
    pub error: Option<ApiError>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum CommandResult {
    CharacterCreated {
        character: Character,
    },
    ActivityStarted {
        activity_id: String,
        description: String,
        estimated_ticks: Tick,
        started_at_tick: Tick,
        completes_at_tick: Tick,
        #[serde(default)]
        movement_path: Vec<GridPosition>,
        promise: Option<Promise>,
    },
    MessageSpoken {
        conversation_id: ConversationId,
    },
    Waited {
        advanced_ticks: Tick,
    },
    LookedAt {
        entity: EntityView,
        description: String,
    },
    QueueAccepted {
        queued_count: usize,
        reserved_coins: u32,
    },
    HomeManual {
        manual: HomeManual,
    },
    HomeUpdated {
        home_id: LocationId,
        locked: bool,
        location_id: LocationId,
    },
    Notifications {
        notifications: Vec<Notification>,
    },
    NotificationAcknowledged {
        notification_id: NotificationId,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HomeManual {
    pub home_id: LocationId,
    pub owner_character_id: CharacterId,
    pub supported_actions: Vec<HomeAction>,
    pub locked: bool,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Promise {
    pub id: PromiseId,
    pub activity_id: String,
    pub trigger: String,
    pub estimated_ready_at_tick: Tick,
    pub resume_hint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, String>,
    pub retry_after_ticks: Option<Tick>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_actions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Observation {
    pub schema_version: String,
    pub observed_at_tick: Tick,
    pub valid_until_tick: Tick,
    pub local_state_hash: String,
    pub staleness_policy: String,
    pub actor: Character,
    pub location: LocationView,
    pub nearby_entities: Vec<EntityView>,
    pub conversations: Vec<Conversation>,
    pub available_actions: Vec<ActionView>,
    pub recent_events: Vec<Event>,
    pub notifications: Vec<Notification>,
    pub world_time: WorldTime,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct WorldTime {
    pub tick: Tick,
    pub ingame_day: u64,
    pub tick_of_day: Tick,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct LocationView {
    pub id: LocationId,
    pub name: String,
    pub description: String,
    pub exits: Vec<LocationId>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct EntityView {
    pub id: EntityId,
    pub entity_type: String,
    pub name: String,
    pub distance: String,
    pub available_actions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ActionView {
    pub action: String,
    pub targets: Vec<EntityId>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct WorldSnapshot {
    pub schema_version: String,
    pub world_id: String,
    pub tick: Tick,
    pub next_event_id: EventId,
    pub next_command_seq: u64,
    pub next_conversation_seq: u64,
    pub world: WorldDefinition,
    pub characters: BTreeMap<CharacterId, Character>,
    pub home_locks: BTreeMap<LocationId, bool>,
    pub conversations: BTreeMap<ConversationId, Conversation>,
    pub notifications: BTreeMap<NotificationId, Notification>,
    #[serde(default)]
    pub command_log: Vec<CommandEnvelope>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Event {
    pub schema_version: String,
    pub id: EventId,
    pub tick: Tick,
    pub kind: EventKind,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "event")]
pub enum EventKind {
    WorldLoaded {
        world_id: String,
    },
    WorldTimeAdvanced {
        from_tick: Tick,
        to_tick: Tick,
    },
    CharacterCreated {
        character_id: CharacterId,
        home_id: LocationId,
    },
    CharacterMoved {
        character_id: CharacterId,
        from: LocationId,
        to: LocationId,
    },
    MessageSpoken {
        conversation_id: ConversationId,
        speaker_id: CharacterId,
        target: SpeechTarget,
        text: String,
    },
    ActivityStarted {
        character_id: CharacterId,
        activity_id: String,
        description: String,
        started_at_tick: Tick,
        completes_at_tick: Tick,
        #[serde(default)]
        movement_path: Vec<GridPosition>,
    },
    WorldExpanded {
        world_id: String,
        block_id: String,
        homes_added: usize,
        services_added: usize,
        parks_added: usize,
    },
    ActivityCompleted {
        character_id: CharacterId,
        activity_id: String,
    },
    ActivityFailed {
        character_id: CharacterId,
        activity_id: String,
        reason: String,
    },
    QueueAccepted {
        character_id: CharacterId,
        queued_count: usize,
        reserved_coins: u32,
    },
    QueueStepStarted {
        character_id: CharacterId,
        remaining: usize,
    },
    QueueStepFailed {
        character_id: CharacterId,
        code: String,
    },
    PromiseCreated {
        promise: Promise,
    },
    PromiseResolved {
        promise_id: PromiseId,
        character_id: CharacterId,
        resume_hint: String,
    },
    CoinsReserved {
        character_id: CharacterId,
        amount: u32,
    },
    CoinsSpent {
        character_id: CharacterId,
        amount: u32,
    },
    CoinsReleased {
        character_id: CharacterId,
        amount: u32,
    },
    HomeLocked {
        character_id: CharacterId,
        home_id: LocationId,
    },
    HomeUnlocked {
        character_id: CharacterId,
        home_id: LocationId,
    },
    NotificationAcknowledged {
        character_id: CharacterId,
        notification_id: NotificationId,
    },
    CharacterSentHome {
        character_id: CharacterId,
        from: LocationId,
        to: LocationId,
    },
    CommandRejected {
        character_id: CharacterId,
        code: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Notification {
    pub notification_id: NotificationId,
    pub character_id: CharacterId,
    pub kind: String,
    pub priority: String,
    pub created_at_tick: Tick,
    pub expires_at_tick: Tick,
    pub summary: String,
    pub acknowledged: bool,
    pub related: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct TokenCharacter {
    pub token_id: String,
    pub character_id: CharacterId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct AuthenticatedCharacterRequest {
    pub name: String,
    pub body_color: String,
    pub face_color: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct LiveEnvelope {
    pub kind: String,
    pub world_id: String,
    pub snapshot: Option<WorldSnapshot>,
    #[serde(default)]
    pub events: Vec<Event>,
}
