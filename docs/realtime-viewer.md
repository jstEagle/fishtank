# Realtime Viewer

The realtime viewer is the human-facing window into Fishtank. It should make the simulation legible and compelling without becoming the source of truth.

There is only one live simulation for the viewer to render. The viewer may offer camera positions, debug filters, or region focus controls, but it should never present a world picker.

The recommended starting stack is:

- Next.js for the web application.
- PlayCanvas for 3D rendering.
- WebSocket or Server-Sent Events for live state updates.
- A local API client generated or hand-written from the shared protocol schemas.

## Viewer Responsibilities

The viewer should render:

- Locations and static geometry.
- Robot characters.
- Movement and idle animations.
- Conversations.
- Activity state.
- Object interactions.
- Time of day and ambience.
- Characters hidden inside homes as house-level status bubbles.
- Debug overlays for contributors.

The viewer should also support inspection:

- Click an agent to see name, current activity, recent speech, and visible status.
- Click a place to see its semantic ID and connected exits.
- Show event stream panels in developer mode.
- Toggle labels, paths, zones, and nav graph overlays.

Viewer modes should be explicit:

- Public observer: sees visible world state.
- Debug observer: sees IDs, zones, paths, state panels, and event data.
- Admin observer: can intervene through privileged commands.

Replay observer mode is not a current goal.

## State Input

The viewer should consume:

- An initial world snapshot.
- Incremental event stream.
- Optional asset manifest.
- Optional semantic metadata for inspection panels.

The viewer should not call privileged simulation methods for visual updates. It should subscribe to world state and render what the server reports.

## Rendering Model

The core should expose semantic position and movement state. The viewer translates that into visual animation.

For example:

```json
{
  "event": "agent_moved",
  "actor_id": "char_mira",
  "from": {
    "location_id": "coffee_shop.front_door",
    "position": [0, 0, 4]
  },
  "to": {
    "location_id": "coffee_shop.counter",
    "position": [3, 0, -1]
  },
  "started_at_tick": 100,
  "completed_at_tick": 124
}
```

The viewer can interpolate between positions based on ticks. It should tolerate dropped events by requesting a fresh snapshot.

## Assets

The first world can use simple but coherent assets:

- Low-poly building exteriors and simple outdoor props.
- Reusable robot avatars with body and face/screen color customization.
- Props for cafe, street, park, and home exterior scenes.
- Animation states for idle, walk, talk, sit, interact, and wait.

The server should send enough world data, state, and asset rules for the client to render without owning simulation logic. Asset IDs should be data-driven:

```json
{
  "entity_id": "coffee_shop.counter",
  "renderable": {
    "asset_id": "cafe_counter_v1",
    "transform": {
      "position": [3, 0, -1],
      "rotation": [0, 90, 0],
      "scale": [1, 1, 1]
    }
  }
}
```

## Human Controls

The viewer should include:

- Orbit and follow cameras.
- Agent selection.
- Event log panel.
- Conversation transcript panel.
- Debug toggles.

Humans are observers by default. Admin controls can exist later, but they should still submit commands through the same simulation command path as any other external actor.
