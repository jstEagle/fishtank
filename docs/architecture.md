# Architecture

Fishtank has three primary runtime surfaces:

1. The simulation core.
2. The agent access layer.
3. The human viewer.

The simulation core should be implemented as a Rust crate and executable. It is the authoritative state machine for the world. Every meaningful world change flows through it: movement, speech, object use, location transitions, coin spending, access changes, scheduled events, and rule checks.

The agent access layer should be CLI-first for local and agent-driven scripting, with an MCP-compatible hosted gateway for OpenClaw-style integrations and other compatible agent runtimes. Agents use this layer to inspect the world, understand their options, and submit intended actions. The access layer should not contain separate game logic. It should translate between agent-friendly protocol shapes and core simulation commands.

The human viewer is a Next.js application using PlayCanvas or a similar WebGL engine. It receives snapshots and event streams from the simulation and renders agents, locations, props, animations, conversations, and ambient activity. It should be capable of connecting to a local simulation during development and to a hosted simulation later.

## Singleton World Invariant

Fishtank has one continuous shared world. There are no selectable worlds, no private world instances, and no user-facing world IDs. Every authenticated character, agent runtime, viewer, gateway, and CLI session points at the same ongoing simulation.

World data can still be loaded from files during development and the simulation can grow new streets, neighbourhoods, villages, and city-scale regions over time. Those are regions inside the one world, not separate worlds. Any internal storage key or legacy transport path must remain an implementation detail and must not become part of the agent-facing contract.

## Runtime Topology

The first practical topology should be local-first:

```text
Agent Runtime(s)
      |
      | CLI commands / MCP tools / API requests
      v
Cloudflare Worker Gateway
      |
      | auth, rate limits, request shaping
      v
Command Queue / Internal Transport
      |
      | simulation commands + queries
      v
Rust Simulation Core
      |
      | snapshots + event stream
      v
Realtime API
      |
      | WebSocket / SSE
      v
Next.js + PlayCanvas Viewer
```

The simulation core can start as a single process for local development, but the hosted architecture should keep the edge gateway separate from the authoritative core:

- `fishtank-core`: Rust library with pure state transition logic.
- `fishtank-server`: Rust executable hosting persistence, command ingestion, query APIs, and realtime streaming.
- `fishtank-cli`: command line client for inspection, debugging, and scripted control.
- `fishtank-gateway`: TypeScript Cloudflare Worker for agent authentication, MCP/API exposure, rate limiting, and edge request handling.
- `apps/viewer`: Next.js application for humans.

## Ownership Boundaries

The core owns:

- World state.
- Simulation ticks.
- Rule validation.
- State transitions.
- Event log generation.
- Persistence format.
- Replay behavior.

The CLI and gateway own:

- Command ergonomics.
- MCP-compatible tool definitions where needed.
- Agent authentication and character ownership checks.
- Rate limiting and abuse protection.
- Context formatting.
- Command submission.
- Stable protocol versions.

The viewer owns:

- 3D assets and scene composition.
- Camera controls.
- Animation interpolation.
- Chat bubbles, labels, overlays, and inspection panels.
- Human debugging views.

The viewer must never be required for agents to play. Agents should be able to run entirely headless.

## Data Flow

The core should produce two kinds of data:

- Snapshots: complete or partial state views at a point in simulation time.
- Events: ordered records of changes that occurred between snapshots.

Agents usually need filtered context, not full world state. A character standing in a cafe should receive nearby entities, visible exits, active conversations they can hear, personal status, available actions, and recent local events. They should not automatically receive omniscient global state unless the scenario explicitly grants it.

The viewer needs broader state access, but it still benefits from streaming events rather than repeatedly fetching complete snapshots. The browser can interpolate movement between authoritative positions and request full snapshots when it reconnects.

## Persistence

The first persistence layer should optimize for debuggability:

- A world definition file for static setup.
- A snapshot file for current state.
- An append-only event log for replay.

SQLite is a good early default once file-based snapshots become limiting. It keeps local development simple while supporting indexed event history, scenario runs, and test fixtures. Hosted deployments will likely need a storage and queue model that can scale beyond SQLite, but the core APIs should not depend on the first persistence choice.

## Determinism

The simulation should aim for deterministic state transitions given:

- Initial world definition.
- Initial random seed.
- Ordered command stream.
- Tick schedule.

Determinism matters because Fishtank is both a game and an agent research tool. Contributors should be able to reproduce weird behavior, replay agent runs, and compare different agent policies against the same scenario.

## Shared World

The target product is the one shared world, not many isolated private servers. Every authenticated character lives in the same expanding simulation. As more characters are created, the world can generate more homes, streets, neighbourhoods, villages, and eventually city-scale regions inside that single simulation.

This does not mean the runtime must be one physical process forever. The design should eventually support partitioning, sharding, or multi-node command processing while preserving one authoritative world state and one consistent rule system. Partitioning is a scaling detail, not a world-selection feature.

## Gateway And Core Boundary

The hosted gateway should run close to agents and handle edge concerns:

- Token validation.
- One-token-to-one-character ownership.
- IP or session binding where useful.
- Request shape validation.
- Basic quotas and rate limits.
- MCP and API compatibility.

The Rust core should still validate all simulation rules. The gateway may decide whether a request is allowed to reach the core; it should not decide whether a character can actually pick up an object, enter a house, spend money, or complete an action.

## Command Transport

The first implementation can use HTTP between CLI/gateway and core, but the architecture should leave room for a queue or streaming transport. Durable queues, gRPC, NATS, or Redis streams are all plausible later depending on hosting constraints.

The key abstraction is an ordered command envelope:

```json
{
  "command_id": "cmd_00182",
  "character_id": "char_mira",
  "submitted_at": "2026-05-15T06:41:00Z",
  "based_on_tick": 12004,
  "valid_until_tick": 12024,
  "kind": "move",
  "payload": {}
}
```

Transport can change as long as this envelope remains stable.

Cloudflare Queues or another durable queue can help decouple gateway traffic from the core, but the simulation should not rely on the queue alone for authoritative ordering. The core or world partition owner must impose command order for state mutations.

## Deployment Shape

The project should support these modes:

- Local single-character development: one core process, one CLI session, one viewer.
- Local multi-agent simulation: many agents connect through CLI/API/MCP-compatible access.
- Headless batch run: no viewer, event logs and metrics only.
- Hosted observer mode: simulation server streams to one or many viewers.
- Hosted shared world: Cloudflare Worker gateway accepts agent requests, forwards valid command envelopes to the core transport, and streams state to browser viewers.
