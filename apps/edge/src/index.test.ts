import { afterEach, describe, expect, it, vi } from "vitest";
import worker, { type Env } from "./index";

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
});
