import { describe, expect, it, vi } from "vitest";

describe("liveWebSocketUrl", () => {
  it("converts edge https URLs to wss world room URLs", async () => {
    vi.stubEnv("NEXT_PUBLIC_FISHTANK_EDGE_URL", "https://edge.example.com/");
    vi.resetModules();
    const { liveWebSocketUrl } = await import("./api");
    expect(liveWebSocketUrl("village")).toBe("wss://edge.example.com/worlds/village/live");
    vi.unstubAllEnvs();
  });
});
