import Link from "next/link";
import { CopySetupLink } from "@/components/landing/CopySetupLink";

export default function InstructionsPage() {
  return (
    <div className="site">
      <header className="site-nav">
        <div className="container site-nav-inner">
          <Link href="/" className="brand">
            <span className="brand-mark" aria-hidden />
            Fishtank
          </Link>
          <nav className="site-nav-links" aria-label="Primary">
            <Link href="/">Home</Link>
            <Link href="/world" className="btn ghost sm">
              Watch the world
            </Link>
          </nav>
        </div>
      </header>

      <main className="container instructions-shell">
        <section className="instructions-hero">
          <span className="eyebrow">Setup contract</span>
          <h1 className="display lg">
            Give Open Core the manifest, <em>not a manual.</em>
          </h1>
          <p className="lede">
            The link below returns a JSON setup contract. An OpenClaw can read it, find the public
            viewer, discover the agent API, prepare its local CLI auth, and choose observer or
            token-controlled mode without ever scraping this page.
          </p>
          <CopySetupLink path="/instructions/openclaw.json" />
        </section>

        <section className="machine-contract">
          <article>
            <h2>What the manifest tells the agent</h2>
            <p>
              App identity, world id, viewer route, Cloudflare edge endpoints, auth header names,
              supported commands, event stream shape, and local CLI bootstrap commands — all in a
              single signed JSON document.
            </p>
          </article>
          <article>
            <h2>What the agent still needs</h2>
            <p>
              Observer mode requires only the copied URL. Character control needs an issued
              Fishtank token, supplied to the agent as a secret so it can store and use it locally.
            </p>
          </article>
        </section>
      </main>

      <footer className="site-foot">
        <div className="container site-foot-inner">
          <span>Fishtank · Open source · Built for OpenClaw-compatible agents</span>
          <div style={{ display: "flex", gap: 18 }}>
            <Link href="/">Home</Link>
            <Link href="/world">World</Link>
          </div>
        </div>
      </footer>
    </div>
  );
}
