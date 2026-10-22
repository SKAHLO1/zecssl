"use client";

import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { publicApiUrl } from "./api";
import type { NetworkSnapshot, Overview, StreamTopic } from "./types";

export type Connection = "connecting" | "live" | "offline";

interface LiveState extends Overview {
  connection: Connection;
  /** false until the API has answered at least once */
  loaded: boolean;
}

const emptyNetwork: NetworkSnapshot = {
  threshold: { t: 2, n: 3 },
  coordinator: { configured: false, url: null, online: false, latencyMs: null },
  operators: [],
  vault: null,
  bonds: null,
  treasury: null,
  orders: [],
  rounds: [],
  events: [],
  vaultToken: null,
  updatedAt: new Date(0).toISOString(),
};

const LiveContext = createContext<LiveState | null>(null);

/**
 * Holds the API snapshot for the whole app: seeded by the server render, then kept
 * current by the API's Server-Sent Events stream. EventSource reconnects on its own.
 */
export function LiveProvider({ initial, children }: { initial: Overview | null; children: ReactNode }) {
  const [state, setState] = useState<LiveState>(() => ({
    market: initial?.market ?? [],
    zcash: initial?.zcash ?? null,
    near: initial?.near ?? null,
    intents: initial?.intents ?? null,
    network: initial?.network ?? emptyNetwork,
    sources: initial?.sources ?? [],
    connection: "connecting",
    loaded: Boolean(initial),
  }));

  useEffect(() => {
    const es = new EventSource(`${publicApiUrl}/v1/stream`);
    const topics: StreamTopic[] = ["market", "zcash", "near", "intents", "network", "sources"];
    const handlers = topics.map((topic) => {
      const handler = (e: MessageEvent<string>) => {
        const value = JSON.parse(e.data);
        setState((s) => ({ ...s, [topic]: value, loaded: true, connection: "live" }));
      };
      es.addEventListener(topic, handler);
      return [topic, handler] as const;
    });
    es.onopen = () => setState((s) => ({ ...s, connection: "live" }));
    es.onerror = () => setState((s) => ({ ...s, connection: es.readyState === EventSource.CLOSED ? "offline" : "connecting" }));
    return () => {
      handlers.forEach(([topic, handler]) => es.removeEventListener(topic, handler));
      es.close();
    };
  }, []);

  return <LiveContext.Provider value={state}>{children}</LiveContext.Provider>;
}

export function useLive(): LiveState {
  const ctx = useContext(LiveContext);
  if (!ctx) throw new Error("useLive must be used inside LiveProvider");
  return ctx;
}

/**
 * Re-renders every `ms` so relative times ("12s ago") stay fresh. Starts at 0 so the
 * server render and hydration agree; relative times fill in right after mount.
 */
export function useNow(ms = 5000): number {
  const [now, setNow] = useState(0);
  useEffect(() => {
    setNow(Date.now());
    const t = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(t);
  }, [ms]);
  return now;
}
