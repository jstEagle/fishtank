import { afterEach, describe, expect, it, vi } from "vitest";
import worker, { type Env, mergeCachedSnapshot, snapshotPath, upstreamStreamPath } from "./index";

const env = {
  FISHTANK_CORE_URL: "https://core.example.com",
  FISHTANK_GATEWAY_SECRET: "secret",
  WORLD_ROOM: {} as DurableObjectNamespace
} satisfies Env;

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("edge worker", () => {
  it("returns service health for non-api requests", async () => {
    const response = await worker.fetch(new Request("https://edge.example.com/"), env);
    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toMatchObject({
      ok: true,
      service: "fishtank-edge",
      world_model: "single_shared_world"
    });
  });

  it("proxies compact agent observe through the core gateway", async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ wake_reason: "idle_timeout" })));
    vi.stubGlobal("fetch", fetchMock);

    const response = await worker.fetch(new Request("https://edge.example.com/v1/observe/agent"), env);

    expect(response.status).toBe(200);
    expect(fetchMock).toHaveBeenCalledOnce();
    const [upstream, init] = fetchMock.mock.calls[0] as unknown as [URL, RequestInit];
    expect(upstream.toString()).toBe("https://core.example.com/v1/observe/agent");
    expect(new Headers(init.headers).get("authorization")).toBe("Bearer secret");
    await expect(response.json()).resolves.toMatchObject({ wake_reason: "idle_timeout" });
  });

  it("caches public compact observer snapshots at the edge", async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ tick: 12, next_event_id: 34 })));
    const stored = new Map<string, Response>();
    const cache = {
      match: vi.fn(async (request: Request) => stored.get(request.url)),
      put: vi.fn(async (request: Request, response: Response) => {
        stored.set(request.url, response);
      })
    };
    const waitUntilTasks: Promise<unknown>[] = [];
    const ctx = {
      waitUntil: vi.fn((task: Promise<unknown>) => {
        waitUntilTasks.push(task);
      })
    } as unknown as ExecutionContext;
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("caches", { default: cache });

    const first = await worker.fetch(
      new Request("https://edge.example.com/v1/snapshot?debug=true&compact=viewer"),
      env,
      ctx
    );
    await Promise.all(waitUntilTasks);
    const second = await worker.fetch(new Request("https://edge.example.com/v1/snapshot?compact=viewer"), env, ctx);

    expect(first.status).toBe(200);
    expect(second.status).toBe(200);
    expect(first.headers.get("cache-control")).toBe("public, max-age=5");
    expect(fetchMock).toHaveBeenCalledOnce();
    expect(cache.match).toHaveBeenCalledTimes(2);
    expect(cache.put).toHaveBeenCalledOnce();
    expect(cache.match.mock.calls[0][0].url).toBe("https://edge.example.com/v1/snapshot?compact=viewer");
    expect(cache.match.mock.calls[1][0].url).toBe("https://edge.example.com/v1/snapshot?compact=viewer");
    await expect(second.json()).resolves.toMatchObject({ tick: 12, next_event_id: 34 });
  });

  it("skips the upstream initial stream snapshot only when the edge cache is fresh", () => {
    expect(upstreamStreamPath(41, false)).toBe("/v1/stream?after=41&compact=viewer");
    expect(upstreamStreamPath(41, true)).toBe("/v1/stream?after=41&compact=viewer&snapshot=false");
  });

  it("requests slim state snapshots for warm live refreshes", () => {
    expect(snapshotPath("viewer")).toBe("/v1/snapshot?compact=viewer");
    expect(snapshotPath("viewer_state")).toBe("/v1/snapshot?compact=viewer_state");
  });

  it("merges slim live state without dropping static snapshot fields", () => {
    const snapshot = {
      tick: 1,
      next_event_id: 2,
      world: { id: "village" },
      conversations: { conv_one: {} },
      notifications: { note_one: {} },
      command_log: [{ command_id: "cmd.one" }]
    };
    const state = {
      tick: 3,
      next_event_id: 4,
      characters: { char_one: { id: "char_one" } },
      world: { id: "should-not-replace" }
    };

    expect(mergeCachedSnapshot(snapshot, state)).toMatchObject({
      tick: 3,
      next_event_id: 4,
      world: { id: "village" },
      conversations: { conv_one: {} },
      notifications: { note_one: {} },
      command_log: [{ command_id: "cmd.one" }],
      characters: { char_one: { id: "char_one" } }
    });
  });
});
