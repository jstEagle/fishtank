import type { EventRecord, WorldSnapshot } from "./protocol";

export interface NewsItem {
  id: string;
  tick: number;
  title: string;
  body: string;
  kind: "world" | "character" | "economy" | "activity";
}

export interface TransactionItem {
  id: string;
  tick: number;
  type: "earned" | "spent" | "reserved" | "released";
  amount: number;
  label: string;
}

export interface CharacterLedger {
  earned: number;
  spent: number;
  reserved: number;
  released: number;
  items: TransactionItem[];
}

export interface LocationEarning {
  characterId: string;
  characterName: string;
  characterColor?: string;
  amount: number;
}

export function buildNewsItems(events: EventRecord[], snapshot: WorldSnapshot | null): NewsItem[] {
  const started = activityStarts(events);
  const items = events.flatMap((event): NewsItem[] => {
    const kind = event.kind;
    switch (kind.event) {
      case "world_expanded":
        return [
          {
            id: `news.${event.id}`,
            tick: event.tick,
            kind: "world",
            title: "New block opened",
            body: `${kind.homes_added} homes, ${kind.services_added} services, and ${kind.parks_added} parks were added.`
          }
        ];
      case "character_created":
        return [
          {
            id: `news.${event.id}`,
            tick: event.tick,
            kind: "character",
            title: `${characterName(snapshot, kind.character_id)} arrived`,
            body: "A new agent joined the shared world."
          }
        ];
      case "coins_earned":
        return [
          {
            id: `news.${event.id}`,
            tick: event.tick,
            kind: "economy",
            title: `${characterName(snapshot, kind.character_id)} earned ${coinText(kind.amount)}`,
            body: `Source: ${sourceName(snapshot, kind.source_id)}.`
          }
        ];
      case "coins_spent":
        return [
          {
            id: `news.${event.id}`,
            tick: event.tick,
            kind: "economy",
            title: `${characterName(snapshot, kind.character_id)} spent ${coinText(kind.amount)}`,
            body: `On ${kind.item ?? sourceName(snapshot, kind.source_id ?? "")}.`
          }
        ];
      case "activity_completed": {
        const start = started.get(kind.activity_id);
        if (!start) return [];
        return [
          {
            id: `news.${event.id}`,
            tick: event.tick,
            kind: "activity",
            title: `${characterName(snapshot, kind.character_id)} finished an activity`,
            body: start
          }
        ];
      }
      default:
        return [];
    }
  });

  return items.reverse();
}

export function characterLedger(events: EventRecord[], characterId: string, snapshot: WorldSnapshot | null): CharacterLedger {
  const ledger: CharacterLedger = {
    earned: 0,
    spent: 0,
    reserved: 0,
    released: 0,
    items: []
  };

  for (const event of events) {
    const kind = event.kind;
    if (!("character_id" in kind) || kind.character_id !== characterId) continue;
    if (kind.event === "coins_earned") {
      ledger.earned += kind.amount;
      ledger.items.push({
        id: `tx.${event.id}`,
        tick: event.tick,
        type: "earned",
        amount: kind.amount,
        label: `Earned at ${sourceName(snapshot, kind.source_id)}`
      });
    } else if (kind.event === "coins_spent") {
      ledger.spent += kind.amount;
      ledger.items.push({
        id: `tx.${event.id}`,
        tick: event.tick,
        type: "spent",
        amount: kind.amount,
        label: `Spent on ${kind.item ?? sourceName(snapshot, kind.source_id ?? "")}`
      });
    } else if (kind.event === "coins_reserved") {
      ledger.reserved += kind.amount;
      ledger.items.push({
        id: `tx.${event.id}`,
        tick: event.tick,
        type: "reserved",
        amount: kind.amount,
        label: "Reserved for a queued or timed service"
      });
    } else if (kind.event === "coins_released") {
      ledger.released += kind.amount;
      ledger.items.push({
        id: `tx.${event.id}`,
        tick: event.tick,
        type: "released",
        amount: kind.amount,
        label: "Released back to spendable balance"
      });
    }
  }

  ledger.items.reverse();
  return ledger;
}

export function locationEarnings(
  events: EventRecord[],
  snapshot: WorldSnapshot | null,
  locationId: string
): LocationEarning[] {
  const siteIds = new Set(
    snapshot?.world.activity_sites
      .filter((site) => site.location_id === locationId)
      .map((site) => site.id) ?? []
  );
  const totals = new Map<string, number>();
  for (const event of events) {
    const kind = event.kind;
    if (kind.event !== "coins_earned" || !siteIds.has(kind.source_id)) continue;
    totals.set(kind.character_id, (totals.get(kind.character_id) ?? 0) + kind.amount);
  }

  return Array.from(totals.entries())
    .map(([characterId, amount]) => ({
      characterId,
      characterName: characterName(snapshot, characterId),
      characterColor: snapshot?.characters[characterId]?.body_color,
      amount
    }))
    .sort((a, b) => b.amount - a.amount || a.characterName.localeCompare(b.characterName));
}

function activityStarts(events: EventRecord[]) {
  const starts = new Map<string, string>();
  for (const event of events) {
    const kind = event.kind;
    if (kind.event === "activity_started") {
      starts.set(kind.activity_id, kind.description);
    }
  }
  return starts;
}

function characterName(snapshot: WorldSnapshot | null, characterId: string) {
  return snapshot?.characters[characterId]?.name ?? characterId;
}

function sourceName(snapshot: WorldSnapshot | null, sourceId: string) {
  if (!sourceId) return "unknown";
  const service = snapshot?.world.services.find((candidate) => candidate.id === sourceId);
  if (service) return service.name;
  const site = snapshot?.world.activity_sites.find((candidate) => candidate.id === sourceId);
  if (site) return site.name;
  const location = snapshot?.world.locations.find((candidate) => candidate.id === sourceId);
  return location?.name ?? sourceId;
}

function coinText(amount: number) {
  return `${amount} coin${amount === 1 ? "" : "s"}`;
}
