use fishtank_core::{Engine, MOVE_BASE_TICKS};
use fishtank_protocol::{
    Command, CommandEnvelope, Direction, FacingDirection, GridPosition, GridSize, GroundType,
    HomeAction, HomeDefinition, LocationDefinition, MoveMode, Precondition, PreconditionKind,
    QueueableCommand, QueuedCommand, SCHEMA_VERSION, ServiceDefinition, SpeechTarget,
    WorldDefinition, WorldGrid,
};
use std::collections::BTreeMap;
use time::OffsetDateTime;

const GRID_WIDTH: usize = 36;
const GRID_HEIGHT: usize = 36;
const PLAYER_COUNT: usize = 1_200;
const ACTION_COUNT: usize = 7_500;

#[test]
fn generated_world_handles_thousands_of_players_and_actions_deterministically() {
    let world = generated_world(GRID_WIDTH, GRID_HEIGHT, PLAYER_COUNT);
    let mut first = Engine::new(world.clone()).unwrap();
    let mut second = Engine::new(world.clone()).unwrap();
    let mut plan = Plan::new(world.seed, GRID_WIDTH, GRID_HEIGHT, PLAYER_COUNT);

    for player_index in 0..PLAYER_COUNT {
        let command = create_character(player_index);
        assert!(first.apply(command.clone()).ok);
        assert!(second.apply(command).ok);
    }

    let mut accepted = 0;
    let mut rejected = 0;
    for step in 0..ACTION_COUNT {
        if step % 32 == 0 {
            let ticks = 1 + (step as u64 % 5);
            first.advance_ticks(ticks);
            second.advance_ticks(ticks);
        }

        let command = plan.next_command(step);
        let first_response = first.apply(command.clone());
        let second_response = second.apply(command);
        assert_eq!(first_response.ok, second_response.ok, "step {step}");
        assert_eq!(
            first_response
                .error
                .as_ref()
                .map(|error| error.code.as_str()),
            second_response
                .error
                .as_ref()
                .map(|error| error.code.as_str()),
            "step {step}"
        );

        if first_response.ok {
            accepted += 1;
        } else {
            rejected += 1;
        }
    }

    assert!(accepted > ACTION_COUNT / 2, "accepted={accepted}");
    assert!(rejected > 100, "rejected={rejected}");
    assert_eq!(first.state(), second.state());
    assert_eq!(first.events(), second.events());
    assert_eq!(first.command_log().len(), accepted + PLAYER_COUNT);
    assert!(first.events().len() > accepted);
}

#[test]
fn generated_world_edge_cases_reject_invalid_state_transitions() {
    let world = generated_world(12, 12, 64);
    let mut engine = Engine::new(world).unwrap();
    assert!(engine.apply(create_character(0)).ok);
    assert!(engine.apply(create_character(1)).ok);

    let duplicate = engine.apply(create_character(0));
    assert_eq!(duplicate.error.unwrap().code, "character_exists");

    let impossible_move = engine.apply(envelope(
        0,
        "edge.impossible_move",
        Command::Move {
            mode: MoveMode::ToTarget {
                target: location_id(11, 11),
            },
        },
    ));
    assert_eq!(impossible_move.error.unwrap().code, "target_not_reachable");

    let stale = engine.apply(CommandEnvelope {
        valid_until_tick: Some(0),
        ..envelope(0, "edge.wait", Command::Wait { ticks: 1 })
    });
    assert!(stale.ok);
    let stale = engine.apply(CommandEnvelope {
        valid_until_tick: Some(0),
        ..envelope(0, "edge.stale", Command::Observe)
    });
    assert_eq!(stale.error.unwrap().code, "stale_command");

    let changed = engine.apply(CommandEnvelope {
        local_state_hash: Some("wrong".to_string()),
        ..envelope(0, "edge.hash", Command::Observe)
    });
    assert_eq!(changed.error.unwrap().code, "local_state_changed");

    let blocked_precondition = engine.apply(CommandEnvelope {
        preconditions: vec![Precondition {
            entity: location_id(7, 7),
            condition: PreconditionKind::ActorAtLocation,
        }],
        ..envelope(0, "edge.precondition", Command::Observe)
    });
    assert_eq!(
        blocked_precondition.error.unwrap().code,
        "precondition_failed"
    );

    let lock = engine.apply(envelope(
        0,
        "edge.lock",
        Command::Home {
            action: HomeAction::Lock,
        },
    ));
    assert!(lock.ok);
    let blocked_home = engine.apply(envelope(
        1,
        "edge.blocked_home",
        Command::Move {
            mode: MoveMode::ToTarget {
                target: location_id(0, 0),
            },
        },
    ));
    assert_eq!(blocked_home.error.unwrap().code, "home_locked");

    let queue_too_long = engine.apply(envelope(
        0,
        "edge.queue_too_long",
        Command::Queue {
            actions: vec![queued_wait(), queued_wait(), queued_wait(), queued_wait()],
        },
    ));
    assert_eq!(queue_too_long.error.unwrap().code, "queue_too_long");

    engine.advance_ticks(MOVE_BASE_TICKS);
    let observe = engine.apply(envelope(0, "edge.observe", Command::Observe));
    assert!(observe.ok);
}

