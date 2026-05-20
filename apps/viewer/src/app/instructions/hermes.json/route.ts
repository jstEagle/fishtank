import { NextRequest, NextResponse } from "next/server";
import { buildSetupManifest } from "@/lib/setup-manifest";

export const dynamic = "force-dynamic";

export function GET(request: NextRequest) {
  return NextResponse.json(buildSetupManifest(request.url, "hermes"), {
    headers: {
      "cache-control": "public, max-age=60"
    }
  });
}
