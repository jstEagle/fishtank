"use client";

import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import { getEvents } from "@/lib/api";
import type { EventRecord } from "@/lib/protocol";
import { buildNewsItems } from "@/lib/social-summaries";
import { useFishtank } from "@/lib/use-fishtank";

export default function NewsPage() {
  const { apiUrl, connected, loading, error, snapshot, events } = useFishtank();
  const [history, setHistory] = useState<EventRecord[]>([]);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const newsEvents = useMemo(() => {
    const byId = new Map([...history, ...events].map((event) => [event.id, event]));
    return Array.from(byId.values()).sort((a, b) => a.id - b.id);
  }, [events, history]);
  const news = useMemo(() => buildNewsItems(newsEvents, snapshot), [newsEvents, snapshot]);

  useEffect(() => {
    const controller = new AbortController();
    getEvents(undefined, apiUrl, controller.signal)
      .then((next) => {
        setHistory(next);
        setHistoryError(null);
      })
      .catch((loadError) => {
        if (!controller.signal.aborted) {
          setHistoryError(loadError instanceof Error ? loadError.message : String(loadError));
        }
      });
    return () => controller.abort();
  }, [apiUrl]);

  return (
    <main className="news-page">
      <header className="news-header">
        <div>
          <span className="eyebrow">Fishtank</span>
          <h1>World News</h1>
        </div>
        <div className="news-actions">
          <span className={`chip ${connected ? "live" : loading ? "" : "offline"}`}>
            <span className="dot" />
            {loading ? "connecting" : connected ? `tick ${snapshot?.tick ?? "-"}` : "offline"}
          </span>
          <Link className="chip nav-chip" href="/world">
            World
          </Link>
        </div>
      </header>

      {error || historyError ? <div className="news-error">{error ?? historyError}</div> : null}

      <section className="news-list" aria-label="World news">
        {news.length === 0 ? (
          <div className="news-empty">No notable public events yet.</div>
        ) : (
          news.map((item) => (
            <article key={item.id} className={`news-item news-item-${item.kind}`}>
              <span className="news-tick">t{item.tick}</span>
              <div>
                <h2>{item.title}</h2>
                <p>{item.body}</p>
              </div>
            </article>
          ))
        )}
      </section>
    </main>
  );
}
