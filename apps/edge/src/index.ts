import { EDGE_CONFIG } from "./config";

export interface Env {
  WORLD_ROOM: DurableObjectNamespace;
  FISHTANK_CORE_URL: string;
  FISHTANK_GATEWAY_SECRET: string;
}

type EventRecord = { id: number; tick: number; [key: string]: unknown };
type WorldSnapshot = { tick: number; next_event_id: number; [key: string]: unknown };
type ViewerStateSnapshot = Pick<WorldSnapshot, "tick" | "next_event_id"> & Record<string, unknown>;

const JSON_HEADERS = {
  "content-type": "application/json; charset=utf-8",
  "access-control-allow-origin": "*",
  "access-control-allow-headers": "authorization,content-type,x-fishtank-agent-token",
  "access-control-allow-methods": "GET,POST,OPTIONS"
};

export default {
  async fetch(request: Request, env: Env, ctx?: ExecutionContext): Promise<Response> {
    if (request.method === "OPTIONS") {
      return new Response(null, { headers: JSON_HEADERS });
    }

    const url = new URL(request.url);
    if (url.pathname === "/live" || /^\/worlds\/[^/]+\/live$/.test(url.pathname)) {
      const room = env.WORLD_ROOM.get(env.WORLD_ROOM.idFromName("singleton"));
      return room.fetch(request);
    }

    if (url.pathname.startsWith("/v1/")) {
      return proxyApi(request, env, ctx);
    }

    return json({ ok: true, service: "fishtank-edge", world_model: "single_shared_world" });
  }
};

export class WorldRoom {
  private env: Env;
  private ctx: DurableObjectState;
  private upstreamAbort: AbortController | null = null;
  private lastEventId = 0;
  private snapshotRefresh: Promise<void> | null = null;
  private lastSnapshotBroadcastAt = 0;
  private cachedSnapshot: WorldSnapshot | null = null;
  private cachedSnapshotAt = 0;

  constructor(ctx: DurableObjectState, env: Env) {
    this.ctx = ctx;
    this.env = env;
  }

  async fetch(request: Request): Promise<Response> {
    if (request.headers.get("upgrade") !== "websocket") {
      return json({ ok: false, error: "websocket required" }, 426);
    }

    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);
    this.ctx.acceptWebSocket(server);
    this.sendCachedSnapshot(server);
    this.ensureUpstream();
    return new Response(null, { status: 101, webSocket: client });
  }

  async webSocketMessage(ws: WebSocket, message: ArrayBuffer | string) {
    if (message === "ping") {
      this.ensureUpstream();
      ws.send(JSON.stringify({ kind: "pong", at: Date.now(), last_event_id: this.lastEventId }));
    }
  }

  async webSocketClose() {
    if (this.ctx.getWebSockets().length === 0) {
      this.upstreamAbort?.abort();
      this.upstreamAbort = null;
    }
  }

  private sendCachedSnapshot(ws: WebSocket) {
    const snapshot = this.cachedSnapshot;
    if (!snapshot || Date.now() - this.cachedSnapshotAt > EDGE_CONFIG.cachedSnapshotMaxAgeMs) {
      return;
    }
    this.lastEventId = Math.max(this.lastEventId, snapshot.next_event_id - 1);
    ws.send(JSON.stringify({ kind: "snapshot", snapshot }));
  }

  private async fetchSnapshot(compact: "viewer" | "viewer_state" = "viewer") {
    const response = await coreFetch(this.env, snapshotPath(compact));
    if (!response.ok) {
      throw new Error(`snapshot fetch failed: ${response.status}`);
    }
    return response.json() as Promise<WorldSnapshot | ViewerStateSnapshot>;
  }

  private async broadcastSnapshot() {
    const now = Date.now();
    if (
      this.ctx.getWebSockets().length === 0 ||
      now - this.lastSnapshotBroadcastAt < EDGE_CONFIG.snapshotBroadcastMinMs
    ) {
      return;
    }

    if (this.snapshotRefresh) {
      return this.snapshotRefresh;
    }

    this.snapshotRefresh = (async () => {
      try {
        if (this.cachedSnapshot) {
          const state = await this.fetchSnapshot("viewer_state");
          this.cachedSnapshot = mergeCachedSnapshot(this.cachedSnapshot, state);
          this.cachedSnapshotAt = Date.now();
          this.lastSnapshotBroadcastAt = Date.now();
          this.lastEventId = Math.max(this.lastEventId, state.next_event_id - 1);
          this.broadcast({ kind: "state", snapshot: state });
          return;
        }
        const snapshot = (await this.fetchSnapshot("viewer")) as WorldSnapshot;
        this.cachedSnapshot = snapshot;
        this.cachedSnapshotAt = Date.now();
        this.lastSnapshotBroadcastAt = Date.now();
        this.lastEventId = Math.max(this.lastEventId, snapshot.next_event_id - 1);
        this.broadcast({ kind: "snapshot", snapshot });
      } catch (error) {
        this.broadcast({ kind: "connection_error", message: String(error) });
      } finally {
        this.snapshotRefresh = null;
      }
    })();

    return this.snapshotRefresh;
  }

  private ensureUpstream() {
    if (this.upstreamAbort || this.ctx.getWebSockets().length === 0) {
      return;
    }
    this.upstreamAbort = new AbortController();
    void this.runUpstream(this.upstreamAbort);
  }

  private async runUpstream(controller: AbortController) {
    try {
      while (!controller.signal.aborted && this.ctx.getWebSockets().length > 0) {
        const response = await coreFetch(
          this.env,
          upstreamStreamPath(this.lastEventId, this.hasFreshCachedSnapshot()),
          { signal: controller.signal }
        );
        await parseSse(response, (event, data, id) => {
          if (id) this.lastEventId = Number(id);
          if (event === "snapshot") {
            const snapshot = JSON.parse(data) as WorldSnapshot;
            this.cachedSnapshot = snapshot;
            this.cachedSnapshotAt = Date.now();
            this.lastEventId = Math.max(this.lastEventId, snapshot.next_event_id - 1);
            this.broadcast({ kind: "snapshot", snapshot });
          } else if (event === "event") {
            const record = JSON.parse(data) as EventRecord;
            this.lastEventId = Math.max(this.lastEventId, record.id);
            this.broadcast({ kind: "events", events: [record] });
            void this.broadcastSnapshot();
          }
        });
      }
    } catch (error) {
      this.broadcast({ kind: "connection_error", message: String(error) });
      await new Promise((resolve) => setTimeout(resolve, 1000));
    } finally {
      this.upstreamAbort = null;
      if (!controller.signal.aborted) this.ensureUpstream();
    }
  }

  private broadcast(payload: unknown) {
    const message = JSON.stringify(payload);
    for (const socket of this.ctx.getWebSockets()) {
      socket.send(message);
    }
  }

  private hasFreshCachedSnapshot() {
    return Boolean(
      this.cachedSnapshot && Date.now() - this.cachedSnapshotAt <= EDGE_CONFIG.cachedSnapshotMaxAgeMs
    );
  }
}

