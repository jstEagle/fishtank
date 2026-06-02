export const SCHEMA_VERSION = "fishtank.v1";

export type Tick = number;
export type EventId = number;

export type Direction =
  | "forward"
  | "back"
  | "left"
  | "right"
  | "north"
  | "south"
  | "east"
  | "west";

export interface WorldDefinition {
  schema_version: string;
  id: string;
  name: string;
  seed: number;
  grid: WorldGrid;
  starting_coins: number;
  allowance_coins: number;
  max_coins: number;
  locations: LocationDefinition[];
  homes: HomeDefinition[];
  services: ServiceDefinition[];
  activity_sites: ActivitySiteDefinition[];
  interactables: PublicInteractableDefinition[];
  spawn_location_id: string;
}

export interface WorldGrid {
  width: number;
  height: number;
  cell_size: number;
  terrain: GroundType[][];
}

export type GroundType = "ground" | "grass" | "path" | "water";

export interface GridPosition {
  x: number;
  y: number;
}

export interface GridSize {
  width: number;
  height: number;
}

export interface LocationDefinition {
  id: string;
  name: string;
  description: string;
  grid_position: GridPosition;
  grid_size: GridSize;
  facing: FacingDirection;
  exits: string[];
  directional_exits: Partial<Record<Direction, string>>;
  poi_ids: string[];
  private_home: boolean;
}

export type FacingDirection = "north" | "south" | "east" | "west";

export interface HomeDefinition {
  id: string;
  name: string;
  owner_character_id: string | null;
}

export interface ServiceDefinition {
  id: string;
  name: string;
  location_id: string;
  item: string;
  description: string;
  price_coins: number;
  duration_ticks: Tick;
  capacity: number;
  overflow_behavior: string;
}

export interface ActivitySiteDefinition {
  id: string;
  name: string;
  location_id: string;
  action: string;
  description: string;
  duration_ticks: Tick;
  coin_reward: number;
}

export interface PublicInteractableDefinition {
  id: string;
  name: string;
  location_id: string;
  description: string;
  actions: string[];
  price_coins: number;
  reward_coins: number;
  duration_ticks: Tick;
  capacity: number;
  public_state: Record<string, string>;
}

export interface Character {
  id: string;
  name: string;
  body_color: string;
  face_color: string;
  location_id: string;
  home_id: string;
  coins: number;
  reserved_coins: number;
  current_activity: Activity | null;
  queued_commands: QueuedCommand[];
  last_agent_action_tick: Tick;
  status: CharacterStatus;
}

export type CharacterStatus =
  | "idle"
  | "moving"
  | "ordering"
  | "performing"
  | "waiting"
  | "inside_home"
  | "offline_returning_home";

export interface Activity {
  id: string;
  kind: "moving" | "ordering" | "performing" | "waiting" | "returning_home";
  status: "active" | "completed" | "failed";
  target_id: string | null;
  movement_path: GridPosition[];
  started_at_tick: Tick;
  completes_at_tick: Tick;
  description: string;
  promise_id: string | null;
  reserved_coins: number;
  queued: boolean;
}

export interface QueuedCommand {
  command: Record<string, unknown>;
}

export interface SpeechMessage {
  speaker_id: string;
  target: SpeechTarget;
  text: string;
  tick: Tick;
}

export type SpeechTarget =
  | { mode: "room" }
  | { mode: "character"; value: string }
  | { mode: "shout" };

export interface Conversation {
  id: string;
  participant_ids: string[];
  recent_messages: SpeechMessage[];
  open: boolean;
}

export interface Notification {
  notification_id: string;
  character_id: string;
  kind: string;
  priority: string;
  created_at_tick: Tick;
  expires_at_tick: Tick;
  summary: string;
  acknowledged: boolean;
  related: Record<string, string>;
}

export interface WorldSnapshot {
  schema_version: string;
  world_id: string;
  tick: Tick;
  next_event_id: EventId;
  next_command_seq: number;
  next_conversation_seq: number;
  world: WorldDefinition;
  characters: Record<string, Character>;
  home_locks: Record<string, boolean>;
  conversations: Record<string, Conversation>;
  notifications: Record<string, Notification>;
  command_log: unknown[];
}

export type ViewerStateSnapshot = Pick<
  WorldSnapshot,
  | "schema_version"
  | "world_id"
  | "tick"
  | "next_event_id"
  | "next_command_seq"
  | "next_conversation_seq"
  | "characters"
  | "home_locks"
>;

