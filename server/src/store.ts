import { EventEmitter } from "node:events";
import type {
  IntentsSnapshot,
  MarketAsset,
  NearChain,
  NetworkSnapshot,
  SourceName,
  SourceStatus,
  StreamTopic,
  ZcashChain,
} from "./types.js";

const labels: Record<SourceName, string> = {
  coingecko: "CoinGecko markets",
  zcash: "Zcash chain",
  near: "NEAR RPC",
  intents: "NEAR Intents 1Click",
  coordinator: "ZEC SSL coordinator",
  operators: "Operator nodes",
  vault: "NEAR vault contract",
};

interface State {
  market: MarketAsset[];
  zcash: ZcashChain | null;
  near: NearChain | null;
  intents: IntentsSnapshot | null;
  network: NetworkSnapshot | null;
}

/** In-memory snapshot of every source, with change notifications for the SSE stream. */
class Store extends EventEmitter {
  readonly state: State = { market: [], zcash: null, near: null, intents: null, network: null };
  private readonly sources = new Map<SourceName, SourceStatus>();

  constructor() {
    super();
    this.setMaxListeners(0);
  }

  set<K extends keyof State>(topic: K, value: State[K]): void {
    this.state[topic] = value;
    this.emit("update", topic satisfies StreamTopic, value);
  }

  declareSource(name: SourceName, configured: boolean): void {
    this.sources.set(name, {
      name,
      label: labels[name],
      configured,
      ok: false,
      lastSuccess: null,
      lastError: configured ? null : "not configured",
      latencyMs: null,
    });
  }

  succeeded(name: SourceName, latencyMs: number | null): void {
    const prev = this.sources.get(name);
    if (!prev) return;
    this.sources.set(name, { ...prev, ok: true, lastSuccess: new Date().toISOString(), lastError: null, latencyMs });
    this.emit("update", "sources", this.sourceList());
  }

  failed(name: SourceName, error: string): void {
    const prev = this.sources.get(name);
    if (!prev) return;
    this.sources.set(name, { ...prev, ok: false, lastError: error });
    this.emit("update", "sources", this.sourceList());
  }

  sourceList(): SourceStatus[] {
    return [...this.sources.values()];
  }
}

export const store = new Store();
