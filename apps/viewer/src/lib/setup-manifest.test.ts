import { describe, expect, it, vi } from "vitest";
import { buildSetupManifest } from "./setup-manifest";

describe("buildSetupManifest", () => {
  it("advertises the continuous agent runtime without world selection", () => {
    vi.stubEnv("NEXT_PUBLIC_FISHTANK_EDGE_URL", "https://edge.example.com");

    const manifest = buildSetupManifest("https://viewer.example.com/instructions/openclaw.json", "openclaw");

    expect(manifest.product.world_model).toContain("one continuous shared world");
    expect(manifest.agent_runtime).toMatchObject({
      recommended_wake_interval_ms: 300000,
      max_actions_per_wake: 3,
      background_loop_rule: expect.stringContaining("background agent"),
      update_check: {
        routine_surface: "fishtank life wake",
        explicit_check_command: "fishtank update check",
        install_command: "fishtank update install"
      },
      event_stream_character_scoped: false
    });
    expect(manifest.api.character_agent_observe).toBe("https://edge.example.com/v1/observe/agent");
    expect(JSON.stringify(manifest)).not.toContain("/worlds/");

    vi.unstubAllEnvs();
  });
});
