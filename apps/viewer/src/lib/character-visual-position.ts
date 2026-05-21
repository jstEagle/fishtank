import type { Character, WorldSnapshot } from "./protocol";
import type { LocationRenderNode, Vec3 } from "./world-layout";

export function characterPosition(
  character: Character,
  snapshot: WorldSnapshot,
  byLocation: Map<string, LocationRenderNode>,
  estimatedTick: number
): Vec3 {
  const from = byLocation.get(character.location_id)?.position ?? { x: 0, y: 0, z: 0 };
  const activity = character.current_activity;

  if (
    activity &&
    (activity.kind === "moving" || activity.kind === "returning_home") &&
    activity.target_id
  ) {
    const to = byLocation.get(activity.target_id)?.position;
    if (to) {
      const duration = Math.max(1, activity.completes_at_tick - activity.started_at_tick);
      const timelineTick = Math.min(
        activity.completes_at_tick - 0.2,
        Math.max(snapshot.tick, estimatedTick)
      );
      const progress = clamp((timelineTick - activity.started_at_tick) / duration, 0, 0.94);
      const eased = easeInOut(progress);
      return lerp(from, to, eased);
    }
  }

  return from;
}

export function characterVisualPositions(
  snapshot: WorldSnapshot,
  byLocation: Map<string, LocationRenderNode>,
  estimatedTick: number
): Map<string, Vec3> {
  const basePositions = new Map<string, Vec3>();
  const openClusters = new Map<string, Character[]>();

  for (const character of Object.values(snapshot.characters)) {
    const base = characterPosition(character, snapshot, byLocation, estimatedTick);
    basePositions.set(character.id, base);

    const location = byLocation.get(character.location_id);
    if (!location || !isOpenLocation(location) || isWalking(character)) continue;
    const cluster = openClusters.get(location.id) ?? [];
    cluster.push(character);
    openClusters.set(location.id, cluster);
  }

  for (const [locationId, characters] of openClusters) {
    if (characters.length < 2) continue;
    const location = byLocation.get(locationId);
    if (!location) continue;

    const sorted = [...characters].sort((a, b) => a.id.localeCompare(b.id));
    const angleOffset = stableUnit(locationId) * Math.PI * 2;

    sorted.forEach((character, index) => {
      const base = basePositions.get(character.id);
      if (!base) return;
      const offset = clusterOffset(index, sorted.length, angleOffset, character.id);
      basePositions.set(character.id, clampToLocation(base, offset, location));
    });
  }

  return basePositions;
}

export function isCharacterRigVisible(
  character: Character,
  byLocation: Map<string, LocationRenderNode>
) {
  const location = byLocation.get(character.location_id);
  return isWalking(character) || !location || isOpenLocation(location);
}

export function buildingOccupants(
  snapshot: WorldSnapshot,
  byLocation: Map<string, LocationRenderNode>
): Map<string, Character[]> {
  const occupants = new Map<string, Character[]>();

  for (const character of Object.values(snapshot.characters)) {
    const location = byLocation.get(character.location_id);
    if (!location || isOpenLocation(location) || isWalking(character)) continue;
    const group = occupants.get(location.id) ?? [];
    group.push(character);
    occupants.set(location.id, group);
  }

  for (const group of occupants.values()) {
    group.sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id));
  }

  return occupants;
}

function isOpenLocation(location: LocationRenderNode) {
  return location.kind === "street" || location.kind === "park";
}

function isWalking(character: Character) {
  const activity = character.current_activity;
  return Boolean(
    activity &&
      activity.status === "active" &&
      (activity.kind === "moving" || activity.kind === "returning_home")
  );
}

function clusterOffset(index: number, total: number, angleOffset: number, id: string): Vec3 {
  const goldenAngle = Math.PI * (3 - Math.sqrt(5));
  const ring = Math.floor(index / 6);
  const radius = 0.38 + ring * 0.28 + Math.min(total, 10) * 0.018;
  const jitter = (stableUnit(id) - 0.5) * 0.22;
  const angle = angleOffset + index * goldenAngle + jitter;

  return {
    x: Math.cos(angle) * radius,
    y: 0,
    z: Math.sin(angle) * radius
  };
}

function clampToLocation(base: Vec3, offset: Vec3, location: LocationRenderNode): Vec3 {
  const margin = 0.42;
  const halfX = Math.max(0, location.size.x / 2 - margin);
  const halfZ = Math.max(0, location.size.z / 2 - margin);
  const minX = location.position.x - halfX;
  const maxX = location.position.x + halfX;
  const minZ = location.position.z - halfZ;
  const maxZ = location.position.z + halfZ;

  return {
    x: clamp(base.x + offset.x, minX, maxX),
    y: base.y,
    z: clamp(base.z + offset.z, minZ, maxZ)
  };
}

function stableUnit(input: string) {
  let hash = 2166136261;
  for (let i = 0; i < input.length; i += 1) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0) / 4294967295;
}

function lerp(from: Vec3, to: Vec3, progress: number): Vec3 {
  return {
    x: from.x + (to.x - from.x) * progress,
    y: from.y + (to.y - from.y) * progress,
    z: from.z + (to.z - from.z) * progress
  };
}

function easeInOut(t: number) {
  return t * t * (3 - 2 * t);
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}
