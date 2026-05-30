use fishtank_protocol::{
    ActionView, Activity, ActivityKind, ActivityStatus, AgentActorView, AgentInteractionSummary,
    AgentMemoryHints, AgentObservation, AgentPromiseView, AgentRecommendedAction, AgentWakeLimits,
    ApiError, Character, CharacterId, CharacterStatus, ChessCommand, Command, CommandEnvelope,
    CommandResponse, CommandResult, Conversation, ConversationId, Direction, EntityId, EntityView,
    Event, EventId, EventKind, ExternalGame, ExternalGameStatus, FacingDirection, GridPosition,
    GridSize, GroundType, HomeAction, HomeManual, InviteStatus, LocationDefinition, LocationId,
    LocationView, MAX_ACTIONS_PER_WAKE, MoveMode, NearbyAgentView, Notification,
    NotificationAction, OFFLINE_RETURN_HOME_TICKS, Observation, PreconditionKind, Promise,
    PublicInteractableDefinition, PublicInvite, PublicNotice, QueueableCommand, QueuedCommand,
    SCHEMA_VERSION, ServiceDefinition, SpeechMessage, SpeechTarget, TICKS_PER_INGAME_DAY, Tick,
    WorldDefinition, WorldSnapshot, WorldTime,
};
use std::collections::BTreeMap;
use thiserror::Error;

pub const DEFAULT_OBSERVATION_TTL_TICKS: Tick = 20;
pub const DEFAULT_NOTIFICATION_TTL_TICKS: Tick = 3_600;
pub const MOVE_BASE_TICKS: Tick = 3;
pub const MOVE_TICKS_PER_TILE: Tick = 2;
pub const MAX_QUEUE_LEN: usize = 3;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("failed to parse world definition: {0}")]
    ParseWorld(#[from] serde_json::Error),
    #[error("world definition has no locations")]
    EmptyWorld,
    #[error("spawn location {0} does not exist")]
    MissingSpawn(LocationId),
    #[error("location {0} references missing exit {1}")]
    MissingExit(LocationId, LocationId),
    #[error("home {0} does not reference a known location")]
    MissingHome(LocationId),
    #[error("service {0} references missing location {1}")]
    MissingServiceLocation(EntityId, LocationId),
    #[error("world grid must be at least 1x1")]
    InvalidGrid,
    #[error("world grid terrain must define exactly one ground type per tile")]
    InvalidTerrain,
    #[error("location {0} has an invalid grid footprint")]
    InvalidLocationFootprint(LocationId),
    #[error("location {0} overlaps location {1} on the world grid")]
    OverlappingLocation(LocationId, LocationId),
}

#[derive(Clone, Debug)]
pub struct Engine {
    state: WorldSnapshot,
    events: Vec<Event>,
}

impl Engine {
    pub fn from_world_json(input: &str) -> Result<Self, CoreError> {
        let world: WorldDefinition = serde_json::from_str(input)?;
        Self::new(world)
    }

    pub fn new(world: WorldDefinition) -> Result<Self, CoreError> {
        validate_world(&world)?;
        let home_locks = world
            .homes
            .iter()
            .map(|home| (home.id.clone(), false))
            .collect();
        let mut engine = Self {
            state: WorldSnapshot {
                schema_version: SCHEMA_VERSION.to_string(),
                world_id: world.id.clone(),
                tick: 0,
                next_event_id: 1,
                next_command_seq: 1,
                next_conversation_seq: 1,
                world,
                characters: BTreeMap::new(),
                home_locks,
                conversations: BTreeMap::new(),
                notifications: BTreeMap::new(),
                public_invites: BTreeMap::new(),
                public_notices: BTreeMap::new(),
                external_games: BTreeMap::new(),
                command_log: Vec::new(),
            },
            events: Vec::new(),
        };
        let world_id = engine.state.world_id.clone();
        engine.record(EventKind::WorldLoaded { world_id });
        Ok(engine)
    }

    pub fn from_snapshot(snapshot: WorldSnapshot, events: Vec<Event>) -> Self {
        Self {
            state: snapshot,
            events,
        }
    }

    pub fn from_snapshot_with_world_definition(
        mut snapshot: WorldSnapshot,
        events: Vec<Event>,
        world: WorldDefinition,
    ) -> Result<Self, CoreError> {
        merge_world_definition(&mut snapshot.world, world);
        validate_world(&snapshot.world)?;
        Ok(Self::from_snapshot(snapshot, events))
    }

    pub fn replay(world: WorldDefinition, commands: &[CommandEnvelope]) -> Result<Self, CoreError> {
        let mut engine = Self::new(world)?;
        for command in commands {
            engine.apply(command.clone());
        }
        Ok(engine)
    }

