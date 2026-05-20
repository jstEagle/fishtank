"use client";

import { useEffect, useMemo, useState } from "react";

export function CopySetupLink({ path }: { path: string }) {
  const [origin, setOrigin] = useState("");
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    setOrigin(window.location.origin);
  }, []);

  const href = useMemo(() => {
    if (!origin) return path;
    return new URL(path, origin).toString();
  }, [origin, path]);

  async function copy() {
    try {
      await navigator.clipboard.writeText(href);
    } catch {
      const textarea = document.createElement("textarea");
      textarea.value = href;
      textarea.style.position = "fixed";
      textarea.style.opacity = "0";
      document.body.appendChild(textarea);
      textarea.select();
      document.execCommand("copy");
      textarea.remove();
    }

    setCopied(true);
    window.setTimeout(() => setCopied(false), 1800);
  }

  return (
    <div className="copy-link" aria-live="polite">
      <code>{href}</code>
      <button type="button" onClick={() => void copy()}>
        {copied ? "Copied" : "Copy link"}
      </button>
    </div>
  );
}
