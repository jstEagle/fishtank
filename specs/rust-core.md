# Rust Core Spec

This spec translates the project docs into the first complete Rust-core contract.
The core is the authoritative deterministic state machine. The server, CLI,
gateway, and viewer are adapters over these rules.

## Scope

The Rust core must support:

- Loading a data-defined world fixture.
- Creating agent characters with one character per controlling id.
- Assigning homes and enforcing home lock rules.
- Tracking ticks, in-world day progress, activities, queues, promises, and notifications.
- Producing filtered observations scoped to the acting character.
- Validating every command against the current state.
- Emitting ordered schema-versioned events.
- Producing snapshots and accepting snapshots for persistence round trips.
- Recording command envelopes so runs can be replayed deterministically.

## World Rules

- The world owns an authoritative rectangular grid.
- Every grid tile has an explicit terrain type: `ground`, `grass`, `path`, or `water`.
- Locations occupy validated grid footprints; every footprint must fit within the grid and cannot overlap another location footprint.
- Locations are semantic graph nodes connected by exits, with grid footprints used for storage, rendering, and future procedural growth.
- Homes are ordinary locations with ownership and lock state.
- Services are deterministic POIs with an item, cost, duration, and capacity.
- Characters can observe while active and can speak while moving when the target is audible.
- Movement is long-running and completes on ticks.
- Service orders reserve coins at acceptance and spend them at completion.
- Promise resolution creates a durable notification.
- Queues are capped at three commands and execute one step at a time.
- Queue spending is reserved before the queue is accepted.
- If a queued step fails, remaining reservations are released and a failure event is emitted.

## Public Commands

- `create_character`
- `observe`
- `move` by target or direction
- `say` to room or a nearby character
- `order`
- `wait`
- `queue`
- `home_manual`
- `home`
- `notifications`

## Test Contract

The test suite must cover:

- World loading failures and valid fixture loading.
- Character creation, duplicate ids, and color validation.
- Observation filtering and public character surface.
- Reachability, home locks, direction movement, and activity completion.
- Speech and conversation creation.
- Service orders, coin reservation, spending, insufficient funds, and notifications.
- Queue acceptance, queue cap, reservations, sequential execution, and failure cleanup.
- Command freshness errors.
- Snapshot round trip and deterministic command replay.
- Server persistence helper behavior where practical.
- Procedural stress tests with thousands of characters and actions.
- Release-mode performance runs that report throughput plus p5, p50, p95,
  p99, and max command latency under a server-sized memory cap.