    pub fn state(&self) -> &WorldSnapshot {
        &self.state
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn command_log(&self) -> &[CommandEnvelope] {
        &self.state.command_log
    }

    pub fn compact_history(&mut self, retained_events: usize, retained_commands: usize) -> bool {
        let mut compacted = false;
        if self.events.len() > retained_events {
            let drop_count = self.events.len() - retained_events;
            self.events.drain(0..drop_count);
            compacted = true;
        }
        if self.state.command_log.len() > retained_commands {
            let drop_count = self.state.command_log.len() - retained_commands;
            self.state.command_log.drain(0..drop_count);
            compacted = true;
        }
        compacted
    }

    pub fn events_after(&self, after_id: Option<EventId>) -> Vec<Event> {
        let min_id = after_id.unwrap_or(0);
        self.events
            .iter()
            .filter(|event| event.id > min_id)
            .cloned()
            .collect()
    }

    pub fn delete_character(&mut self, character_id: &str) -> Option<Character> {
        let character = self.state.characters.remove(character_id)?;
        for home in &mut self.state.world.homes {
            if home.owner_character_id.as_deref() == Some(character_id) {
                home.owner_character_id = None;
            }
        }
        self.state.home_locks.remove(&character.home_id);
        self.state.conversations.retain(|_, conversation| {
            !conversation
                .participant_ids
                .iter()
                .any(|id| id == character_id)
        });
        self.state
            .notifications
            .retain(|_, notification| notification.character_id != character_id);
        self.record(EventKind::CharacterDeleted {
            character_id: character.id.clone(),
            name: character.name.clone(),
        });
        Some(character)
    }

    pub fn apply(&mut self, envelope: CommandEnvelope) -> CommandResponse {
        let command_id = envelope.command_id.clone();
        let character_id = envelope.character_id.clone();
        let result = self.apply_inner(envelope.clone());
        match result {
            Ok((result, observation)) => {
                self.state.command_log.push(envelope);
                CommandResponse {
                    ok: true,
                    accepted: true,
                    command_id,
                    tick: self.state.tick,
                    result,
                    observation,
                    error: None,
                }
            }
            Err(error) => {
                self.record(EventKind::CommandRejected {
                    character_id,
                    code: error.code.clone(),
                });
                CommandResponse {
                    ok: false,
                    accepted: false,
                    command_id,
                    tick: self.state.tick,
                    result: None,
                    observation: None,
                    error: Some(error),
                }
            }
        }
    }

    pub fn advance_ticks(&mut self, ticks: Tick) {
        if ticks == 0 {
            return;
        }
        let from_tick = self.state.tick;
        for _ in 0..ticks {
            self.state.tick += 1;
            self.complete_due_activities();
            self.return_inactive_characters_home();
        }
        self.record(EventKind::WorldTimeAdvanced {
            from_tick,
            to_tick: self.state.tick,
        });
    }

    pub fn observe(&self, character_id: &str) -> Result<Observation, ApiError> {
        let actor = self.require_character(character_id)?.clone();
        let location = self.location(&actor.location_id).ok_or_else(|| {
            api_error("location_missing", "The actor's location no longer exists.")
        })?;
        let nearby_entities = self.nearby_entities(&actor, location);
        let conversations = self.visible_conversations(&actor);
        let notifications = self.notifications_for(character_id, false);
        Ok(Observation {
            schema_version: SCHEMA_VERSION.to_string(),
            observed_at_tick: self.state.tick,
            valid_until_tick: self.state.tick + DEFAULT_OBSERVATION_TTL_TICKS,
            local_state_hash: self.local_state_hash(&actor),
            staleness_policy: "valid_if_local_state_compatible".to_string(),
            actor,
            location: LocationView {
                id: location.id.clone(),
                name: location.name.clone(),
                description: location.description.clone(),
                exits: location.exits.clone(),
            },
            nearby_entities,
            conversations,
            available_actions: self.available_actions(character_id)?,
            recent_events: self.recent_events(),
            notifications,
            world_time: self.world_time(),
        })
    }

    pub fn observe_agent(&self, character_id: &str) -> Result<AgentObservation, ApiError> {
        let observation = self.observe(character_id)?;
        let actor = observation.actor.clone();
        let nearby_agents = self.nearby_agent_views(&actor);
        let recent_relevant_events = self.recent_relevant_events(&actor);
        let open_promises = self.open_promises(&actor);
        let recommended_actions = self.recommended_actions(&actor, &nearby_agents, &open_promises);
        let memory_hints = self.agent_memory_hints(&actor, &nearby_agents, &recent_relevant_events);
        Ok(AgentObservation {
            schema_version: SCHEMA_VERSION.to_string(),
            wake_reason: self.wake_reason(&observation.notifications),
            actor: AgentActorView {
                id: actor.id.clone(),
                name: actor.name.clone(),
                status: actor.status.clone(),
                current_activity: actor.current_activity.clone(),
                location_id: actor.location_id.clone(),
                home_id: actor.home_id.clone(),
                coins: actor.coins,
                reserved_coins: actor.reserved_coins,
            },
            world_time: observation.world_time,
            location: observation.location,
            nearby_agents,
            recent_relevant_events,
            notifications: observation.notifications,
            open_promises,
            available_affordances: observation.available_actions,
            recommended_actions,
            memory_hints,
            limits: AgentWakeLimits {
                max_actions_this_wake: MAX_ACTIONS_PER_WAKE,
            },
        })
    }

    fn apply_inner(
        &mut self,
        envelope: CommandEnvelope,
    ) -> Result<(Option<CommandResult>, Option<Observation>), ApiError> {
        self.validate_freshness(&envelope)?;
        self.validate_preconditions(&envelope)?;
        self.touch_actor(&envelope.character_id);

        match envelope.command {
            Command::CreateCharacter {
                name,
                body_color,
                face_color,
            } => {
                let character =
                    self.create_character(envelope.character_id, name, body_color, face_color)?;
                Ok((Some(CommandResult::CharacterCreated { character }), None))
            }
            Command::Observe => {
                let observation = self.observe(&envelope.character_id)?;
                Ok((None, Some(observation)))
            }
            Command::LookAt { target } => {
                let result = self.look_at(&envelope.character_id, &target)?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::Move { mode } => {
                let result = self.start_move(&envelope.character_id, mode)?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::Say { target, text } => {
                let result = self.say(&envelope.character_id, target, text)?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::ReplyTo {
                target_event_id,
                target,
                text,
            } => {
                let result =
                    self.reply_to(&envelope.character_id, target_event_id, target, text)?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::Ask {
                target_character_id,
                text,
            } => {
                let result = self.ask(&envelope.character_id, &target_character_id, text)?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::Invite {
                target_character_id,
                action,
                target_id,
                message,
            } => {
                let result = self.invite(
                    &envelope.character_id,
                    &target_character_id,
                    &action,
                    &target_id,
                    message,
                )?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::RespondInvite { invite_id, accept } => {
                let result = self.respond_invite(&envelope.character_id, &invite_id, accept)?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::JoinActivity { activity_id } => {
                let result = self.join_activity(&envelope.character_id, &activity_id)?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::Follow {
                target_character_id,
            } => {
                let result = self.invite(
                    &envelope.character_id,
                    &target_character_id,
                    "follow",
                    &target_character_id,
                    "Can I follow you for a bit?".to_string(),
                )?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::WalkWith {
                target_character_id,
                destination_id,
            } => {
                let result = self.invite(
                    &envelope.character_id,
                    &target_character_id,
                    "walk_with",
                    &destination_id,
                    format!("Want to walk with me to {destination_id}?"),
                )?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::UseInteractable {
                target_id,
                action,
                args,
            } => {
                let result =
                    self.use_interactable(&envelope.character_id, &target_id, &action, args)?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::Chess { action } => {
                let result = self.chess_action(&envelope.character_id, action)?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::Order { service_id, item } => {
                let result = self.start_order(&envelope.character_id, &service_id, &item, false)?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::PerformActivity { site_id } => {
                let result = self.start_activity_site(&envelope.character_id, &site_id)?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::Wait { ticks } => {
                self.advance_ticks(ticks);
                Ok((
                    Some(CommandResult::Waited {
                        advanced_ticks: ticks,
                    }),
                    Some(self.observe(&envelope.character_id)?),
                ))
            }
            Command::Queue { actions } => {
                let result = self.accept_queue(&envelope.character_id, actions)?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::HomeManual => {
                let manual = self.home_manual(&envelope.character_id)?;
                Ok((Some(CommandResult::HomeManual { manual }), None))
            }
            Command::Home { action } => {
                let result = self.home_action(&envelope.character_id, action)?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
            Command::Notifications { action } => {
                let result = self.notification_action(&envelope.character_id, action)?;
                Ok((Some(result), Some(self.observe(&envelope.character_id)?)))
            }
        }
    }

    fn validate_freshness(&self, envelope: &CommandEnvelope) -> Result<(), ApiError> {
        if let Some(valid_until_tick) = envelope.valid_until_tick
            && self.state.tick > valid_until_tick
        {
            return Err(api_error(
                "stale_command",
                "The command was based on an observation that is no longer fresh.",
            )
            .with_suggestions(["observe"]));
        }
        if let Some(local_state_hash) = &envelope.local_state_hash {
            let actor = self.require_character(&envelope.character_id)?;
            if local_state_hash != &self.local_state_hash(actor) {
                return Err(api_error(
                    "local_state_changed",
                    "The local state changed since the observation.",
                )
                .with_suggestions(["observe"]));
            }
        }
        Ok(())
    }

    fn validate_preconditions(&self, envelope: &CommandEnvelope) -> Result<(), ApiError> {
        if envelope.preconditions.is_empty() {
            return Ok(());
        }
        let actor = self.require_character(&envelope.character_id)?;
        for precondition in &envelope.preconditions {
            match precondition.condition {
                PreconditionKind::NearbyOrVisible => {
                    if !self.is_visible_to(actor, &precondition.entity) {
                        return Err(api_error(
                            "precondition_failed",
                            "A required entity is no longer nearby or visible.",
                        )
                        .with_suggestions(["observe"]));
                    }
                }
                PreconditionKind::ActorAtLocation => {
                    if actor.location_id != precondition.entity {
                        return Err(api_error(
                            "precondition_failed",
                            "The actor is no longer at the required location.",
                        )
                        .with_suggestions(["observe"]));
                    }
                }
            }
        }
        Ok(())
    }

    fn create_character(
        &mut self,
        character_id: CharacterId,
        name: String,
        body_color: String,
        face_color: String,
    ) -> Result<Character, ApiError> {
        if self.state.characters.contains_key(&character_id) {
            return Err(api_error(
                "character_exists",
                "A character already exists for this token or character id.",
            ));
        }
        validate_hex_color(&body_color, "body_color")?;
        validate_hex_color(&face_color, "face_color")?;

        self.ensure_growth_capacity();
        let home_id = self.allocate_home(&character_id);
        let location_id = home_id
            .clone()
            .unwrap_or_else(|| self.state.world.spawn_location_id.clone());
        let character = Character {
            id: character_id.clone(),
            name,
            body_color,
            face_color,
            location_id,
            home_id: home_id.unwrap_or_else(|| self.state.world.spawn_location_id.clone()),
            coins: self.state.world.starting_coins,
            reserved_coins: 0,
            current_activity: None,
            queued_commands: Vec::new(),
            last_agent_action_tick: self.state.tick,
            status: CharacterStatus::Idle,
        };
        self.state
            .characters
            .insert(character_id.clone(), character.clone());
        self.record(EventKind::CharacterCreated {
            character_id,
            home_id: character.home_id.clone(),
        });
        self.ensure_growth_capacity();
        Ok(character)
    }

    fn look_at(&self, character_id: &str, target: &str) -> Result<CommandResult, ApiError> {
        let actor = self.require_character(character_id)?;
        let entity = self
            .entity_view(actor, target)
            .ok_or_else(|| api_error("not_visible", "The target is not currently visible."))?;
        let description = match entity.entity_type.as_str() {
            "character" => format!("{} is nearby.", entity.name),
            "service" => self
                .service(&entity.id)
                .map(|service| {
                    if service.description.trim().is_empty() {
                        format!("{} can be used here.", service.name)
                    } else {
                        service.description.clone()
                    }
                })
                .unwrap_or_else(|_| format!("{} can be used here.", entity.name)),
            "activity_site" => self
                .activity_site(&entity.id)
                .map(|site| site.description.clone())
                .unwrap_or_else(|_| entity.name.clone()),
            "public_interactable" => self
                .interactable(&entity.id)
                .map(|interactable| {
                    let mut description = interactable.description.clone();
                    if !interactable.public_state.is_empty() {
                        description.push_str(" Public state: ");
                        description.push_str(
                            &interactable
                                .public_state
                                .iter()
                                .map(|(key, value)| format!("{key}={value}"))
                                .collect::<Vec<_>>()
                                .join(", "),
                        );
                    }
                    description
                })
                .unwrap_or_else(|_| entity.name.clone()),
            "location" => self
                .location(&entity.id)
                .map(|location| location.description.clone())
                .unwrap_or_else(|| entity.name.clone()),
            _ => entity.name.clone(),
        };
        Ok(CommandResult::LookedAt {
            entity,
            description,
        })
    }

    fn start_move(
        &mut self,
        character_id: &str,
        mode: MoveMode,
    ) -> Result<CommandResult, ApiError> {
        let target_location_id = self.resolve_move_target(character_id, mode)?;
        self.ensure_can_start_activity(character_id, false)?;
        self.ensure_home_access(character_id, &target_location_id)?;
        let from = self.require_character(character_id)?.location_id.clone();
        let movement_path = self.movement_path(&from, &target_location_id)?;
        let estimated_ticks = self.movement_ticks(&movement_path);
        let activity_id = self.next_activity_id("move");
        let description = format!("{character_id} walks from {from} to {target_location_id}.");
        let started_at_tick = self.state.tick;
        let completes_at_tick = started_at_tick + estimated_ticks;
        {
            let actor = self.require_character_mut(character_id)?;
            actor.current_activity = Some(Activity {
                id: activity_id.clone(),
                kind: ActivityKind::Moving,
                status: ActivityStatus::Active,
                target_id: Some(target_location_id),
                movement_path: movement_path.clone(),
                started_at_tick,
                completes_at_tick,
                description: description.clone(),
                promise_id: None,
                reserved_coins: 0,
                queued: false,
            });
            actor.status = CharacterStatus::Moving;
        }
        self.record(EventKind::ActivityStarted {
            character_id: character_id.to_string(),
            activity_id: activity_id.clone(),
            description: description.clone(),
            started_at_tick,
            completes_at_tick,
            movement_path: movement_path.clone(),
        });
        Ok(CommandResult::ActivityStarted {
            activity_id,
            description,
            estimated_ticks,
            started_at_tick,
            completes_at_tick,
            movement_path,
            promise: None,
        })
    }

    fn say(
        &mut self,
        character_id: &str,
        target: SpeechTarget,
        text: String,
    ) -> Result<CommandResult, ApiError> {
        let actor = self.require_character(character_id)?.clone();
        if text.trim().is_empty() {
            return Err(api_error("empty_speech", "Speech text cannot be empty."));
        }
        if let SpeechTarget::Character(target_id) = &target {
            let target_character = self.require_character(target_id)?;
            if target_character.location_id != actor.location_id {
                return Err(api_error(
                    "target_not_audible",
                    "The target character is not near enough to hear this.",
                ));
            }
        }

        let conversation_id = conversation_id_for(&actor.location_id);
        let message = SpeechMessage {
            speaker_id: character_id.to_string(),
            target: target.clone(),
            text: text.clone(),
            tick: self.state.tick,
        };
        let conversation = self
            .state
            .conversations
            .entry(conversation_id.clone())
            .or_insert_with(|| Conversation {
                id: conversation_id.clone(),
                participant_ids: Vec::new(),
                recent_messages: Vec::new(),
                open: true,
            });
        insert_unique(&mut conversation.participant_ids, character_id.to_string());
        if let SpeechTarget::Character(target_id) = &target {
            insert_unique(&mut conversation.participant_ids, target_id.clone());
        }
        conversation.recent_messages.push(message);
        if conversation.recent_messages.len() > 12 {
            conversation.recent_messages.remove(0);
        }
        self.record(EventKind::MessageSpoken {
            conversation_id: conversation_id.clone(),
            speaker_id: character_id.to_string(),
            target: target.clone(),
            text,
        });
        if let SpeechTarget::Character(target_id) = target
            && self.state.characters.contains_key(&target_id)
        {
            let speaker_name = self
                .state
                .characters
                .get(character_id)
                .map(|character| character.name.clone())
                .unwrap_or_else(|| character_id.to_string());
            self.create_notification(
                &target_id,
                "directed_speech",
                &format!("{speaker_name} spoke directly to you."),
                [
                    ("speaker_id".to_string(), character_id.to_string()),
                    ("conversation_id".to_string(), conversation_id.clone()),
                ],
            );
        }
        Ok(CommandResult::MessageSpoken { conversation_id })
    }

    fn reply_to(
        &mut self,
        character_id: &str,
        target_event_id: EventId,
        target: SpeechTarget,
        text: String,
    ) -> Result<CommandResult, ApiError> {
        if !self.events.iter().any(|event| event.id == target_event_id) {
            return Err(api_error(
                "unknown_reply_target",
                "The referenced event is not in recent world history.",
            ));
        }
        let actor = self.require_character(character_id)?.clone();
        let result = self.say(character_id, target.clone(), text.clone())?;
        let conversation_id = match result {
            CommandResult::MessageSpoken { conversation_id } => conversation_id,
            _ => conversation_id_for(&actor.location_id),
        };
        self.record(EventKind::ReplySpoken {
            conversation_id: conversation_id.clone(),
            speaker_id: character_id.to_string(),
            target_event_id,
            target,
            text,
        });
        Ok(CommandResult::MessageSpoken { conversation_id })
    }

    fn ask(
        &mut self,
        character_id: &str,
        target_character_id: &str,
        text: String,
    ) -> Result<CommandResult, ApiError> {
        self.say(
            character_id,
            SpeechTarget::Character(target_character_id.to_string()),
            format!("Question: {text}"),
        )
    }

    fn invite(
        &mut self,
        character_id: &str,
        target_character_id: &str,
        action: &str,
        target_id: &str,
        message: String,
    ) -> Result<CommandResult, ApiError> {
        let actor = self.require_character(character_id)?.clone();
        let target = self.require_character(target_character_id)?.clone();
        if actor.location_id != target.location_id {
            return Err(api_error(
                "target_not_nearby",
                "Invites require the target character to be nearby in v1.",
            ));
        }
        if !self.is_visible_to(&actor, target_id) && self.interactable(target_id).is_err() {
            return Err(api_error(
                "invite_target_not_visible",
                "The invite target is not visible or usable from here.",
            ));
        }
        let invite_id = format!(
            "invite.{}.{}",
            self.state.tick,
            self.state.public_invites.len() + 1
        );
        let invite = PublicInvite {
            id: invite_id.clone(),
            from_character_id: character_id.to_string(),
            to_character_id: target_character_id.to_string(),
            action: action.to_string(),
            target_id: target_id.to_string(),
            message,
            status: InviteStatus::Pending,
            created_at_tick: self.state.tick,
            responded_at_tick: None,
        };
        self.state
            .public_invites
            .insert(invite_id.clone(), invite.clone());
        self.record(EventKind::InviteCreated {
            invite_id: invite_id.clone(),
            from_character_id: character_id.to_string(),
            to_character_id: target_character_id.to_string(),
            action: action.to_string(),
            target_id: target_id.to_string(),
        });
        let notification_kind = if action.contains("chess") {
            "chess_invite"
        } else {
            "invite_received"
        };
        self.create_notification(
            target_character_id,
            notification_kind,
            &format!("{} invited you to {}.", actor.name, action),
            [
                ("invite_id".to_string(), invite_id.clone()),
                ("from_character_id".to_string(), character_id.to_string()),
                ("target_id".to_string(), target_id.to_string()),
            ],
        );
        Ok(CommandResult::SocialUpdated {
            kind: "invite_created".to_string(),
            id: invite_id,
            summary: format!("Invite sent to {}.", target.name),
        })
    }

    fn respond_invite(
        &mut self,
        character_id: &str,
        invite_id: &str,
        accept: bool,
    ) -> Result<CommandResult, ApiError> {
        let invite = self
            .state
            .public_invites
            .get_mut(invite_id)
            .ok_or_else(|| api_error("unknown_invite", "The invite does not exist."))?;
        if invite.to_character_id != character_id {
            return Err(api_error(
                "invite_not_owned",
                "Only the invited character can respond to this invite.",
            ));
        }
        if invite.status != InviteStatus::Pending {
            return Err(api_error(
                "invite_already_resolved",
                "This invite has already been accepted or declined.",
            ));
        }
        invite.status = if accept {
            InviteStatus::Accepted
        } else {
            InviteStatus::Declined
        };
        invite.responded_at_tick = Some(self.state.tick);
        let from_character_id = invite.from_character_id.clone();
        let action = invite.action.clone();
        let target_id = invite.target_id.clone();
        self.record(EventKind::InviteResponded {
            invite_id: invite_id.to_string(),
            responder_id: character_id.to_string(),
            accepted: accept,
        });
        self.create_notification(
            &from_character_id,
            if accept {
                "invite_accepted"
            } else {
                "invite_declined"
            },
            if accept {
                "Your invite was accepted."
            } else {
                "Your invite was declined."
            },
            [
                ("invite_id".to_string(), invite_id.to_string()),
                ("responder_id".to_string(), character_id.to_string()),
                ("target_id".to_string(), target_id.clone()),
            ],
        );
        if accept && action.contains("chess") && self.interactable(&target_id).is_ok() {
            self.reserve_chess_board(&from_character_id, character_id, &target_id)?;
        }
        Ok(CommandResult::SocialUpdated {
            kind: if accept {
                "invite_accepted".to_string()
            } else {
                "invite_declined".to_string()
            },
            id: invite_id.to_string(),
            summary: "Invite response recorded.".to_string(),
        })
    }

    fn join_activity(
        &mut self,
        character_id: &str,
        activity_id: &str,
    ) -> Result<CommandResult, ApiError> {
        let activity = self
            .state
            .characters
            .values()
            .find_map(|character| {
                character.current_activity.as_ref().and_then(|activity| {
                    (activity.id == activity_id).then_some((character.id.clone(), activity.clone()))
                })
            })
            .ok_or_else(|| {
                api_error("unknown_activity", "The activity is not currently active.")
            })?;
        let actor = self.require_character(character_id)?.clone();
        let leader = self.require_character(&activity.0)?.clone();
        if actor.location_id != leader.location_id {
            return Err(api_error(
                "activity_not_nearby",
                "The activity is not happening nearby.",
            ));
        }
        let site_id = activity
            .1
            .target_id
            .ok_or_else(|| api_error("activity_not_joinable", "This activity cannot be joined."))?;
        self.start_activity_site(character_id, &site_id)
    }

    fn use_interactable(
        &mut self,
        character_id: &str,
        target_id: &str,
        action: &str,
        args: BTreeMap<String, String>,
    ) -> Result<CommandResult, ApiError> {
        let actor = self.require_character(character_id)?.clone();
        let interactable = self.interactable(target_id)?.clone();
        if interactable.location_id != actor.location_id {
            return Err(api_error(
                "interactable_not_nearby",
                "The requested public object is not available at this location.",
            ));
        }
        if !interactable
            .actions
            .iter()
            .any(|candidate| candidate == action)
        {
            return Err(api_error(
                "action_unavailable",
                "That action is not available on this public object.",
            ));
        }
        match action {
            "post_notice" => {
                let text = args
                    .get("text")
                    .map(String::as_str)
                    .unwrap_or_default()
                    .trim();
                if text.is_empty() {
                    return Err(api_error("empty_notice", "A notice needs text."));
                }
                let notice_id = format!(
                    "notice.{}.{}",
                    self.state.tick,
                    self.state.public_notices.len() + 1
                );
                self.state.public_notices.insert(
                    notice_id.clone(),
                    PublicNotice {
                        id: notice_id.clone(),
                        board_id: target_id.to_string(),
                        author_id: character_id.to_string(),
                        text: text.to_string(),
                        created_at_tick: self.state.tick,
                    },
                );
                self.record(EventKind::PublicNoticePosted {
                    notice_id: notice_id.clone(),
                    board_id: target_id.to_string(),
                    author_id: character_id.to_string(),
                });
                self.notify_location_except(
                    character_id,
                    &actor.location_id,
                    "notice_posted",
                    "A new public notice was posted nearby.",
                    [
                        ("notice_id".to_string(), notice_id.clone()),
                        ("board_id".to_string(), target_id.to_string()),
                    ],
                );
                Ok(CommandResult::InteractableUpdated {
                    interactable_id: target_id.to_string(),
                    action: action.to_string(),
                    summary: "Notice posted.".to_string(),
                })
            }
            "reserve_board" => {
                let game = self.reserve_chess_board(character_id, character_id, target_id)?;
                Ok(CommandResult::ChessUpdated { games: vec![game] })
            }
            "view_game" => Ok(CommandResult::ChessUpdated {
                games: self.games_for_board(target_id),
            }),
            _ if interactable.duration_ticks > 0 => {
                self.start_interactable_activity(character_id, &interactable)
            }
            _ => {
                self.record(EventKind::PublicInteractableUsed {
                    interactable_id: target_id.to_string(),
                    character_id: character_id.to_string(),
                    action: action.to_string(),
                });
                self.notify_location_except(
                    character_id,
                    &actor.location_id,
                    "interactable_updated",
                    "A nearby public object was used.",
                    [
                        ("interactable_id".to_string(), target_id.to_string()),
                        ("action".to_string(), action.to_string()),
                    ],
                );
                Ok(CommandResult::InteractableUpdated {
                    interactable_id: target_id.to_string(),
                    action: action.to_string(),
                    summary: "Public object used.".to_string(),
                })
            }
        }
    }

    fn start_order(
        &mut self,
        character_id: &str,
        service_id: &str,
        item: &str,
        already_reserved: bool,
    ) -> Result<CommandResult, ApiError> {
        self.ensure_can_start_activity(character_id, false)?;
        let actor = self.require_character(character_id)?.clone();
        let service = self.service(service_id)?.clone();
        if service.location_id != actor.location_id {
            return Err(api_error(
                "service_not_nearby",
                "The requested service is not available at this location.",
            )
            .with_suggestions(["observe", "move"]));
        }
        if service.item != item {
            return Err(api_error(
                "item_unavailable",
                "The requested item is not available from this service.",
            ));
        }
        if !already_reserved {
            let spendable = actor.coins.saturating_sub(actor.reserved_coins);
            if spendable < service.price_coins {
                return Err(api_error(
                    "insufficient_coins",
                    "The character does not have enough unreserved coins.",
                ));
            }
            self.require_character_mut(character_id)?.reserved_coins += service.price_coins;
            self.record(EventKind::CoinsReserved {
                character_id: character_id.to_string(),
                amount: service.price_coins,
            });
        }

        let activity_id = self.next_activity_id("order");
        let promise_id = self.next_promise_id();
        let completes_at_tick = self.state.tick + service.duration_ticks;
        let started_at_tick = self.state.tick;
        let description = format!(
            "{character_id} orders {} at {}.",
            service.item, service.name
        );
        {
            let actor = self.require_character_mut(character_id)?;
            actor.current_activity = Some(Activity {
                id: activity_id.clone(),
                kind: ActivityKind::Ordering,
                status: ActivityStatus::Active,
                target_id: Some(service.id.clone()),
                movement_path: Vec::new(),
                started_at_tick,
                completes_at_tick,
                description: description.clone(),
                promise_id: Some(promise_id.clone()),
                reserved_coins: service.price_coins,
                queued: false,
            });
            actor.status = CharacterStatus::Ordering;
        }
        let promise = Promise {
            id: promise_id,
            activity_id: activity_id.clone(),
            trigger: "activity_ready".to_string(),
            estimated_ready_at_tick: completes_at_tick,
            resume_hint: format!("Your {} is ready at {}.", service.item, service.name),
        };
        self.record(EventKind::ActivityStarted {
            character_id: character_id.to_string(),
            activity_id: activity_id.clone(),
            description: description.clone(),
            started_at_tick,
            completes_at_tick,
            movement_path: Vec::new(),
        });
        self.record(EventKind::PromiseCreated {
            promise: promise.clone(),
        });
        Ok(CommandResult::ActivityStarted {
            activity_id,
            description,
            estimated_ticks: service.duration_ticks,
            started_at_tick,
            completes_at_tick,
            movement_path: Vec::new(),
            promise: Some(promise),
        })
    }

    fn start_activity_site(
        &mut self,
        character_id: &str,
        site_id: &str,
    ) -> Result<CommandResult, ApiError> {
        self.ensure_can_start_activity(character_id, false)?;
        let actor = self.require_character(character_id)?.clone();
        let site = self.activity_site(site_id)?.clone();
        if site.location_id != actor.location_id {
            return Err(api_error(
                "activity_site_not_nearby",
                "The requested activity site is not available at this location.",
            )
            .with_suggestions(["observe", "move"]));
        }
        if site.coin_reward > 0 && actor.coins >= self.state.world.max_coins {
            return Err(api_error(
                "coin_cap_reached",
                "The character cannot earn more coins right now.",
            ));
        }

        let activity_id = self.next_activity_id(&site.action);
        let started_at_tick = self.state.tick;
        let completes_at_tick = started_at_tick + site.duration_ticks;
        let description = format!("{character_id} starts {} at {}.", site.action, site.name);
        {
            let actor = self.require_character_mut(character_id)?;
            actor.current_activity = Some(Activity {
                id: activity_id.clone(),
                kind: ActivityKind::Performing,
                status: ActivityStatus::Active,
                target_id: Some(site.id.clone()),
                movement_path: Vec::new(),
                started_at_tick,
                completes_at_tick,
                description: description.clone(),
                promise_id: None,
                reserved_coins: 0,
                queued: false,
            });
            actor.status = CharacterStatus::Performing;
        }
        self.record(EventKind::ActivityStarted {
            character_id: character_id.to_string(),
            activity_id: activity_id.clone(),
            description: description.clone(),
            started_at_tick,
            completes_at_tick,
            movement_path: Vec::new(),
        });
        Ok(CommandResult::ActivityStarted {
            activity_id,
            description,
            estimated_ticks: site.duration_ticks,
            started_at_tick,
            completes_at_tick,
            movement_path: Vec::new(),
            promise: None,
        })
    }

    fn start_interactable_activity(
        &mut self,
        character_id: &str,
        interactable: &PublicInteractableDefinition,
    ) -> Result<CommandResult, ApiError> {
        self.ensure_can_start_activity(character_id, false)?;
        let actor = self.require_character(character_id)?.clone();
        if interactable.price_coins > 0
            && actor.coins.saturating_sub(actor.reserved_coins) < interactable.price_coins
        {
            return Err(api_error(
                "insufficient_coins",
                "The character does not have enough unreserved coins.",
            ));
        }
        if interactable.reward_coins > 0 && actor.coins >= self.state.world.max_coins {
            return Err(api_error(
                "coin_cap_reached",
                "The character cannot earn more coins right now.",
            ));
        }
        if interactable.price_coins > 0 {
            self.require_character_mut(character_id)?.reserved_coins += interactable.price_coins;
            self.record(EventKind::CoinsReserved {
                character_id: character_id.to_string(),
                amount: interactable.price_coins,
            });
        }
        let activity_id = self.next_activity_id("interactable");
        let started_at_tick = self.state.tick;
        let completes_at_tick = started_at_tick + interactable.duration_ticks;
        let description = format!("{character_id} uses {}.", interactable.name);
        {
            let actor = self.require_character_mut(character_id)?;
            actor.current_activity = Some(Activity {
                id: activity_id.clone(),
                kind: ActivityKind::Performing,
                status: ActivityStatus::Active,
                target_id: Some(interactable.id.clone()),
                movement_path: Vec::new(),
                started_at_tick,
                completes_at_tick,
                description: description.clone(),
                promise_id: None,
                reserved_coins: interactable.price_coins,
                queued: false,
            });
            actor.status = CharacterStatus::Performing;
        }
        self.record(EventKind::ActivityStarted {
            character_id: character_id.to_string(),
            activity_id: activity_id.clone(),
            description: description.clone(),
            started_at_tick,
            completes_at_tick,
            movement_path: Vec::new(),
        });
        self.notify_location_except(
            character_id,
            &interactable.location_id,
            "public_activity_started",
            "A nearby public activity started.",
            [
                ("activity_id".to_string(), activity_id.clone()),
                ("interactable_id".to_string(), interactable.id.clone()),
            ],
        );
        Ok(CommandResult::ActivityStarted {
            activity_id,
            description,
            estimated_ticks: interactable.duration_ticks,
            started_at_tick,
            completes_at_tick,
            movement_path: Vec::new(),
            promise: None,
        })
    }

    fn chess_action(
        &mut self,
        character_id: &str,
        action: ChessCommand,
    ) -> Result<CommandResult, ApiError> {
        self.require_character(character_id)?;
        match action {
            ChessCommand::Status { board_id } => {
                let games = board_id
                    .map(|board_id| self.games_for_board(&board_id))
                    .unwrap_or_else(|| {
                        self.state
                            .external_games
                            .values()
                            .cloned()
                            .collect::<Vec<_>>()
                    });
                Ok(CommandResult::ChessUpdated { games })
            }
            ChessCommand::RegisterExternalGame {
                board_id,
                opponent_character_id,
                provider,
                external_game_id,
                url,
            } => {
                if provider != "lichess" {
                    return Err(api_error(
                        "unsupported_chess_provider",
                        "Chess v1 only supports externally enforced Lichess games.",
                    ));
                }
                let board = self.interactable(&board_id)?.clone();
                if !board
                    .actions
                    .iter()
                    .any(|action| action == "register_external_game")
                {
                    return Err(api_error(
                        "not_a_chess_board",
                        "This public object cannot register external chess games.",
                    ));
                }
                let actor = self.require_character(character_id)?.clone();
                let opponent = self.require_character(&opponent_character_id)?.clone();
                if actor.location_id != board.location_id
                    || opponent.location_id != board.location_id
                {
                    return Err(api_error(
                        "players_not_at_board",
                        "Both chess participants must be at the chess board location.",
                    ));
                }
                if let Some(existing_id) = self
                    .state
                    .external_games
                    .values()
                    .find(|game| {
                        game.board_id == board_id
                            && game.status != ExternalGameStatus::Confirmed
                            && game.external_game_id.is_empty()
                            && game.participant_ids.iter().any(|id| id == character_id)
                            && game
                                .participant_ids
                                .iter()
                                .any(|id| id == &opponent_character_id)
                    })
                    .map(|game| game.id.clone())
                {
                    let tick = self.state.tick;
                    let game = self.require_external_game_mut(&existing_id)?;
                    game.provider = provider;
                    game.external_game_id = external_game_id;
                    game.url = url;
                    game.last_reported_tick = tick;
                    let game = game.clone();
                    self.record(EventKind::ExternalGameRegistered {
                        game_id: existing_id,
                        board_id: board_id.clone(),
                        provider: "lichess".to_string(),
                    });
                    return Ok(CommandResult::ChessUpdated { games: vec![game] });
                }
                if self.state.external_games.values().any(|game| {
                    game.board_id == board_id && game.status != ExternalGameStatus::Confirmed
                }) {
                    return Err(api_error(
                        "board_already_reserved",
                        "This chess board already has an active external game.",
                    ));
                }
                let game_id = format!(
                    "game.{}.{}",
                    self.state.tick,
                    self.state.external_games.len() + 1
                );
                let game = ExternalGame {
                    id: game_id.clone(),
                    board_id: board_id.clone(),
                    participant_ids: vec![character_id.to_string(), opponent_character_id],
                    provider,
                    external_game_id,
                    url,
                    status: ExternalGameStatus::Registered,
                    started_at_tick: self.state.tick,
                    last_reported_tick: self.state.tick,
                    result: None,
                    reported_by: None,
                    confirmations: Vec::new(),
                    disputed_by: Vec::new(),
                };
                self.state
                    .external_games
                    .insert(game_id.clone(), game.clone());
                self.record(EventKind::ExternalGameRegistered {
                    game_id: game_id.clone(),
                    board_id: board_id.clone(),
                    provider: "lichess".to_string(),
                });
                for participant_id in &game.participant_ids {
                    if participant_id != character_id {
                        self.create_notification(
                            participant_id,
                            "external_game_pending",
                            "A Lichess game was registered for your chess board.",
                            [
                                ("game_id".to_string(), game_id.clone()),
                                ("board_id".to_string(), board_id.clone()),
                            ],
                        );
                    }
                }
                Ok(CommandResult::ChessUpdated { games: vec![game] })
            }
            ChessCommand::RecordResult { game_id, result } => {
                let tick = self.state.tick;
                let game = self.require_external_game_mut(&game_id)?;
                if !game.participant_ids.iter().any(|id| id == character_id) {
                    return Err(api_error(
                        "not_game_participant",
                        "Only participants can report this game result.",
                    ));
                }
                game.status = ExternalGameStatus::ResultReported;
                game.result = Some(result.clone());
                game.reported_by = Some(character_id.to_string());
                game.last_reported_tick = tick;
                insert_unique(&mut game.confirmations, character_id.to_string());
                let game = game.clone();
                self.record(EventKind::ExternalGameResultReported {
                    game_id: game_id.clone(),
                    result,
                    reporter_id: character_id.to_string(),
                });
                for participant_id in &game.participant_ids {
                    if participant_id != character_id {
                        self.create_notification(
                            participant_id,
                            "chess_game_update",
                            "A Lichess result was reported for your chess game.",
                            [("game_id".to_string(), game_id.clone())],
                        );
                    }
                }
                Ok(CommandResult::ChessUpdated { games: vec![game] })
            }
            ChessCommand::ConfirmResult { game_id, accept } => {
                let game = self.require_external_game_mut(&game_id)?;
                if !game.participant_ids.iter().any(|id| id == character_id) {
                    return Err(api_error(
                        "not_game_participant",
                        "Only participants can confirm this game result.",
                    ));
                }
                if accept {
                    insert_unique(&mut game.confirmations, character_id.to_string());
                    if game
                        .participant_ids
                        .iter()
                        .all(|id| game.confirmations.iter().any(|confirmed| confirmed == id))
                    {
                        game.status = ExternalGameStatus::Confirmed;
                    }
                } else {
                    insert_unique(&mut game.disputed_by, character_id.to_string());
                    game.status = ExternalGameStatus::Disputed;
                }
                Ok(CommandResult::ChessUpdated {
                    games: vec![game.clone()],
                })
            }
        }
    }

    fn accept_queue(
        &mut self,
        character_id: &str,
        actions: Vec<QueuedCommand>,
    ) -> Result<CommandResult, ApiError> {
        self.ensure_can_start_activity(character_id, false)?;
        if actions.is_empty() {
            return Err(api_error(
                "empty_queue",
                "A queue must include at least one action.",
            ));
        }
        if actions.len() > MAX_QUEUE_LEN {
            return Err(api_error(
                "queue_too_long",
                "Queues are capped at three actions.",
            ));
        }
        let reserve = self.required_queue_reservation(character_id, &actions)?;
        let actor = self.require_character(character_id)?.clone();
        if actor.coins.saturating_sub(actor.reserved_coins) < reserve {
            return Err(api_error(
                "insufficient_coins",
                "The character cannot reserve enough coins for this queue.",
            ));
        }
        {
            let actor = self.require_character_mut(character_id)?;
            actor.reserved_coins += reserve;
            actor.queued_commands = actions;
        }
        if reserve > 0 {
            self.record(EventKind::CoinsReserved {
                character_id: character_id.to_string(),
                amount: reserve,
            });
        }
        let queued_count = self.require_character(character_id)?.queued_commands.len();
        self.record(EventKind::QueueAccepted {
            character_id: character_id.to_string(),
            queued_count,
            reserved_coins: reserve,
        });
        self.start_next_queue_step(character_id);
        Ok(CommandResult::QueueAccepted {
            queued_count,
            reserved_coins: reserve,
        })
    }

    fn home_manual(&self, character_id: &str) -> Result<HomeManual, ApiError> {
        let actor = self.require_character(character_id)?;
        Ok(HomeManual {
            home_id: actor.home_id.clone(),
            owner_character_id: actor.id.clone(),
            supported_actions: vec![
                HomeAction::Enter,
                HomeAction::Leave,
                HomeAction::Lock,
                HomeAction::Unlock,
                HomeAction::ReturnHome,
            ],
            locked: self.home_locked(&actor.home_id),
            description: "Homes support entering, leaving, locking, unlocking, and returning home."
                .to_string(),
        })
    }

    fn home_action(
        &mut self,
        character_id: &str,
        action: HomeAction,
    ) -> Result<CommandResult, ApiError> {
        match action {
            HomeAction::Enter | HomeAction::ReturnHome => {
                let home_id = self.require_character(character_id)?.home_id.clone();
                if self.require_character(character_id)?.location_id == home_id {
                    self.require_character_mut(character_id)?.status = CharacterStatus::InsideHome;
                    return Ok(CommandResult::HomeUpdated {
                        home_id: home_id.clone(),
                        locked: self.home_locked(&home_id),
                        location_id: home_id,
                    });
                }
                self.start_move(
                    character_id,
                    MoveMode::ToTarget {
                        target: home_id.clone(),
                    },
                )?;
                Ok(CommandResult::HomeUpdated {
                    home_id: home_id.clone(),
                    locked: self.home_locked(&home_id),
                    location_id: self.require_character(character_id)?.location_id.clone(),
                })
            }
            HomeAction::Leave => {
                let actor = self.require_character(character_id)?.clone();
                if actor.location_id != actor.home_id {
                    return Err(api_error("not_at_home", "The character is not at home."));
                }
                let location = self.location(&actor.home_id).ok_or_else(|| {
                    api_error(
                        "location_missing",
                        "The character's home location is missing.",
                    )
                })?;
                let exit = location
                    .exits
                    .first()
                    .cloned()
                    .ok_or_else(|| api_error("no_exit", "The home has no exit."))?;
                self.require_character_mut(character_id)?.status = CharacterStatus::Idle;
                self.start_move(character_id, MoveMode::ToTarget { target: exit })?;
                Ok(CommandResult::HomeUpdated {
                    home_id: actor.home_id.clone(),
                    locked: self.home_locked(&actor.home_id),
                    location_id: actor.location_id,
                })
            }
            HomeAction::Lock => {
                let home_id = self.require_character(character_id)?.home_id.clone();
                self.state.home_locks.insert(home_id.clone(), true);
                self.record(EventKind::HomeLocked {
                    character_id: character_id.to_string(),
                    home_id: home_id.clone(),
                });
                Ok(CommandResult::HomeUpdated {
                    home_id: home_id.clone(),
                    locked: true,
                    location_id: self.require_character(character_id)?.location_id.clone(),
                })
            }
            HomeAction::Unlock => {
                let home_id = self.require_character(character_id)?.home_id.clone();
                self.state.home_locks.insert(home_id.clone(), false);
                self.record(EventKind::HomeUnlocked {
                    character_id: character_id.to_string(),
                    home_id: home_id.clone(),
                });
                Ok(CommandResult::HomeUpdated {
                    home_id: home_id.clone(),
                    locked: false,
                    location_id: self.require_character(character_id)?.location_id.clone(),
                })
            }
        }
    }

    fn notification_action(
        &mut self,
        character_id: &str,
        action: NotificationAction,
    ) -> Result<CommandResult, ApiError> {
        self.require_character(character_id)?;
        match action {
            NotificationAction::List => Ok(CommandResult::Notifications {
                notifications: self.notifications_for(character_id, false),
            }),
            NotificationAction::Ack { notification_id } => {
                let notification = self
                    .state
                    .notifications
                    .get_mut(&notification_id)
                    .ok_or_else(|| {
                        api_error("unknown_notification", "The notification does not exist.")
                    })?;
                if notification.character_id != character_id {
                    return Err(api_error(
                        "notification_not_owned",
                        "The notification belongs to another character.",
                    ));
                }
                if notification.acknowledged {
                    return Ok(CommandResult::NotificationAcknowledged { notification_id });
                }
                notification.acknowledged = true;
                self.record(EventKind::NotificationAcknowledged {
                    character_id: character_id.to_string(),
                    notification_id: notification_id.clone(),
                });
                Ok(CommandResult::NotificationAcknowledged { notification_id })
            }
        }
    }

    fn complete_due_activities(&mut self) {
        let due_ids = self
            .state
            .characters
            .iter()
            .filter_map(|(character_id, character)| {
                character.current_activity.as_ref().and_then(|activity| {
                    (activity.completes_at_tick <= self.state.tick).then_some(character_id.clone())
                })
            })
            .collect::<Vec<_>>();

        for character_id in due_ids {
            let Some(activity) = self
                .state
                .characters
                .get(&character_id)
                .and_then(|character| character.current_activity.clone())
            else {
                continue;
            };
            let was_queued = activity.queued;
            self.complete_activity(&character_id, activity);
            let had_remaining_queue = self
                .state
                .characters
                .get(&character_id)
                .is_some_and(|actor| !actor.queued_commands.is_empty());
            self.start_next_queue_step(&character_id);
            if was_queued && !had_remaining_queue && self.queue_is_idle(&character_id) {
                self.create_notification(
                    &character_id,
                    "queue_completed",
                    "Your queued routine is complete.",
                    [("character_id".to_string(), character_id.clone())],
                );
            }
        }
    }

    fn complete_activity(&mut self, character_id: &str, activity: Activity) {
        match activity.kind {
            ActivityKind::Moving | ActivityKind::ReturningHome => {
                if let Some(target_id) = &activity.target_id {
                    let from = self.state.characters[character_id].location_id.clone();
                    let actor = self
                        .state
                        .characters
                        .get_mut(character_id)
                        .expect("character exists");
                    actor.location_id = target_id.clone();
                    actor.status = if target_id == &actor.home_id {
                        CharacterStatus::InsideHome
                    } else {
                        CharacterStatus::Idle
                    };
                    self.record(EventKind::CharacterMoved {
                        character_id: character_id.to_string(),
                        from,
                        to: target_id.clone(),
                    });
                    self.create_arrival_notifications(character_id, target_id);
                }
            }
            ActivityKind::Ordering => {
                if let Some(service) = activity
                    .target_id
                    .as_ref()
                    .and_then(|service_id| self.service(service_id).ok())
                    .cloned()
                {
                    let actor = self
                        .state
                        .characters
                        .get_mut(character_id)
                        .expect("character exists");
                    actor.reserved_coins = actor.reserved_coins.saturating_sub(service.price_coins);
                    actor.coins = actor.coins.saturating_sub(service.price_coins);
                    actor.status = CharacterStatus::Idle;
                    self.record(EventKind::CoinsSpent {
                        character_id: character_id.to_string(),
                        amount: service.price_coins,
                        source_id: Some(service.id.clone()),
                        item: Some(service.item.clone()),
                    });
                }
                if let Some(promise_id) = &activity.promise_id {
                    let resume_hint = self
                        .service(activity.target_id.as_deref().unwrap_or_default())
                        .map(|service| {
                            format!("Your {} is ready at {}.", service.item, service.name)
                        })
                        .unwrap_or_else(|_| "Your activity is ready.".to_string());
                    self.record(EventKind::PromiseResolved {
                        promise_id: promise_id.clone(),
                        character_id: character_id.to_string(),
                        resume_hint: resume_hint.clone(),
                    });
                    self.create_notification(
                        character_id,
                        "promise_resolved",
                        &resume_hint,
                        [
                            ("promise_id".to_string(), promise_id.clone()),
                            ("activity_id".to_string(), activity.id.clone()),
                        ],
                    );
                }
            }
            ActivityKind::Waiting => {
                self.require_character_mut(character_id)
                    .expect("character exists")
                    .status = CharacterStatus::Idle;
            }
            ActivityKind::Performing => {
                let target_id = activity.target_id.clone().unwrap_or_default();
                let reward = self
                    .activity_site(&target_id)
                    .map(|site| (site.id.clone(), site.coin_reward))
                    .or_else(|_| {
                        self.interactable(&target_id).map(|interactable| {
                            (interactable.id.clone(), interactable.reward_coins)
                        })
                    })
                    .unwrap_or_else(|_| (target_id.clone(), 0));
                let earned = {
                    let actor = self
                        .state
                        .characters
                        .get_mut(character_id)
                        .expect("character exists");
                    actor.status = CharacterStatus::Idle;
                    if activity.reserved_coins > 0 {
                        actor.reserved_coins =
                            actor.reserved_coins.saturating_sub(activity.reserved_coins);
                        actor.coins = actor.coins.saturating_sub(activity.reserved_coins);
                    }
                    if reward.1 > 0 {
                        let before = actor.coins;
                        actor.coins = actor
                            .coins
                            .saturating_add(reward.1)
                            .min(self.state.world.max_coins);
                        actor.coins.saturating_sub(before)
                    } else {
                        0
                    }
                };
                if earned > 0 {
                    self.record(EventKind::CoinsEarned {
                        character_id: character_id.to_string(),
                        amount: earned,
                        source_id: reward.0.clone(),
                    });
                }
                if activity.reserved_coins > 0 {
                    self.record(EventKind::CoinsSpent {
                        character_id: character_id.to_string(),
                        amount: activity.reserved_coins,
                        source_id: Some(reward.0.clone()),
                        item: None,
                    });
                }
                self.create_notification(
                    character_id,
                    "activity_completed",
                    "Your activity is complete.",
                    [
                        ("activity_id".to_string(), activity.id.clone()),
                        ("source_id".to_string(), reward.0),
                    ],
                );
            }
        }

        self.state
            .characters
            .get_mut(character_id)
            .expect("character exists")
            .current_activity = None;
        self.record(EventKind::ActivityCompleted {
            character_id: character_id.to_string(),
            activity_id: activity.id,
        });
    }

    fn start_next_queue_step(&mut self, character_id: &str) {
        let next = {
            let Some(actor) = self.state.characters.get_mut(character_id) else {
                return;
            };
            if actor.current_activity.is_some() || actor.queued_commands.is_empty() {
                return;
            }
            actor.queued_commands.remove(0)
        };

        let remaining = self
            .state
            .characters
            .get(character_id)
            .map(|actor| actor.queued_commands.len())
            .unwrap_or_default();
        self.record(EventKind::QueueStepStarted {
            character_id: character_id.to_string(),
            remaining,
        });

        let result = match next.command {
            QueueableCommand::Move { mode } => self.start_move(character_id, mode),
            QueueableCommand::Say { target, text } => self.say(character_id, target, text),
            QueueableCommand::Order { service_id, item } => {
                self.start_order(character_id, &service_id, &item, true)
            }
            QueueableCommand::PerformActivity { site_id } => {
                self.start_activity_site(character_id, &site_id)
            }
            QueueableCommand::Wait { ticks } => self.start_wait_activity(character_id, ticks),
            QueueableCommand::Home { action } => self.home_action(character_id, action),
        };

        match result {
            Ok(_) => {
                if let Some(activity) = self
                    .state
                    .characters
                    .get_mut(character_id)
                    .and_then(|actor| actor.current_activity.as_mut())
                {
                    activity.queued = true;
                } else if self.queue_is_idle(character_id) {
                    self.create_notification(
                        character_id,
                        "queue_completed",
                        "Your queued routine is complete.",
                        [("character_id".to_string(), character_id.to_string())],
                    );
                } else {
                    self.start_next_queue_step(character_id);
                }
            }
            Err(error) => {
                let code = error.code;
                self.release_all_reservations(character_id);
                if let Some(actor) = self.state.characters.get_mut(character_id) {
                    actor.queued_commands.clear();
                }
                self.record(EventKind::QueueStepFailed {
                    character_id: character_id.to_string(),
                    code: code.clone(),
                });
                self.create_notification(
                    character_id,
                    "queue_failed",
                    "Your queued routine stopped before it finished.",
                    [
                        ("character_id".to_string(), character_id.to_string()),
                        ("code".to_string(), code),
                    ],
                );
            }
        }
    }

    fn start_wait_activity(
        &mut self,
        character_id: &str,
        ticks: Tick,
    ) -> Result<CommandResult, ApiError> {
        self.ensure_can_start_activity(character_id, false)?;
        let activity_id = self.next_activity_id("wait");
        let description = format!("{character_id} waits for {ticks} ticks.");
        let completes_at_tick = self.state.tick + ticks;
        let started_at_tick = self.state.tick;
        let actor = self.require_character_mut(character_id)?;
        actor.current_activity = Some(Activity {
            id: activity_id.clone(),
            kind: ActivityKind::Waiting,
            status: ActivityStatus::Active,
            target_id: None,
            movement_path: Vec::new(),
            started_at_tick,
            completes_at_tick,
            description: description.clone(),
            promise_id: None,
            reserved_coins: 0,
            queued: false,
        });
        actor.status = CharacterStatus::Waiting;
        self.record(EventKind::ActivityStarted {
            character_id: character_id.to_string(),
            activity_id: activity_id.clone(),
            description: description.clone(),
            started_at_tick,
            completes_at_tick,
            movement_path: Vec::new(),
        });
        Ok(CommandResult::ActivityStarted {
            activity_id,
            description,
            estimated_ticks: ticks,
            started_at_tick,
            completes_at_tick,
            movement_path: Vec::new(),
            promise: None,
        })
    }

    fn return_inactive_characters_home(&mut self) {
        let ids = self
            .state
            .characters
            .iter()
            .filter_map(|(character_id, character)| {
                let inactive = self
                    .state
                    .tick
                    .saturating_sub(character.last_agent_action_tick)
                    >= OFFLINE_RETURN_HOME_TICKS;
                let can_return = inactive
                    && character.current_activity.is_none()
                    && character.location_id != character.home_id;
                can_return.then_some(character_id.clone())
            })
            .collect::<Vec<_>>();
        for character_id in ids {
            let actor = self
                .state
                .characters
                .get_mut(&character_id)
                .expect("character exists");
            let from = actor.location_id.clone();
            let to = actor.home_id.clone();
            actor.location_id = to.clone();
            actor.status = CharacterStatus::InsideHome;
            self.record(EventKind::CharacterSentHome {
                character_id,
                from,
                to,
            });
        }
    }

    fn resolve_move_target(
        &self,
        character_id: &str,
        mode: MoveMode,
    ) -> Result<LocationId, ApiError> {
        let actor = self.require_character(character_id)?;
        let current = self
            .location(&actor.location_id)
            .ok_or_else(|| api_error("location_missing", "Current location is missing."))?;
        let target = match mode {
            MoveMode::ToTarget { target } => target,
            MoveMode::Direction {
                direction,
                distance,
            } => {
                if distance == 0 {
                    return Err(api_error(
                        "invalid_distance",
                        "Directional movement distance must be greater than zero.",
                    ));
                }
                current
                    .directional_exits
                    .get(&direction)
                    .cloned()
                    .or_else(|| match direction {
                        Direction::Forward => current.exits.first().cloned(),
                        Direction::Back => current.exits.last().cloned(),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        api_error(
                            "direction_not_available",
                            "There is no reachable exit in that direction.",
                        )
                        .with_suggestions(["observe"])
                    })?
            }
        };

        if !self
            .state
            .world
            .locations
            .iter()
            .any(|location| location.id == target)
        {
            return Err(api_error(
                "unknown_target",
                "The target location does not exist.",
            ));
        }
        if !current.exits.iter().any(|exit| exit == &target) {
            return Err(api_error(
                "target_not_reachable",
                "The target location is not directly reachable from here.",
            )
            .with_suggestions(["observe"]));
        }
        Ok(target)
    }

    fn required_queue_reservation(
        &self,
        character_id: &str,
        actions: &[QueuedCommand],
    ) -> Result<u32, ApiError> {
        self.require_character(character_id)?;
        let mut total = 0_u32;
        for action in actions {
            if let QueueableCommand::Order { service_id, item } = &action.command {
                let service = self.service(service_id)?;
                if &service.item != item {
                    return Err(api_error(
                        "item_unavailable",
                        "A queued item is not available from the requested service.",
                    ));
                }
                total = total.saturating_add(service.price_coins);
            }
        }
        Ok(total)
    }

    fn ensure_can_start_activity(
        &self,
        character_id: &str,
        allow_while_moving: bool,
    ) -> Result<(), ApiError> {
        let actor = self.require_character(character_id)?;
        if let Some(activity) = &actor.current_activity
            && !(allow_while_moving && activity.kind == ActivityKind::Moving)
        {
            return Err(
                api_error("actor_busy", "The character is already doing an activity.")
                    .with_retry(activity.completes_at_tick.saturating_sub(self.state.tick)),
            );
        }
        Ok(())
    }

    fn ensure_home_access(
        &self,
        character_id: &str,
        target_location_id: &str,
    ) -> Result<(), ApiError> {
        if !self.home_locked(target_location_id) {
            return Ok(());
        }
        let actor = self.require_character(character_id)?;
        if actor.home_id == target_location_id {
            Ok(())
        } else {
            Err(api_error(
                "home_locked",
                "The target home is locked and this character does not own it.",
            ))
        }
    }

    fn available_actions(&self, character_id: &str) -> Result<Vec<ActionView>, ApiError> {
        let actor = self.require_character(character_id)?;
        let location = self
            .location(&actor.location_id)
            .ok_or_else(|| api_error("location_missing", "Current location is missing."))?;
        let mut actions = vec![
            ActionView {
                action: "observe".to_string(),
                targets: Vec::new(),
            },
            ActionView {
                action: "wait".to_string(),
                targets: Vec::new(),
            },
            ActionView {
                action: "say".to_string(),
                targets: vec!["room".to_string()],
            },
            ActionView {
                action: "home_manual".to_string(),
                targets: vec![actor.home_id.clone()],
            },
            ActionView {
                action: "notifications".to_string(),
                targets: Vec::new(),
            },
        ];
        actions.push(ActionView {
            action: "move".to_string(),
            targets: location.exits.clone(),
        });
        let service_targets = self
            .state
            .world
            .services
            .iter()
            .filter(|service| service.location_id == actor.location_id)
            .map(|service| service.id.clone())
            .collect::<Vec<_>>();
        if !service_targets.is_empty() {
            actions.push(ActionView {
                action: "order".to_string(),
                targets: service_targets,
            });
        }
        let activity_targets = self
            .state
            .world
            .activity_sites
            .iter()
            .filter(|site| site.location_id == actor.location_id)
            .map(|site| site.id.clone())
            .collect::<Vec<_>>();
        if !activity_targets.is_empty() {
            actions.push(ActionView {
                action: "perform_activity".to_string(),
                targets: activity_targets,
            });
        }
        let interactable_targets = self
            .state
            .world
            .interactables
            .iter()
            .filter(|interactable| interactable.location_id == actor.location_id)
            .map(|interactable| interactable.id.clone())
            .collect::<Vec<_>>();
        if !interactable_targets.is_empty() {
            actions.push(ActionView {
                action: "use_interactable".to_string(),
                targets: interactable_targets.clone(),
            });
            actions.push(ActionView {
                action: "invite".to_string(),
                targets: interactable_targets,
            });
        }
        Ok(actions)
    }

    fn nearby_entities(&self, actor: &Character, location: &LocationDefinition) -> Vec<EntityView> {
        let actor_location_id = actor.location_id.clone();
        let nearby_characters = self
            .state
            .characters
            .values()
            .filter(|character| {
                character.id != actor.id && character.location_id == actor_location_id
            })
            .map(|character| EntityView {
                id: character.id.clone(),
                entity_type: "character".to_string(),
                name: character.name.clone(),
                distance: "near".to_string(),
                available_actions: vec!["say".to_string(), "look_at".to_string()],
            });

        let services = self
            .state
            .world
            .services
            .iter()
            .filter(|service| service.location_id == actor_location_id)
            .map(|service| EntityView {
                id: service.id.clone(),
                entity_type: "service".to_string(),
                name: service.name.clone(),
                distance: "near".to_string(),
                available_actions: vec!["order".to_string(), "look_at".to_string()],
            });

        let activity_sites = self
            .state
            .world
            .activity_sites
            .iter()
            .filter(|site| site.location_id == actor_location_id)
            .map(|site| EntityView {
                id: site.id.clone(),
                entity_type: "activity_site".to_string(),
                name: site.name.clone(),
                distance: "near".to_string(),
                available_actions: vec!["perform_activity".to_string(), "look_at".to_string()],
            });

        let interactables = self
            .state
            .world
            .interactables
            .iter()
            .filter(|interactable| interactable.location_id == actor_location_id)
            .map(|interactable| EntityView {
                id: interactable.id.clone(),
                entity_type: "public_interactable".to_string(),
                name: interactable.name.clone(),
                distance: "near".to_string(),
                available_actions: interactable
                    .actions
                    .iter()
                    .cloned()
                    .chain(["look_at".to_string()])
                    .collect(),
            });

        let exits = location.exits.iter().map(|exit| EntityView {
            id: exit.clone(),
            entity_type: "location".to_string(),
            name: self
                .location(exit)
                .map(|location| location.name.clone())
                .unwrap_or_else(|| exit.clone()),
            distance: "near".to_string(),
            available_actions: vec!["move".to_string(), "look_at".to_string()],
        });

        nearby_characters
            .chain(services)
            .chain(activity_sites)
            .chain(interactables)
            .chain(exits)
            .collect()
    }

    fn visible_conversations(&self, actor: &Character) -> Vec<Conversation> {
        self.state
            .conversations
            .values()
            .filter(|conversation| {
                conversation.participant_ids.iter().any(|participant_id| {
                    self.state
                        .characters
                        .get(participant_id)
                        .is_some_and(|character| character.location_id == actor.location_id)
                })
            })
            .cloned()
            .collect()
    }

    fn entity_view(&self, actor: &Character, target: &str) -> Option<EntityView> {
        let location = self.location(&actor.location_id)?;
        self.nearby_entities(actor, location)
            .into_iter()
            .find(|entity| entity.id == target)
            .or_else(|| {
                (target == actor.id).then(|| EntityView {
                    id: actor.id.clone(),
                    entity_type: "character".to_string(),
                    name: actor.name.clone(),
                    distance: "self".to_string(),
                    available_actions: vec!["observe".to_string()],
                })
            })
    }

    fn is_visible_to(&self, actor: &Character, entity_id: &str) -> bool {
        entity_id == actor.location_id
            || entity_id == actor.id
            || self.entity_view(actor, entity_id).is_some()
    }

    fn notifications_for(
        &self,
        character_id: &str,
        include_acknowledged: bool,
    ) -> Vec<Notification> {
        self.state
            .notifications
            .values()
            .filter(|notification| notification.character_id == character_id)
            .filter(|notification| include_acknowledged || !notification.acknowledged)
            .cloned()
            .collect()
    }

    fn wake_reason(&self, notifications: &[Notification]) -> String {
        for kind in [
            "directed_speech",
            "promise_resolved",
            "activity_completed",
            "queue_failed",
            "queue_completed",
            "chess_invite",
            "invite_received",
            "invite_accepted",
            "invite_declined",
            "notice_posted",
            "public_activity_started",
            "chess_game_update",
            "external_game_pending",
            "interactable_updated",
            "same_location_entry",
        ] {
            if notifications
                .iter()
                .any(|notification| notification.kind == kind)
            {
                return match kind {
                    "promise_resolved" => "promise_ready",
                    other => other,
                }
                .to_string();
            }
        }
        "idle_timeout".to_string()
    }

    fn nearby_agent_views(&self, actor: &Character) -> Vec<NearbyAgentView> {
        self.state
            .characters
            .values()
            .filter(|character| {
                character.id != actor.id && character.location_id == actor.location_id
            })
            .map(|character| NearbyAgentView {
                id: character.id.clone(),
                name: character.name.clone(),
                body_color: character.body_color.clone(),
                face_color: character.face_color.clone(),
                status: character.status.clone(),
                current_activity: character.current_activity.clone(),
                location_id: character.location_id.clone(),
            })
            .collect()
    }

    fn recent_relevant_events(&self, actor: &Character) -> Vec<Event> {
        self.events
            .iter()
            .rev()
            .filter(|event| self.event_is_relevant_to_actor(event, actor))
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    fn event_is_relevant_to_actor(&self, event: &Event, actor: &Character) -> bool {
        match &event.kind {
            EventKind::CharacterMoved {
                character_id,
                from,
                to,
            } => {
                character_id == &actor.id || from == &actor.location_id || to == &actor.location_id
            }
            EventKind::MessageSpoken {
                speaker_id, target, ..
            }
            | EventKind::ReplySpoken {
                speaker_id, target, ..
            } => {
                speaker_id == &actor.id
                    || matches!(target, SpeechTarget::Character(target_id) if target_id == &actor.id)
                    || self
                        .state
                        .characters
                        .get(speaker_id)
                        .is_some_and(|speaker| speaker.location_id == actor.location_id)
            }
            EventKind::ActivityStarted { character_id, .. }
            | EventKind::ActivityCompleted { character_id, .. }
            | EventKind::ActivityFailed { character_id, .. }
            | EventKind::QueueAccepted { character_id, .. }
            | EventKind::QueueStepStarted { character_id, .. }
            | EventKind::QueueStepFailed { character_id, .. }
            | EventKind::PromiseResolved { character_id, .. } => character_id == &actor.id,
            EventKind::InviteCreated {
                from_character_id,
                to_character_id,
                target_id,
                ..
            } => {
                from_character_id == &actor.id
                    || to_character_id == &actor.id
                    || self
                        .interactable(target_id)
                        .is_ok_and(|target| target.location_id == actor.location_id)
            }
            EventKind::InviteResponded {
                responder_id,
                invite_id,
                ..
            } => {
                responder_id == &actor.id
                    || self
                        .state
                        .public_invites
                        .get(invite_id)
                        .is_some_and(|invite| {
                            invite.from_character_id == actor.id
                                || invite.to_character_id == actor.id
                        })
            }
            EventKind::PublicNoticePosted { board_id, .. }
            | EventKind::PublicInteractableUsed {
                interactable_id: board_id,
                ..
            } => self
                .interactable(board_id)
                .is_ok_and(|interactable| interactable.location_id == actor.location_id),
            EventKind::ExternalGameRegistered { game_id, .. }
            | EventKind::ExternalGameResultReported { game_id, .. } => self
                .state
                .external_games
                .get(game_id)
                .is_some_and(|game| game.participant_ids.iter().any(|id| id == &actor.id)),
            EventKind::PromiseCreated { promise } => actor
                .current_activity
                .as_ref()
                .is_some_and(|activity| activity.id == promise.activity_id),
            _ => false,
        }
    }

    fn open_promises(&self, actor: &Character) -> Vec<AgentPromiseView> {
        actor
            .current_activity
            .as_ref()
            .and_then(|activity| {
                activity.promise_id.as_ref().map(|promise_id| {
                    let resume_hint = self
                        .service(activity.target_id.as_deref().unwrap_or_default())
                        .map(|service| {
                            format!("Your {} is ready at {}.", service.item, service.name)
                        })
                        .unwrap_or_else(|_| activity.description.clone());
                    AgentPromiseView {
                        id: promise_id.clone(),
                        activity_id: activity.id.clone(),
                        trigger: "activity_ready".to_string(),
                        estimated_ready_at_tick: activity.completes_at_tick,
                        resume_hint,
                    }
                })
            })
            .into_iter()
            .collect()
    }

    fn recommended_actions(
        &self,
        actor: &Character,
        nearby_agents: &[NearbyAgentView],
        open_promises: &[AgentPromiseView],
    ) -> Vec<AgentRecommendedAction> {
        let mut recommendations = Vec::new();
        if !open_promises.is_empty() {
            recommendations.push(AgentRecommendedAction {
                reason: "promise_ready".to_string(),
                action: "observe".to_string(),
                target: open_promises
                    .first()
                    .map(|promise| promise.activity_id.clone()),
                summary: "A promised activity has a handle worth checking.".to_string(),
            });
        }
        if actor.coins <= 2 {
            if let Some(job) = self
                .state
                .world
                .activity_sites
                .iter()
                .find(|site| site.coin_reward > 0)
            {
                recommendations.push(AgentRecommendedAction {
                    reason: "low_coins".to_string(),
                    action: "perform_activity".to_string(),
                    target: Some(job.id.clone()),
                    summary: "Coins are low; a paid activity exists in the world.".to_string(),
                });
            }
            if let Some(job) = self
                .state
                .world
                .interactables
                .iter()
                .find(|interactable| interactable.reward_coins > 0)
            {
                recommendations.push(AgentRecommendedAction {
                    reason: "job_available".to_string(),
                    action: "use_interactable".to_string(),
                    target: Some(job.id.clone()),
                    summary: "A public errand can earn coins.".to_string(),
                });
            }
        }
        for invite in self
            .state
            .public_invites
            .values()
            .filter(|invite| {
                invite.to_character_id == actor.id && invite.status == InviteStatus::Pending
            })
            .take(2)
        {
            recommendations.push(AgentRecommendedAction {
                reason: if invite.action.contains("chess") {
                    "chess_invite".to_string()
                } else {
                    "pending_invite".to_string()
                },
                action: "respond_invite".to_string(),
                target: Some(invite.target_id.clone()),
                summary: format!(
                    "{} invited you to {}.",
                    invite.from_character_id, invite.action
                ),
            });
        }
        if let Some(agent) = nearby_agents
            .iter()
            .find(|agent| agent.status == CharacterStatus::Idle)
        {
            recommendations.push(AgentRecommendedAction {
                reason: "nearby_agent_idle".to_string(),
                action: "say".to_string(),
                target: Some(agent.id.clone()),
                summary: "A nearby agent is idle and available for social follow-up.".to_string(),
            });
        }
        if let Some(interactable) = self
            .state
            .world
            .interactables
            .iter()
            .find(|interactable| interactable.location_id == actor.location_id)
        {
            recommendations.push(AgentRecommendedAction {
                reason: if interactable
                    .actions
                    .iter()
                    .any(|action| action == "post_notice")
                {
                    "notice_unread".to_string()
                } else {
                    "social_opportunity".to_string()
                },
                action: "use_interactable".to_string(),
                target: Some(interactable.id.clone()),
                summary: format!("{} is available here.", interactable.name),
            });
        }
        recommendations.truncate(5);
        recommendations
    }

    fn agent_memory_hints(
        &self,
        actor: &Character,
        nearby_agents: &[NearbyAgentView],
        recent_events: &[Event],
    ) -> AgentMemoryHints {
        let mut stable_ids = vec![
            actor.id.clone(),
            actor.location_id.clone(),
            actor.home_id.clone(),
        ];
        stable_ids.extend(
            actor
                .current_activity
                .iter()
                .map(|activity| activity.id.clone()),
        );
        stable_ids.extend(nearby_agents.iter().map(|agent| agent.id.clone()));
        stable_ids.extend(
            self.location(&actor.location_id)
                .into_iter()
                .flat_map(|location| location.exits.iter().cloned()),
        );
        stable_ids.sort();
        stable_ids.dedup();

        let recent_interactions =
            recent_events
                .iter()
                .filter_map(|event| match &event.kind {
                    EventKind::MessageSpoken {
                        speaker_id,
                        target,
                        text,
                        ..
                    } if speaker_id == &actor.id => {
                        let with = match target {
                            SpeechTarget::Character(target_id) => target_id.clone(),
                            _ => speaker_id.clone(),
                        };
                        Some(AgentInteractionSummary {
                            with,
                            summary: format!("You said: {text}"),
                            last_seen_tick: event.tick,
                            last_spoke_tick: Some(event.tick),
                            last_shared_activity_tick: None,
                            pending_invite_id: None,
                            unanswered_directed_speech: false,
                            recent_event_ids: vec![event.id],
                        })
                    }
                    EventKind::MessageSpoken {
                        speaker_id, text, ..
                    } => Some(AgentInteractionSummary {
                        with: speaker_id.clone(),
                        summary: format!("{speaker_id} said: {text}"),
                        last_seen_tick: event.tick,
                        last_spoke_tick: Some(event.tick),
                        last_shared_activity_tick: None,
                        pending_invite_id: self.pending_invite_between(&actor.id, speaker_id),
                        unanswered_directed_speech: true,
                        recent_event_ids: vec![event.id],
                    }),
                    EventKind::CharacterMoved {
                        character_id, to, ..
                    } if character_id != &actor.id => Some(AgentInteractionSummary {
                        with: character_id.clone(),
                        summary: format!("{character_id} arrived at {to}."),
                        last_seen_tick: event.tick,
                        last_spoke_tick: None,
                        last_shared_activity_tick: None,
                        pending_invite_id: self.pending_invite_between(&actor.id, character_id),
                        unanswered_directed_speech: false,
                        recent_event_ids: vec![event.id],
                    }),
                    EventKind::InviteCreated {
                        invite_id,
                        from_character_id,
                        to_character_id,
                        action,
                        ..
                    } if from_character_id == &actor.id || to_character_id == &actor.id => {
                        let with = if from_character_id == &actor.id {
                            to_character_id.clone()
                        } else {
                            from_character_id.clone()
                        };
                        Some(AgentInteractionSummary {
                            with,
                            summary: format!("Pending invite: {action}."),
                            last_seen_tick: event.tick,
                            last_spoke_tick: None,
                            last_shared_activity_tick: None,
                            pending_invite_id: Some(invite_id.clone()),
                            unanswered_directed_speech: false,
                            recent_event_ids: vec![event.id],
                        })
                    }
                    EventKind::ActivityStarted { character_id, .. }
                        if character_id != &actor.id
                            && self.state.characters.get(character_id).is_some_and(
                                |character| character.location_id == actor.location_id,
                            ) =>
                    {
                        Some(AgentInteractionSummary {
                            with: character_id.clone(),
                            summary: "Nearby agent started an activity.".to_string(),
                            last_seen_tick: event.tick,
                            last_spoke_tick: None,
                            last_shared_activity_tick: Some(event.tick),
                            pending_invite_id: self.pending_invite_between(&actor.id, character_id),
                            unanswered_directed_speech: false,
                            recent_event_ids: vec![event.id],
                        })
                    }
                    _ => None,
                })
                .take(8)
                .collect();

        AgentMemoryHints {
            stable_ids,
            recent_interactions,
        }
    }

    fn queue_is_idle(&self, character_id: &str) -> bool {
        self.state
            .characters
            .get(character_id)
            .is_some_and(|actor| {
                actor.current_activity.is_none() && actor.queued_commands.is_empty()
            })
    }

    fn create_arrival_notifications(&mut self, mover_id: &str, location_id: &str) {
        let mover_name = self
            .state
            .characters
            .get(mover_id)
            .map(|character| character.name.clone())
            .unwrap_or_else(|| mover_id.to_string());
        let recipients = self
            .state
            .characters
            .values()
            .filter(|character| character.id != mover_id && character.location_id == location_id)
            .map(|character| character.id.clone())
            .collect::<Vec<_>>();
        for recipient in recipients {
            self.create_notification(
                &recipient,
                "same_location_entry",
                &format!("{mover_name} arrived nearby."),
                [
                    ("character_id".to_string(), mover_id.to_string()),
                    ("location_id".to_string(), location_id.to_string()),
                ],
            );
        }
    }

    fn notify_location_except<const N: usize>(
        &mut self,
        actor_id: &str,
        location_id: &str,
        kind: &str,
        summary: &str,
        related: [(String, String); N],
    ) {
        let recipients = self
            .state
            .characters
            .values()
            .filter(|character| character.id != actor_id && character.location_id == location_id)
            .map(|character| character.id.clone())
            .collect::<Vec<_>>();
        let related = BTreeMap::from(related);
        for recipient in recipients {
            self.create_notification_from_map(&recipient, kind, summary, related.clone());
        }
    }

    fn reserve_chess_board(
        &mut self,
        actor_id: &str,
        opponent_id: &str,
        board_id: &str,
    ) -> Result<ExternalGame, ApiError> {
        let board = self.interactable(board_id)?.clone();
        if !board.actions.iter().any(|action| action == "reserve_board") {
            return Err(api_error(
                "not_a_chess_board",
                "This public object cannot be reserved as a chess board.",
            ));
        }
        if self
            .state
            .external_games
            .values()
            .any(|game| game.board_id == board_id && game.status != ExternalGameStatus::Confirmed)
        {
            return Err(api_error(
                "board_already_reserved",
                "This chess board already has an active external game.",
            ));
        }
        let game_id = format!(
            "game.{}.{}",
            self.state.tick,
            self.state.external_games.len() + 1
        );
        let mut participants = vec![actor_id.to_string()];
        insert_unique(&mut participants, opponent_id.to_string());
        let game = ExternalGame {
            id: game_id.clone(),
            board_id: board_id.to_string(),
            participant_ids: participants,
            provider: "lichess".to_string(),
            external_game_id: String::new(),
            url: String::new(),
            status: ExternalGameStatus::Registered,
            started_at_tick: self.state.tick,
            last_reported_tick: self.state.tick,
            result: None,
            reported_by: None,
            confirmations: Vec::new(),
            disputed_by: Vec::new(),
        };
        self.state
            .external_games
            .insert(game_id.clone(), game.clone());
        self.record(EventKind::ExternalGameRegistered {
            game_id,
            board_id: board_id.to_string(),
            provider: "lichess".to_string(),
        });
        Ok(game)
    }

    fn games_for_board(&self, board_id: &str) -> Vec<ExternalGame> {
        self.state
            .external_games
            .values()
            .filter(|game| game.board_id == board_id)
            .cloned()
            .collect()
    }

    fn pending_invite_between(&self, actor_id: &str, other_id: &str) -> Option<String> {
        self.state
            .public_invites
            .values()
            .find(|invite| {
                invite.status == InviteStatus::Pending
                    && ((invite.from_character_id == actor_id
                        && invite.to_character_id == other_id)
                        || (invite.from_character_id == other_id
                            && invite.to_character_id == actor_id))
            })
            .map(|invite| invite.id.clone())
    }

    pub fn ensure_growth_capacity(&mut self) {
        let free_homes = self.available_home_count();
        let cafe_capacity = self
            .state
            .world
            .services
            .iter()
            .filter(|service| service.item == "coffee")
            .map(|service| service.capacity)
            .sum::<u32>()
            .max(1);
        let cafe_load = self.state.characters.len() as f32 / cafe_capacity as f32;
        if free_homes < 3 || cafe_load >= 0.75 {
            self.expand_world(cafe_load >= 0.75);
        }
    }

    fn available_home_count(&self) -> usize {
        let occupied_home_ids = self
            .state
            .characters
            .values()
            .map(|character| character.home_id.clone())
            .collect::<Vec<_>>();
        self.state
            .world
            .homes
            .iter()
            .filter(|home| {
                home.owner_character_id.is_none()
                    && !occupied_home_ids.iter().any(|home_id| home_id == &home.id)
            })
            .count()
    }

    fn expand_world(&mut self, force_cafe: bool) {
        let block_index = self
            .state
            .world
            .locations
            .iter()
            .filter(|location| location.id.contains(".block_"))
            .count()
            + 1;
        let block_id = format!("block_{block_index}");
        let start_x = self.state.world.grid.width as i32;
        let height = self.state.world.grid.height.max(12);
        let street_y = (height / 2) as i32;
        let block_width = 8_u32;

        if height > self.state.world.grid.height {
            let existing_width = self.state.world.grid.width as usize;
            for _ in self.state.world.grid.height..height {
                self.state
                    .world
                    .grid
                    .terrain
                    .push(vec![GroundType::Ground; existing_width]);
            }
            self.state.world.grid.height = height;
        }

        for (y, row) in self.state.world.grid.terrain.iter_mut().enumerate() {
            let ground = if y as i32 == street_y {
                GroundType::Path
            } else if y >= height as usize - 3 {
                GroundType::Grass
            } else {
                GroundType::Ground
            };
            row.extend(std::iter::repeat_n(ground, block_width as usize));
        }
        self.state.world.grid.width += block_width;

        let street_id = format!("village.{block_id}.street");
        let previous_street = self
            .state
            .world
            .locations
            .iter()
            .filter(|location| location.id.contains("street"))
            .max_by_key(|location| location.grid_position.x)
            .map(|location| location.id.clone())
            .unwrap_or_else(|| self.state.world.spawn_location_id.clone());

        self.add_exit(&previous_street, &street_id, Some(Direction::East));
        let mut street_exits = vec![previous_street.clone()];
        let mut street_directions = BTreeMap::from([(Direction::West, previous_street)]);

        let home_count = 3 + ((self.state.world.seed + block_index as u64) % 4) as usize;
        let mut homes_added = 0;
        for index in 0..home_count {
            let home_id = format!("village.{block_id}.home_{}", index + 1);
            let y = if index % 2 == 0 {
                street_y - 2
            } else {
                street_y + 2
            };
            let x = start_x + 1 + (index as i32 % 6);
            self.state.world.locations.push(LocationDefinition {
                id: home_id.clone(),
                name: format!("{} Home {}", human_block_name(block_index), index + 1),
                description: "A compact robot home on the expanding edge of the village."
                    .to_string(),
                grid_position: GridPosition { x, y },
                grid_size: GridSize {
                    width: 1,
                    height: 1,
                },
                facing: if y < street_y {
                    FacingDirection::South
                } else {
                    FacingDirection::North
                },
                exits: vec![street_id.clone()],
                directional_exits: BTreeMap::from([(Direction::Forward, street_id.clone())]),
                poi_ids: Vec::new(),
                private_home: true,
            });
            self.state
                .world
                .homes
                .push(fishtank_protocol::HomeDefinition {
                    id: home_id.clone(),
                    name: format!("{} Home {}", human_block_name(block_index), index + 1),
                    owner_character_id: None,
                });
            street_exits.push(home_id);
            homes_added += 1;
        }

        let mut parks_added = 0;
        if block_index % 2 == 0 {
            let park_id = format!("village.{block_id}.park");
            self.state.world.locations.push(LocationDefinition {
                id: park_id.clone(),
                name: format!("{} Park", human_block_name(block_index)),
                description: "A small green pocket with benches and a clear view of the street."
                    .to_string(),
                grid_position: GridPosition {
                    x: start_x + 4,
                    y: height as i32 - 3,
                },
                grid_size: GridSize {
                    width: 3,
                    height: 2,
                },
                facing: FacingDirection::North,
                exits: vec![street_id.clone()],
                directional_exits: BTreeMap::from([(Direction::North, street_id.clone())]),
                poi_ids: Vec::new(),
                private_home: false,
            });
            street_directions.insert(Direction::South, park_id.clone());
            street_exits.push(park_id);
            parks_added = 1;
        }

        let mut services_added = 0;
        if force_cafe || block_index % 4 == 0 {
            let cafe_id = format!("village.{block_id}.cafe");
            let service_id = format!("{cafe_id}.service_window");
            self.state.world.locations.push(LocationDefinition {
                id: cafe_id.clone(),
                name: format!("{} Coffee", human_block_name(block_index)),
                description: "A deterministic service-window cafe for coffee and short queues."
                    .to_string(),
                grid_position: GridPosition {
                    x: start_x + 2,
                    y: street_y - 3,
                },
                grid_size: GridSize {
                    width: 2,
                    height: 1,
                },
                facing: FacingDirection::South,
                exits: vec![street_id.clone()],
                directional_exits: BTreeMap::from([(Direction::South, street_id.clone())]),
                poi_ids: vec![service_id.clone()],
                private_home: false,
            });
            self.state.world.services.push(ServiceDefinition {
                id: service_id,
                name: format!("{} Service Window", human_block_name(block_index)),
                location_id: cafe_id.clone(),
                item: "coffee".to_string(),
                description: "A service window that sells coffee for coins.".to_string(),
                price_coins: 2,
                duration_ticks: 30,
                capacity: 8,
                overflow_behavior: "queue_nearby".to_string(),
            });
            street_directions.insert(Direction::North, cafe_id.clone());
            street_exits.push(cafe_id);
            services_added = 1;
        }

        street_directions.insert(Direction::Forward, street_exits[0].clone());
        self.state.world.locations.push(LocationDefinition {
            id: street_id,
            name: format!("{} Street", human_block_name(block_index)),
            description: "A newly paved street segment grown from village demand.".to_string(),
            grid_position: GridPosition {
                x: start_x,
                y: street_y,
            },
            grid_size: GridSize {
                width: block_width,
                height: 1,
            },
            facing: FacingDirection::East,
            exits: street_exits,
            directional_exits: street_directions,
            poi_ids: Vec::new(),
            private_home: false,
        });

        self.record(EventKind::WorldExpanded {
            world_id: self.state.world_id.clone(),
            block_id,
            homes_added,
            services_added,
            parks_added,
        });
    }

    fn add_exit(&mut self, location_id: &str, exit: &str, direction: Option<Direction>) {
        if let Some(location) = self
            .state
            .world
            .locations
            .iter_mut()
            .find(|location| location.id == location_id)
        {
            if !location.exits.iter().any(|candidate| candidate == exit) {
                location.exits.push(exit.to_string());
            }
            if let Some(direction) = direction {
                location
                    .directional_exits
                    .insert(direction, exit.to_string());
            }
        }
    }

    fn movement_path(&self, from: &str, to: &str) -> Result<Vec<GridPosition>, ApiError> {
        let from_location = self
            .location(from)
            .ok_or_else(|| api_error("location_missing", "The source location is missing."))?;
        let to_location = self
            .location(to)
            .ok_or_else(|| api_error("location_missing", "The target location is missing."))?;
        Ok(vec![
            location_center(from_location),
            location_center(to_location),
        ])
    }

    fn movement_ticks(&self, path: &[GridPosition]) -> Tick {
        let distance = path
            .windows(2)
            .map(|pair| {
                pair[0].x.abs_diff(pair[1].x) as Tick + pair[0].y.abs_diff(pair[1].y) as Tick
            })
            .sum::<Tick>();
        MOVE_BASE_TICKS.max(distance.saturating_mul(MOVE_TICKS_PER_TILE))
    }

    fn create_notification<const N: usize>(
        &mut self,
        character_id: &str,
        kind: &str,
        summary: &str,
        related: [(String, String); N],
    ) {
        self.create_notification_from_map(character_id, kind, summary, BTreeMap::from(related));
    }

    fn create_notification_from_map(
        &mut self,
        character_id: &str,
        kind: &str,
        summary: &str,
        related: BTreeMap<String, String>,
    ) {
        let notification_id = format!(
            "notif.{}.{}",
            self.state.tick,
            self.state.notifications.len() + 1
        );
        self.state.notifications.insert(
            notification_id.clone(),
            Notification {
                notification_id,
                character_id: character_id.to_string(),
                kind: kind.to_string(),
                priority: "normal".to_string(),
                created_at_tick: self.state.tick,
                expires_at_tick: self.state.tick + DEFAULT_NOTIFICATION_TTL_TICKS,
                summary: summary.to_string(),
                acknowledged: false,
                related,
            },
        );
    }

    fn release_all_reservations(&mut self, character_id: &str) {
        let amount = self
            .state
            .characters
            .get(character_id)
            .map(|actor| actor.reserved_coins)
            .unwrap_or_default();
        if amount == 0 {
            return;
        }
        self.require_character_mut(character_id)
            .expect("character exists")
            .reserved_coins = 0;
        self.record(EventKind::CoinsReleased {
            character_id: character_id.to_string(),
            amount,
        });
    }

    fn allocate_home(&mut self, character_id: &str) -> Option<LocationId> {
        let occupied_home_ids = self
            .state
            .characters
            .values()
            .map(|character| character.home_id.clone())
            .collect::<Vec<_>>();
        self.state
            .world
            .homes
            .iter_mut()
            .find(|home| {
                home.owner_character_id.is_none()
                    && !occupied_home_ids.iter().any(|home_id| home_id == &home.id)
            })
            .map(|home| {
                home.owner_character_id = Some(character_id.to_string());
                home.id.clone()
            })
    }

    fn touch_actor(&mut self, character_id: &str) {
        let tick = self.state.tick;
        if let Some(actor) = self.state.characters.get_mut(character_id) {
            actor.last_agent_action_tick = tick;
        }
    }

    fn next_activity_id(&mut self, prefix: &str) -> String {
        let id = format!("activity.{prefix}.{}", self.state.next_command_seq);
        self.state.next_command_seq += 1;
        id
    }

    fn next_promise_id(&mut self) -> String {
        let id = format!("promise.{}", self.state.next_command_seq);
        self.state.next_command_seq += 1;
        id
    }

    fn record(&mut self, kind: EventKind) {
        let event = Event {
            schema_version: SCHEMA_VERSION.to_string(),
            id: self.state.next_event_id,
            tick: self.state.tick,
            kind,
        };
        self.state.next_event_id += 1;
        self.events.push(event);
    }

    fn require_character(&self, character_id: &str) -> Result<&Character, ApiError> {
        self.state.characters.get(character_id).ok_or_else(|| {
            api_error(
                "unknown_character",
                "No character exists for this token or character id.",
            )
            .with_suggestions(["character create"])
        })
    }

    fn require_character_mut(&mut self, character_id: &str) -> Result<&mut Character, ApiError> {
        self.state.characters.get_mut(character_id).ok_or_else(|| {
            api_error(
                "unknown_character",
                "No character exists for this token or character id.",
            )
            .with_suggestions(["character create"])
        })
    }

    fn service(&self, service_id: &str) -> Result<&ServiceDefinition, ApiError> {
        self.state
            .world
            .services
            .iter()
            .find(|service| service.id == service_id)
            .ok_or_else(|| api_error("unknown_service", "The requested service does not exist."))
    }

    fn interactable(
        &self,
        interactable_id: &str,
    ) -> Result<&PublicInteractableDefinition, ApiError> {
        self.state
            .world
            .interactables
            .iter()
            .find(|interactable| interactable.id == interactable_id)
            .ok_or_else(|| {
                api_error(
                    "unknown_interactable",
                    "The requested public interactable does not exist.",
                )
            })
    }

    fn require_external_game_mut(&mut self, game_id: &str) -> Result<&mut ExternalGame, ApiError> {
        self.state
            .external_games
            .get_mut(game_id)
            .ok_or_else(|| api_error("unknown_external_game", "The external game does not exist."))
    }

    fn activity_site(
        &self,
        site_id: &str,
    ) -> Result<&fishtank_protocol::ActivitySiteDefinition, ApiError> {
        self.state
            .world
            .activity_sites
            .iter()
            .find(|site| site.id == site_id)
            .ok_or_else(|| {
                api_error(
                    "unknown_activity_site",
                    "The requested activity site does not exist.",
                )
            })
    }

    fn location(&self, location_id: &str) -> Option<&LocationDefinition> {
        self.state
            .world
            .locations
            .iter()
            .find(|location| location.id == location_id)
    }

    fn home_locked(&self, home_id: &str) -> bool {
        self.state.home_locks.get(home_id).copied().unwrap_or(false)
    }

    fn local_state_hash(&self, actor: &Character) -> String {
        format!(
            "obs_{}_{}_{}_{}",
            actor.location_id,
            actor
                .current_activity
                .as_ref()
                .map(|a| &a.id)
                .unwrap_or(&"none".to_string()),
            actor.queued_commands.len(),
            self.state.tick
        )
    }

    fn recent_events(&self) -> Vec<Event> {
        self.events
            .iter()
            .rev()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    fn world_time(&self) -> WorldTime {
        WorldTime {
            tick: self.state.tick,
            ingame_day: self.state.tick / TICKS_PER_INGAME_DAY,
            tick_of_day: self.state.tick % TICKS_PER_INGAME_DAY,
        }
    }
}

fn validate_world(world: &WorldDefinition) -> Result<(), CoreError> {
    if world.locations.is_empty() {
        return Err(CoreError::EmptyWorld);
    }
    if world.grid.width == 0 || world.grid.height == 0 || world.grid.cell_size == 0 {
        return Err(CoreError::InvalidGrid);
    }
    if world.grid.terrain.len() != world.grid.height as usize
        || world
            .grid
            .terrain
            .iter()
            .any(|row| row.len() != world.grid.width as usize)
    {
        return Err(CoreError::InvalidTerrain);
    }
    if !world
        .locations
        .iter()
        .any(|location| location.id == world.spawn_location_id)
    {
        return Err(CoreError::MissingSpawn(world.spawn_location_id.clone()));
    }
    for location in &world.locations {
        for exit in &location.exits {
            if !world
                .locations
                .iter()
                .any(|candidate| candidate.id == *exit)
            {
                return Err(CoreError::MissingExit(location.id.clone(), exit.clone()));
            }
        }
    }
    let mut occupied = BTreeMap::<(i32, i32), LocationId>::new();
    for location in &world.locations {
        if location.grid_size.width == 0
            || location.grid_size.height == 0
            || location.grid_position.x < 0
            || location.grid_position.y < 0
            || location.grid_position.x as u32 + location.grid_size.width > world.grid.width
            || location.grid_position.y as u32 + location.grid_size.height > world.grid.height
        {
            return Err(CoreError::InvalidLocationFootprint(location.id.clone()));
        }

        for y in
            location.grid_position.y..location.grid_position.y + location.grid_size.height as i32
        {
            for x in
                location.grid_position.x..location.grid_position.x + location.grid_size.width as i32
            {
                if let Some(existing) = occupied.insert((x, y), location.id.clone()) {
                    return Err(CoreError::OverlappingLocation(
                        location.id.clone(),
                        existing,
                    ));
                }
            }
        }
    }
    for home in &world.homes {
        if !world
            .locations
            .iter()
            .any(|location| location.id == home.id)
        {
            return Err(CoreError::MissingHome(home.id.clone()));
        }
    }
    for service in &world.services {
        if !world
            .locations
            .iter()
            .any(|location| location.id == service.location_id)
        {
            return Err(CoreError::MissingServiceLocation(
                service.id.clone(),
                service.location_id.clone(),
            ));
        }
    }
    for site in &world.activity_sites {
        if site.duration_ticks == 0 {
            return Err(CoreError::InvalidLocationFootprint(site.id.clone()));
        }
        if !world
            .locations
            .iter()
            .any(|location| location.id == site.location_id)
        {
            return Err(CoreError::MissingServiceLocation(
                site.id.clone(),
                site.location_id.clone(),
            ));
        }
    }
    for interactable in &world.interactables {
        if !world
            .locations
            .iter()
            .any(|location| location.id == interactable.location_id)
        {
            return Err(CoreError::MissingServiceLocation(
                interactable.id.clone(),
                interactable.location_id.clone(),
            ));
        }
        if interactable.actions.is_empty() {
            return Err(CoreError::InvalidLocationFootprint(interactable.id.clone()));
        }
    }
    Ok(())
}

fn merge_world_definition(target: &mut WorldDefinition, source: WorldDefinition) {
    target.schema_version = source.schema_version;
    target.id = source.id;
    target.name = source.name;
    target.seed = source.seed;
    if source.grid.width >= target.grid.width && source.grid.height >= target.grid.height {
        target.grid = source.grid;
    }
    target.starting_coins = source.starting_coins;
    target.allowance_coins = source.allowance_coins;
    target.max_coins = source.max_coins;
    target.spawn_location_id = source.spawn_location_id;

    for location in source.locations {
        if let Some(existing) = target
            .locations
            .iter_mut()
            .find(|candidate| candidate.id == location.id)
        {
            *existing = location;
        } else {
            target.locations.push(location);
        }
    }

    for home in source.homes {
        if let Some(existing) = target
            .homes
            .iter_mut()
            .find(|candidate| candidate.id == home.id)
        {
            existing.name = home.name;
        } else {
            target.homes.push(home);
        }
    }

    for service in source.services {
        if let Some(existing) = target
            .services
            .iter_mut()
            .find(|candidate| candidate.id == service.id)
        {
            *existing = service;
        } else {
            target.services.push(service);
        }
    }

    for site in source.activity_sites {
        if let Some(existing) = target
            .activity_sites
            .iter_mut()
            .find(|candidate| candidate.id == site.id)
        {
            *existing = site;
        } else {
            target.activity_sites.push(site);
        }
    }

    for interactable in source.interactables {
        if let Some(existing) = target
            .interactables
            .iter_mut()
            .find(|candidate| candidate.id == interactable.id)
        {
            *existing = interactable;
        } else {
            target.interactables.push(interactable);
        }
    }
}

fn validate_hex_color(value: &str, field: &str) -> Result<(), ApiError> {
    let valid = value.len() == 7
        && value.starts_with('#')
        && value[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit());
    if valid {
        Ok(())
    } else {
        Err(api_error(
            "invalid_color",
            &format!("{field} must be a #RRGGBB hex color."),
        ))
    }
}

fn conversation_id_for(location_id: &str) -> ConversationId {
    format!("conversation.{location_id}")
}

fn location_center(location: &LocationDefinition) -> GridPosition {
    GridPosition {
        x: location.grid_position.x + (location.grid_size.width as i32 / 2),
        y: location.grid_position.y + (location.grid_size.height as i32 / 2),
    }
}

fn human_block_name(index: usize) -> String {
    format!("Block {index}")
}

fn insert_unique<T: Eq>(values: &mut Vec<T>, value: T) {
    if !values.iter().any(|candidate| candidate == &value) {
        values.push(value);
    }
}

pub fn api_error(code: &str, message: &str) -> ApiError {
    ApiError {
        code: code.to_string(),
        message: message.to_string(),
        details: BTreeMap::new(),
        retry_after_ticks: None,
        suggested_actions: Vec::new(),
    }
}

trait ApiErrorExt {
    fn with_suggestions<const N: usize>(self, suggestions: [&str; N]) -> Self;
    fn with_retry(self, retry_after_ticks: Tick) -> Self;
}

impl ApiErrorExt for ApiError {
    fn with_suggestions<const N: usize>(mut self, suggestions: [&str; N]) -> Self {
        self.suggested_actions = suggestions
            .into_iter()
            .map(std::string::ToString::to_string)
            .collect();
        self
    }

    fn with_retry(mut self, retry_after_ticks: Tick) -> Self {
        self.retry_after_ticks = Some(retry_after_ticks);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fishtank_protocol::{Precondition, SCHEMA_VERSION};
    use time::OffsetDateTime;

    fn world() -> WorldDefinition {
        serde_json::from_str(include_str!("../../../worlds/village.json")).unwrap()
    }

    fn engine() -> Engine {
        Engine::new(world()).unwrap()
    }

    fn env(character_id: &str, command: Command) -> CommandEnvelope {
        CommandEnvelope {
            schema_version: SCHEMA_VERSION.to_string(),
            command_id: format!("cmd.{character_id}.test"),
            character_id: character_id.to_string(),
            submitted_at: OffsetDateTime::UNIX_EPOCH.to_string(),
            based_on_tick: None,
            valid_until_tick: None,
            local_state_hash: None,
            preconditions: Vec::new(),
            command,
        }
    }

    fn create(engine: &mut Engine, character_id: &str, name: &str) {
        let response = engine.apply(env(
            character_id,
            Command::CreateCharacter {
                name: name.to_string(),
                body_color: "#4ea1ff".to_string(),
                face_color: "#101820".to_string(),
            },
        ));
        assert!(response.ok, "{response:?}");
    }

    fn move_to(engine: &mut Engine, character_id: &str, target: &str) {
        let response = engine.apply(env(
            character_id,
            Command::Move {
                mode: MoveMode::ToTarget {
                    target: target.to_string(),
                },
            },
        ));
        assert!(response.ok, "{response:?}");
        let ticks = match response.result {
            Some(CommandResult::ActivityStarted {
                estimated_ticks, ..
            }) => estimated_ticks,
            _ => MOVE_BASE_TICKS,
        };
        engine.advance_ticks(ticks);
    }

    #[test]
    fn world_validation_rejects_bad_worlds() {
        let mut missing_spawn = world();
        missing_spawn.spawn_location_id = "missing".to_string();
        assert!(matches!(
            Engine::new(missing_spawn),
            Err(CoreError::MissingSpawn(_))
        ));

        let mut missing_exit = world();
        missing_exit.locations[0].exits.push("missing".to_string());
        assert!(matches!(
            Engine::new(missing_exit),
            Err(CoreError::MissingExit(_, _))
        ));

        let mut missing_home = world();
        missing_home.homes[0].id = "missing".to_string();
        assert!(matches!(
            Engine::new(missing_home),
            Err(CoreError::MissingHome(_))
        ));

        let mut missing_service_location = world();
        missing_service_location.services[0].location_id = "missing".to_string();
        assert!(matches!(
            Engine::new(missing_service_location),
            Err(CoreError::MissingServiceLocation(_, _))
        ));

        let mut invalid_grid = world();
        invalid_grid.grid.width = 0;
        assert!(matches!(
            Engine::new(invalid_grid),
            Err(CoreError::InvalidGrid)
        ));

        let mut invalid_terrain = world();
        invalid_terrain.grid.terrain[0].pop();
        assert!(matches!(
            Engine::new(invalid_terrain),
            Err(CoreError::InvalidTerrain)
        ));

        let mut invalid_footprint = world();
        invalid_footprint.locations[0].grid_position.x = -1;
        assert!(matches!(
            Engine::new(invalid_footprint),
            Err(CoreError::InvalidLocationFootprint(_))
        ));

        let mut overlapping = world();
        overlapping.locations[1].grid_position = overlapping.locations[0].grid_position;
        assert!(matches!(
            Engine::new(overlapping),
            Err(CoreError::OverlappingLocation(_, _))
        ));

        let mut empty = world();
        empty.locations.clear();
        assert!(matches!(Engine::new(empty), Err(CoreError::EmptyWorld)));
        assert!(Engine::from_world_json("{").is_err());
    }

    #[test]
    fn character_creation_assigns_homes_and_validates_identity() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        create(&mut engine, "char_ren", "Ren");
        assert_eq!(
            engine.state().characters["char_mira"].home_id,
            "village.home_1"
        );
        assert_eq!(
            engine.state().characters["char_ren"].home_id,
            "village.home_2"
        );

        let duplicate = engine.apply(env(
            "char_mira",
            Command::CreateCharacter {
                name: "Mira Again".to_string(),
                body_color: "#4ea1ff".to_string(),
                face_color: "#101820".to_string(),
            },
        ));
        assert_eq!(duplicate.error.unwrap().code, "character_exists");

        let invalid_color = engine.apply(env(
            "char_bad",
            Command::CreateCharacter {
                name: "Bad".to_string(),
                body_color: "blue".to_string(),
                face_color: "#101820".to_string(),
            },
        ));
        assert_eq!(invalid_color.error.unwrap().code, "invalid_color");
    }

    #[test]
    fn observations_are_filtered_and_include_affordances() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        create(&mut engine, "char_ren", "Ren");
        move_to(&mut engine, "char_mira", "village.main_street");
        move_to(&mut engine, "char_ren", "village.main_street");

        let observation = engine.observe("char_mira").unwrap();
        assert_eq!(observation.actor.id, "char_mira");
        assert!(observation.valid_until_tick > observation.observed_at_tick);
        assert_eq!(
            observation.staleness_policy,
            "valid_if_local_state_compatible"
        );
        assert!(
            observation
                .nearby_entities
                .iter()
                .any(|entity| entity.id == "char_ren" && entity.entity_type == "character")
        );
        assert!(
            observation
                .available_actions
                .iter()
                .any(|action| action.action == "move")
        );
        assert_eq!(observation.world_time.tick, engine.state().tick);
    }

    #[test]
    fn movement_supports_targets_directions_and_validation() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");

        let unreachable = engine.apply(env(
            "char_mira",
            Command::Move {
                mode: MoveMode::ToTarget {
                    target: "village.cafe".to_string(),
                },
            },
        ));
        assert_eq!(unreachable.error.unwrap().code, "target_not_reachable");

        let response = engine.apply(env(
            "char_mira",
            Command::Move {
                mode: MoveMode::Direction {
                    direction: Direction::Forward,
                    distance: 1,
                },
            },
        ));
        assert!(response.ok);
        let Some(CommandResult::ActivityStarted {
            estimated_ticks,
            movement_path,
            started_at_tick,
            completes_at_tick,
            ..
        }) = response.result
        else {
            panic!("expected movement activity");
        };
        assert!(estimated_ticks >= MOVE_BASE_TICKS);
        assert_eq!(movement_path.len(), 2);
        assert_eq!(completes_at_tick - started_at_tick, estimated_ticks);
        engine.advance_ticks(estimated_ticks);
        assert_eq!(
            engine.state().characters["char_mira"].location_id,
            "village.main_street"
        );

        let bad_distance = engine.apply(env(
            "char_mira",
            Command::Move {
                mode: MoveMode::Direction {
                    direction: Direction::North,
                    distance: 0,
                },
            },
        ));
        assert_eq!(bad_distance.error.unwrap().code, "invalid_distance");
    }

    #[test]
    fn command_freshness_and_preconditions_are_enforced() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        let stale = engine.apply(CommandEnvelope {
            valid_until_tick: Some(0),
            ..env("char_mira", Command::Wait { ticks: 1 })
        });
        assert!(stale.ok);
        let stale = engine.apply(CommandEnvelope {
            valid_until_tick: Some(0),
            ..env("char_mira", Command::Observe)
        });
        assert_eq!(stale.error.unwrap().code, "stale_command");

        let changed = engine.apply(CommandEnvelope {
            local_state_hash: Some("wrong".to_string()),
            ..env("char_mira", Command::Observe)
        });
        assert_eq!(changed.error.unwrap().code, "local_state_changed");

        let failed_precondition = engine.apply(CommandEnvelope {
            preconditions: vec![Precondition {
                entity: "village.cafe".to_string(),
                condition: PreconditionKind::ActorAtLocation,
            }],
            ..env("char_mira", Command::Observe)
        });
        assert_eq!(
            failed_precondition.error.unwrap().code,
            "precondition_failed"
        );
    }

    #[test]
    fn speech_creates_visible_conversation_and_rejects_distant_target() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        create(&mut engine, "char_ren", "Ren");
        move_to(&mut engine, "char_mira", "village.main_street");

        let distant = engine.apply(env(
            "char_mira",
            Command::Say {
                target: SpeechTarget::Character("char_ren".to_string()),
                text: "Hello?".to_string(),
            },
        ));
        assert_eq!(distant.error.unwrap().code, "target_not_audible");

        move_to(&mut engine, "char_ren", "village.main_street");
        let spoken = engine.apply(env(
            "char_mira",
            Command::Say {
                target: SpeechTarget::Character("char_ren".to_string()),
                text: "Want coffee?".to_string(),
            },
        ));
        assert!(spoken.ok);
        let observation = engine.observe("char_ren").unwrap();
        assert_eq!(observation.conversations.len(), 1);
        assert_eq!(
            observation.conversations[0].recent_messages[0].text,
            "Want coffee?"
        );

        let empty = engine.apply(env(
            "char_mira",
            Command::Say {
                target: SpeechTarget::Room,
                text: " ".to_string(),
            },
        ));
        assert_eq!(empty.error.unwrap().code, "empty_speech");
    }

    #[test]
    fn look_at_reports_visible_entities() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        move_to(&mut engine, "char_mira", "village.main_street");
        let looked = engine.apply(env(
            "char_mira",
            Command::LookAt {
                target: "village.cafe".to_string(),
            },
        ));
        assert!(looked.ok);
        assert!(matches!(
            looked.result.unwrap(),
            CommandResult::LookedAt { .. }
        ));

        let hidden = engine.apply(env(
            "char_mira",
            Command::LookAt {
                target: "village.cafe.service_window".to_string(),
            },
        ));
        assert_eq!(hidden.error.unwrap().code, "not_visible");
    }

    #[test]
    fn home_manual_and_lock_rules_work() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        create(&mut engine, "char_ren", "Ren");

        let manual = engine.apply(env("char_mira", Command::HomeManual));
        assert!(matches!(
            manual.result.unwrap(),
            CommandResult::HomeManual { .. }
        ));

        let lock = engine.apply(env(
            "char_mira",
            Command::Home {
                action: HomeAction::Lock,
            },
        ));
        assert!(lock.ok);
        move_to(&mut engine, "char_ren", "village.main_street");
        let blocked = engine.apply(env(
            "char_ren",
            Command::Move {
                mode: MoveMode::ToTarget {
                    target: "village.home_1".to_string(),
                },
            },
        ));
        assert_eq!(blocked.error.unwrap().code, "home_locked");

        let unlock = engine.apply(env(
            "char_mira",
            Command::Home {
                action: HomeAction::Unlock,
            },
        ));
        assert!(unlock.ok);
        let allowed = engine.apply(env(
            "char_ren",
            Command::Move {
                mode: MoveMode::ToTarget {
                    target: "village.home_1".to_string(),
                },
            },
        ));
        assert!(allowed.ok);
    }

    #[test]
    fn service_order_reserves_spends_and_notifies() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        move_to(&mut engine, "char_mira", "village.main_street");
        move_to(&mut engine, "char_mira", "village.cafe");

        let wrong_item = engine.apply(env(
            "char_mira",
            Command::Order {
                service_id: "village.cafe.service_window".to_string(),
                item: "tea".to_string(),
            },
        ));
        assert_eq!(wrong_item.error.unwrap().code, "item_unavailable");

        let order = engine.apply(env(
            "char_mira",
            Command::Order {
                service_id: "village.cafe.service_window".to_string(),
                item: "coffee".to_string(),
            },
        ));
        assert!(order.ok);
        assert_eq!(engine.state().characters["char_mira"].reserved_coins, 2);
        engine.advance_ticks(10);
        assert_eq!(engine.state().characters["char_mira"].coins, 8);
        assert_eq!(engine.state().characters["char_mira"].reserved_coins, 0);
        assert_eq!(engine.notifications_for("char_mira", false).len(), 1);

        let notification_id = engine.notifications_for("char_mira", false)[0]
            .notification_id
            .clone();
        let ack = engine.apply(env(
            "char_mira",
            Command::Notifications {
                action: NotificationAction::Ack { notification_id },
            },
        ));
        assert!(ack.ok);
        assert!(engine.notifications_for("char_mira", false).is_empty());
    }

    #[test]
    fn generic_activity_site_earns_coins_and_notifies() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        move_to(&mut engine, "char_mira", "village.main_street");
        move_to(&mut engine, "char_mira", "village.office");

        let observation = engine.observe("char_mira").unwrap();
        assert!(observation.nearby_entities.iter().any(|entity| {
            entity.entity_type == "activity_site" && entity.id == "village.office.workstation"
        }));
        assert!(observation.available_actions.iter().any(|action| {
            action.action == "perform_activity"
                && action.targets == vec!["village.office.workstation".to_string()]
        }));

        let start = engine.apply(env(
            "char_mira",
            Command::PerformActivity {
                site_id: "village.office.workstation".to_string(),
            },
        ));
        assert!(start.ok, "{start:?}");
        assert_eq!(
            engine.state().characters["char_mira"].status,
            CharacterStatus::Performing
        );
        engine.advance_ticks(20);
        assert_eq!(engine.state().characters["char_mira"].coins, 11);
        assert!(engine.events().iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::CoinsEarned {
                    character_id,
                    amount: 1,
                    source_id,
                } if character_id == "char_mira" && source_id == "village.office.workstation"
            )
        }));
        assert!(
            engine
                .notifications_for("char_mira", false)
                .iter()
                .any(|notification| notification.kind == "activity_completed")
        );
    }

    #[test]
    fn activity_site_validates_location_busy_state_and_coin_cap() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");

        let distant = engine.apply(env(
            "char_mira",
            Command::PerformActivity {
                site_id: "village.office.workstation".to_string(),
            },
        ));
        assert_eq!(distant.error.unwrap().code, "activity_site_not_nearby");

        move_to(&mut engine, "char_mira", "village.main_street");
        move_to(&mut engine, "char_mira", "village.office");
        let actor = engine.state.characters.get_mut("char_mira").unwrap();
        actor.coins = engine.state.world.max_coins;
        let capped = engine.apply(env(
            "char_mira",
            Command::PerformActivity {
                site_id: "village.office.workstation".to_string(),
            },
        ));
        assert_eq!(capped.error.unwrap().code, "coin_cap_reached");

        engine.state.characters.get_mut("char_mira").unwrap().coins = 10;
        let started = engine.apply(env(
            "char_mira",
            Command::PerformActivity {
                site_id: "village.park.bench".to_string(),
            },
        ));
        assert_eq!(started.error.unwrap().code, "activity_site_not_nearby");

        let started = engine.apply(env(
            "char_mira",
            Command::PerformActivity {
                site_id: "village.office.workstation".to_string(),
            },
        ));
        assert!(started.ok);
        let busy = engine.apply(env(
            "char_mira",
            Command::PerformActivity {
                site_id: "village.office.workstation".to_string(),
            },
        ));
        assert_eq!(busy.error.unwrap().code, "actor_busy");
    }

    #[test]
    fn vending_machine_uses_service_order_path_with_spend_metadata() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        move_to(&mut engine, "char_mira", "village.main_street");
        move_to(&mut engine, "char_mira", "village.office");

        let order = engine.apply(env(
            "char_mira",
            Command::Order {
                service_id: "village.office.vending_machine".to_string(),
                item: "sparkling_water".to_string(),
            },
        ));
        assert!(order.ok, "{order:?}");
        engine.advance_ticks(5);
        assert_eq!(engine.state().characters["char_mira"].coins, 9);
        assert!(engine.events().iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::CoinsSpent {
                    character_id,
                    amount: 1,
                    source_id: Some(source_id),
                    item: Some(item),
                } if character_id == "char_mira"
                    && source_id == "village.office.vending_machine"
                    && item == "sparkling_water"
            )
        }));
    }

    #[test]
    fn server_state_has_no_plans_or_needs_records() {
        let engine = engine();
        let snapshot = serde_json::to_string(engine.state()).unwrap();
        assert!(!snapshot.contains("\"plans\""));
        assert!(!snapshot.contains("\"routines\""));
        assert!(!snapshot.contains("\"needs\""));
        assert!(!snapshot.contains("\"social_battery\""));
    }

    #[test]
    fn stored_snapshots_merge_new_world_definition_config() {
        let engine = engine();
        let mut stored = engine.state().clone();
        stored
            .world
            .locations
            .retain(|location| location.id != "village.office");
        stored.world.activity_sites.clear();
        stored
            .world
            .services
            .retain(|service| service.id != "village.office.vending_machine");
        let refreshed =
            Engine::from_snapshot_with_world_definition(stored, engine.events().to_vec(), world())
                .unwrap();

        assert!(
            refreshed
                .state()
                .world
                .locations
                .iter()
                .any(|location| location.id == "village.office")
        );
        assert!(
            refreshed
                .state()
                .world
                .activity_sites
                .iter()
                .any(|site| site.id == "village.office.workstation")
        );
        assert!(
            refreshed
                .state()
                .world
                .services
                .iter()
                .any(|service| service.id == "village.office.vending_machine")
        );
    }

    #[test]
    fn queue_executes_steps_and_reserves_spending_upfront() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        move_to(&mut engine, "char_mira", "village.main_street");

        let queued = engine.apply(env(
            "char_mira",
            Command::Queue {
                actions: vec![
                    QueuedCommand {
                        command: QueueableCommand::Move {
                            mode: MoveMode::ToTarget {
                                target: "village.cafe".to_string(),
                            },
                        },
                    },
                    QueuedCommand {
                        command: QueueableCommand::Order {
                            service_id: "village.cafe.service_window".to_string(),
                            item: "coffee".to_string(),
                        },
                    },
                    QueuedCommand {
                        command: QueueableCommand::Move {
                            mode: MoveMode::ToTarget {
                                target: "village.main_street".to_string(),
                            },
                        },
                    },
                ],
            },
        ));
        assert!(queued.ok, "{queued:?}");
        assert_eq!(engine.state().characters["char_mira"].reserved_coins, 2);
        engine.advance_ticks(6);
        assert_eq!(
            engine.state().characters["char_mira"].location_id,
            "village.cafe"
        );
        assert_eq!(
            engine.state().characters["char_mira"]
                .current_activity
                .as_ref()
                .unwrap()
                .kind,
            ActivityKind::Ordering
        );
        engine.advance_ticks(10);
        assert_eq!(engine.state().characters["char_mira"].coins, 8);
        engine.advance_ticks(20);
        assert_eq!(
            engine.state().characters["char_mira"].location_id,
            "village.main_street"
        );
    }

    #[test]
    fn queue_validation_and_failure_cleanup_work() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        let too_long = engine.apply(env(
            "char_mira",
            Command::Queue {
                actions: vec![
                    QueuedCommand {
                        command: QueueableCommand::Wait { ticks: 1 },
                    },
                    QueuedCommand {
                        command: QueueableCommand::Wait { ticks: 1 },
                    },
                    QueuedCommand {
                        command: QueueableCommand::Wait { ticks: 1 },
                    },
                    QueuedCommand {
                        command: QueueableCommand::Wait { ticks: 1 },
                    },
                ],
            },
        ));
        assert_eq!(too_long.error.unwrap().code, "queue_too_long");

        let failing = engine.apply(env(
            "char_mira",
            Command::Queue {
                actions: vec![QueuedCommand {
                    command: QueueableCommand::Order {
                        service_id: "village.cafe.service_window".to_string(),
                        item: "coffee".to_string(),
                    },
                }],
            },
        ));
        assert!(failing.ok);
        assert_eq!(engine.state().characters["char_mira"].reserved_coins, 0);
        assert!(
            engine
                .events()
                .iter()
                .any(|event| matches!(event.kind, EventKind::QueueStepFailed { .. }))
        );
    }

    #[test]
    fn snapshot_round_trip_and_command_replay_are_deterministic() {
        let mut engine = engine();
        let commands = vec![
            env(
                "char_mira",
                Command::CreateCharacter {
                    name: "Mira".to_string(),
                    body_color: "#4ea1ff".to_string(),
                    face_color: "#101820".to_string(),
                },
            ),
            env(
                "char_mira",
                Command::Move {
                    mode: MoveMode::ToTarget {
                        target: "village.main_street".to_string(),
                    },
                },
            ),
            env(
                "char_mira",
                Command::Wait {
                    ticks: MOVE_BASE_TICKS,
                },
            ),
        ];
        for command in &commands {
            assert!(engine.apply(command.clone()).ok);
        }

        let snapshot_json = serde_json::to_string(engine.state()).unwrap();
        let snapshot: WorldSnapshot = serde_json::from_str(&snapshot_json).unwrap();
        let restored = Engine::from_snapshot(snapshot, engine.events().to_vec());
        assert_eq!(restored.state(), engine.state());

        let replayed = Engine::replay(world(), &commands).unwrap();
        assert_eq!(
            replayed.state().characters["char_mira"].location_id,
            engine.state().characters["char_mira"].location_id
        );
        assert_eq!(replayed.state().tick, engine.state().tick);
    }

    #[test]
    fn inactive_characters_return_home() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        move_to(&mut engine, "char_mira", "village.main_street");
        engine.advance_ticks(OFFLINE_RETURN_HOME_TICKS);
        assert_eq!(
            engine.state().characters["char_mira"].location_id,
            "village.home_1"
        );
    }

