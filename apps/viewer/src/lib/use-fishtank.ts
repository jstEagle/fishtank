"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { apiBaseUrl, getEvents, getSnapshot, liveWebSocketUrl } from "./api";
import type { EventRecord, LiveMessage, ViewerStateSnapshot, WorldSnapshot } from "./protocol";
import { VIEWER_CONFIG } from "./viewer-config";

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

const { fallbackPollMs, livePingMs, liveStaleMs, maxEvents } = VIEWER_CONFIG;

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
    .slice(-maxEvents);
}

function mergeViewerState(current: WorldSnapshot | null, state: ViewerStateSnapshot) {
  if (!current) {
    return current;
  }

  return {
    ...current,
    ...state,
    world: current.world,
    conversations: current.conversations,
    notifications: current.notifications,
    command_log: current.command_log
  };
}

export function useFishtank(): ViewerState {
  const apiUrl = useMemo(() => apiBaseUrl(), []);
  const liveUrl = useMemo(() => liveWebSocketUrl(), []);
  const [snapshot, setSnapshot] = useState<WorldSnapshot | null>(null);
  const [events, setEvents] = useState<EventRecord[]>([]);
  const [lastEventId, setLastEventId] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [connected, setConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const lastEventIdRef = useRef<number | null>(null);
  const fetchingRef = useRef(false);
  const wsRef = useRef<WebSocket | null>(null);
  const fetchedLiveHistoryRef = useRef(false);

  const refresh = useCallback(async () => {
    const controller = new AbortController();
    try {
      const nextSnapshot = await getSnapshot(apiUrl, controller.signal);
      const nextEvents = await getEvents(lastEventIdRef.current ?? undefined, apiUrl, controller.signal, maxEvents);
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
        const nextEvents = await getEvents(lastEventIdRef.current ?? undefined, apiUrl, controller.signal, maxEvents);
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

    const interval = window.setInterval(() => void poll(), fallbackPollMs);
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
    let pingTimer: number | null = null;
    let staleTimer: number | null = null;
    let retry = 0;
    let lastMessageAt = Date.now();

    function clearLiveTimers() {
      if (pingTimer) window.clearInterval(pingTimer);
      if (staleTimer) window.clearInterval(staleTimer);
      pingTimer = null;
      staleTimer = null;
    }

    function connect() {
      if (disposed) return;
      const socket = new WebSocket(liveUrl!);
      wsRef.current = socket;
      socket.addEventListener("open", () => {
        retry = 0;
        setConnected(true);
        setError(null);
        setLoading(false);
        lastMessageAt = Date.now();
        clearLiveTimers();
        pingTimer = window.setInterval(() => {
          if (socket.readyState === WebSocket.OPEN) {
            socket.send("ping");
          }
        }, livePingMs);
        staleTimer = window.setInterval(() => {
          if (Date.now() - lastMessageAt > liveStaleMs) {
            socket.close();
          }
        }, livePingMs);
      });
      socket.addEventListener("message", (event) => {
        lastMessageAt = Date.now();
        try {
          const message = JSON.parse(event.data) as LiveMessage;
          if (message.kind === "snapshot") {
            setSnapshot(message.snapshot);
            lastEventIdRef.current = Math.max(0, message.snapshot.next_event_id - 1);
            setLastEventId(lastEventIdRef.current);
            setConnected(true);
            setLoading(false);
            if (!fetchedLiveHistoryRef.current) {
              fetchedLiveHistoryRef.current = true;
              const after = Math.max(0, message.snapshot.next_event_id - maxEvents - 1);
              void getEvents(after, apiUrl, undefined, maxEvents)
                .then((history) => {
                  if (disposed) return;
                  setEvents((current) => appendEvents(current, history));
                })
                .catch((historyError) => {
                  if (!disposed) {
                    setError(historyError instanceof Error ? historyError.message : String(historyError));
                  }
                });
            }
          } else if (message.kind === "state") {
            setSnapshot((current) => mergeViewerState(current, message.snapshot));
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
          } else if (message.kind === "pong") {
            setConnected(true);
          }
        } catch (parseError) {
          setError(parseError instanceof Error ? parseError.message : String(parseError));
        }
      });
      socket.addEventListener("close", () => {
        clearLiveTimers();
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
      clearLiveTimers();
      wsRef.current?.close();
    };
  }, [apiUrl, liveUrl]);

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
