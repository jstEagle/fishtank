import { SCHEMA_VERSION, type Command, type CommandEnvelope, type CommandResponse, type EventRecord, type WorldSnapshot } from "./protocol";

export const DEFAULT_API_URL = "http://127.0.0.1:3838";

export function apiBaseUrl() {
  const configuredApi = process.env.NEXT_PUBLIC_FISHTANK_API_URL?.trim();
  if (configuredApi) {
    return configuredApi.replace(/\/$/, "");
  }

  const edge = edgeBaseUrl();
  if (edge) {
    return `${edge}/v1`;
  }

  return DEFAULT_API_URL;
}

export function edgeBaseUrl() {
  return (process.env.NEXT_PUBLIC_FISHTANK_EDGE_URL ?? "").replace(/\/$/, "");
}

export function liveWebSocketUrl() {
  const edge = edgeBaseUrl();
  if (!edge) return null;
  const url = new URL("/live", edge);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return url.toString();
}

async function getJson<T>(url: string, signal?: AbortSignal): Promise<T> {
  const response = await fetch(url, {
    signal,
    cache: "no-store"
  });

  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }

  return response.json() as Promise<T>;
}

export function getSnapshot(baseUrl = apiBaseUrl(), signal?: AbortSignal) {
  const url = new URL(`${baseUrl}/snapshot`);
  url.searchParams.set("compact", "viewer");
  return getJson<WorldSnapshot>(url.toString(), signal);
}

export function getEvents(after?: number, baseUrl = apiBaseUrl(), signal?: AbortSignal, limit?: number) {
  const url = new URL(`${baseUrl}/events`);
  url.searchParams.set("compact", "viewer");
  if (after != null) {
    url.searchParams.set("after", String(after));
  }
  if (limit != null) {
    url.searchParams.set("limit", String(limit));
  }
  return getJson<EventRecord[]>(url.toString(), signal);
}

export async function sendCommand(
  characterId: string,
  command: Command,
  baseUrl = apiBaseUrl(),
  signal?: AbortSignal
) {
  const envelope: CommandEnvelope = {
    schema_version: SCHEMA_VERSION,
    command_id: `web.${Date.now()}.${Math.random().toString(16).slice(2)}`,
    character_id: characterId,
    submitted_at: new Date().toISOString(),
    based_on_tick: null,
    valid_until_tick: null,
    local_state_hash: null,
    preconditions: [],
    command
  };

  const response = await fetch(`${baseUrl}/command`, {
    method: "POST",
    headers: {
      "content-type": "application/json"
    },
    body: JSON.stringify(envelope),
    signal
  });

  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }

  return response.json() as Promise<CommandResponse>;
}
