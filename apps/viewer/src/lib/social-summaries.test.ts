import { describe, expect, it } from "vitest";
import type { EventRecord, WorldSnapshot } from "./protocol";
import { buildNewsItems, characterLedger, locationEarnings } from "./social-summaries";

describe("social summaries", () => {
  it("maps events into public news items", () => {
    const news = buildNewsItems(
      [
        event(1, 10, { event: "character_created", character_id: "char_mira", home_id: "home" }),
        event(2, 20, {
          event: "coins_earned",
          character_id: "char_mira",
          amount: 1,
          source_id: "village.office.workstation"
        })
      ],
      snapshot()
    );

    expect(news.map((item) => item.title)).toEqual([
      "Mira earned 1 coin",
      "Mira arrived"
    ]);
  });

  it("derives office earnings by character from coin events", () => {
    const earnings = locationEarnings(
      [
        event(1, 10, {
          event: "coins_earned",
          character_id: "char_mira",
          amount: 1,
          source_id: "village.office.workstation"
        }),
        event(2, 20, {
          event: "coins_earned",
          character_id: "char_mira",
          amount: 2,
          source_id: "village.office.workstation"
        })
      ],
      snapshot(),
      "village.office"
    );

    expect(earnings).toEqual([
      {
        characterId: "char_mira",
        characterName: "Mira",
        characterColor: "#4ea1ff",
        amount: 3
      }
    ]);
  });

  it("derives character transaction history from coin events", () => {
    const ledger = characterLedger(
      [
        event(1, 10, {
          event: "coins_earned",
          character_id: "char_mira",
          amount: 1,
          source_id: "village.office.workstation"
        }),
        event(2, 20, {
          event: "coins_spent",
          character_id: "char_mira",
          amount: 1,
          source_id: "village.office.vending_machine",
          item: "sparkling_water"
        })
      ],
      "char_mira",
      snapshot()
    );

    expect(ledger.earned).toBe(1);
    expect(ledger.spent).toBe(1);
    expect(ledger.items.map((item) => item.type)).toEqual(["spent", "earned"]);
  });
});

function event(id: number, tick: number, kind: EventRecord["kind"]): EventRecord {
  return {
    schema_version: "fishtank.v1",
    id,
    tick,
    kind
  };
}

function snapshot(): WorldSnapshot {
  return {
    schema_version: "fishtank.v1",
    world_id: "village",
    tick: 20,
    next_event_id: 3,
    next_command_seq: 1,
    next_conversation_seq: 1,
    world: {
      schema_version: "fishtank.v1",
      id: "village",
      name: "Village",
      seed: 1,
      grid: { width: 1, height: 1, cell_size: 1, terrain: [["ground"]] },
      starting_coins: 10,
      allowance_coins: 0,
      max_coins: 20,
      locations: [],
      homes: [],
      services: [
        {
          id: "village.office.vending_machine",
          name: "Office Vending Machine",
          location_id: "village.office",
          item: "sparkling_water",
          description: "",
          price_coins: 1,
          duration_ticks: 5,
          capacity: 1,
          overflow_behavior: "queue_nearby"
        }
      ],
      activity_sites: [
        {
          id: "village.office.workstation",
          name: "Shared Workstation",
          location_id: "village.office",
          action: "work",
          description: "",
          duration_ticks: 20,
          coin_reward: 1
        }
      ],
      interactables: [],
      spawn_location_id: "village.office"
    },
    characters: {
      char_mira: {
        id: "char_mira",
        name: "Mira",
        body_color: "#4ea1ff",
        face_color: "#101820",
        location_id: "village.office",
        home_id: "home",
        coins: 10,
        reserved_coins: 0,
        current_activity: null,
        queued_commands: [],
        last_agent_action_tick: 0,
        status: "idle"
      }
    },
    home_locks: {},
    conversations: {},
    notifications: {},
    command_log: []
  };
}
