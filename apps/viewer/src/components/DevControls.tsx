"use client";

import { FormEvent, useMemo, useState } from "react";
import { sendCommand } from "@/lib/api";
import type {
  Character,
  Command,
  HomeAction,
  ServiceDefinition,
  WorldSnapshot
} from "@/lib/protocol";

interface DevDrawerProps {
  apiUrl: string;
  snapshot: WorldSnapshot | null;
  selected: Character | null;
  onCreated: (characterId: string) => void;
  onRefresh: () => Promise<void>;
  onClose: () => void;
}

const DEFAULT_BODY = "#e26b5b";
const DEFAULT_FACE = "#fffdf6";

export function DevDrawer({
  apiUrl,
  snapshot,
  selected,
  onCreated,
  onRefresh,
  onClose
}: DevDrawerProps) {
  const [characterId, setCharacterId] = useState(
    () => `char_${Date.now().toString(36).slice(-4)}`
  );
  const [name, setName] = useState("New Agent");
  const [bodyColor, setBodyColor] = useState(DEFAULT_BODY);
  const [faceColor, setFaceColor] = useState(DEFAULT_FACE);
  const [moveTarget, setMoveTarget] = useState("");
  const [sayText, setSayText] = useState("Hello.");
  const [waitTicks, setWaitTicks] = useState(3);
  const [orderServiceId, setOrderServiceId] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [result, setResult] = useState<string>("Ready.");

  const currentLocation = useMemo(() => {
    if (!snapshot || !selected) return null;
    return (
      snapshot.world.locations.find((location) => location.id === selected.location_id) ?? null
    );
  }, [selected, snapshot]);

  const moveTargets = useMemo(() => {
    if (!snapshot || !currentLocation) return [];
    return currentLocation.exits
      .map((id) => snapshot.world.locations.find((location) => location.id === id))
      .filter(Boolean) as WorldSnapshot["world"]["locations"];
  }, [currentLocation, snapshot]);

  const localServices = useMemo(() => {
    if (!snapshot || !selected) return [];
    return snapshot.world.services.filter(
      (service) => service.location_id === selected.location_id
    );
  }, [selected, snapshot]);

  async function run(label: string, actorId: string, command: Command) {
    setBusy(label);
    setResult(`Sending ${label}…`);
    try {
      const response = await sendCommand(actorId, command, apiUrl);
      await onRefresh();
      if (response.ok) {
        setResult(`${label} accepted · tick ${response.tick}`);
      } else {
        setResult(`${label} rejected · ${response.error?.message ?? "unknown"}`);
      }
      return response.ok;
    } catch (error) {
      setResult(`${label} failed · ${error instanceof Error ? error.message : String(error)}`);
      return false;
    } finally {
      setBusy(null);
    }
  }

  async function createCharacter(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const id = normalizedCharacterId(characterId);
    const ok = await run("create character", id, {
      kind: "create_character",
      name: name.trim() || id,
      body_color: bodyColor,
      face_color: faceColor
    });
    if (ok) {
      onCreated(id);
      setCharacterId(`char_${Math.random().toString(36).slice(2, 7)}`);
    }
  }

  async function moveSelected(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selected || !moveTarget) return;
    await run("move", selected.id, {
      kind: "move",
      mode: { mode: "to_target", target: moveTarget }
    });
  }

  async function say(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selected || !sayText.trim()) return;
    await run("say", selected.id, {
      kind: "say",
      target: { mode: "room" },
      text: sayText.trim()
    });
  }

  async function wait(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selected) return;
    await run("wait", selected.id, {
      kind: "wait",
      ticks: Math.max(1, Math.floor(waitTicks))
    });
  }

  async function order(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selected || !orderServiceId) return;
    const service = localServices.find((candidate) => candidate.id === orderServiceId);
    await run("order", selected.id, {
      kind: "order",
      service_id: orderServiceId,
      item: service?.item ?? "coffee"
    });
  }

  async function home(action: HomeAction) {
    if (!selected) return;
    await run(`home ${action.replaceAll("_", " ")}`, selected.id, {
      kind: "home",
      action
    });
  }

  return (
    <section className="drawer" role="dialog" aria-label="Dev controls">
      <header style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <h3>Dev controls</h3>
        <button
          type="button"
          className="secondary"
          onClick={onClose}
          aria-label="Close dev controls"
        >
          close
        </button>
      </header>

      <form className="field" onSubmit={createCharacter}>
        <label>id</label>
        <input
          type="text"
          value={characterId}
          onChange={(event) => setCharacterId(event.target.value)}
        />
        <label>name</label>
        <input type="text" value={name} onChange={(event) => setName(event.target.value)} />
        <div className="colors">
          <div className="field">
            <label>body</label>
            <input
              type="color"
              value={bodyColor}
              onChange={(event) => setBodyColor(event.target.value)}
            />
          </div>
          <div className="field">
            <label>face</label>
            <input
              type="color"
              value={faceColor}
              onChange={(event) => setFaceColor(event.target.value)}
            />
          </div>
        </div>
        <button type="submit" className="primary" disabled={Boolean(busy) || !snapshot}>
          create character
        </button>
      </form>

      <div className="divider" />

      <h3>{selected ? `act as ${selected.name}` : "select a character"}</h3>

      <form className="field" onSubmit={moveSelected}>
        <label>move</label>
        <div style={{ display: "flex", gap: 6 }}>
          <select
            value={moveTarget}
            onChange={(event) => setMoveTarget(event.target.value)}
            disabled={!selected || moveTargets.length === 0}
            style={{ flex: 1 }}
          >
            <option value="">target</option>
            {moveTargets.map((location) => (
              <option key={location.id} value={location.id}>
                {location.name}
              </option>
            ))}
          </select>
          <button
            type="submit"
            className="secondary"
            disabled={Boolean(busy) || !selected || !moveTarget}
          >
            go
          </button>
        </div>
      </form>

      <form className="field" onSubmit={say}>
        <label>say</label>
        <textarea
          value={sayText}
          onChange={(event) => setSayText(event.target.value)}
          disabled={!selected}
          rows={2}
        />
        <button
          type="submit"
          className="secondary"
          disabled={Boolean(busy) || !selected || !sayText.trim()}
        >
          say to room
        </button>
      </form>

      <form className="field" onSubmit={wait}>
        <label>wait (ticks)</label>
        <div style={{ display: "flex", gap: 6 }}>
          <input
            type="number"
            min={1}
            max={3600}
            value={waitTicks}
            onChange={(event) => setWaitTicks(Number(event.target.value))}
            disabled={!selected}
            style={{ flex: 1 }}
          />
          <button type="submit" className="secondary" disabled={Boolean(busy) || !selected}>
            wait
          </button>
        </div>
      </form>

      <form className="field" onSubmit={order}>
        <label>order</label>
        <div style={{ display: "flex", gap: 6 }}>
          <select
            value={orderServiceId}
            onChange={(event) => setOrderServiceId(event.target.value)}
            disabled={!selected || localServices.length === 0}
            style={{ flex: 1 }}
          >
            <option value="">service here</option>
            {localServices.map((service) => (
              <option key={service.id} value={service.id}>
                {serviceLabel(service)}
              </option>
            ))}
          </select>
          <button
            type="submit"
            className="secondary"
            disabled={Boolean(busy) || !selected || !orderServiceId}
          >
            order
          </button>
        </div>
      </form>

      <div className="row-buttons">
        <button
          type="button"
          className="secondary"
          disabled={Boolean(busy) || !selected}
          onClick={() => void home("enter")}
        >
          enter home
        </button>
        <button
          type="button"
          className="secondary"
          disabled={Boolean(busy) || !selected}
          onClick={() => void home("leave")}
        >
          leave home
        </button>
        <button
          type="button"
          className="secondary"
          disabled={Boolean(busy) || !selected}
          onClick={() => void home("return_home")}
        >
          return home
        </button>
      </div>

      <p className="result">{result}</p>
    </section>
  );
}

function normalizedCharacterId(input: string) {
  const clean = input.trim().toLowerCase().replace(/[^a-z0-9_.-]+/g, "_").replace(/^_+|_+$/g, "");
  return clean || `char_${Date.now().toString(36)}`;
}

function serviceLabel(service: ServiceDefinition) {
  return `${service.item} · ${service.price_coins}c`;
}
