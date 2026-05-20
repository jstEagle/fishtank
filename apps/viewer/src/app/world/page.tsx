"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { DevDrawer } from "@/components/DevControls";
import {
  PlayCanvasViewer,
  type HoverInfo,
  type PickedInfo
} from "@/components/PlayCanvasViewer";
import { useFishtank } from "@/lib/use-fishtank";
import type { Character, EventRecord, WorldSnapshot } from "@/lib/protocol";

export default function Home() {
  const { apiUrl, liveUrl, realtime, connected, loading, error, snapshot, events, refresh } = useFishtank();
  const [selectedCharacterId, setSelectedCharacterId] = useState<string | null>(null);
  const [selectedLocationId, setSelectedLocationId] = useState<string | null>(null);
  const [hover, setHover] = useState<HoverInfo | null>(null);
  const [selectedScreenPos, setSelectedScreenPos] = useState<{ x: number; y: number } | null>(null);
  const [drawerOpen, setDrawerOpen] = useState(false);

  const characters = useMemo<Character[]>(() => {
    if (!snapshot) return [];
    return Object.values(snapshot.characters);
  }, [snapshot]);

  const selectedCharacter = useMemo(() => {
    if (!snapshot) return null;
    if (selectedCharacterId && snapshot.characters[selectedCharacterId]) {
      return snapshot.characters[selectedCharacterId];
    }
    return characters[0] ?? null;
  }, [characters, snapshot, selectedCharacterId]);

  const selectedLocation = useMemo(() => {
    if (!snapshot || !selectedLocationId) return null;
    return snapshot.world.locations.find((location) => location.id === selectedLocationId) ?? null;
  }, [snapshot, selectedLocationId]);

  const handlePick = useCallback((info: PickedInfo | null) => {
    if (!info) {
      setSelectedCharacterId(null);
      setSelectedLocationId(null);
      return;
    }
    if (info.kind === "character") {
      setSelectedCharacterId(info.id);
      setSelectedLocationId(null);
    } else {
      setSelectedLocationId(info.id);
      setSelectedCharacterId(null);
    }
  }, []);

  useEffect(() => {
    if (selectedCharacterId && snapshot && !snapshot.characters[selectedCharacterId]) {
      setSelectedCharacterId(null);
    }
  }, [snapshot, selectedCharacterId]);

  return (
    <main className="stage">
      <PlayCanvasViewer
        snapshot={snapshot}
        selectedCharacterId={selectedCharacter?.id ?? null}
        onPick={handlePick}
        onHover={setHover}
        onSelectedScreenPosition={setSelectedScreenPos}
      />

      <div className="stage-overlay">
        <div className="chip-row top-left">
          <div className="chip title-chip">
            <span className="eyebrow">Fishtank</span>
            <span className="title">{snapshot?.world.name ?? "Waiting for server"}</span>
          </div>
          <div className={`chip ${connected ? "live" : loading ? "" : "offline"}`}>
            <span className="dot" />
            {loading
              ? "connecting"
              : connected
                ? `${realtime ? "live" : "poll"} · tick ${snapshot?.tick ?? "—"}`
                : "offline"}
          </div>
        </div>

        <div className="chip-row top-right">
          {error ? (
            <div className="chip error-chip" title={error}>
              connection failed
            </div>
          ) : null}
          <button
            type="button"
            className={`icon-button ${drawerOpen ? "active" : ""}`}
            onClick={() => setDrawerOpen((open) => !open)}
            aria-label="Toggle debug panel"
            title="Debug panel"
          >
            {drawerOpen ? "×" : "i"}
          </button>
        </div>

        {drawerOpen ? (
          realtime ? (
            <ObserverPanel
              apiUrl={apiUrl}
              liveUrl={liveUrl}
              snapshot={snapshot}
              events={events}
              selected={selectedCharacter}
              onRefresh={refresh}
              onClose={() => setDrawerOpen(false)}
            />
          ) : (
            <DevDrawer
              apiUrl={apiUrl}
              snapshot={snapshot}
              selected={selectedCharacter}
              onCreated={(id) => {
                setSelectedCharacterId(id);
                setDrawerOpen(false);
              }}
              onRefresh={refresh}
              onClose={() => setDrawerOpen(false)}
            />
          )
        ) : null}

        <div className="chip-row bottom-center">
          {characters.length === 0 ? (
            <div className="chip dock-empty">No characters in view yet</div>
          ) : (
            <div className="dock">
              {characters.map((character) => (
                <button
                  key={character.id}
                  type="button"
                  className={`dock-item ${selectedCharacter?.id === character.id ? "selected" : ""}`}
                  onClick={() =>
                    setSelectedCharacterId((id) => (id === character.id ? null : character.id))
                  }
                >
                  <span
                    className="swatch"
                    style={{ background: character.body_color || "#5aa3d7" }}
                  />
                  {character.name}
                </button>
              ))}
            </div>
          )}
        </div>

        {hover && (!selectedCharacter || selectedCharacter.id !== hover.id) ? (
          <div className="tooltip" style={{ left: hover.x, top: hover.y }}>
            {hover.name}
          </div>
        ) : null}

        {selectedCharacter && selectedScreenPos ? (
          <CharacterInfoCard
            character={selectedCharacter}
            snapshot={snapshot}
            x={selectedScreenPos.x}
            y={selectedScreenPos.y}
          />
        ) : null}

        {selectedLocation &&
        hover &&
        hover.kind === "location" &&
        hover.id === selectedLocation.id ? (
          <LocationInfoCard
            name={selectedLocation.name}
            description={selectedLocation.description}
            x={hover.x}
            y={hover.y}
          />
        ) : null}
      </div>
    </main>
  );
}

