"use client";

import { useEffect, useRef } from "react";
import * as pc from "playcanvas";
import type { Character, GroundType, WorldSnapshot } from "@/lib/protocol";
import {
  buildLocationLayout,
  buildServiceLayout,
  gridBounds,
  gridCellCenter,
  locationMap,
  type LocationRenderNode,
  type Vec3
} from "@/lib/world-layout";

const TICKS_PER_REAL_SECOND = 5;
const PICK_RADIUS_PX = 56;
const DRAG_THRESHOLD_PX = 6;

export interface PickedInfo {
  kind: "character" | "location";
  id: string;
}

export interface HoverInfo extends PickedInfo {
  name: string;
  x: number;
  y: number;
}

export interface PlayCanvasViewerProps {
  snapshot: WorldSnapshot | null;
  selectedCharacterId: string | null;
  onPick: (info: PickedInfo | null) => void;
  onHover: (info: HoverInfo | null) => void;
  onSelectedScreenPosition: (pos: { x: number; y: number } | null) => void;
}

interface CharacterRig {
  entity: pc.Entity;
  body: pc.Entity;
  antennaMaterial: pc.StandardMaterial;
  bodyMaterial: pc.StandardMaterial;
  bobSeed: number;
}

interface LocationRig {
  entity: pc.Entity;
  node: LocationRenderNode;
  topHeight: number;
}