    #[test]
    fn world_growth_adds_capacity_deterministically() {
        let mut first = engine();
        let mut second = engine();
        for index in 0..5 {
            create(
                &mut first,
                &format!("char_a_{index}"),
                &format!("A {index}"),
            );
            create(
                &mut second,
                &format!("char_a_{index}"),
                &format!("A {index}"),
            );
        }
        assert!(first.state().world.homes.len() > 3);
        assert_eq!(first.state().world, second.state().world);
        assert!(
            first
                .events()
                .iter()
                .any(|event| matches!(event.kind, EventKind::WorldExpanded { .. }))
        );
    }

    #[test]
    fn utility_accessors_and_zero_tick_advance_behave() {
        let mut engine = engine();
        assert_eq!(engine.events_after(Some(999)).len(), 0);
        assert_eq!(engine.command_log().len(), 0);
        engine.advance_ticks(0);
        assert_eq!(engine.state().tick, 0);
        assert_eq!(engine.events_after(Some(0)).len(), 1);
    }

    #[test]
    fn history_compaction_keeps_recent_events_and_commands() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        for index in 0..5 {
            assert!(
                engine
                    .apply(env("char_mira", Command::Wait { ticks: index + 1 }))
                    .ok
            );
        }

        let newest_event_id = engine.events().last().unwrap().id;
        assert!(engine.compact_history(3, 2));
        assert_eq!(engine.events().len(), 3);
        assert_eq!(engine.command_log().len(), 2);
        assert_eq!(engine.events().last().unwrap().id, newest_event_id);
        assert_eq!(
            engine
                .events_after(Some(newest_event_id - 1))
                .last()
                .unwrap()
                .id,
            newest_event_id
        );
        assert!(!engine.compact_history(3, 2));
    }

    #[test]
    fn unknown_and_missing_state_errors_are_structured() {
        let mut engine = engine();
        let unknown_observe = engine.apply(env("missing", Command::Observe));
        assert_eq!(unknown_observe.error.unwrap().code, "unknown_character");

        create(&mut engine, "char_mira", "Mira");
        let missing_target = engine.apply(env(
            "char_mira",
            Command::Move {
                mode: MoveMode::ToTarget {
                    target: "missing".to_string(),
                },
            },
        ));
        assert_eq!(missing_target.error.unwrap().code, "unknown_target");

        engine
            .state
            .characters
            .get_mut("char_mira")
            .unwrap()
            .location_id = "missing".to_string();
        assert_eq!(
            engine.observe("char_mira").unwrap_err().code,
            "location_missing"
        );
    }

    #[test]
    fn service_validation_covers_nearby_unknown_and_funds() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        let unknown = engine.apply(env(
            "char_mira",
            Command::Order {
                service_id: "missing".to_string(),
                item: "coffee".to_string(),
            },
        ));
        assert_eq!(unknown.error.unwrap().code, "unknown_service");

        let not_nearby = engine.apply(env(
            "char_mira",
            Command::Order {
                service_id: "village.cafe.service_window".to_string(),
                item: "coffee".to_string(),
            },
        ));
        assert_eq!(not_nearby.error.unwrap().code, "service_not_nearby");

        move_to(&mut engine, "char_mira", "village.main_street");
        move_to(&mut engine, "char_mira", "village.cafe");
        engine.state.characters.get_mut("char_mira").unwrap().coins = 1;
        let poor = engine.apply(env(
            "char_mira",
            Command::Order {
                service_id: "village.cafe.service_window".to_string(),
                item: "coffee".to_string(),
            },
        ));
        assert_eq!(poor.error.unwrap().code, "insufficient_coins");
    }

    #[test]
    fn home_enter_leave_and_errors_are_covered() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        let enter_at_home = engine.apply(env(
            "char_mira",
            Command::Home {
                action: HomeAction::Enter,
            },
        ));
        assert!(enter_at_home.ok);
        assert_eq!(
            engine.state().characters["char_mira"].status,
            CharacterStatus::InsideHome
        );

        let leave = engine.apply(env(
            "char_mira",
            Command::Home {
                action: HomeAction::Leave,
            },
        ));
        assert!(leave.ok);
        engine.advance_ticks(20);
        let leave_again = engine.apply(env(
            "char_mira",
            Command::Home {
                action: HomeAction::Leave,
            },
        ));
        assert_eq!(leave_again.error.unwrap().code, "not_at_home");
    }

    #[test]
    fn notification_listing_and_error_paths_work() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        create(&mut engine, "char_ren", "Ren");
        engine.create_notification(
            "char_mira",
            "test",
            "A test notification.",
            [("x".to_string(), "y".to_string())],
        );
        let listed = engine.apply(env(
            "char_mira",
            Command::Notifications {
                action: NotificationAction::List,
            },
        ));
        assert!(matches!(
            listed.result.unwrap(),
            CommandResult::Notifications { .. }
        ));
        let notification_id = engine.notifications_for("char_mira", false)[0]
            .notification_id
            .clone();
        let wrong_owner = engine.apply(env(
            "char_ren",
            Command::Notifications {
                action: NotificationAction::Ack {
                    notification_id: notification_id.clone(),
                },
            },
        ));
        assert_eq!(wrong_owner.error.unwrap().code, "notification_not_owned");
        let unknown = engine.apply(env(
            "char_mira",
            Command::Notifications {
                action: NotificationAction::Ack {
                    notification_id: "missing".to_string(),
                },
            },
        ));
        assert_eq!(unknown.error.unwrap().code, "unknown_notification");
    }

    #[test]
    fn observe_agent_returns_compact_runtime_payload() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        create(&mut engine, "char_ren", "Ren");
        move_to(&mut engine, "char_mira", "village.main_street");
        move_to(&mut engine, "char_ren", "village.main_street");
        let response = engine.apply(env(
            "char_ren",
            Command::Say {
                target: SpeechTarget::Character("char_mira".to_string()),
                text: "Coffee later?".to_string(),
            },
        ));
        assert!(response.ok, "{response:?}");

        let observation = engine.observe_agent("char_mira").unwrap();
        assert_eq!(observation.wake_reason, "directed_speech");
        assert_eq!(observation.actor.id, "char_mira");
        assert_eq!(
            observation.limits.max_actions_this_wake,
            MAX_ACTIONS_PER_WAKE
        );
        assert!(
            observation
                .nearby_agents
                .iter()
                .any(|agent| agent.id == "char_ren")
        );
        assert!(observation.notifications.iter().any(|notification| {
            notification.kind == "directed_speech" && notification.character_id == "char_mira"
        }));
        assert!(
            observation
                .memory_hints
                .stable_ids
                .iter()
                .any(|id| id == "char_ren")
        );
        assert!(
            observation
                .available_affordances
                .iter()
                .any(|action| action.action == "say")
        );
    }

    #[test]
    fn directed_speech_arrivals_and_queues_create_wake_notifications() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        create(&mut engine, "char_ren", "Ren");
        move_to(&mut engine, "char_mira", "village.main_street");
        move_to(&mut engine, "char_ren", "village.main_street");

        let direct = engine.apply(env(
            "char_mira",
            Command::Say {
                target: SpeechTarget::Character("char_ren".to_string()),
                text: "Hello Ren.".to_string(),
            },
        ));
        assert!(direct.ok, "{direct:?}");
        let ren_notifications = engine.notifications_for("char_ren", false);
        assert_eq!(
            ren_notifications
                .iter()
                .filter(|notification| notification.kind == "directed_speech")
                .count(),
            1
        );

        move_to(&mut engine, "char_ren", "village.cafe");
        move_to(&mut engine, "char_mira", "village.cafe");
        assert!(
            engine
                .notifications_for("char_ren", false)
                .iter()
                .any(|notification| notification.kind == "same_location_entry")
        );

        let queue = engine.apply(env(
            "char_mira",
            Command::Queue {
                actions: vec![QueuedCommand {
                    command: QueueableCommand::Wait { ticks: 1 },
                }],
            },
        ));
        assert!(queue.ok, "{queue:?}");
        engine.advance_ticks(1);
        assert!(
            engine
                .notifications_for("char_mira", false)
                .iter()
                .any(|notification| notification.kind == "queue_completed")
        );

        create(&mut engine, "char_otto", "Otto");
        let failed_queue = engine.apply(env(
            "char_otto",
            Command::Queue {
                actions: vec![QueuedCommand {
                    command: QueueableCommand::Order {
                        service_id: "village.cafe.service_window".to_string(),
                        item: "coffee".to_string(),
                    },
                }],
            },
        ));
        assert!(failed_queue.ok, "{failed_queue:?}");
        assert!(
            engine
                .notifications_for("char_otto", false)
                .iter()
                .any(|notification| notification.kind == "queue_failed")
        );
    }

    #[test]
    fn queue_empty_insufficient_and_wait_steps_are_covered() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        let empty = engine.apply(env("char_mira", Command::Queue { actions: vec![] }));
        assert_eq!(empty.error.unwrap().code, "empty_queue");

        engine.state.characters.get_mut("char_mira").unwrap().coins = 1;
        let cannot_reserve = engine.apply(env(
            "char_mira",
            Command::Queue {
                actions: vec![QueuedCommand {
                    command: QueueableCommand::Order {
                        service_id: "village.cafe.service_window".to_string(),
                        item: "coffee".to_string(),
                    },
                }],
            },
        ));
        assert_eq!(cannot_reserve.error.unwrap().code, "insufficient_coins");

        engine.state.characters.get_mut("char_mira").unwrap().coins = 10;
        let waits = engine.apply(env(
            "char_mira",
            Command::Queue {
                actions: vec![QueuedCommand {
                    command: QueueableCommand::Wait { ticks: 2 },
                }],
            },
        ));
        assert!(waits.ok);
        assert_eq!(
            engine.state().characters["char_mira"]
                .current_activity
                .as_ref()
                .unwrap()
                .kind,
            ActivityKind::Waiting
        );
        engine.advance_ticks(2);
        assert!(
            engine.state().characters["char_mira"]
                .current_activity
                .is_none()
        );
    }

    #[test]
    fn precondition_success_shouting_and_message_window_are_covered() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        move_to(&mut engine, "char_mira", "village.main_street");
        let visible_precondition = engine.apply(CommandEnvelope {
            preconditions: vec![Precondition {
                entity: "village.cafe".to_string(),
                condition: PreconditionKind::NearbyOrVisible,
            }],
            ..env("char_mira", Command::Observe)
        });
        assert!(visible_precondition.ok);

        for index in 0..13 {
            let response = engine.apply(env(
                "char_mira",
                Command::Say {
                    target: SpeechTarget::Shout,
                    text: format!("message {index}"),
                },
            ));
            assert!(response.ok);
        }
        let observation = engine.observe("char_mira").unwrap();
        assert_eq!(observation.conversations[0].recent_messages.len(), 12);
        assert_eq!(
            observation.conversations[0].recent_messages[0].text,
            "message 1"
        );
    }

    #[test]
    fn social_invites_replies_and_life_wake_recommendations_are_factual() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        create(&mut engine, "char_ren", "Ren");
        move_to(&mut engine, "char_mira", "village.main_street");
        move_to(&mut engine, "char_ren", "village.main_street");
        move_to(&mut engine, "char_mira", "village.park");
        move_to(&mut engine, "char_ren", "village.park");

        let invite = engine.apply(env(
            "char_mira",
            Command::Invite {
                target_character_id: "char_ren".to_string(),
                action: "invite_chess".to_string(),
                target_id: "village.park.chess_board_1".to_string(),
                message: "Chess?".to_string(),
            },
        ));
        assert!(invite.ok, "{invite:?}");
        assert!(
            engine
                .notifications_for("char_ren", false)
                .iter()
                .any(|notification| notification.kind == "chess_invite")
        );
        let ren_wake = engine.observe_agent("char_ren").unwrap();
        assert_eq!(ren_wake.wake_reason, "chess_invite");
        assert!(
            ren_wake
                .recommended_actions
                .iter()
                .any(|recommendation| recommendation.reason == "chess_invite")
        );
        assert!(
            ren_wake
                .memory_hints
                .recent_interactions
                .iter()
                .any(|interaction| interaction.pending_invite_id.is_some())
        );

        let invite_id = engine.state.public_invites.keys().next().cloned().unwrap();
        let accept = engine.apply(env(
            "char_ren",
            Command::RespondInvite {
                invite_id: invite_id.clone(),
                accept: true,
            },
        ));
        assert!(accept.ok, "{accept:?}");
        assert_eq!(
            engine.state.public_invites[&invite_id].status,
            InviteStatus::Accepted
        );
        assert_eq!(engine.state.external_games.len(), 1);
        let reserved_game_id = engine.state.external_games.keys().next().cloned().unwrap();
        let register_reserved = engine.apply(env(
            "char_mira",
            Command::Chess {
                action: ChessCommand::RegisterExternalGame {
                    board_id: "village.park.chess_board_1".to_string(),
                    opponent_character_id: "char_ren".to_string(),
                    provider: "lichess".to_string(),
                    external_game_id: "invite123".to_string(),
                    url: "https://lichess.org/invite123".to_string(),
                },
            },
        ));
        assert!(register_reserved.ok, "{register_reserved:?}");
        assert_eq!(
            engine.state.external_games[&reserved_game_id].external_game_id,
            "invite123"
        );

        let target_event_id = engine
            .events()
            .iter()
            .find(|event| matches!(event.kind, EventKind::InviteCreated { .. }))
            .unwrap()
            .id;
        let reply = engine.apply(env(
            "char_ren",
            Command::ReplyTo {
                target_event_id,
                target: SpeechTarget::Character("char_mira".to_string()),
                text: "Accepted.".to_string(),
            },
        ));
        assert!(reply.ok, "{reply:?}");
        assert!(
            engine
                .events()
                .iter()
                .any(|event| matches!(event.kind, EventKind::ReplySpoken { .. }))
        );
    }

    #[test]
    fn public_interactables_post_notices_and_pay_for_errands() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        create(&mut engine, "char_ren", "Ren");
        move_to(&mut engine, "char_mira", "village.main_street");
        move_to(&mut engine, "char_ren", "village.main_street");

        let post = engine.apply(env(
            "char_mira",
            Command::UseInteractable {
                target_id: "village.main_street.notice_board".to_string(),
                action: "post_notice".to_string(),
                args: BTreeMap::from([("text".to_string(), "Meet by the cafe.".to_string())]),
            },
        ));
        assert!(post.ok, "{post:?}");
        assert_eq!(engine.state.public_notices.len(), 1);
        assert!(
            engine
                .notifications_for("char_ren", false)
                .iter()
                .any(|notification| notification.kind == "notice_posted")
        );

        engine.state.characters.get_mut("char_mira").unwrap().coins = 1;
        let errand = engine.apply(env(
            "char_mira",
            Command::UseInteractable {
                target_id: "village.main_street.job_board".to_string(),
                action: "take_errand".to_string(),
                args: BTreeMap::new(),
            },
        ));
        assert!(errand.ok, "{errand:?}");
        engine.advance_ticks(12);
        assert_eq!(engine.state.characters["char_mira"].coins, 2);
        assert!(engine
            .events()
            .iter()
            .any(|event| matches!(&event.kind, EventKind::CoinsEarned { source_id, .. } if source_id == "village.main_street.job_board")));
    }

    #[test]
    fn chess_registers_external_games_and_confirms_results_without_moves() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        create(&mut engine, "char_ren", "Ren");
        move_to(&mut engine, "char_mira", "village.main_street");
        move_to(&mut engine, "char_ren", "village.main_street");
        move_to(&mut engine, "char_mira", "village.park");
        move_to(&mut engine, "char_ren", "village.park");

        let register = engine.apply(env(
            "char_mira",
            Command::Chess {
                action: ChessCommand::RegisterExternalGame {
                    board_id: "village.park.chess_board_1".to_string(),
                    opponent_character_id: "char_ren".to_string(),
                    provider: "lichess".to_string(),
                    external_game_id: "abc123".to_string(),
                    url: "https://lichess.org/abc123".to_string(),
                },
            },
        ));
        assert!(register.ok, "{register:?}");
        let game_id = engine.state.external_games.keys().next().cloned().unwrap();
        let game = &engine.state.external_games[&game_id];
        assert_eq!(game.provider, "lichess");
        assert!(game.result.is_none());
        assert!(game.external_game_id == "abc123");

        let duplicate = engine.apply(env(
            "char_ren",
            Command::Chess {
                action: ChessCommand::RegisterExternalGame {
                    board_id: "village.park.chess_board_1".to_string(),
                    opponent_character_id: "char_mira".to_string(),
                    provider: "lichess".to_string(),
                    external_game_id: "def456".to_string(),
                    url: "https://lichess.org/def456".to_string(),
                },
            },
        ));
        assert_eq!(duplicate.error.unwrap().code, "board_already_reserved");

        let result = engine.apply(env(
            "char_mira",
            Command::Chess {
                action: ChessCommand::RecordResult {
                    game_id: game_id.clone(),
                    result: "1-0".to_string(),
                },
            },
        ));
        assert!(result.ok, "{result:?}");
        let confirm = engine.apply(env(
            "char_ren",
            Command::Chess {
                action: ChessCommand::ConfirmResult {
                    game_id: game_id.clone(),
                    accept: true,
                },
            },
        ));
        assert!(confirm.ok, "{confirm:?}");
        assert_eq!(
            engine.state.external_games[&game_id].status,
            ExternalGameStatus::Confirmed
        );
    }

    #[test]
    fn manually_completing_unusual_activity_shapes_is_stable() {
        let mut engine = engine();
        create(&mut engine, "char_mira", "Mira");
        engine
            .state
            .characters
            .get_mut("char_mira")
            .unwrap()
            .current_activity = Some(Activity {
            id: "activity.return.test".to_string(),
            kind: ActivityKind::ReturningHome,
            status: ActivityStatus::Active,
            target_id: Some("village.home_1".to_string()),
            movement_path: Vec::new(),
            started_at_tick: 0,
            completes_at_tick: 1,
            description: "return".to_string(),
            promise_id: None,
            reserved_coins: 0,
            queued: false,
        });
        engine.advance_ticks(1);
        assert_eq!(
            engine.state().characters["char_mira"].status,
            CharacterStatus::InsideHome
        );

        engine
            .state
            .characters
            .get_mut("char_mira")
            .unwrap()
            .current_activity = Some(Activity {
            id: "activity.move.no_target".to_string(),
            kind: ActivityKind::Moving,
            status: ActivityStatus::Active,
            target_id: None,
            movement_path: Vec::new(),
            started_at_tick: 1,
            completes_at_tick: 2,
            description: "no target".to_string(),
            promise_id: None,
            reserved_coins: 0,
            queued: false,
        });
        engine.advance_ticks(1);
        assert!(
            engine.state().characters["char_mira"]
                .current_activity
                .is_none()
        );
    }
}
