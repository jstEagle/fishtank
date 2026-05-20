"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { DEFAULT_WORLD_ID, apiBaseUrl, getEvents, getSnapshot, liveWebSocketUrl } from "./api";
import type { EventRecord, LiveMessage, WorldSnapshot } from "./protocol";

export interface ViewerState {
  apiUrl: string;
  liveUrl: string | null;
  connected: boolean;
  realtime: boolean;
  loading: boolean;
  error: string | null;
  snapshot: WorldSnapshot | null;
  events: EventRecord[];
  lastEventId: number | null;
  refresh: () => Promise<void>;
}

const POLL_MS = 500;
const MAX_EVENTS = 80;

function appendEvents(current: EventRecord[], nextEvents: EventRecord[]) {
  if (nextEvents.length === 0) {
    return current;
  }

  const byId = new Map(current.map((event) => [event.id, event]));
  for (const event of nextEvents) {
    byId.set(event.id, event);
  }

  return Array.from(byId.values())
    .sort((a, b) => a.id - b.id)
    .slice(-MAX_EVENTS);
}

export function useFishtank(): ViewerState {
  const apiUrl = useMemo(() => apiBaseUrl(), []);
  const liveUrl = useMemo(() => liveWebSocketUrl(DEFAULT_WORLD_ID), []);
  const [snapshot, setSnapshot] = useState<WorldSnapshot | null>(null);
  const [events, setEvents] = useState<EventRecord[]>([]);
  const [lastEventId, setLastEventId] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [connected, setConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const lastEventIdRef = useRef<number | null>(null);
  const fetchingRef = useRef(false);
  const wsRef = useRef<WebSocket | null>(null);

  const refresh = useCallback(async () => {
    const controller = new AbortController();
    try {
      const nextSnapshot = await getSnapshot(apiUrl, controller.signal);
      const nextEvents = await getEvents(lastEventIdRef.current ?? undefined, apiUrl, controller.signal);
      setSnapshot(nextSnapshot);
      setEvents((current) => appendEvents(current, nextEvents));
      setConnected(true);
      setError(null);
      const fetchedLastEvent = nextEvents[nextEvents.length - 1]?.id ?? 0;
      const snapshotLastEvent = Math.max(0, nextSnapshot.next_event_id - 1);
      const nextLastEvent = Math.max(fetchedLastEvent, snapshotLastEvent);
      lastEventIdRef.current = nextLastEvent;
      setLastEventId(nextLastEvent);
    } catch (refreshError) {
      setConnected(false);
      setError(refreshError instanceof Error ? refreshError.message : String(refreshError));
    } finally {
      setLoading(false);
    }
  }, [apiUrl]);

  useEffect(() => {
    if (liveUrl) return;
    void refresh();
  }, [liveUrl, refresh]);

  useEffect(() => {
    if (liveUrl) return;
    let disposed = false;

    async function poll() {
      if (fetchingRef.current) {
        return;
      }

      fetchingRef.current = true;
      const controller = new AbortController();
      try {
        const nextEvents = await getEvents(lastEventIdRef.current ?? undefined, apiUrl, controller.signal);
        if (disposed) {
          return;
        }

        if (nextEvents.length > 0) {
          setEvents((current) => appendEvents(current, nextEvents));
          const nextLastEvent = nextEvents[nextEvents.length - 1]?.id ?? lastEventIdRef.current;
          lastEventIdRef.current = nextLastEvent;
          setLastEventId(nextLastEvent);
          setSnapshot(await getSnapshot(apiUrl, controller.signal));
        }

        setConnected(true);
        setError(null);
      } catch (pollError) {
        if (!disposed) {
          setConnected(false);
          setError(pollError instanceof Error ? pollError.message : String(pollError));
        }
      } finally {
        fetchingRef.current = false;
        if (!disposed) {
          setLoading(false);
        }
      }
    }

    const interval = window.setInterval(() => void poll(), POLL_MS);
    void poll();

    return () => {
      disposed = true;
      window.clearInterval(interval);
    };
  }, [apiUrl, liveUrl]);

  useEffect(() => {
    if (!liveUrl) return;
    let disposed = false;
    let reconnectTimer: number | null = null;
    let retry = 0;

    function connect() {
      if (disposed) return;
      const socket = new WebSocket(liveUrl!);
      wsRef.current = socket;
      socket.addEventListener("open", () => {
        retry = 0;
        setConnected(true);
        setError(null);
        setLoading(false);
      });
      socket.addEventListener("message", (event) => {
        try {
          const message = JSON.parse(event.data) as LiveMessage;
          if (message.kind === "snapshot") {
            setSnapshot(message.snapshot);
            lastEventIdRef.current = Math.max(0, message.snapshot.next_event_id - 1);
            setLastEventId(lastEventIdRef.current);
            setConnected(true);
            setLoading(false);
          } else if (message.kind === "events") {
            setEvents((current) => appendEvents(current, message.events));
            const last = message.events[message.events.length - 1]?.id;
            if (last != null) {
              lastEventIdRef.current = last;
              setLastEventId(last);
            }
          } else if (message.kind === "connection_error") {
            setError(message.message);
          }
        } catch (parseError) {
          setError(parseError instanceof Error ? parseError.message : String(parseError));
        }
      });
      socket.addEventListener("close", () => {
        if (disposed) return;
        setConnected(false);
        const delay = Math.min(10_000, 500 * 2 ** retry++);
        reconnectTimer = window.setTimeout(connect, delay);
      });
      socket.addEventListener("error", () => {
        setConnected(false);
        setError("live connection failed");
      });
    }

    connect();
    return () => {
      disposed = true;
      if (reconnectTimer) window.clearTimeout(reconnectTimer);
      wsRef.current?.close();
    };
  }, [liveUrl]);

  return {
    apiUrl,
    liveUrl,
    connected,
    realtime: Boolean(liveUrl),
    loading,
    error,
    snapshot,
    events,
    lastEventId,
    refresh
  };
}
