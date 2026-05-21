"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { DevDrawer } from "@/components/DevControls";
import {
  PlayCanvasViewer,
  type BuildingOccupantInfo,
  type HoverInfo,
  type PickedInfo
} from "@/components/PlayCanvasViewer";
import { useFishtank } from "@/lib/use-fishtank";
import type { Character, EventKind, EventRecord, WorldSnapshot } from "@/lib/protocol";

export default function Home() {
  const { apiUrl, realtime, connected, loading, error, snapshot, events, refresh } = useFishtank();
  const [selectedCharacterId, setSelectedCharacterId] = useState<string | null>(null);
  const [selectedLocationId, setSelectedLocationId] = useState<string | null>(null);
  const [hover, setHover] = useState<HoverInfo | null>(null);
  const [selectedScreenPos, setSelectedScreenPos] = useState<{ x: number; y: number } | null>(null);
  const [buildingOccupants, setBuildingOccupants] = useState<BuildingOccupantInfo[]>([]);
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

  const handleBuildingOccupants = useCallback((next: BuildingOccupantInfo[]) => {
    setBuildingOccupants((previous) =>
      sameBuildingOccupants(previous, next) ? previous : next
    );
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
        onBuildingOccupants={handleBuildingOccupants}
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
            <LiveFeedCard
              snapshot={snapshot}
              events={events}
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

        {buildingOccupants.map((entry) => (
          <BuildingOccupantsCard
            key={entry.locationId}
            entry={entry}
            selectedCharacterId={selectedCharacterId}
            onSelectCharacter={(id) => {
              setSelectedCharacterId(id);
              setSelectedLocationId(null);
            }}
          />
        ))}

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

function sameBuildingOccupants(
  previous: BuildingOccupantInfo[],
  next: BuildingOccupantInfo[]
) {
  if (previous.length !== next.length) return false;

  return previous.every((entry, index) => {
    const nextEntry = next[index];
    if (!nextEntry) return false;
    if (entry.locationId !== nextEntry.locationId) return false;
    if (entry.locationName !== nextEntry.locationName) return false;
    if (Math.round(entry.x) !== Math.round(nextEntry.x)) return false;
    if (Math.round(entry.y) !== Math.round(nextEntry.y)) return false;
    if (entry.characters.length !== nextEntry.characters.length) return false;

    return entry.characters.every((character, characterIndex) => {
      const nextCharacter = nextEntry.characters[characterIndex];
      return (
        nextCharacter &&
        character.id === nextCharacter.id &&
        character.name === nextCharacter.name &&
        character.body_color === nextCharacter.body_color &&
        character.face_color === nextCharacter.face_color
      );
    });
  });
}

function BuildingOccupantsCard({
  entry,
  selectedCharacterId,
  onSelectCharacter
}: {
  entry: BuildingOccupantInfo;
  selectedCharacterId: string | null;
  onSelectCharacter: (id: string) => void;
}) {
  return (
    <div className="occupants-card" style={{ left: entry.x, top: entry.y }}>
      <div className="occupants-card-head">
        <span>{entry.locationName}</span>
        <strong>{entry.characters.length}</strong>
      </div>
      <div className="occupants-list">
        {entry.characters.map((character) => (
          <button
            key={character.id}
            type="button"
            className={`occupant-row ${selectedCharacterId === character.id ? "selected" : ""}`}
            onClick={() => onSelectCharacter(character.id)}
          >
            <span className="bot-mini" style={{ background: character.body_color || "#5aa3d7" }}>
              <span style={{ background: character.face_color || "#fffdf6" }} />
            </span>
            <span>{character.name}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

interface FeedItem {
  id: string;
  tick: number;
  kind: "speech" | "event";
  speaker?: string;
  speakerColor?: string;
  text: string;
}

function LiveFeedCard({
  snapshot,
  events,
  onClose
}: {
  snapshot: WorldSnapshot | null;
  events: EventRecord[];
  onClose: () => void;
}) {
  const items = useMemo(() => buildFeed(events, snapshot), [events, snapshot]);

  return (
    <aside className="live-feed" role="dialog" aria-label="Live feed">
      <header className="live-feed-head">
        <div className="live-feed-title">
          <span className="eyebrow">
            <span className="dot" />
            Live feed
          </span>
          <span className="live-feed-tick">tick {snapshot?.tick ?? "—"}</span>
        </div>
        <button
          type="button"
          className="icon-button live-feed-close"
          onClick={onClose}
          aria-label="Close live feed"
        >
          ×
        </button>
      </header>
      <div className="live-feed-list">
        {items.length === 0 ? (
          <p className="live-feed-empty">Waiting for the world to stir…</p>
        ) : (
          items.map((item) => (
            <div key={item.id} className={`live-feed-item live-feed-item-${item.kind}`}>
              <span className="live-feed-tick-stamp">t{item.tick}</span>
              <div className="live-feed-body">
                {item.speaker ? (
                  <span className="live-feed-speaker">
                    <span
                      className="live-feed-swatch"
                      style={{ background: item.speakerColor ?? "var(--ink-faint)" }}
                    />
                    {item.speaker}
                  </span>
                ) : null}
                <span className="live-feed-text">{item.text}</span>
              </div>
            </div>
          ))
        )}
      </div>
    </aside>
  );
}

function buildFeed(events: EventRecord[], snapshot: WorldSnapshot | null): FeedItem[] {
  const nameFor = (characterId: string | null | undefined) => {
    if (!characterId) return "Someone";
    return snapshot?.characters[characterId]?.name ?? characterId;
  };
  const colorFor = (characterId: string | null | undefined) =>
    characterId ? snapshot?.characters[characterId]?.body_color : undefined;
  const locationFor = (locationId: string | null | undefined) => {
    if (!locationId) return "somewhere";
    return (
      snapshot?.world.locations.find((location) => location.id === locationId)?.name ?? locationId
    );
  };

  const feed: FeedItem[] = [];
  for (const event of events) {
    const item = describeEvent(event.kind, { nameFor, colorFor, locationFor });
    if (!item) continue;
    feed.push({
      id: `e${event.id}`,
      tick: event.tick,
      kind: item.kind,
      speaker: item.speaker,
      speakerColor: item.speakerColor,
      text: item.text
    });
  }

  return feed.slice(-30).reverse();
}

interface DescribeHelpers {
  nameFor: (characterId: string | null | undefined) => string;
  colorFor: (characterId: string | null | undefined) => string | undefined;
  locationFor: (locationId: string | null | undefined) => string;
}

function describeEvent(
  kind: EventKind,
  helpers: DescribeHelpers
):
  | {
      kind: "speech" | "event";
      speaker?: string;
      speakerColor?: string;
      text: string;
    }
  | null {
  switch (kind.event) {
    case "message_spoken":
      return {
        kind: "speech",
        speaker: helpers.nameFor(kind.speaker_id),
        speakerColor: helpers.colorFor(kind.speaker_id),
        text: `“${kind.text}”`
      };
    case "character_created":
      return {
        kind: "event",
        speaker: helpers.nameFor(kind.character_id),
        speakerColor: helpers.colorFor(kind.character_id),
        text: "arrived in the world"
      };
    case "character_moved":
      return {
        kind: "event",
        speaker: helpers.nameFor(kind.character_id),
        speakerColor: helpers.colorFor(kind.character_id),
        text: `moved to ${helpers.locationFor(kind.to)}`
      };
    case "activity_started": {
      const description = kind.description?.trim();
      if (!description) return null;
      return {
        kind: "event",
        speaker: helpers.nameFor(kind.character_id),
        speakerColor: helpers.colorFor(kind.character_id),
        text: description
      };
    }
    case "activity_failed":
      return {
        kind: "event",
        speaker: helpers.nameFor(kind.character_id),
        speakerColor: helpers.colorFor(kind.character_id),
        text: `couldn't finish — ${kind.reason}`
      };
    case "coins_spent":
      return {
        kind: "event",
        speaker: helpers.nameFor(kind.character_id),
        speakerColor: helpers.colorFor(kind.character_id),
        text: `spent ${kind.amount} coin${kind.amount === 1 ? "" : "s"}`
      };
    case "character_sent_home":
      return {
        kind: "event",
        speaker: helpers.nameFor(kind.character_id),
        speakerColor: helpers.colorFor(kind.character_id),
        text: "headed home"
      };
    case "world_expanded":
      return {
        kind: "event",
        text: `The world expanded — new block opened up`
      };
    default:
      return null;
  }
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