function ObserverPanel({
  apiUrl,
  liveUrl,
  snapshot,
  events,
  selected,
  onRefresh,
  onClose
}: {
  apiUrl: string;
  liveUrl: string | null;
  snapshot: WorldSnapshot | null;
  events: EventRecord[];
  selected: Character | null;
  onRefresh: () => Promise<void>;
  onClose: () => void;
}) {
  const transcript = Object.values(snapshot?.conversations ?? {})
    .flatMap((conversation) => conversation.recent_messages)
    .slice(-8);
  return (
    <aside className="dev-drawer observer-panel">
      <div className="drawer-header">
        <div>
          <span className="eyebrow">Observer</span>
          <h2>Live world</h2>
        </div>
        <button type="button" className="icon-button" onClick={onClose} aria-label="Close panel">
          ×
        </button>
      </div>
      <div className="drawer-section">
        <div className="row">
          <span>edge</span>
          <strong>{liveUrl ? new URL(liveUrl).host : "local"}</strong>
        </div>
        <div className="row">
          <span>fallback api</span>
          <strong>{new URL(apiUrl).host}</strong>
        </div>
        <div className="row">
          <span>selected</span>
          <strong>{selected?.name ?? "none"}</strong>
        </div>
        <button type="button" onClick={() => void onRefresh()}>
          refresh snapshot
        </button>
      </div>
      <div className="drawer-section">
        <h3>Transcript</h3>
        {transcript.length ? (
          transcript.map((message, index) => (
            <p key={`${message.tick}-${index}`} className="event-line">
              <strong>{message.speaker_id}</strong> {message.text}
            </p>
          ))
        ) : (
          <p className="muted">No speech yet.</p>
        )}
      </div>
      <div className="drawer-section">
        <h3>Events</h3>
        {events.slice(-10).map((event) => (
          <p key={event.id} className="event-line">
            #{event.id} · tick {event.tick} · {event.kind.event}
          </p>
        ))}
      </div>
    </aside>
  );
}

function CharacterInfoCard({
  character,
  snapshot,
  x,
  y
}: {
  character: Character;
  snapshot: WorldSnapshot | null;
  x: number;
  y: number;
}) {
  const locationName = snapshot
    ? snapshot.world.locations.find((location) => location.id === character.location_id)?.name ??
      character.location_id
    : character.location_id;
  const activity = character.current_activity;
  const status = character.status.replaceAll("_", " ");

  return (
    <div className="info-card" style={{ left: x, top: y }}>
      <h3>
        <span className="badge" style={{ background: character.body_color }} />
        {character.name}
      </h3>
      <div className="row">
        <span>status</span>
        <strong>{status}</strong>
      </div>
      <div className="row">
        <span>place</span>
        <strong>{locationName}</strong>
      </div>
      <div className="row">
        <span>coins</span>
        <strong>
          {character.coins}
          {character.reserved_coins ? ` (${character.reserved_coins} held)` : ""}
        </strong>
      </div>
      {activity ? (
        <div className="quote">
          {activity.description ||
            `${activity.kind.replaceAll("_", " ")} → ${activity.target_id ?? ""}`}
        </div>
      ) : null}
    </div>
  );
}

function LocationInfoCard({
  name,
  description,
  x,
  y
}: {
  name: string;
  description: string;
  x: number;
  y: number;
}) {
  return (
    <div className="info-card" style={{ left: x, top: y }}>
      <h3>{name}</h3>
      <div className="quote">{description}</div>
    </div>
  );
}
