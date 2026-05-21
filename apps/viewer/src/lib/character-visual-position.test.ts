import { describe, expect, it } from "vitest";
import type { Character, WorldSnapshot } from "./protocol";
import {
  buildingOccupants,
  characterPosition,
  characterVisualPositions,
  isCharacterRigVisible
} from "./character-visual-position";
import type { LocationRenderNode } from "./world-layout";

describe("characterVisualPositions", () => {
  it("spreads idle characters sharing a street location without changing backend state", () => {
    const snapshot = snapshotWithCharacters([
      character("char_mira", "village.main_street"),
      character("char_ren", "village.main_street"),
      character("char_sol", "village.main_street")
    ]);
    const positions = characterVisualPositions(snapshot, locationMap(), 1);

    const mira = positions.get("char_mira");
    const ren = positions.get("char_ren");
    const sol = positions.get("char_sol");

    expect(mira).toBeDefined();
    expect(ren).toBeDefined();
    expect(sol).toBeDefined();
    expect(snapshot.characters.char_mira.location_id).toBe("village.main_street");
    expect(distance(mira!, ren!)).toBeGreaterThan(0.3);
    expect(distance(mira!, sol!)).toBeGreaterThan(0.3);
    expect(distance(ren!, sol!)).toBeGreaterThan(0.3);
  });

  it("keeps single and actively walking street characters on their ordinary render path", () => {
    const walking = character("char_ren", "village.main_street", {
      current_activity: {
        id: "act_1",
        kind: "moving",
        status: "active",
        target_id: "village.cafe",
        movement_path: [],
        started_at_tick: 0,
        completes_at_tick: 10,
        description: "Walking to the cafe.",
        promise_id: null,
        reserved_coins: 0,
        queued: false
      },
      status: "moving"
    });
    const snapshot = snapshotWithCharacters([
      character("char_mira", "village.main_street"),
      walking,
      character("char_sol", "village.cafe")
    ]);
    const byLocation = locationMap();
    const positions = characterVisualPositions(snapshot, byLocation, 5);

    expect(positions.get("char_ren")).toEqual(characterPosition(walking, snapshot, byLocation, 5));
    expect(positions.get("char_sol")).toEqual({ x: 5, y: 0, z: 0 });
    expect(positions.get("char_mira")).toEqual({ x: 0, y: 0, z: 0 });
  });

  it("spreads idle characters sharing a park location", () => {
    const snapshot = snapshotWithCharacters([
      character("char_mira", "village.park"),
      character("char_ren", "village.park"),
      character("char_sol", "village.park")
    ]);
    const positions = characterVisualPositions(snapshot, locationMap(), 1);

    expect(distance(positions.get("char_mira")!, positions.get("char_ren")!)).toBeGreaterThan(0.3);
    expect(distance(positions.get("char_mira")!, positions.get("char_sol")!)).toBeGreaterThan(0.3);
    expect(distance(positions.get("char_ren")!, positions.get("char_sol")!)).toBeGreaterThan(0.3);
  });

  it("hides idle building occupants from full bot rendering and groups them by location", () => {
    const walking = character("char_sol", "village.cafe", {
      current_activity: {
        id: "act_2",
        kind: "moving",
        status: "active",
        target_id: "village.main_street",
        movement_path: [],
        started_at_tick: 0,
        completes_at_tick: 10,
        description: "Walking back to the street.",
        promise_id: null,
        reserved_coins: 0,
        queued: false
      },
      status: "moving"
    });
    const snapshot = snapshotWithCharacters([
      character("char_mira", "village.cafe", { name: "Mira" }),
      character("char_ren", "village.cafe", { name: "Ren" }),
      walking,
      character("char_park", "village.park")
    ]);
    const byLocation = locationMap();
    const occupants = buildingOccupants(snapshot, byLocation);

    expect(isCharacterRigVisible(snapshot.characters.char_mira, byLocation)).toBe(false);
    expect(isCharacterRigVisible(snapshot.characters.char_ren, byLocation)).toBe(false);
    expect(isCharacterRigVisible(walking, byLocation)).toBe(true);
    expect(isCharacterRigVisible(snapshot.characters.char_park, byLocation)).toBe(true);
    expect(occupants.get("village.cafe")?.map((entry) => entry.name)).toEqual(["Mira", "Ren"]);
    expect(occupants.has("village.park")).toBe(false);
  });
});

function locationMap() {
  return new Map<string, LocationRenderNode>([
    ["village.main_street", location("village.main_street", "street", { x: 0, y: 0, z: 0 })],
    ["village.cafe", location("village.cafe", "cafe", { x: 5, y: 0, z: 0 })],
    ["village.park", location("village.park", "park", { x: -5, y: 0, z: 0 })]
  ]);
}

function location(
  id: string,
  kind: LocationRenderNode["kind"],
  position: LocationRenderNode["position"]
): LocationRenderNode {
  return {
    id,
    name: id,
    description: "",
    privateHome: false,
    kind,
    position,
    exits: [],
    poiIds: [],
    color: "#000000",
    topColor: "#ffffff",
    size: { x: 6, y: 0.08, z: 2.2 },
    gridPosition: { x: 0, y: 0 },
    gridSize: { width: 4, height: 1 },
    facing: "north"
  };
}

function snapshotWithCharacters(characters: Character[]): WorldSnapshot {
  return {
    schema_version: "fishtank.v1",
    world_id: "world",
    tick: 1,
    next_event_id: 1,
    next_command_seq: 1,
    next_conversation_seq: 1,
    world: {
      schema_version: "fishtank.v1",
      id: "world",
      name: "World",
      seed: 1,
      grid: { width: 10, height: 10, cell_size: 1, terrain: [] },
      locations: [],
      homes: [],
      services: [],
      spawn_location_id: "village.main_street",
      starting_coins: 10,
      allowance_coins: 1,
      max_coins: 100
    },
    characters: Object.fromEntries(characters.map((entry) => [entry.id, entry])),
    home_locks: {},
    conversations: {},
    notifications: {},
    command_log: []
  };
}

function character(id: string, locationId: string, overrides: Partial<Character> = {}): Character {
  return {
    id,
    name: id,
    body_color: "#86a8e7",
    face_color: "#fff4a8",
    location_id: locationId,
    home_id: "home",
    coins: 0,
    reserved_coins: 0,
    current_activity: null,
    queued_commands: [],
    last_agent_action_tick: 0,
    status: "idle",
    ...overrides
  };
}

function distance(a: { x: number; z: number }, b: { x: number; z: number }) {
  return Math.hypot(a.x - b.x, a.z - b.z);
}
