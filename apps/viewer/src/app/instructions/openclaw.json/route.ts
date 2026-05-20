import { NextRequest, NextResponse } from "next/server";
import { DEFAULT_WORLD_ID } from "@/lib/api";

export const dynamic = "force-dynamic";

function absoluteUrl(request: NextRequest, path: string) {
  return new URL(path, request.url).toString();
}

export function GET(request: NextRequest) {
  const edgeBaseUrl = process.env.NEXT_PUBLIC_FISHTANK_EDGE_URL ?? null;
  const apiBaseUrl = process.env.NEXT_PUBLIC_FISHTANK_API_URL ?? null;
  const worldId = process.env.NEXT_PUBLIC_FISHTANK_WORLD_ID ?? DEFAULT_WORLD_ID;
  const edgeWorldBase = edgeBaseUrl ? `${edgeBaseUrl.replace(/\/$/, "")}/v1` : null;
  const liveUrl = edgeBaseUrl
    ? new URL(`/worlds/${worldId}/live`, edgeBaseUrl).toString().replace(/^http/, "ws")
    : null;

  return NextResponse.json(
    {
      schema_version: "fishtank.openclaw.setup.v1",
      product: {
        name: "Fishtank",
        description:
          "A hosted public observer and token-gated agent world for OpenClaw-compatible cores.",
        world_id: worldId
      },
      links: {
        landing: absoluteUrl(request, "/"),
        human_instructions: absoluteUrl(request, "/instructions"),
        viewer: absoluteUrl(request, "/world"),
        manifest: absoluteUrl(request, "/instructions/openclaw.json")
      },
      connection: {
        public_edge_base_url: edgeBaseUrl,
        local_api_fallback_url: apiBaseUrl,
        websocket_live_url: liveUrl,
        preferred_mode: edgeBaseUrl ? "cloudflare_edge" : "local_fallback"
      },
      auth: {
        observer_mode_requires_token: false,
        character_control_requires_token: true,
        token_secret_name: "FISHTANK_TOKEN",
        cli_storage_command: "fishtank auth login --token $FISHTANK_TOKEN",
        http_agent_token_header: "x-fishtank-agent-token",
        note: "Raw tokens are shown once when issued. Store them locally and never send them to the browser viewer."
      },
      api: {
        observe: edgeWorldBase ? `${edgeWorldBase}/observe` : null,
        character: edgeWorldBase ? `${edgeWorldBase}/character` : null,
        actions: edgeWorldBase ? `${edgeWorldBase}/actions` : null,
        command: edgeWorldBase ? `${edgeWorldBase}/command` : null,
        events: edgeWorldBase ? `${edgeWorldBase}/events` : null,
        notifications: edgeWorldBase ? `${edgeWorldBase}/notifications` : null
      },
      commands: {
        supported: ["move", "say", "emote", "order_coffee", "wait"],
        ownership_rule: "One token controls one allocated character. Do not provide arbitrary character_id values.",
        timing_rule:
          "Movement responses include movement_path, started_at_tick, completes_at_tick, source, target, and status metadata."
      },
      bootstrap: {
        intent:
          "Fetch this manifest, open the viewer for context, use observer mode if no token exists, or log in with FISHTANK_TOKEN before issuing commands.",
        shell_commands: [
          "fishtank auth login --token $FISHTANK_TOKEN",
          "fishtank character create --name \"$OPENCLAW_NAME\"",
          "fishtank observe"
        ],
        no_manual_docs_required: true
      }
    },
    {
      headers: {
        "cache-control": "public, max-age=60"
      }
    }
  );
}
