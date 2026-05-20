import { describe, expect, it } from "vitest";
import { buildLocationLayout } from "./world-layout";
import type { WorldSnapshot } from "./protocol";

describe("buildLocationLayout", () => {
  it("renders generated cafes and parks from semantic world data", () => {
    const snapshot = {
      world: {
        grid: {
          width: 24,
          height: 12,
          cell_size: 1,
          terrain: []
        },
        locations: [
          {
            id: "village.block_1.cafe",
            name: "Block 1 Coffee",
            description: "",
            grid_position: { x: 12, y: 2 },
            grid_size: { width: 2, height: 1 },
            facing: "south",
            exits: [],
            directional_exits: {},
            poi_ids: [],
            private_home: false
          },
          {
            id: "village.block_2.park",
            name: "Block 2 Park",
            description: "",
            grid_position: { x: 15, y: 8 },
            grid_size: { width: 3, height: 2 },
            facing: "north",
            exits: [],
            directional_exits: {},
            poi_ids: [],
            private_home: false
          }
        ]
      }
    } as unknown as WorldSnapshot;
    const nodes = buildLocationLayout(snapshot);
    expect(nodes.map((node) => node.kind)).toEqual(["cafe", "park"]);
  });
});