export function upstreamStreamPath(lastEventId: number, skipInitialSnapshot: boolean) {
  const params = new URLSearchParams({
    after: String(lastEventId),
    compact: "viewer"
  });
  if (skipInitialSnapshot) {
    params.set("snapshot", "false");
  }
  return `/v1/stream?${params.toString()}`;
}

export function snapshotPath(compact: "viewer" | "viewer_state") {
  const params = new URLSearchParams({ compact });
  return `/v1/snapshot?${params.toString()}`;
}

export function mergeCachedSnapshot(snapshot: WorldSnapshot, state: ViewerStateSnapshot): WorldSnapshot {
  return {
    ...snapshot,
    ...state,
    world: snapshot.world,
    conversations: snapshot.conversations,
    notifications: snapshot.notifications,
    public_invites: snapshot.public_invites,
    public_notices: snapshot.public_notices,
    external_games: snapshot.external_games,
    command_log: snapshot.command_log
  };
}

async function proxyApi(request: Request, env: Env, ctx?: ExecutionContext): Promise<Response> {
  const url = new URL(request.url);
  const cache = publicCompactCache(request, url);
  if (cache) {
    const hit = await cache.store.match(cache.key);
    if (hit) {
      return hit;
    }
  }

  const upstream = new URL(url.pathname + url.search, env.FISHTANK_CORE_URL);
  const headers = new Headers(request.headers);
  headers.set("authorization", `Bearer ${env.FISHTANK_GATEWAY_SECRET}`);
  const response = await fetch(upstream, {
    method: request.method,
    headers,
    body: request.method === "GET" || request.method === "HEAD" ? undefined : request.body
  });
  const proxied = new Response(response.body, {
    status: response.status,
    headers: cache ? cacheHeaders() : JSON_HEADERS
  });
  if (cache && proxied.ok) {
    ctx?.waitUntil(cache.store.put(cache.key, proxied.clone()));
  }
  return proxied;
}

function publicCompactCache(request: Request, url: URL) {
  if (request.method !== "GET" || typeof caches === "undefined") {
    return null;
  }
  if (request.headers.has("authorization") || request.headers.has("x-fishtank-agent-token")) {
    return null;
  }
  if (url.searchParams.get("compact") !== "viewer") {
    return null;
  }
  if (url.pathname === "/v1/snapshot") {
    const key = new URL(url.origin + url.pathname);
    key.searchParams.set("compact", "viewer");
    return { store: caches.default, key: new Request(key.toString(), { method: "GET" }) };
  }
  if (url.pathname === "/v1/events" && !url.searchParams.has("after")) {
    const key = new URL(url.origin + url.pathname);
    key.searchParams.set("compact", "viewer");
    const limit = url.searchParams.get("limit");
    if (limit) {
      key.searchParams.set("limit", limit);
    }
    return { store: caches.default, key: new Request(key.toString(), { method: "GET" }) };
  }
  return null;
}

function cacheHeaders() {
  return {
    ...JSON_HEADERS,
    "cache-control": `public, max-age=${EDGE_CONFIG.publicCompactCacheTtlSeconds}`
  };
}

async function coreFetch(env: Env, path: string, init: RequestInit = {}) {
  return fetch(new URL(path, env.FISHTANK_CORE_URL), {
    ...init,
    headers: {
      ...(init.headers ?? {}),
      authorization: `Bearer ${env.FISHTANK_GATEWAY_SECRET}`
    }
  });
}

async function parseSse(response: Response, onMessage: (event: string, data: string, id?: string) => void) {
  if (!response.ok || !response.body) {
    throw new Error(`upstream stream failed: ${response.status}`);
  }
  const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
  let buffer = "";
  let event = "message";
  let data = "";
  let id: string | undefined;
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += value;
    const lines = buffer.split(/\r?\n/);
    buffer = lines.pop() ?? "";
    for (const line of lines) {
      if (line === "") {
        if (data) onMessage(event, data.replace(/\n$/, ""), id);
        event = "message";
        data = "";
        id = undefined;
      } else if (line.startsWith("event:")) {
        event = line.slice(6).trim();
      } else if (line.startsWith("data:")) {
        data += `${line.slice(5).trimStart()}\n`;
      } else if (line.startsWith("id:")) {
        id = line.slice(3).trim();
      }
    }
  }
}

function json(value: unknown, status = 200) {
  return new Response(JSON.stringify(value), { status, headers: JSON_HEADERS });
}
