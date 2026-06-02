import { describe, expect, it, vi } from "vitest";

describe("liveWebSocketUrl", () => {
  it("converts edge https URLs to the singleton live world socket", async () => {
    vi.stubEnv("NEXT_PUBLIC_FISHTANK_EDGE_URL", "https://edge.example.com/");
    vi.resetModules();
    const { liveWebSocketUrl } = await import("./api");
    expect(liveWebSocketUrl()).toBe("wss://edge.example.com/live");
    vi.unstubAllEnvs();
  });
});

describe("apiBaseUrl", () => {
  it("uses an explicit API URL when one is configured", async () => {
    vi.stubEnv("NEXT_PUBLIC_FISHTANK_API_URL", "https://api.example.com/");
    vi.stubEnv("NEXT_PUBLIC_FISHTANK_EDGE_URL", "https://edge.example.com/");
    vi.resetModules();
    const { apiBaseUrl } = await import("./api");
    expect(apiBaseUrl()).toBe("https://api.example.com");
    vi.unstubAllEnvs();
  });

  it("derives the public API base from the edge URL in hosted production", async () => {
    vi.stubEnv("NEXT_PUBLIC_FISHTANK_EDGE_URL", "https://edge.example.com/");
    vi.resetModules();
    const { apiBaseUrl } = await import("./api");
    expect(apiBaseUrl()).toBe("https://edge.example.com/v1");
    vi.unstubAllEnvs();
  });
});

describe("getSnapshot", () => {
  it("requests the compact viewer snapshot", async () => {
    const fetchMock = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify({ tick: 1, next_event_id: 2 }))
    );
    vi.stubGlobal("fetch", fetchMock);
    vi.resetModules();
    const { getSnapshot } = await import("./api");

    await getSnapshot("https://api.example.com/v1");

    expect(fetchMock).toHaveBeenCalledOnce();
    expect(fetchMock.mock.calls[0][0]).toBe("https://api.example.com/v1/snapshot?compact=viewer");
    vi.unstubAllGlobals();
  });
});

describe("getEvents", () => {
  it("passes through after and limit query parameters", async () => {
    const fetchMock = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify([]))
    );
    vi.stubGlobal("fetch", fetchMock);
    vi.resetModules();
    const { getEvents } = await import("./api");

    await getEvents(41, "https://api.example.com/v1", undefined, 80);

    expect(fetchMock).toHaveBeenCalledOnce();
    expect(fetchMock.mock.calls[0][0]).toBe(
      "https://api.example.com/v1/events?compact=viewer&after=41&limit=80"
    );
    vi.unstubAllGlobals();
  });
});