export function PlayCanvasViewer(props: PlayCanvasViewerProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const appRef = useRef<pc.Application | null>(null);
  const cameraRef = useRef<pc.Entity | null>(null);
  const rootRef = useRef<pc.Entity | null>(null);
  const characterRigs = useRef<Map<string, CharacterRig>>(new Map());
  const locationRigs = useRef<Map<string, LocationRig>>(new Map());
  const snapshotRef = useRef<WorldSnapshot | null>(props.snapshot);
  const selectedRef = useRef<string | null>(props.selectedCharacterId);
  const hoveredRef = useRef<PickedInfo | null>(null);
  const snapshotArrivedAtRef = useRef<number>(0);
  const propsRef = useRef(props);

  useEffect(() => {
    propsRef.current = props;
  }, [props]);

  useEffect(() => {
    snapshotRef.current = props.snapshot;
    snapshotArrivedAtRef.current = performance.now();
  }, [props.snapshot]);

  useEffect(() => {
    selectedRef.current = props.selectedCharacterId;
  }, [props.selectedCharacterId]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const container = containerRef.current;
    if (!canvas || !container || appRef.current) return;

    const app = new pc.Application(canvas, {
      mouse: new pc.Mouse(canvas),
      touch: new pc.TouchDevice(canvas),
      graphicsDeviceOptions: { antialias: true, alpha: true }
    });
    appRef.current = app;
    app.setCanvasFillMode(pc.FILLMODE_NONE);
    app.setCanvasResolution(pc.RESOLUTION_AUTO);
    app.graphicsDevice.maxPixelRatio = Math.min(2, window.devicePixelRatio || 1);
    app.scene.ambientLight = new pc.Color(0.86, 0.84, 0.78);
    const scene = app.scene as pc.Scene & { gammaCorrection?: number; toneMapping?: number };
    scene.gammaCorrection = pc.GAMMA_SRGB;
    scene.toneMapping = pc.TONEMAP_LINEAR;
    app.start();

    // -------- camera rig --------
    const camera = new pc.Entity("camera");
    camera.addComponent("camera", {
      clearColor: color("#f4ecdc"),
      fov: 26,
      nearClip: 0.1,
      farClip: 240
    });
    cameraRef.current = camera;
    app.root.addChild(camera);

    // Orbit state — slightly tilted overhead view.
    const cameraState = {
      theta: Math.PI * 0.18,
      phi: Math.PI * 0.36,
      radius: 36,
      targetX: 0,
      targetZ: 0,
      targetTheta: Math.PI * 0.18,
      targetPhi: Math.PI * 0.36,
      targetRadius: 36
    };
    const applyCamera = () => {
      const r = cameraState.radius;
      const px = cameraState.targetX + Math.cos(cameraState.phi) * Math.sin(cameraState.theta) * r;
      const py = Math.sin(cameraState.phi) * r;
      const pz = cameraState.targetZ + Math.cos(cameraState.phi) * Math.cos(cameraState.theta) * r;
      camera.setPosition(px, py, pz);
      camera.lookAt(cameraState.targetX, 0, cameraState.targetZ);
    };
    applyCamera();

    // -------- lighting --------
    const sun = new pc.Entity("sun");
    sun.addComponent("light", {
      type: "directional",
      color: new pc.Color(1.0, 0.97, 0.9),
      intensity: 1.05,
      castShadows: true,
      shadowType: pc.SHADOW_PCF3,
      shadowDistance: 50,
      shadowResolution: 4096,
      shadowBias: 0.04,
      normalOffsetBias: 0.04
    });
    sun.setEulerAngles(52, 30, 0);
    app.root.addChild(sun);

    const fill = new pc.Entity("fill");
    fill.addComponent("light", {
      type: "directional",
      color: new pc.Color(0.72, 0.8, 0.88),
      intensity: 0.32
    });
    fill.setEulerAngles(-25, -140, 0);
    app.root.addChild(fill);

    // -------- world root + ground --------
    const root = new pc.Entity("fishtank-world");
    rootRef.current = root;
    app.root.addChild(root);
    const currentCharacterRigs = characterRigs.current;
    const currentLocationRigs = locationRigs.current;

    const ground = primitive("ground", "box", flat("#ede2c8"), { x: 90, y: 0.2, z: 90 });
    setReceiveOnly(ground);
    ground.setPosition(0, -0.11, 0);
    app.root.addChild(ground);

    // -------- resize --------
    const resize = () => {
      const rect = container.getBoundingClientRect();
      app.resizeCanvas(Math.max(2, rect.width), Math.max(2, rect.height));
    };
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(container);

    // -------- update loop --------
    let lastTime = performance.now();
    app.on("update", () => {
      const now = performance.now();
      const dt = Math.min(0.05, (now - lastTime) / 1000);
      lastTime = now;

      // Smooth camera follow on selected character.
      const selectedId = selectedRef.current;
      const selectedRig = selectedId ? characterRigs.current.get(selectedId) : null;
      let targetX = 0;
      let targetZ = 0;
      if (selectedRig) {
        const p = selectedRig.entity.getPosition();
        targetX = p.x * 0.6;
        targetZ = p.z * 0.6;
      }
      cameraState.targetX += (targetX - cameraState.targetX) * Math.min(1, dt * 2.5);
      cameraState.targetZ += (targetZ - cameraState.targetZ) * Math.min(1, dt * 2.5);
      cameraState.theta += (cameraState.targetTheta - cameraState.theta) * Math.min(1, dt * 6);
      cameraState.phi += (cameraState.targetPhi - cameraState.phi) * Math.min(1, dt * 6);
      cameraState.radius += (cameraState.targetRadius - cameraState.radius) * Math.min(1, dt * 6);
      applyCamera();

      const current = snapshotRef.current;
      if (!current) {
        propsRef.current.onSelectedScreenPosition(null);
        return;
      }

      const nodes = buildLocationLayout(current);
      const byLocation = locationMap(nodes);

      // Update characters: position lerp + bob + scale highlight.
      for (const character of Object.values(current.characters)) {
        const rig = characterRigs.current.get(character.id);
        if (!rig) continue;
        const position = characterPosition(character, current, byLocation, snapshotArrivedAtRef.current);
        const bob = Math.sin(now / 220 + rig.bobSeed) * 0.08;
        rig.entity.setPosition(position.x, position.y + 0.55 + bob, position.z);

        const selected = character.id === selectedRef.current;
        const hovered = hoveredRef.current?.kind === "character" && hoveredRef.current.id === character.id;
        const targetScale = selected ? 1.18 : hovered ? 1.08 : 1.0;
        const currentScale = rig.entity.getLocalScale();
        const next = currentScale.x + (targetScale - currentScale.x) * Math.min(1, dt * 12);
        rig.entity.setLocalScale(next, next, next);

        rig.antennaMaterial.diffuse.copy(color(statusColor(character.status)));
        rig.antennaMaterial.update();
      }

      // Subtle hover lift on locations.
      for (const [id, rig] of locationRigs.current) {
        const hovered = hoveredRef.current?.kind === "location" && hoveredRef.current.id === id;
        const targetY = hovered ? 0.08 : 0;
        const cur = rig.entity.getLocalPosition();
        const ny = cur.y + (targetY - cur.y) * Math.min(1, dt * 10);
        rig.entity.setLocalPosition(cur.x, ny, cur.z);
      }

      // Emit selected screen position for floating info card.
      const cameraComp = camera.camera;
      if (cameraComp && selectedRig) {
        const world = selectedRig.entity.getPosition();
        const screen = new pc.Vec3();
        cameraComp.worldToScreen(new pc.Vec3(world.x, world.y + 0.95, world.z), screen);
        propsRef.current.onSelectedScreenPosition({
          x: screen.x,
          y: screen.y
        });
      } else {
        propsRef.current.onSelectedScreenPosition(null);
      }
    });

    // -------- input: orbit / zoom / pick --------
    let isDown = false;
    let dragTotal = 0;
    let lastX = 0;
    let lastY = 0;
    let pickX = 0;
    let pickY = 0;
    let suppressClick = false;

    const onPointerDown = (event: PointerEvent) => {
      isDown = true;
      dragTotal = 0;
      lastX = event.clientX;
      lastY = event.clientY;
      pickX = event.clientX;
      pickY = event.clientY;
      canvas.setPointerCapture(event.pointerId);
      canvas.classList.add("grabbing");
    };

    const onPointerMove = (event: PointerEvent) => {
      if (isDown) {
        const dx = event.clientX - lastX;
        const dy = event.clientY - lastY;
        lastX = event.clientX;
        lastY = event.clientY;
        dragTotal += Math.abs(dx) + Math.abs(dy);
        cameraState.targetTheta -= dx * 0.008;
        cameraState.targetPhi = clamp(
          cameraState.targetPhi + dy * 0.005,
          Math.PI * 0.16,
          Math.PI * 0.48
        );
      }

      // Hover pick (only when not dragging hard).
      if (!isDown || dragTotal < DRAG_THRESHOLD_PX) {
        const rect = canvas.getBoundingClientRect();
        const x = event.clientX - rect.left;
        const y = event.clientY - rect.top;
        const pick = pickAtScreen(x, y);
        if (pick) {
          hoveredRef.current = pick;
          canvas.classList.add("hover-pick");
          propsRef.current.onHover({
            kind: pick.kind,
            id: pick.id,
            name: pick.name,
            x,
            y
          });
        } else {
          if (hoveredRef.current) {
            hoveredRef.current = null;
            propsRef.current.onHover(null);
          }
          canvas.classList.remove("hover-pick");
        }
      }
    };

    const onPointerUp = (event: PointerEvent) => {
      const wasDragging = dragTotal >= DRAG_THRESHOLD_PX;
      isDown = false;
      canvas.classList.remove("grabbing");
      try {
        canvas.releasePointerCapture(event.pointerId);
      } catch {
        /* ignore */
      }
      if (!wasDragging && !suppressClick) {
        const rect = canvas.getBoundingClientRect();
        const x = pickX - rect.left;
        const y = pickY - rect.top;
        const pick = pickAtScreen(x, y);
        propsRef.current.onPick(pick ? { kind: pick.kind, id: pick.id } : null);
      }
      suppressClick = false;
    };

    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      const factor = event.deltaY > 0 ? 1.1 : 0.9;
      cameraState.targetRadius = clamp(cameraState.targetRadius * factor, 14, 80);
    };

    const onLeave = () => {
      if (hoveredRef.current) {
        hoveredRef.current = null;
        propsRef.current.onHover(null);
      }
      canvas.classList.remove("hover-pick");
    };

    // Screen-space picking against character + location anchors.
    const pickAtScreen = (x: number, y: number): HoverInfo | null => {
      const cameraComp = camera.camera;
      if (!cameraComp) return null;
      let best: HoverInfo | null = null;
      let bestDist = PICK_RADIUS_PX * PICK_RADIUS_PX;
      let bestDepth = Number.POSITIVE_INFINITY;

      const consider = (
        worldPos: pc.Vec3,
        info: { kind: "character" | "location"; id: string; name: string }
      ) => {
        const screen = new pc.Vec3();
        cameraComp.worldToScreen(worldPos, screen);
        const sx = screen.x;
        const sy = screen.y;
        if (screen.z < 0) return;
        const dx = sx - x;
        const dy = sy - y;
        const d2 = dx * dx + dy * dy;
        if (d2 < bestDist || (Math.abs(d2 - bestDist) < 1 && screen.z < bestDepth)) {
          if (d2 < PICK_RADIUS_PX * PICK_RADIUS_PX) {
            bestDist = d2;
            bestDepth = screen.z;
            best = { kind: info.kind, id: info.id, name: info.name, x: sx, y: sy };
          }
        }
      };

      for (const [id, rig] of characterRigs.current) {
        const snap = snapshotRef.current;
        const name = snap?.characters[id]?.name ?? id;
        const p = rig.entity.getPosition();
        consider(new pc.Vec3(p.x, p.y + 0.5, p.z), { kind: "character", id, name });
      }
      for (const [id, rig] of locationRigs.current) {
        const p = rig.entity.getPosition();
        consider(new pc.Vec3(p.x, p.y + rig.topHeight + 0.2, p.z), {
          kind: "location",
          id,
          name: rig.node.name
        });
      }
      return best;
    };

    canvas.addEventListener("pointerdown", onPointerDown);
    canvas.addEventListener("pointermove", onPointerMove);
    canvas.addEventListener("pointerup", onPointerUp);
    canvas.addEventListener("pointercancel", onPointerUp);
    canvas.addEventListener("wheel", onWheel, { passive: false });
    canvas.addEventListener("pointerleave", onLeave);

    return () => {
      observer.disconnect();
      canvas.removeEventListener("pointerdown", onPointerDown);
      canvas.removeEventListener("pointermove", onPointerMove);
      canvas.removeEventListener("pointerup", onPointerUp);
      canvas.removeEventListener("pointercancel", onPointerUp);
      canvas.removeEventListener("wheel", onWheel);
      canvas.removeEventListener("pointerleave", onLeave);
      try {
        app.destroy();
      } catch {
        /* ignore */
      }
      appRef.current = null;
      cameraRef.current = null;
      rootRef.current = null;
      currentCharacterRigs.clear();
      currentLocationRigs.clear();
    };
  }, []);

  // Rebuild scene whenever the snapshot world changes (locations/services/characters set).
  useEffect(() => {
    const app = appRef.current;
    const root = rootRef.current;
    const snapshot = props.snapshot;
    if (!app || !root || !snapshot) return;

    // Clear prior children.
    root.children.slice().forEach((child) => child.destroy());
    characterRigs.current.clear();
    locationRigs.current.clear();

    const locationNodes = buildLocationLayout(snapshot);
    const byLocation = locationMap(locationNodes);
    const serviceNodes = buildServiceLayout(snapshot.world.services, locationNodes);

    root.addChild(buildWorldGrid(snapshot));

    // Locations.
    for (const node of locationNodes) {
      const rig = buildLocation(node);
      rig.setPosition(node.position.x, 0, node.position.z);
      if (node.kind === "home" || node.kind === "cafe") {
        rig.setEulerAngles(0, facingAngle(node.facing), 0);
      }
      root.addChild(rig);
      locationRigs.current.set(node.id, {
        entity: rig,
        node,
        topHeight: locationTopHeight(node)
      });
    }

    // Services as little tokens on top of host buildings.
    for (const service of serviceNodes) {
      const host = byLocation.get(service.locationId);
      if (!host) continue;
      const token = primitive(`service-${service.id}`, "cylinder", flat("#f4ecdc"), {
        x: 0.55,
        y: 0.16,
        z: 0.55
      });
      token.setPosition(service.position.x, locationTopHeight(host) + 0.1, service.position.z);
      root.addChild(token);

      const inner = primitive(`service-${service.id}-dot`, "cylinder", flat("#2a2622"), {
        x: 0.18,
        y: 0.18,
        z: 0.18
      });
      inner.setPosition(service.position.x, locationTopHeight(host) + 0.18, service.position.z);
      root.addChild(inner);
    }

    // Characters.
    for (const character of Object.values(snapshot.characters)) {
      const rig = buildCharacter(character);
      const position = characterPosition(character, snapshot, byLocation, snapshotArrivedAtRef.current);
      rig.entity.setPosition(position.x, position.y + 0.55, position.z);
      root.addChild(rig.entity);
      characterRigs.current.set(character.id, rig);
    }
  }, [props.snapshot]);

  return (
    <div ref={containerRef} className="stage">
      <canvas ref={canvasRef} className="stage-canvas" aria-label="Fishtank scene" />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

function buildLocation(node: LocationRenderNode): pc.Entity {
  const group = new pc.Entity(`loc-${node.id}`);

  if (node.kind === "park") {
    const padRadius = Math.min(node.size.x, node.size.z) * 0.22;
    const pad = roundedBox(
      `${node.id}-pad`,
      flat(node.color),
      node.size.x,
      0.18,
      node.size.z,
      padRadius
    );
    pad.setLocalPosition(0, 0.09, 0);
    group.addChild(pad);

    const inner = roundedBox(
      `${node.id}-inner`,
      flat(node.topColor),
      node.size.x * 0.64,
      0.22,
      node.size.z * 0.64,
      padRadius * 0.65
    );
    inner.setLocalPosition(0, 0.2, 0);
    group.addChild(inner);

    const treeSpots = [
      { x: -node.size.x * 0.32, z: -node.size.z * 0.28 },
      { x: node.size.x * 0.28, z: node.size.z * 0.18 },
      { x: -node.size.x * 0.1, z: node.size.z * 0.36 }
    ];
    for (let i = 0; i < treeSpots.length; i++) {
      const spot = treeSpots[i];
      const trunk = primitive(`${node.id}-trunk-${i}`, "cylinder", flat("#7a5d3e"), {
        x: 0.16,
        y: 0.36,
        z: 0.16
      });
      trunk.setPosition(spot.x, 0.36, spot.z);
      group.addChild(trunk);

      const crown = primitive(`${node.id}-crown-${i}`, "sphere", flat("#6caa6b"), {
        x: 0.72,
        y: 0.78,
        z: 0.72
      });
      crown.setPosition(spot.x, 0.82, spot.z);
      group.addChild(crown);
    }
    return group;
  }

  if (node.kind === "street") {
    const slab = roundedBox(
      `${node.id}-slab`,
      flat(node.topColor),
      node.size.x,
      0.1,
      node.size.z,
      Math.min(node.size.x, node.size.z) * 0.16
    );
    setReceiveOnly(slab);
    slab.setLocalPosition(0, 0.05, 0);
    group.addChild(slab);
    return group;
  }

  // Building (home / cafe / generic): chunky stacked rounded cards.
  const baseHeight = Math.max(0.5, node.size.y * 0.6);
  const topHeight = Math.max(0.36, node.size.y * 0.5);
  const cornerRadius = Math.min(node.size.x, node.size.z) * 0.18;

  const base = roundedBox(
    `${node.id}-base`,
    flat(node.color),
    node.size.x,
    baseHeight,
    node.size.z,
    cornerRadius
  );
  base.setLocalPosition(0, baseHeight / 2, 0);
  group.addChild(base);

  const top = roundedBox(
    `${node.id}-top`,
    flat(node.topColor),
    node.size.x * 0.82,
    topHeight,
    node.size.z * 0.82,
    cornerRadius * 0.85
  );
  top.setLocalPosition(0, baseHeight + topHeight / 2, 0);
  group.addChild(top);

  if (node.kind === "home") {
    const door = primitive(`${node.id}-door`, "box", flat(darken(node.color, 0.7)), {
      x: node.size.x * 0.3,
      y: baseHeight * 0.62,
      z: 0.05
    });
    door.setPosition(0, baseHeight * 0.31, node.size.z / 2 + 0.005);
    group.addChild(door);

    const eyeMat = flat("#fffdf6");
    const eye = (offset: number) => {
      const e = primitive(`${node.id}-eye-${offset}`, "box", eyeMat, {
        x: 0.2,
        y: 0.2,
        z: 0.04
      });
      e.setPosition(offset, baseHeight + topHeight * 0.55, (node.size.z * 0.82) / 2 + 0.005);
      group.addChild(e);
    };
    eye(-node.size.x * 0.16);
    eye(node.size.x * 0.16);
  }

  if (node.kind === "cafe") {
    const awning = roundedBox(
      `${node.id}-awning`,
      flat("#f1c264"),
      node.size.x + 0.18,
      0.16,
      node.size.z + 0.18,
      cornerRadius + 0.06
    );
    awning.setLocalPosition(0, baseHeight + 0.04, 0);
    group.addChild(awning);

    const window = primitive(`${node.id}-window`, "box", flat("#2a2622"), {
      x: node.size.x * 0.5,
      y: 0.3,
      z: 0.05
    });
    window.setPosition(0, baseHeight * 0.55, node.size.z / 2 + 0.005);
    group.addChild(window);
  }

  return group;
}

function buildWorldGrid(snapshot: WorldSnapshot) {
  const bounds = gridBounds(snapshot);
  const root = new pc.Entity("world-grid");

  for (let y = 0; y < bounds.height; y++) {
    for (let x = 0; x < bounds.width; x++) {
      const terrain = snapshot.world.grid.terrain[y]?.[x] ?? "ground";
      const center = gridCellCenter(snapshot, x, y);
      const tile = primitive(`tile-${x}-${y}`, "box", flat(groundColor(terrain)), {
        x: bounds.cell,
        y: terrain === "water" ? 0.025 : 0.035,
        z: bounds.cell
      });
      setReceiveOnly(tile);
      tile.setPosition(center.x, terrain === "water" ? -0.002 : 0.004, center.z);
      root.addChild(tile);
    }
  }

  const lineMaterial = flat("#eadfca");
  const majorMaterial = flat("#ded2bd");
  const lineWidth = 0.012;
  for (let x = 0; x <= bounds.width; x++) {
    const worldX = bounds.minX + x * bounds.cell;
    const line = primitive(
      `grid-x-${x}`,
      "box",
      x % 4 === 0 ? majorMaterial : lineMaterial,
      {
        x: x % 4 === 0 ? lineWidth * 1.8 : lineWidth,
        y: 0.035,
        z: bounds.worldDepth
      }
    );
    setReceiveOnly(line);
    line.setPosition(worldX, 0.032, 0);
    root.addChild(line);
  }
  for (let y = 0; y <= bounds.height; y++) {
    const worldZ = bounds.minZ + y * bounds.cell;
    const line = primitive(
      `grid-y-${y}`,
      "box",
      y % 4 === 0 ? majorMaterial : lineMaterial,
      {
        x: bounds.worldWidth,
        y: 0.035,
        z: y % 4 === 0 ? lineWidth * 1.8 : lineWidth
      }
    );
    setReceiveOnly(line);
    line.setPosition(0, 0.034, worldZ);
    root.addChild(line);
  }

  return root;
}

function groundColor(terrain: GroundType) {
  switch (terrain) {
    case "grass":
      return "#d7ecc5";
    case "path":
      return "#fff9e8";
    case "water":
      return "#9ed7e8";
    case "ground":
    default:
      return "#f2ead9";
  }
}

function facingAngle(facing: LocationRenderNode["facing"]) {
  switch (facing) {
    case "north":
      return 180;
    case "east":
      return 90;
    case "west":
      return -90;
    case "south":
    default:
      return 0;
  }
}

function buildCharacter(character: Character): CharacterRig {
  const root = new pc.Entity(`char-${character.id}`);

  const bodyColor = flat(character.body_color || "#5aa3d7");
  const body = primitive(`char-${character.id}-body`, "box", bodyColor, {
    x: 0.46,
    y: 0.62,
    z: 0.42
  });
  body.setPosition(0, 0, 0);
  root.addChild(body);

  // Head: smaller cube perched on top with face panel.
  const head = primitive(`char-${character.id}-head`, "box", bodyColor, {
    x: 0.34,
    y: 0.3,
    z: 0.34
  });
  head.setPosition(0, 0.45, 0);
  root.addChild(head);

  const face = primitive(
    `char-${character.id}-face`,
    "box",
    flat(character.face_color || "#fffdf6"),
    {
      x: 0.22,
      y: 0.12,
      z: 0.02
    }
  );
  face.setPosition(0, 0.46, 0.17);
  root.addChild(face);

  const antennaMaterial = flat(statusColor(character.status));
  const antenna = primitive(`char-${character.id}-antenna`, "sphere", antennaMaterial, {
    x: 0.18,
    y: 0.18,
    z: 0.18
  });
  antenna.setPosition(0, 0.74, 0);
  root.addChild(antenna);

  return {
    entity: root,
    body,
    antennaMaterial,
    bodyMaterial: bodyColor,
    bobSeed: hashSeed(character.id)
  };
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

function locationTopHeight(node: LocationRenderNode) {
  if (node.kind === "park") return 0.22;
  if (node.kind === "street") return 0.08;
  const baseHeight = Math.max(0.45, node.size.y * 0.55);
  const topHeight = Math.max(0.32, node.size.y * 0.45);
  return baseHeight + topHeight;
}

function primitive(
  name: string,
  type: "box" | "sphere" | "cylinder",
  mat: pc.StandardMaterial,
  scale: { x: number; y: number; z: number }
) {
  const entity = new pc.Entity(name);
  entity.addComponent("render", {
    type,
    material: mat,
    castShadows: true,
    receiveShadows: true
  });
  entity.setLocalScale(scale.x, scale.y, scale.z);
  return entity;
}

// Composes a rounded-rectangle prism out of two crossed boxes + 4 quarter
// cylinders at the corners. All parts share the same material so they read
// as one chunky shape with soft silhouettes from above.
function roundedBox(
  name: string,
  mat: pc.StandardMaterial,
  w: number,
  h: number,
  d: number,
  r: number
): pc.Entity {
  const radius = Math.max(0, Math.min(r, Math.min(w, d) / 2 - 0.001));
  if (radius <= 0.02) {
    return primitive(name, "box", mat, { x: w, y: h, z: d });
  }
  const group = new pc.Entity(name);
  const innerW = w - 2 * radius;
  const innerD = d - 2 * radius;

  const ribX = primitive(`${name}-rx`, "box", mat, { x: w, y: h, z: innerD });
  ribX.setLocalPosition(0, 0, 0);
  group.addChild(ribX);

  const ribZ = primitive(`${name}-rz`, "box", mat, { x: innerW, y: h, z: d });
  ribZ.setLocalPosition(0, 0, 0);
  group.addChild(ribZ);

  const cornerOffsets = [
    { sx: 1, sz: 1 },
    { sx: -1, sz: 1 },
    { sx: 1, sz: -1 },
    { sx: -1, sz: -1 }
  ];
  for (const off of cornerOffsets) {
    const cy = primitive(`${name}-c${off.sx}${off.sz}`, "cylinder", mat, {
      x: 2 * radius,
      y: h,
      z: 2 * radius
    });
    cy.setLocalPosition(off.sx * (innerW / 2), 0, off.sz * (innerD / 2));
    group.addChild(cy);
  }
  return group;
}

function setReceiveOnly(entity: pc.Entity) {
  const visit = (current: pc.Entity) => {
    const render = current.render;
    if (render) {
      render.castShadows = false;
      render.receiveShadows = true;
      const instances = render.meshInstances;
      if (instances) {
        for (const inst of instances) {
          inst.castShadow = false;
          inst.receiveShadow = true;
        }
      }
    }
    for (const child of current.children) {
      visit(child as pc.Entity);
    }
  };
  visit(entity);
}

function flat(hex: string) {
  const mat = new pc.StandardMaterial() as pc.StandardMaterial & {
    shininess?: number;
    useMetalness?: boolean;
  };
  mat.diffuse = color(hex);
  mat.specular = new pc.Color(0.04, 0.04, 0.04);
  mat.shininess = 12;
  mat.useMetalness = false;
  mat.update();
  return mat;
}

function color(hex: string) {
  const clean = hex.replace("#", "");
  if (!/^[0-9a-fA-F]{6}$/.test(clean)) {
    return new pc.Color(0.6, 0.7, 0.75, 1);
  }
  return new pc.Color(
    parseInt(clean.slice(0, 2), 16) / 255,
    parseInt(clean.slice(2, 4), 16) / 255,
    parseInt(clean.slice(4, 6), 16) / 255,
    1
  );
}

function darken(hex: string, factor: number) {
  const c = color(hex);
  c.r *= factor;
  c.g *= factor;
  c.b *= factor;
  return rgbToHex(c.r, c.g, c.b);
}

function rgbToHex(r: number, g: number, b: number) {
  const to = (v: number) =>
    Math.max(0, Math.min(255, Math.round(v * 255)))
      .toString(16)
      .padStart(2, "0");
  return `#${to(r)}${to(g)}${to(b)}`;
}

function statusColor(status: Character["status"]) {
  switch (status) {
    case "moving":
      return "#5fc4d3";
    case "ordering":
      return "#e7b95b";
    case "waiting":
      return "#b39ddb";
    case "inside_home":
      return "#c8bfae";
    case "offline_returning_home":
      return "#e8896b";
    case "idle":
    default:
      return "#74c089";
  }
}

function characterPosition(
  character: Character,
  snapshot: WorldSnapshot,
  byLocation: Map<string, LocationRenderNode>,
  arrivedAt: number
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
      const wallClockTicks = ((performance.now() - arrivedAt) / 1000) * TICKS_PER_REAL_SECOND;
      const estimatedTick = Math.min(activity.completes_at_tick - 0.2, snapshot.tick + wallClockTicks);
      const progress = clamp((estimatedTick - activity.started_at_tick) / duration, 0, 0.94);
      const eased = easeInOut(progress);
      return lerp(from, to, eased);
    }
  }

  return from;
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

function hashSeed(id: string) {
  let h = 0;
  for (let i = 0; i < id.length; i++) {
    h = (h * 31 + id.charCodeAt(i)) | 0;
  }
  return (h & 0xffff) / 1000;
}
