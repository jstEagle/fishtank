export interface Env {
  WORLD_ROOM: DurableObjectNamespace;
  FISHTANK_CORE_URL: string;
  FISHTANK_GATEWAY_SECRET: string;
  FISHTANK_WORLD_ID: string;
}

type EventRecord = { id: number; tick: number; [key: string]: unknown };
type WorldSnapshot = { tick: number; next_event_id: number; [key: string]: unknown };

const JSON_HEADERS = {
  "content-type": "application/json; charset=utf-8",
  "access-control-allow-origin": "*",
  "access-control-allow-headers": "authorization,content-type,x-fishtank-agent-token",
  "access-control-allow-methods": "GET,POST,OPTIONS"
};

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    if (request.method === "OPTIONS") {
      return new Response(null, { headers: JSON_HEADERS });
    }

    const url = new URL(request.url);
    const worldId = url.pathname.match(/^\/worlds\/([^/]+)\/live$/)?.[1];
    if (worldId) {
      const room = env.WORLD_ROOM.get(env.WORLD_ROOM.idFromName(worldId));
      return room.fetch(request);
    }

    if (url.pathname.startsWith("/v1/")) {
      return proxyApi(request, env);
    }

    return json({ ok: true, service: "fishtank-edge", world_id: env.FISHTANK_WORLD_ID });
  }
};

export class WorldRoom {
  private env: Env;
  private ctx: DurableObjectState;
  private upstreamAbort: AbortController | null = null;
  private lastEventId = 0;
  private snapshotRefresh: Promise<void> | null = null;

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
    await this.sendSnapshot(server);
    this.ensureUpstream();
    return new Response(null, { status: 101, webSocket: client });
  }

  async webSocketMessage(ws: WebSocket, message: ArrayBuffer | string) {
    if (message === "ping") {
      this.ensureUpstream();
      ws.send(JSON.stringify({ kind: "pong", at: Date.now(), last_event_id: this.lastEventId }));
      void this.broadcastSnapshot();
    }
  }

  async webSocketClose() {
    if (this.ctx.getWebSockets().length === 0) {
      this.upstreamAbort?.abort();
      this.upstreamAbort = null;
    }
  }

  private async sendSnapshot(ws: WebSocket) {
    const snapshot = await this.fetchSnapshot();
    ws.send(JSON.stringify({ kind: "snapshot", world_id: this.env.FISHTANK_WORLD_ID, snapshot }));
  }

  private async fetchSnapshot() {
    const response = await coreFetch(this.env, `/v1/worlds/${this.env.FISHTANK_WORLD_ID}/snapshot`);
    if (!response.ok) {
      throw new Error(`snapshot fetch failed: ${response.status}`);
    }
    return response.json() as Promise<WorldSnapshot>;
  }

  private async broadcastSnapshot() {
    if (this.snapshotRefresh) {
      return this.snapshotRefresh;
    }

    this.snapshotRefresh = (async () => {
      try {
        const snapshot = await this.fetchSnapshot();
        this.lastEventId = Math.max(this.lastEventId, snapshot.next_event_id - 1);
        this.broadcast({ kind: "snapshot", world_id: this.env.FISHTANK_WORLD_ID, snapshot });
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
          `/v1/worlds/${this.env.FISHTANK_WORLD_ID}/stream?after=${this.lastEventId}`,
          { signal: controller.signal }
        );
        await parseSse(response, (event, data, id) => {
          if (id) this.lastEventId = Number(id);
          if (event === "snapshot") {
            this.broadcast({ kind: "snapshot", world_id: this.env.FISHTANK_WORLD_ID, snapshot: JSON.parse(data) });
          } else if (event === "event") {
            const record = JSON.parse(data) as EventRecord;
            this.lastEventId = Math.max(this.lastEventId, record.id);
            this.broadcast({ kind: "events", world_id: this.env.FISHTANK_WORLD_ID, events: [record] });
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
}

async function proxyApi(request: Request, env: Env): Promise<Response> {
  const url = new URL(request.url);
  const upstream = new URL(url.pathname + url.search, env.FISHTANK_CORE_URL);
  const headers = new Headers(request.headers);
  headers.set("authorization", `Bearer ${env.FISHTANK_GATEWAY_SECRET}`);
  const response = await fetch(upstream, {
    method: request.method,
    headers,
    body: request.method === "GET" || request.method === "HEAD" ? undefined : request.body
  });
  return new Response(response.body, {
    status: response.status,
    headers: JSON_HEADERS
  });
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
