# Roadmap

This roadmap is staged around proving the core loop before expanding fidelity. Work should be organized so multiple people or agents can build against clear specs and integration tests in parallel.

Product invariant: Fishtank has one continuous shared world. Roadmap items may add regions, fixtures, local dev data, and scale-out infrastructure, but they must not introduce selectable worlds or world IDs into the agent-facing contract.

## Phase 0: Written Foundation

Create the initial project docs:

- Vision.
- Architecture.
- Simulation core design.
- Agent interface design.
- Viewer design.
- Protocol approach.
- Contributor and agent onboarding plan.

This phase is the current repository baseline.

## Phase 1: Minimal Headless Simulation

Goal: prove agents can enter the shared world, observe, move, speak, and generate events.

Deliverables:

- Rust workspace.
- Core state structs.
- Static village world fixture with houses, a cafe, and a small park.
- Command validation.
- Tick loop.
- Event log.
- Basic persistence snapshot.
- Character creation with name, body color, face color, home, and starting coins.
- Home lock state and home manual basics.
- Cafe exterior/service POI with enforced coin cost.
- CLI for auth, character, observe, actions, move, say, act, wait, and events.
- Unit tests for command application and replay.
- Integration tests covering every implemented command and state transition.

Success criteria:

- Two scripted characters can enter the village, move between houses, cafe, and park, buy coffee with coins, and exchange messages.
- A run can be replayed deterministically from initial world plus command log.
- The Rust core can be tested completely locally without the viewer or hosted gateway.

## Workstreams

The project should be split into parallelizable workstreams with explicit specs and test contracts:

- Core ticks, base architecture, and state machine.
- World data handling, storage, procedural growth, and state persistence.
- 3D viewer interface, world rendering, and world geometry.
- Worker gateway, authentication, rate limiting, and transport.
- Agent interface, CLI, OpenClaw skills, plugins, and runtime adapters.

The Rust core should come first because every other surface depends on its contracts. After that, workstreams can proceed in parallel as long as schemas, fixtures, and integration tests define the boundaries.

## Phase 2: Agent Access And Gateway

Goal: make the simulation playable by OpenClaw-style agents and compatible runtimes through CLI and MCP-compatible hosted access.

Deliverables:

- TypeScript Cloudflare Worker gateway prototype.
- Token-to-character auth.
- Rate limiting.
- Tool definitions for observe, list actions, move, say, act, wait, and history.
- Agent session handling.
- Observation filtering.
- Structured errors.
- Initial `fishtank-player` skill.

Success criteria:

- A compatible agent can join the village, inspect surroundings, choose valid actions, and recover from invalid ones.

## Phase 3: Realtime Event API

Goal: expose state updates for a viewer without coupling the viewer to core internals.

Deliverables:

- Snapshot endpoint.
- Event stream endpoint.
- Command envelope format with `based_on_tick` and validity windows.
- Shared TypeScript protocol types or generated schemas.
- Resume from last event ID.
- Durable notifications for promise resolution.
- Local dev server integration.

Success criteria:

- A browser client can subscribe to world events and keep a simple 2D or debug view synchronized.

## Phase 4: 3D Viewer Prototype

Goal: render the village world and agents in real time.

Deliverables:

- Next.js app.
- PlayCanvas scene.
- Small village geometry.
- Agent avatars.
- Movement interpolation.
- Conversation bubbles or transcript overlay.
- Agent inspector panel.
- Debug overlay for locations, IDs, and event stream.

Success criteria:

- Humans can watch robot characters walk around a village, visit the cafe and park, and talk in near real time.

## Phase 5: Richer Simulation Mechanics

Goal: make the world feel more like a game.

Deliverables:

- Activities with durations and interruption rules.
- Object interactions.
- Short action queues.
- POI capacity and queueing behavior.
- Long-running action promises and notification wakeups.
- Schedules.
- Conversation membership.
- Basic needs or preferences.
- Scenario goals.

Success criteria:

- Agents can perform multi-step routines such as walking to the cafe service window, ordering coffee, waiting, going to the park, and talking.

## Phase 6: Contributor-Ready Open Source Release

Goal: make the project approachable for external contributors.

Deliverables:

- Installation docs.
- Contribution guide.
- Example regions and fixtures.
- Example agents.
- Protocol reference.
- Test fixtures.
- CI.
- License review.

Success criteria:

- A new contributor can clone the repo, run the simulation, open the viewer, and make a small world change in under 30 minutes.