fn queued_wait() -> QueuedCommand {
    QueuedCommand {
        command: QueueableCommand::Wait { ticks: 1 },
    }
}

struct Plan {
    rng: Lcg,
    width: usize,
    height: usize,
    player_count: usize,
    positions: Vec<(usize, usize)>,
}

impl Plan {
    fn new(seed: u64, width: usize, height: usize, player_count: usize) -> Self {
        let positions = (0..player_count)
            .map(|index| (index % width, (index / width) % height))
            .collect();
        Self {
            rng: Lcg::new(seed),
            width,
            height,
            player_count,
            positions,
        }
    }

    fn next_command(&mut self, step: usize) -> CommandEnvelope {
        let player_index = self.rng.next_usize(self.player_count);
        let command = match self.rng.next_u64() % 11 {
            0 => Command::Observe,
            1 => Command::Wait {
                ticks: 1 + self.rng.next_u64() % 3,
            },
            2 => Command::Say {
                target: SpeechTarget::Room,
                text: format!("step {step}"),
            },
            3 => Command::HomeManual,
            4 => Command::Home {
                action: HomeAction::ReturnHome,
            },
            5 => Command::Move {
                mode: MoveMode::ToTarget {
                    target: self.next_reachable(player_index),
                },
            },
            6 => Command::Move {
                mode: MoveMode::Direction {
                    direction: self.next_direction(),
                    distance: 1,
                },
            },
            7 => Command::LookAt {
                target: self.next_reachable(player_index),
            },
            8 => Command::Queue {
                actions: vec![QueuedCommand {
                    command: QueueableCommand::Wait { ticks: 1 },
                }],
            },
            9 => Command::Order {
                service_id: service_id(self.rng.next_usize(self.width * self.height)),
                item: "snack".to_string(),
            },
            _ => Command::Move {
                mode: MoveMode::ToTarget {
                    target: location_id(self.width - 1, self.height - 1),
                },
            },
        };
        envelope(player_index, &format!("load.{step}"), command)
    }

    fn next_reachable(&mut self, player_index: usize) -> String {
        let (x, y) = self.positions[player_index];
        let mut candidates = Vec::with_capacity(4);
        if x > 0 {
            candidates.push((x - 1, y));
        }
        if x + 1 < self.width {
            candidates.push((x + 1, y));
        }
        if y > 0 {
            candidates.push((x, y - 1));
        }
        if y + 1 < self.height {
            candidates.push((x, y + 1));
        }
        let next = candidates[self.rng.next_usize(candidates.len())];
        self.positions[player_index] = next;
        location_id(next.0, next.1)
    }

