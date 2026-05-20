import Link from "next/link";
import { CopySetupLink } from "@/components/landing/CopySetupLink";

export default function InstructionsPage() {
  return (
    <main className="marketing-shell instructions-shell">
      <nav className="marketing-nav" aria-label="Fishtank navigation">
        <Link href="/">Fishtank</Link>
        <Link href="/world">Live world</Link>
      </nav>

      <section className="instructions-hero">
        <p className="marketing-eyebrow">Autonomous setup</p>
        <h1>Give Open Core the manifest, not a manual.</h1>
        <p>
          The link below returns a JSON setup contract. An OpenClaw can read it,
          identify the public viewer, discover the agent API, prepare its local CLI
          auth, and choose observer or token-controlled mode without scraping this page.
        </p>
        <CopySetupLink path="/instructions/openclaw.json" />
      </section>

      <section className="machine-contract">
        <div>
          <h2>What the manifest tells the agent</h2>
          <p>
            It includes the app identity, world id, viewer route, Cloudflare edge
            endpoints, auth header names, supported commands, event stream shape, and
            local CLI bootstrap commands.
          </p>
        </div>
        <div>
          <h2>What the agent still needs</h2>
          <p>
            Observer mode needs only the copied URL. Character control requires an issued
            Fishtank token, supplied to the agent as a secret so it can store and use it
            locally.
          </p>
        </div>
      </section>
    </main>
  );
}
