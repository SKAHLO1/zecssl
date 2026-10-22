import { config } from "../config.js";
import { errorMessage, fetchJson } from "../http.js";
import { store } from "../store.js";
import type {
  NetworkEvent,
  NetworkSnapshot,
  Operator,
  Order,
  SigningRound,
  TreasuryState,
  VaultAccount,
} from "../types.js";
import { fetchAccount, viewCall } from "./near.js";

/*
 * ZEC SSL's own services. Each is optional until deployed: the coordinator (orders,
 * signing rounds, treasury scan), the operator nodes (health), and the NEAR vault and
 * bond contracts. Unconfigured parts are reported as such, never filled in.
 */

interface OperatorHealth {
  status?: "ok" | "degraded";
  version?: string;
  nearAccount?: string;
  bondUsd?: number;
  signaturesTotal?: number;
  lastSignatureAt?: string;
}

const lastSeen = new Map<string, string>();

async function pollOperator(op: { id: string; name: string; url: string }): Promise<Operator> {
  try {
    const { data, latencyMs } = await fetchJson<OperatorHealth>(`${op.url}/health`, { timeoutMs: 4_000 });
    const now = new Date().toISOString();
    lastSeen.set(op.id, now);
    return {
      id: op.id,
      name: op.name,
      endpoint: op.url,
      state: data.status === "degraded" ? "degraded" : "online",
      latencyMs,
      version: data.version ?? null,
      nearAccount: data.nearAccount ?? null,
      bondUsd: data.bondUsd ?? null,
      signaturesTotal: data.signaturesTotal ?? null,
      lastSignatureAt: data.lastSignatureAt ?? null,
      lastSeen: now,
    };
  } catch {
    return {
      id: op.id,
      name: op.name,
      endpoint: op.url,
      state: "offline",
      latencyMs: null,
      version: null,
      nearAccount: null,
      bondUsd: null,
      signaturesTotal: null,
      lastSignatureAt: null,
      lastSeen: lastSeen.get(op.id) ?? null,
    };
  }
}

async function pollCoordinator() {
  const base = config.coordinatorUrl;
  const started = performance.now();
  const [orders, rounds, events, treasury] = await Promise.all([
    fetchJson<Order[]>(`${base}/v1/orders?limit=50`, { timeoutMs: 5_000 }),
    fetchJson<SigningRound[]>(`${base}/v1/signing-rounds?limit=50`, { timeoutMs: 5_000 }),
    fetchJson<NetworkEvent[]>(`${base}/v1/events?limit=50`, { timeoutMs: 5_000 }),
    fetchJson<Pick<TreasuryState, "shieldedBalanceZat" | "addressesIssued" | "lastScanHeight">>(`${base}/v1/treasury`, {
      timeoutMs: 5_000,
    }),
  ]);
  return {
    orders: orders.data,
    rounds: rounds.data,
    events: events.data,
    treasury: treasury.data,
    latencyMs: Math.round(performance.now() - started),
  };
}

async function pollContracts() {
  let vault: VaultAccount | null = null;
  let bonds: VaultAccount | null = null;
  let liquidity: TreasuryState["vaultLiquidity"] = [];
  let totalBondedUsd: number | null = null;
  let latencyMs = 0;

  if (config.nearVaultContract) {
    const res = await fetchAccount(config.nearVaultContract);
    vault = res.account;
    latencyMs = res.latencyMs;
    if (vault.exists) liquidity = (await viewCall<TreasuryState["vaultLiquidity"]>(vault.contract, "get_liquidity")) ?? [];
  }
  if (config.nearBondsContract) {
    const res = await fetchAccount(config.nearBondsContract);
    bonds = res.account;
    latencyMs = Math.max(latencyMs, res.latencyMs);
    if (bonds.exists) totalBondedUsd = (await viewCall<{ usd: number }>(bonds.contract, "get_total_bonded"))?.usd ?? null;
  }
  return { vault, bonds, liquidity, totalBondedUsd, latencyMs };
}

export async function pollNetwork(): Promise<NetworkSnapshot> {
  const prev = store.state.network;

  const operators = await Promise.all(config.operators.map(pollOperator));
  if (config.operators.length) {
    const up = operators.filter((o) => o.state !== "offline");
    if (up.length) store.succeeded("operators", Math.max(...up.map((o) => o.latencyMs ?? 0)));
    else store.failed("operators", "no operator reachable");
  }

  let coordinator: NetworkSnapshot["coordinator"] = {
    configured: Boolean(config.coordinatorUrl),
    url: config.coordinatorUrl || null,
    online: false,
    latencyMs: null,
  };
  let orders = prev?.orders ?? [];
  let rounds = prev?.rounds ?? [];
  let events = prev?.events ?? [];
  let scan: Pick<TreasuryState, "shieldedBalanceZat" | "addressesIssued" | "lastScanHeight"> | null = null;
  if (config.coordinatorUrl) {
    try {
      const res = await pollCoordinator();
      ({ orders, rounds, events } = res);
      scan = res.treasury;
      coordinator = { ...coordinator, online: true, latencyMs: res.latencyMs };
      store.succeeded("coordinator", res.latencyMs);
    } catch (err) {
      store.failed("coordinator", errorMessage(err));
    }
  }

  let contracts: Awaited<ReturnType<typeof pollContracts>> | null = null;
  if (config.nearVaultContract || config.nearBondsContract) {
    try {
      contracts = await pollContracts();
      store.succeeded("vault", contracts.latencyMs);
    } catch (err) {
      store.failed("vault", errorMessage(err));
    }
  }

  const hasTreasury = scan || contracts;
  return {
    threshold: config.threshold,
    coordinator,
    operators,
    vault: contracts?.vault ?? prev?.vault ?? null,
    bonds: contracts?.bonds ?? prev?.bonds ?? null,
    treasury: hasTreasury
      ? {
          shieldedBalanceZat: scan?.shieldedBalanceZat ?? null,
          addressesIssued: scan?.addressesIssued ?? null,
          lastScanHeight: scan?.lastScanHeight ?? null,
          vaultLiquidity: contracts?.liquidity ?? [],
          totalBondedUsd: contracts?.totalBondedUsd ?? null,
        }
      : (prev?.treasury ?? null),
    orders,
    rounds,
    events,
    updatedAt: new Date().toISOString(),
  };
}

/** Forwards an order request to the coordinator, which owns quoting and deposit addresses. */
export async function createOrder(body: unknown) {
  const { data } = await fetchJson<unknown>(`${config.coordinatorUrl}/v1/orders`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    timeoutMs: 15_000,
  });
  return data;
}
