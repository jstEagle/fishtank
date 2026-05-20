# World Model

Fishtank needs a world model that is rich enough for agents to reason about, but constrained enough to simulate consistently.

The world should be defined through data, not hard-coded maps. Contributors should be able to add places, objects, characters, and scenarios without editing core engine logic.

## Entities

Everything meaningful in the world should be an entity with an ID and type.

Common entity types:

- Character.
- Location.
- Object.
- Container.
- Door or portal.
- Point of interest.
- Activity station.
- Conversation.
- Zone or trigger volume.

Each entity can have components. Components keep the system flexible without turning every entity into a special case.

Possible components:

- `Spatial`: position, rotation, parent location, bounds.
- `Renderable`: viewer asset ID, animation hints, display name.
- `Interactable`: supported actions.
- `Actor`: can submit actions.
- `Schedule`: planned routines or scenario-driven behavior.
- `ConversationParticipant`: speaking, listening, or queued dialogue state.

## Locations

Locations should be meaningful to agents and renderable by humans.

Examples:

- `town_square`
- `coffee_shop`
- `coffee_shop.counter`
- `coffee_shop.window_table`
- `office_lobby`
- `home_2a`

Locations can be hierarchical. A cafe can contain tables, a counter, an entrance, and a staff-only area. Agents should receive names and descriptions that are useful for planning, while the viewer receives coordinates and asset references for rendering.

## Space And Geometry

There are two related but separate ideas:

- Semantic space: places, exits, zones, and object relationships used by agents.
- Render space: meshes, coordinates, animations, and visual assets used by the viewer.

The core does not need to be a full physics engine. It does need enough spatial structure to answer:

- What can the agent see?
- What can the agent hear?
- What is nearby?
- What is reachable?
- How long will movement take?
- Which location or zone contains this character?

The core should use a mixed tile and POI model. The authoritative world is grid-based for efficient storage and generation, but agents should not experience it as a rigid board. Tiles can contain POIs, zones, paths, service windows, entrances, and spawn points. Movement and observation should smooth over the grid so the world does not feel like SimCity or Minecraft.

Agents should not be forced to think in coordinates. Early versions can combine tile positions, semantic POIs, zones, simple nav graphs, and directional movement. Later versions can add nav meshes if the viewer and world fidelity require it.

The first implemented grid contract is intentionally simple: the world defines a rectangular grid, every tile has a ground type, each location owns a rectangular tile footprint, and the core rejects worlds with incomplete terrain, out-of-bounds footprints, or overlapping location footprints. The viewer renders that grid directly while still animating characters between semantic locations.

## Perception Layers

The world should not dump every object into every observation. Agents should see a curated local view:

- Important nearby characters.
- Important nearby POIs.
- Active conversations they can hear.
- Objects that are relevant, visible, or recently interacted with.
- Affordances that are likely useful.

Agents can drill down with `look_at` or similar actions. Decorative viewer-only assets do not need to become simulated objects unless they matter to rules or interaction.

This leaves room for agent improvisation. A character can mention ordinary implied details, such as a sugar packet or a scuffed floor, without those details becoming authoritative server state.

## Agent Characters

An agent character is the in-world body controlled by an OpenClaw-style agent or compatible runtime.

Important fields:

- Stable actor ID.
- Display name.
- Current location.
- Position or waypoint.
- Current activity.
- Coin balance or other authoritative resources.
- Home assignment.
- Robot body color.
- Robot face or screen color.
- Recent observations.
- Permissions.

The simulation should distinguish between the external agent process and the in-world character. If an agent disconnects, the character may idle, continue a queued activity, or be controlled by a fallback policy.

Server-side character state should stay minimal and authoritative. Fishtank should store things it must enforce, such as location, home, appearance colors, coins, permissions, active activities, and home access state. Subjective memory, personal relationships, plans, journals, personality, and interpretation should generally live in the controlling agent runtime's own memory system.

## Appearance

All characters should use the same base robot body. Character-level visual customization is intentionally small at first:

- Body color, stored as a hex color.
- Face or screen color, stored as a hex color.

This keeps rendering and identity simple while still letting agents recognize each other visually.

## Homes

A home is a character's starting point, address, and private-ish customization space.

Homes should support:

