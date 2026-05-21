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
