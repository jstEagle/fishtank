use fishtank_core::{Engine, MOVE_BASE_TICKS};
use fishtank_protocol::{
    Command, CommandEnvelope, Direction, FacingDirection, GridPosition, GridSize, GroundType,
    HomeAction, HomeDefinition, LocationDefinition, MoveMode, QueueableCommand, QueuedCommand,
    SCHEMA_VERSION, ServiceDefinition, SpeechTarget, WorldDefinition, WorldGrid,
};
use std::cmp::max;
use std::collections::BTreeMap;
use std::env;
use std::process::Command as ProcessCommand;
use std::time::{Duration, Instant};
use time::OffsetDateTime;

const DEFAULT_WIDTH: usize = 72;
const DEFAULT_HEIGHT: usize = 72;
const DEFAULT_PLAYERS: usize = 3_000;
const DEFAULT_ACTIONS: usize = 30_000;

fn main() {
    let width = env_usize("FISHTANK_PERF_WIDTH", DEFAULT_WIDTH);
    let height = env_usize("FISHTANK_PERF_HEIGHT", DEFAULT_HEIGHT);
    let player_count = env_usize("FISHTANK_PERF_PLAYERS", DEFAULT_PLAYERS);
    let action_count = env_usize("FISHTANK_PERF_ACTIONS", DEFAULT_ACTIONS);

    assert!(
        width * height >= player_count,
        "grid must have at least one location per player home"
    );

    let world = generated_world(width, height, player_count);
    let mut engine = Engine::new(world.clone()).expect("generated world is valid");
    let mut plan = Plan::new(world.seed, width, height, player_count);
    let mut latencies = Vec::with_capacity(player_count + action_count);
    let mut accepted = 0usize;
    let mut rejected = 0usize;

    let started = Instant::now();
    for player_index in 0..player_count {
        let latency = time_apply(
            &mut engine,
            create_character(player_index),
            &mut accepted,
            &mut rejected,
        );
        latencies.push(latency);
    }

    for step in 0..action_count {
        if step % 48 == 0 {
            let advance_started = Instant::now();
            engine.advance_ticks(1 + (step as u64 % MOVE_BASE_TICKS));
            latencies.push(advance_started.elapsed());
        }
        let command = plan.next_command(step);
        let latency = time_apply(&mut engine, command, &mut accepted, &mut rejected);
        latencies.push(latency);
    }

    let elapsed = started.elapsed();
    latencies.sort_unstable();
    let actions = player_count + action_count;
    let throughput = actions as f64 / elapsed.as_secs_f64();
    let p5 = percentile(&latencies, 5.0);
    let p50 = percentile(&latencies, 50.0);
    let p95 = percentile(&latencies, 95.0);
    let p99 = percentile(&latencies, 99.0);
    let max_latency = *latencies.last().unwrap_or(&Duration::ZERO);

    println!();
    println!("Fishtank core performance");
    println!("=========================");
    println!(
        "world:       {width}x{height} grid, {} locations, {} services",
        width * height,
        engine.state().world.services.len()
    );
    println!("players:     {player_count}");
    println!("actions:     {actions} total ({action_count} simulated after creation)");
    println!("accepted:    {accepted}");
    println!("rejected:    {rejected}");
    println!("events:      {}", engine.events().len());
    println!("commands:    {}", engine.command_log().len());
    println!("elapsed:     {}", format_duration(elapsed));
    println!("throughput:  {:.0} actions/sec", throughput);
    println!(
        "rss:         {}",
        current_rss_kib()
            .map(format_kib)
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!();
    println!("Latency");
    println!("-------");
    println!("p5:          {}", format_duration(p5));
    println!("p50:         {}", format_duration(p50));
    println!("p95:         {}", format_duration(p95));
    println!("p99:         {}", format_duration(p99));
    println!("max:         {}", format_duration(max_latency));
    println!();
    println!("Latency histogram");
    println!("-----------------");
    print_histogram(&latencies);
}

fn time_apply(
    engine: &mut Engine,
    command: CommandEnvelope,
    accepted: &mut usize,
    rejected: &mut usize,
) -> Duration {
    let started = Instant::now();
    let response = engine.apply(command);
    let latency = started.elapsed();
    if response.ok {
        *accepted += 1;
    } else {
        *rejected += 1;
    }
    latency
}

fn percentile(sorted: &[Duration], percentile: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let rank = ((percentile / 100.0) * (sorted.len().saturating_sub(1)) as f64).round() as usize;
    sorted[rank]
}

fn print_histogram(sorted: &[Duration]) {
    let buckets = [
        ("<= 10us", Duration::from_micros(10)),
        ("<= 25us", Duration::from_micros(25)),
        ("<= 50us", Duration::from_micros(50)),
        ("<= 100us", Duration::from_micros(100)),
        ("<= 250us", Duration::from_micros(250)),
        ("<= 500us", Duration::from_micros(500)),
        ("<= 1ms", Duration::from_millis(1)),
        ("<= 2ms", Duration::from_millis(2)),
        ("> 2ms", Duration::MAX),
    ];
    let mut previous = 0usize;
    let mut counts = Vec::with_capacity(buckets.len());
    for (_, ceiling) in buckets {
        let upper = sorted.partition_point(|latency| *latency <= ceiling);
        counts.push(upper - previous);
        previous = upper;
    }
    let max_count = counts.iter().copied().max().unwrap_or(1);
    for ((label, _), count) in buckets.into_iter().zip(counts) {
        let bar_len = max(1, count * 42 / max_count);
        println!("{label:>8} | {:<42} {count}", "#".repeat(bar_len));
    }
}

fn format_duration(duration: Duration) -> String {
    let nanos = duration.as_nanos();
    if nanos < 1_000 {
        format!("{nanos}ns")
    } else if nanos < 1_000_000 {
        format!("{:.2}us", nanos as f64 / 1_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.2}ms", nanos as f64 / 1_000_000.0)
    } else {
        format!("{:.2}s", duration.as_secs_f64())
    }
}

fn current_rss_kib() -> Option<u64> {
    let output = ProcessCommand::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

fn format_kib(kib: u64) -> String {
    if kib < 1024 {
        format!("{kib} KiB")
    } else {
        format!("{:.1} MiB", kib as f64 / 1024.0)
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
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
        let command = match self.rng.next_u64() % 12 {
            0 => Command::Observe,
            1 => Command::Wait {
                ticks: 1 + self.rng.next_u64() % 4,
            },
            2 => Command::Say {
                target: SpeechTarget::Room,
                text: format!("perf step {step}"),
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
            10 => Command::Queue {
                actions: vec![
                    QueuedCommand {
                        command: QueueableCommand::Wait { ticks: 1 },
                    },
                    QueuedCommand {
                        command: QueueableCommand::Say {
                            target: SpeechTarget::Room,
                            text: "queued hello".to_string(),
                        },
                    },
                ],
            },
            _ => Command::Move {
                mode: MoveMode::ToTarget {
                    target: location_id(self.width - 1, self.height - 1),
                },
            },
        };
        envelope(player_index, &format!("perf.{step}"), command)
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
            let poi_ids = (index % 8 == 0)
                .then(|| service_id(index))
                .into_iter()
                .collect();
            locations.push(LocationDefinition {
                id: location_id(x, y),
                name: format!("Block {x},{y}"),
                description: format!("Procedural performance block {x},{y}."),
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
        .filter(|index| index % 8 == 0)
        .map(|index| ServiceDefinition {
            id: service_id(index),
            name: format!("Snack Window {index}"),
            location_id: location_id(index % width, index / width),
            item: "snack".to_string(),
            price_coins: 1,
            duration_ticks: 2,
            capacity: 64,
            overflow_behavior: "queue_nearby".to_string(),
        })
        .collect();

    WorldDefinition {
        schema_version: SCHEMA_VERSION.to_string(),
        id: format!("performance-grid-{width}x{height}"),
        name: "Procedural Performance Grid".to_string(),
        seed: 0x9e37_79b9_7f4a_7c15,
        grid: WorldGrid {
            width: width as u32,
            height: height as u32,
            cell_size: 1,
            terrain: vec![vec![GroundType::Ground; width]; height],
        },
        starting_coins: 100,
        allowance_coins: 0,
        max_coins: 100,
        locations,
        homes,
        services,
        spawn_location_id: location_id(0, 0),
    }
}

fn create_character(index: usize) -> CommandEnvelope {
    envelope(
        index,
        &format!("create.{index}"),
        Command::CreateCharacter {
            name: format!("Player {index}"),
            body_color: format!("#{:06x}", 0x303030 + index as u32 % 0xcfcfcf),
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
