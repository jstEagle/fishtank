# Wakeups And Notifications

Fishtank needs an agent-agnostic way to tell external agent runtimes that something happened and attention may be useful.

The core concept should be a durable notification, not a runtime-specific callback. OpenClaw, Hermes, local scripts, MCP clients, and future agent systems can then adapt that notification stream into their own wake mechanisms.

## Requirements

The system should support:

- Promise resolution notifications for long-running activities.
- Character event notifications, such as direct speech and someone entering the same location.
- Service notifications, such as coffee being ready.
- Queue completion and queue failure notifications.
- Blocking waits for local scripts.
- Polling for runtimes that do not support push.
- Optional webhook delivery for runtimes that do.
- Idempotent acknowledgement.
- Durable storage until delivery or expiry.

## Notification Model

Notifications should be server-generated records:

```json
{
  "notification_id": "notif_001",
  "character_id": "char_mira",
  "kind": "promise_resolved",
  "priority": "normal",
  "created_at_tick": 38120,
  "expires_at_tick": 41720,
  "summary": "Your coffee is ready at the cafe service window.",
  "related": {
    "promise_id": "promise_982",
    "activity_id": "activity_order_440",
    "location_id": "village.cafe.service_window"
  },
  "acknowledged": false
}
```

Notifications should not contain hidden world state. They are wake hints and links back into normal observation.

## CLI Commands

The CLI should expose notification operations:

```bash
fishtank notifications list --json
fishtank notifications wait --json
fishtank notifications ack notif_001
```

`wait` can block until a notification arrives or a timeout is reached. This is useful for scripts that want to sleep locally while the simulation progresses.

Autonomous agents should normally pair notification waiting with the compact wake loop:

```bash
fishtank life wake
fishtank notifications wait --timeout-ms 30000
```

The server stores notifications as wake hints only. Agent goals, relationships, routines, journals, and private memory remain local to the agent runtime.

## Delivery Adapters

Delivery should be adapter-based:

- Polling adapter: agent calls `notifications list`.
- Blocking adapter: agent calls `notifications wait`.
- Webhook adapter: gateway POSTs to a registered runtime URL.
- OpenClaw adapter: maps notifications to OpenClaw wake hooks or scheduled task events.
- MCP adapter: maps long-running operations to MCP tasks where supported.

The core should not know which runtime controls a character. The gateway can handle runtime-specific delivery.

## MCP Task Compatibility

MCP tasks are useful for deferred results and polling. Fishtank promises can map naturally to MCP task-like state:

- `working`: activity is still running.
- `completed`: promise resolved successfully.
- `failed`: activity failed.
- `cancelled`: activity or queue was canceled.
- `input_required`: the character needs a new agent decision.

MCP task notifications can be used when the connected client supports them, but clients should still be able to poll because status notifications are optional in the MCP draft.

## Queueing And Ordering

Cloudflare Queues or a similar system can decouple gateway delivery from core processing. They should be treated as delivery infrastructure, not as the source of simulation ordering. The simulation core or world partition owner must still impose authoritative ordering for commands that mutate state.

Notification delivery should be at-least-once. Agents and adapters must acknowledge notifications idempotently.
