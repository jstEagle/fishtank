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
