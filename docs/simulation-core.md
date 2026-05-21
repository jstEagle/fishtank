# Simulation Core

The simulation core is the authoritative state machine for Fishtank. It should be written in Rust unless a strong implementation reason emerges to choose otherwise.

The core should be usable as both:

- A Rust library for tests, tools, and embedding.
- A server executable for normal simulation runs.

## Responsibilities

The core is responsible for:

- Loading world definitions.
- Tracking simulation time.
- Applying valid commands.
- Rejecting invalid commands with clear reasons.
- Updating entity state on ticks.
- Emitting ordered events.
- Producing filtered observations for agents.
- Producing snapshots for viewers and persistence.
- Supporting replay from event logs.

## State Machine Model

The core should expose a small number of primitives:

- `WorldState`: complete authoritative state.
- `Command`: an intent submitted by an agent, script, admin, or scenario.
- `Observation`: filtered state returned to an agent.
- `Event`: immutable record of what changed.
- `System`: deterministic logic that transforms state over time.
- `Tick`: one simulation step.

Example command categories:

- `Look`: inspect surroundings.
- `MoveTo`: move toward a location, exit, waypoint, or entity.
- `Say`: speak to nearby entities or a selected conversation.
- `UseObject`: interact with an object.
- `StartActivity`: begin a longer action such as ordering coffee.
- `Wait`: intentionally spend time doing nothing or continuing current activity.
- `Queue`: submit a short ordered list of commands with preconditions.

Commands should describe intent, not guaranteed outcome. For example, an agent may request `MoveTo(cafe_counter)`, but the core decides whether the path is available, how long it takes, and what events are produced.

Queued commands should not become an AI planner. They are a convenience for small routines. Each queued command is still validated at execution time, can fail independently, and should produce useful events and errors.

Long-running commands can return promise-like handles. A promise represents a future simulation event that may be useful to the controlling agent, such as arriving at a destination or a coffee order becoming ready. Promises are not separate authority; they are references to scheduled or in-progress activity outcomes owned by the core.

## Tick And Action Timing

The first version uses fixed ticks at 5 real seconds per simulation tick. Most agent-facing actions should not require frame-perfect timing. Higher-level actions can span many ticks:

- Walking to a table.
- Waiting in line.
- Ordering a drink.
- Sitting down.
- Joining a conversation.

Long-running actions should be represented explicitly so agents and viewers can inspect what is happening.

Agents should not need to poll constantly while long-running actions progress. The server should be able to notify or wake an agent when a promise resolves, depending on the integration. This is especially important for actions like walking to a distant POI or waiting for a service order.

## Validation

Every command must be validated against the current world state:

- Is the actor known?
- Is the actor allowed to act?
- Is the target visible, reachable, or known?
- Is the actor busy?
- Is the command allowed by rate limits or action cadence?
- Is the command still compatible with the observation tick or preconditions it was based on?
- Does a queued command require coin reservations?
- Does the object support the requested interaction?
- Does the action require resources, permissions, or proximity?

Rejected commands should produce structured errors that are useful to agents:

```json
{
  "ok": false,
  "error": {
    "code": "target_not_reachable",
    "message": "The cafe counter is not reachable from the current room.",
    "suggested_actions": ["look", "move_to:front_door"]
  }
}
```

## Event Log

The event log is the audit trail and replay source. Events should be stable, timestamped, ordered, and schema-versioned.

Examples:

- `agent_spawned`
- `agent_moved`
- `agent_started_speaking`
- `message_spoken`
- `conversation_joined`
- `object_used`
- `activity_started`
- `activity_completed`
- `promise_created`
- `promise_resolved`
- `coins_reserved`
- `coins_spent`
- `coins_released`
- `relationship_updated`
- `world_time_advanced`

Events should be readable enough for debugging and structured enough for tools.

## Testing Strategy

The core should get the strongest test coverage in the project. Useful test classes:

- Command validation tests.
- Deterministic replay tests.
- Scenario fixture tests.
- Observation filtering tests.
- Movement and reachability tests.
- Persistence round-trip tests.
- Integration tests for every public command and state transition.
- Procedural world generation invariant tests.
- Queue, promise, coin reservation, and notification tests.

The viewer can tolerate visual iteration. The core cannot tolerate ambiguous world truth. The goal should be comprehensive local integration coverage for implemented core behavior, with clear fixtures that other contributors and agents can build against.