export type Command =
  | {
      kind: "create_character";
      name: string;
      body_color: string;
      face_color: string;
    }
  | { kind: "observe" }
  | { kind: "look_at"; target: string }
  | { kind: "move"; mode: MoveMode }
  | { kind: "say"; target: SpeechTarget; text: string }
  | { kind: "reply_to"; target_event_id: number; target: SpeechTarget; text: string }
  | { kind: "ask"; target_character_id: string; text: string }
  | {
      kind: "invite";
      target_character_id: string;
      action: string;
      target_id: string;
      message: string;
    }
  | { kind: "respond_invite"; invite_id: string; accept: boolean }
  | { kind: "join_activity"; activity_id: string }
  | { kind: "follow"; target_character_id: string }
  | { kind: "walk_with"; target_character_id: string; destination_id: string }
  | {
      kind: "use_interactable";
      target_id: string;
      action: string;
      args: Record<string, string>;
    }
  | { kind: "chess"; action: ChessCommand }
  | { kind: "order"; service_id: string; item: string }
  | { kind: "perform_activity"; site_id: string }
  | { kind: "wait"; ticks: Tick }
  | { kind: "home_manual" }
  | { kind: "home"; action: HomeAction }
  | { kind: "notifications"; action: NotificationAction };

export type MoveMode =
  | { mode: "to_target"; target: string }
  | { mode: "direction"; direction: Direction; distance: number };

export type HomeAction = "enter" | "leave" | "lock" | "unlock" | "return_home";

export type NotificationAction =
  | { mode: "list" }
  | { mode: "ack"; notification_id: string };

export type ChessCommand =
  | { mode: "status"; board_id: string | null }
  | {
      mode: "register_external_game";
      board_id: string;
      opponent_character_id: string;
      provider: string;
      external_game_id: string;
      url: string;
    }
  | { mode: "record_result"; game_id: string; result: string }
  | { mode: "confirm_result"; game_id: string; accept: boolean };

export interface CommandEnvelope {
  schema_version: string;
  command_id: string;
  character_id: string;
  submitted_at: string;
  based_on_tick: Tick | null;
  valid_until_tick: Tick | null;
  local_state_hash: string | null;
  preconditions: unknown[];
  command: Command;
}

export interface CommandResponse {
  ok: boolean;
  accepted: boolean;
  command_id: string;
  tick: Tick;
  result: unknown | null;
  observation: unknown | null;
  error: {
    code: string;
    message: string;
    details?: Record<string, string>;
    retry_after_ticks?: Tick | null;
    suggested_actions?: string[];
  } | null;
}

export interface EventRecord {
  schema_version: string;
  id: EventId;
  tick: Tick;
  kind: EventKind;
}

export type EventKind =
  | { event: "world_loaded"; world_id: string }
  | { event: "world_time_advanced"; from_tick: Tick; to_tick: Tick }
  | { event: "character_created"; character_id: string; home_id: string }
  | { event: "character_moved"; character_id: string; from: string; to: string }
  | {
      event: "message_spoken";
      conversation_id: string;
      speaker_id: string;
      target: SpeechTarget;
      text: string;
    }
  | {
      event: "activity_started";
      character_id: string;
      activity_id: string;
      description: string;
      started_at_tick: Tick;
      completes_at_tick: Tick;
      movement_path: GridPosition[];
    }
  | {
      event: "world_expanded";
      world_id: string;
      block_id: string;
      homes_added: number;
      services_added: number;
      parks_added: number;
    }
  | { event: "activity_completed"; character_id: string; activity_id: string }
  | {
      event: "activity_failed";
      character_id: string;
      activity_id: string;
      reason: string;
    }
  | { event: "queue_accepted"; character_id: string; queued_count: number; reserved_coins: number }
  | { event: "queue_step_started"; character_id: string; remaining: number }
  | { event: "queue_step_failed"; character_id: string; code: string }
  | { event: "promise_created"; promise: unknown }
  | { event: "promise_resolved"; promise_id: string; character_id: string; resume_hint: string }
  | { event: "coins_reserved"; character_id: string; amount: number }
  | {
      event: "coins_spent";
      character_id: string;
      amount: number;
      source_id?: string | null;
      item?: string | null;
    }
  | { event: "coins_earned"; character_id: string; amount: number; source_id: string }
  | { event: "coins_released"; character_id: string; amount: number }
  | { event: "home_locked"; character_id: string; home_id: string }
  | { event: "home_unlocked"; character_id: string; home_id: string }
  | { event: "notification_acknowledged"; character_id: string; notification_id: string }
  | { event: "character_sent_home"; character_id: string; from: string; to: string }
  | { event: "command_rejected"; character_id: string; code: string };

export type LiveMessage =
  | { kind: "snapshot"; snapshot: WorldSnapshot }
  | { kind: "state"; snapshot: ViewerStateSnapshot }
  | { kind: "events"; events: EventRecord[] }
  | { kind: "connection_error"; message: string }
  | { kind: "pong"; at: number };
