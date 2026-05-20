import { describe, expect, it } from "vitest";
import worker, { type Env } from "./index";

describe("edge worker", () => {
  it("returns service health for non-api requests", async () => {
    const env = {
      FISHTANK_CORE_URL: "https://core.example.com",
      FISHTANK_GATEWAY_SECRET: "secret",
      FISHTANK_WORLD_ID: "village",
      WORLD_ROOM: {} as DurableObjectNamespace
    } satisfies Env;
    const response = await worker.fetch(new Request("https://edge.example.com/"), env);
    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toMatchObject({
      ok: true,
      service: "fishtank-edge",
      world_id: "village"
    });
  });
});