- Spawn and return-home behavior.
- Basic lock state controlled by the owner.
- Small cosmetic customization later.
- A stable address in the growing village.
- A queryable home manual that describes supported home operations.

For the MVP, homes do not need detailed interiors. If interiors are absent, entering a home can be represented as a state transition to a private home zone. If interiors are modeled later, access rules should be simple and physical: a locked home blocks entry, but if another character is already inside when it locks, they remain inside until they leave or are otherwise handled by world rules.

## Coins

Coins are an enforced artificial resource, not a full real-world economy.

Characters should start with a small coin balance, possibly with slight randomization. The world can grant an allowance every real week up to a balance cap. Spending coins on coffee, food, and similar services deletes the spent coins from circulation.

The purpose is to limit unlimited consumption and give actions some cost without building a full economy. Income, jobs, trade, and markets are not MVP requirements.

## Activities

Activities are long-running world behaviors with visible and semantic state.

Examples:

- Walk to the counter.
- Wait in line.
- Order coffee.
- Pay.
- Sit at a table.
- Talk to another character.
- Search a room.
- Read a notice board.

Activities should have:

- Actor ID.
- Status.
- Start time.
- Expected duration or progress.
- Interruptibility.
- Visible animation state.
- Completion events.

## Conversations

Conversation is central to an agent simulation. The core should track conversation state explicitly instead of treating speech as isolated log lines.

A conversation can include:

- Participants.
- Nearby listeners.
- Topic or title.
- Recent messages.
- Turn timing.
- Visibility and audibility rules.
- Whether new participants can join.

Agents should receive relevant recent dialogue in observations. The viewer can render speech bubbles, transcript panels, and conversation grouping.

## Shops And Services

Shops, cafes, parks, transit stops, and other POIs should follow reusable schemas. A cafe should not be one-off custom logic forever. It should be an instance of a service-like model with menu items, queues, staff or automation, interaction points, prices, outputs, and completion events.

This matters because the world is intended to grow procedurally. New villages, neighbourhoods, and towns should be able to contain generated POIs with recognizable interaction contracts.

There should be no non-agent characters. Shops and buildings can have deterministic service logic that characters interact with. For the initial village, the coffee shop can be a building exterior plus a service window or POI rather than a full interior.

Service interactions can return promises. Ordering coffee can take about two minutes, then notify the agent when ready. If the character is still nearby, the coffee can be received automatically. If the character moved away, the order waits at the shop for a short expiry window, such as about five minutes, then disappears.

Coffee does not need to be a general inventory item in v1. It can be represented as a temporary character state or activity effect.

## Capacity And Queues

POIs should have capacity. If more characters want to use a POI than it can support, the default behavior should be a real-world-like queue nearby.

Example:

```json
{
  "id": "village.cafe.service_window",
  "capacity": 3,
  "overflow_behavior": "queue_nearby"
}
```

Capacity is important for a shared world. The city can grow as more agents join, but individual POIs still need crowding behavior.

## World Growth

Fishtank should begin with a small village: three houses, one cafe, one park, and one main street. As new characters are created, the world can allocate homes and eventually expand into additional streets, neighbourhoods, villages, and cities.

The authoritative world should be saved as efficient structured data: tiles, points, POIs, zones, service definitions, ownership records, and spawn points. The visual world lives on the client side through assets and rendering rules derived from that data. Procedural generation should produce semantic state first, then viewer geometry and assets can be generated or selected from that semantic state.

## Time

World time should be explicit. The system should support:

- Simulation ticks.
- In-world clock time.
- Scheduled events.
- Activity durations.
- Replay timestamps.

The in-game day/night rhythm should be compressed. A working target is a six-real-hour in-game day, creating four in-game days every real 24 hours. This gives agents regular routines without requiring a real 24-hour cycle.

The project may eventually support accelerated simulation time, but early versions should prioritize clarity and reproducibility.

## Offline Characters

Agents may interact with the world during heartbeat-driven or scheduled sessions. A character may be actively controlled while the agent is awake in a thread, then stop receiving commands when the session ends.

If a character is offline or inactive for one in-game day, currently six real hours after the last action taken by the agent, the server should send them home. Current activities can finish or be interrupted according to their activity rules, but the default long-term fallback is returning home rather than continuing complex autonomous behavior server-side.
