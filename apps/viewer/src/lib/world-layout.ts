import type { LocationDefinition, ServiceDefinition, WorldSnapshot } from "./protocol";

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

export type LocationKind = "home" | "cafe" | "park" | "street" | "generic";

export interface LocationRenderNode {
  id: string;
  name: string;
  description: string;
  privateHome: boolean;
  kind: LocationKind;
  position: Vec3;
  exits: string[];
  poiIds: string[];
  color: string;
  topColor: string;
  size: { x: number; y: number; z: number };
  gridPosition: { x: number; y: number };
  gridSize: { width: number; height: number };
  facing: LocationDefinition["facing"];
}

export interface ServiceRenderNode {
  id: string;
  name: string;
  item: string;
  locationId: string;
  position: Vec3;
}

export interface RoadSegment {
  from: string;
  to: string;
  a: Vec3;
  b: Vec3;
}

export interface GridBounds {
  width: number;
  height: number;
  cell: number;
  worldWidth: number;
  worldDepth: number;
  minX: number;
  maxX: number;
  minZ: number;
  maxZ: number;
}

export const GRID_CELL_UNITS = 1.45;

const PALETTE: Record<LocationKind, { color: string; topColor: string }> = {
  home: { color: "#e26b5b", topColor: "#f4b9a4" },
  cafe: { color: "#3f6f8f", topColor: "#e9d9b7" },
  park: { color: "#a8d58a", topColor: "#88c46c" },
  street: { color: "#e8dec8", topColor: "#f4ecdc" },
  generic: { color: "#cab9a0", topColor: "#e3d6bc" }
};

const HOME_TINTS: { color: string; topColor: string }[] = [
  { color: "#e26b5b", topColor: "#f4b9a4" },
  { color: "#7fb6c8", topColor: "#cfe6ee" },
  { color: "#e7b95b", topColor: "#f7e0aa" },
  { color: "#9b7fb6", topColor: "#d6c5ea" },
  { color: "#6fb98f", topColor: "#bfe1cd" }
];

export function locationKind(location: LocationDefinition): LocationKind {
  if (location.private_home) return "home";
  if (location.id.includes("cafe")) return "cafe";
  if (location.id.includes("park")) return "park";
  if (location.id.includes("street") || location.id.includes("road") || location.id.includes("plaza")) {
    return "street";
  }
  return "generic";
}

export function buildLocationLayout(snapshot: WorldSnapshot): LocationRenderNode[] {
  let homeIndex = 0;

  return snapshot.world.locations.map((location) => {
    const kind = locationKind(location);
    const basePalette = PALETTE[kind];
    let palette = basePalette;
    if (kind === "home") {
      palette = HOME_TINTS[homeIndex % HOME_TINTS.length];
      homeIndex += 1;
    }

    const footprint = gridFootprintSize(location.grid_size.width, location.grid_size.height);
    const size =
      kind === "home"
        ? { x: footprint.x, y: 1.2, z: footprint.z }
        : kind === "cafe"
        ? { x: footprint.x, y: 0.95, z: footprint.z }
        : kind === "park"
        ? { x: footprint.x, y: 0.18, z: footprint.z }
        : kind === "street"
        ? { x: footprint.x, y: 0.08, z: footprint.z }
        : { x: footprint.x, y: 0.9, z: footprint.z };

    return {
      id: location.id,
      name: location.name,
      description: location.description,
      privateHome: location.private_home,
      kind,
      position: gridToWorld(snapshot, location.grid_position, location.grid_size),
      exits: location.exits,
      poiIds: location.poi_ids,
      color: palette.color,
      topColor: palette.topColor,
      size,
      gridPosition: location.grid_position,
      gridSize: location.grid_size,
      facing: location.facing
    };
  });
}

export function buildServiceLayout(
  services: ServiceDefinition[],
  locations: LocationRenderNode[]
): ServiceRenderNode[] {
  const byLocation = new Map(locations.map((location) => [location.id, location]));

  return services.map((service, index) => {
    const host = byLocation.get(service.location_id);
    const base = host?.position ?? { x: 0, y: 0, z: 0 };
    const offset = host?.size.z ?? 1.6;
    return {
      id: service.id,
      name: service.name,
      item: service.item,
      locationId: service.location_id,
      position: {
        x: base.x + 0.6 + index * 0.4,
        y: 0,
        z: base.z + offset / 2 + 0.25
      }
    };
  });
}

export function buildRoadSegments(nodes: LocationRenderNode[]): RoadSegment[] {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const seen = new Set<string>();
  const segments: RoadSegment[] = [];

  for (const node of nodes) {
    for (const exitId of node.exits) {
      const key = [node.id, exitId].sort().join("|");
      if (seen.has(key)) continue;
      seen.add(key);
      const target = byId.get(exitId);
      if (!target) continue;
      segments.push({
        from: node.id,
        to: target.id,
        a: node.position,
        b: target.position
      });
    }
  }

  return segments;
}

export function locationMap(nodes: LocationRenderNode[]) {
  return new Map(nodes.map((node) => [node.id, node]));
}

export function gridBounds(snapshot: WorldSnapshot): GridBounds {
  const width = Math.max(1, snapshot.world.grid.width);
  const height = Math.max(1, snapshot.world.grid.height);
  const cell = Math.max(0.5, snapshot.world.grid.cell_size * GRID_CELL_UNITS);
  const worldWidth = width * cell;
  const worldDepth = height * cell;
  return {
    width,
    height,
    cell,
    worldWidth,
    worldDepth,
    minX: -worldWidth / 2,
    maxX: worldWidth / 2,
    minZ: -worldDepth / 2,
    maxZ: worldDepth / 2
  };
}

function gridToWorld(
  snapshot: WorldSnapshot,
  position: LocationDefinition["grid_position"],
  size: LocationDefinition["grid_size"]
): Vec3 {
  const bounds = gridBounds(snapshot);
  return {
    x: bounds.minX + (position.x + size.width / 2) * bounds.cell,
    y: 0,
    z: bounds.minZ + (position.y + size.height / 2) * bounds.cell
  };
}

export function gridCellCenter(snapshot: WorldSnapshot, x: number, y: number): Vec3 {
  const bounds = gridBounds(snapshot);
  return {
    x: bounds.minX + (x + 0.5) * bounds.cell,
    y: 0,
    z: bounds.minZ + (y + 0.5) * bounds.cell
  };
}

function gridFootprintSize(width: number, height: number) {
  return {
    x: Math.max(1, width) * GRID_CELL_UNITS,
    z: Math.max(1, height) * GRID_CELL_UNITS
  };
}
