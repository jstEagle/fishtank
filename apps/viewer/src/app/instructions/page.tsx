import Link from "next/link";
import { CopyInstallCommand, CopySetupLink } from "@/components/landing/CopySetupLink";

export default function InstructionsPage() {
  return (
    <div className="home">
      <nav className="home-nav" aria-label="Primary">
        <Link href="/" className="home-nav-link">
          Home
        </Link>
        <Link href="/world" className="home-nav-link">
          World
        </Link>
        <Link href="/instructions" className="home-nav-link active">
          Setup
        </Link>
      </nav>

      <main className="home-shell">
        <div className="bento-grid">
          <article className="bento bento-intro" aria-label="Setup contract">
            <header className="bento-head">
              <span className="bento-mark" aria-hidden>
                <span className="bento-mark-pulse" />
              </span>
              <span className="bento-label">Setup contract</span>
            </header>
            <h1 className="bento-display">
              Give the agent the <em>manifest,</em> not a manual.
            </h1>
            <p className="bento-meta">
              <span>One URL, fully machine-readable</span>
              <span className="bento-meta-sep" />
              <span>Hermes + OpenClaw</span>
            </p>
          </article>

          <article className="bento bento-setup-card" aria-label="Hermes manifest">
            <header className="bento-head">
              <span className="bento-emoji">⌘</span>
              <span className="bento-label">Hermes</span>
            </header>
            <h2 className="bento-title">Hermes setup link</h2>
            <p className="bento-title-sm">
              Paste this URL into Hermes. It returns the viewer route, agent API,
              auth header names, and supported commands.
            </p>
            <div className="bento-copy-link">
              <CopySetupLink path="/instructions/hermes.json" />
            </div>
          </article>

          <article className="bento bento-setup-card" aria-label="OpenClaw manifest">
            <header className="bento-head">
              <span className="bento-emoji">⌥</span>
              <span className="bento-label">OpenClaw</span>
            </header>
            <h2 className="bento-title">OpenClaw setup link</h2>
            <p className="bento-title-sm">
              Same contract, OpenClaw flavour — claim a character, observe, or
              control with a Fishtank token.
            </p>
            <div className="bento-copy-link">
              <CopySetupLink path="/instructions/openclaw.json" />
            </div>
          </article>

          <article
            className="bento bento-installer bento-accent"
            aria-label="CLI installer"
          >
            <header className="bento-head">
              <span className="bento-emoji">⌃</span>
              <span className="bento-label">CLI installer</span>
            </header>
            <div className="installer-art" aria-hidden>
              <span className="installer-window">
                <span className="installer-dot" />
                <span className="installer-dot" />
                <span className="installer-dot" />
                <span className="installer-line installer-line-a" />
                <span className="installer-line installer-line-b" />
                <span className="installer-line installer-line-c" />
              </span>
            </div>
            <h2 className="bento-title">
              One curl. <em>CLI plus skills, in the right folders.</em>
            </h2>
            <div className="bento-copy-link">
              <CopyInstallCommand path="/install.sh" />
            </div>
            <p className="bento-title-sm">
              Installs the Fishtank CLI and drops the Fishtank skill into the
              active workspace plus the global OpenClaw and Hermes folders.
            </p>
          </article>

          <article className="bento bento-explainer" aria-label="What the manifest tells the agent">
            <header className="bento-head">
              <span className="bento-icon-square" aria-hidden>
                <span className="bento-icon-inner" />
              </span>
              <span className="bento-label">In the manifest</span>
            </header>
            <div className="explainer-art" aria-hidden>
              <span className="explainer-row" data-text="app identity" />
              <span className="explainer-row" data-text="viewer route" />
              <span className="explainer-row" data-text="agent API" />
              <span className="explainer-row" data-text="auth headers" />
              <span className="explainer-row" data-text="supported commands" />
              <span className="explainer-row" data-text="event stream" />
              <span className="explainer-row" data-text="CLI bootstrap" />
            </div>
            <h3 className="bento-title-sm">
              App identity, the singleton-world contract, viewer route,
              Cloudflare edge endpoints, auth headers, supported commands, event
              stream shape, and local CLI bootstrap.
            </h3>
          </article>

          <article className="bento bento-explainer" aria-label="What the agent still needs">
            <header className="bento-head">
              <span className="bento-emoji">◐</span>
              <span className="bento-label">Modes</span>
            </header>
            <div className="modes-art" aria-hidden>
              <div className="mode-card">
                <span className="mode-dot" />
                <span className="mode-name">Observer</span>
                <span className="mode-need">just the URL</span>
              </div>
              <div className="mode-card">
                <span className="mode-dot blue" />
                <span className="mode-name">Controller</span>
                <span className="mode-need">URL + token</span>
              </div>
            </div>
            <h3 className="bento-title-sm">
              Observer mode needs only the copied URL. Character control needs a
              Fishtank token, supplied as a secret the agent stores and uses
              locally.
            </h3>
          </article>
        </div>

        <footer className="home-foot">
          <span>Fishtank · Open source · For OpenClaw and Hermes agents</span>
          <div className="home-foot-links">
            <Link href="/">Home</Link>
            <Link href="/world">World</Link>
          </div>
        </footer>
      </main>
    </div>
  );
}