    fn next_direction(&mut self) -> Direction {
        match self.rng.next_u64() % 4 {
            0 => Direction::North,
            1 => Direction::South,
            2 => Direction::East,
            _ => Direction::West,
        }
    }
}

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        self.0
    }

    fn next_usize(&mut self, upper: usize) -> usize {
        (self.next_u64() as usize) % upper
    }
}

fn generated_world(width: usize, height: usize, player_count: usize) -> WorldDefinition {
    let mut locations = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let mut exits = Vec::with_capacity(4);
            let mut directional_exits = BTreeMap::new();
            if y > 0 {
                let target = location_id(x, y - 1);
                exits.push(target.clone());
                directional_exits.insert(Direction::North, target);
            }
            if x + 1 < width {
                let target = location_id(x + 1, y);
                exits.push(target.clone());
                directional_exits.insert(Direction::East, target);
            }
            if y + 1 < height {
                let target = location_id(x, y + 1);
                exits.push(target.clone());
                directional_exits.insert(Direction::South, target);
            }
            if x > 0 {
                let target = location_id(x - 1, y);
                exits.push(target.clone());
                directional_exits.insert(Direction::West, target);
            }

            let index = y * width + x;
            let poi_ids = (index % 9 == 0)
                .then(|| service_id(index))
                .into_iter()
                .collect();
            locations.push(LocationDefinition {
                id: location_id(x, y),
                name: format!("Block {x},{y}"),
                description: format!("Procedural stress-test block {x},{y}."),
                grid_position: GridPosition {
                    x: x as i32,
                    y: y as i32,
                },
                grid_size: GridSize {
                    width: 1,
                    height: 1,
                },
                facing: FacingDirection::South,
                exits,
                directional_exits,
                poi_ids,
                private_home: index < player_count,
            });
        }
    }

    let homes = (0..player_count)
        .map(|index| HomeDefinition {
            id: location_id(index % width, (index / width) % height),
            name: format!("Home {index}"),
            owner_character_id: None,
        })
        .collect();

    let services = (0..width * height)
        .filter(|index| index % 9 == 0)
        .map(|index| ServiceDefinition {
            id: service_id(index),
            name: format!("Snack Window {index}"),
            location_id: location_id(index % width, index / width),
            item: "snack".to_string(),
            description: "A deterministic snack service for stress tests.".to_string(),
            price_coins: 1,
            duration_ticks: 2,
            capacity: 32,
            overflow_behavior: "queue_nearby".to_string(),
        })
        .collect();

    WorldDefinition {
        schema_version: SCHEMA_VERSION.to_string(),
        id: format!("stress-grid-{width}x{height}"),
        name: "Procedural Stress Grid".to_string(),
        seed: 0x5eed_f15a,
        grid: WorldGrid {
            width: width as u32,
            height: height as u32,
            cell_size: 1,
            terrain: vec![vec![GroundType::Ground; width]; height],
        },
        starting_coins: 50,
        allowance_coins: 0,
        max_coins: 50,
        locations,
        homes,
        services,
        activity_sites: Vec::new(),
        spawn_location_id: location_id(0, 0),
    }
}

fn create_character(index: usize) -> CommandEnvelope {
    envelope(
        index,
        &format!("create.{index}"),
        Command::CreateCharacter {
            name: format!("Player {index}"),
            body_color: format!("#{:06x}", 0x202020 + index as u32 % 0xdfdfdf),
            face_color: "#f8d7a8".to_string(),
        },
    )
}

fn envelope(player_index: usize, command_id: &str, command: Command) -> CommandEnvelope {
    CommandEnvelope {
        schema_version: SCHEMA_VERSION.to_string(),
        command_id: command_id.to_string(),
        character_id: format!("player.{player_index}"),
        submitted_at: OffsetDateTime::UNIX_EPOCH.to_string(),
        based_on_tick: None,
        valid_until_tick: None,
        local_state_hash: None,
        preconditions: Vec::new(),
        command,
    }
}

fn location_id(x: usize, y: usize) -> String {
    format!("grid.{x}.{y}")
}

fn service_id(index: usize) -> String {
    format!("service.{index}")
}
