import { config } from "../config.js";
import { errorMessage, fetchJson, HttpError } from "../http.js";
import { store } from "../store.js";
import type {
  NetworkEvent,
  NetworkSnapshot,
  Operator,
  Order,
  OrderDetail,
  OrderState,
  Quote,
  SigningRound,
  TreasuryState,
  VaultAccount,
} from "../types.js";
import { fetchAccount, viewCall } from "./near.js";

/*
 * ZEC SSL's own services. Each is optional until deployed: the coordinator (orders,
 * signing rounds, treasury scan), the operator nodes (health), and the NEAR contract
 * (vault, bonds, orders; contracts/ssl). Unconfigured parts are reported as such, never
 * filled in. The contract is the source of truth for bonds and order state; the
 * coordinator adds what only it knows (signing rounds, the shielded side).
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
// The NEAR account each endpoint last reported, so an operator that goes down is still
// matched to its on-chain entry and shown as offline rather than unmonitored.
const lastAccount = new Map<string, string>();

function blankOperator(id: string, name: string, endpoint: string, state: Operator["state"]): Operator {
  return {
    id,
    name,
    endpoint,
    state,
    latencyMs: null,
    version: null,
    nearAccount: null,
    bondUsd: null,
    active: null,
    equivocations: null,
    slashedUsd: null,
    signaturesTotal: null,
    lastSignatureAt: null,
    lastSeen: null,
  };
}

async function pollOperator(op: { id: string; name: string; url: string }): Promise<Operator> {
  try {
    const { data, latencyMs } = await fetchJson<OperatorHealth>(`${op.url}/health`, { timeoutMs: 4_000 });
    const now = new Date().toISOString();
    lastSeen.set(op.id, now);
    if (data.nearAccount) lastAccount.set(op.id, data.nearAccount);
    return {
      ...blankOperator(op.id, op.name, op.url, data.status === "degraded" ? "degraded" : "online"),
      latencyMs,
      version: data.version ?? null,
      nearAccount: data.nearAccount ?? null,
      bondUsd: data.bondUsd ?? null,
      signaturesTotal: data.signaturesTotal ?? null,
      lastSignatureAt: data.lastSignatureAt ?? null,
      lastSeen: now,
    };
  } catch {
    // An endpoint named after its NEAR account (OPERATOR_ENDPOINTS=<account>=<url>) is
    // matched even if it has never answered since this API started.
    const account = lastAccount.get(op.id) ?? (op.name.includes(".") ? op.name : null);
    return { ...blankOperator(op.id, op.name, op.url, "offline"), nearAccount: account, lastSeen: lastSeen.get(op.id) ?? null };
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

// ---------- NEAR contract (contracts/ssl) ----------

interface ContractConfig {
  token: string;
  token_symbol: string;
  token_decimals: number;
  threshold: number;
}

interface ContractOperator {
  account_id: string;
  active: boolean;
  bond: string;
  unbonding: string;
  slashed: string;
  equivocations: number;
}

interface ContractOrder {
  id: string;
  direction: "zec_to_near" | "near_to_zec";
  state: string;
  terms: { recipient: string; amount_out: string; zec_amount_zat: string; deposit_deadline: string };
  approvers: string[];
  release_approvals: string[];
  refund_approvals: string[];
  created_at: string;
  opened_at: string;
  closed_at: string;
  claimed_at: string;
  zcash_txid?: string | null;
}

const orderStates: Record<string, OrderState> = {
  proposed: "quoted",
  open: "awaiting_deposit",
  released: "released",
  refunded: "refunded",
  expired: "expired",
  disputed: "disputed",
  compensated: "compensated",
  locked: "locked",
  claimed: "claimed",
  challenged: "disputed",
  settled: "settled",
  reimbursed: "reimbursed",
};

const nsToIso = (ns: string) => new Date(Number(BigInt(ns) / 1_000_000n)).toISOString();
const units = (raw: string, decimals: number) => Number(raw) / 10 ** decimals;

function mapOrder(o: ContractOrder, cfg: ContractConfig): Order {
  // Show progress toward whichever threshold the order is waiting on.
  const signatures = o.state === "open" ? o.release_approvals.length : o.approvers.length;
  const last = [o.closed_at, o.claimed_at, o.opened_at, o.created_at].find((t) => t !== "0") ?? o.created_at;
  return {
    id: o.id,
    direction: o.direction,
    state: orderStates[o.state] ?? "quoted",
    amountInZec: String(Number(o.terms.zec_amount_zat) / 1e8),
    amountOut: String(units(o.terms.amount_out, cfg.token_decimals)),
    destinationSymbol: cfg.token_symbol,
    signatures,
    threshold: cfg.threshold,
    createdAt: nsToIso(o.created_at),
    updatedAt: nsToIso(last),
    nearTxHash: null,
  };
}

const SLOW_REFRESH_MS = 5 * 60_000;
let slow: { account: VaultAccount; cfg: ContractConfig | null; at: number } | null = null;

async function pollContracts() {
  let vault: VaultAccount | null = null;
  let bonds: VaultAccount | null = null;
  let latencyMs = 0;
  let liquidity: TreasuryState["vaultLiquidity"] = [];
  let totalBondedUsd: number | null = null;
  let reserves: TreasuryState["reserves"] = null;
  let operators: ContractOperator[] = [];
  let orders: Order[] = [];
  let cfg: ContractConfig | null = null;

  if (config.nearVaultContract) {
    // The account and config rarely change; refresh them every few minutes, not every poll.
    if (!slow || Date.now() - slow.at > SLOW_REFRESH_MS) {
      const res = await fetchAccount(config.nearVaultContract);
      const conf = res.account.exists ? await viewCall<ContractConfig>(res.account.contract, "get_config") : null;
      slow = { account: res.account, cfg: conf, at: Date.now() };
      latencyMs = res.latencyMs;
    }
    vault = slow.account;
    cfg = slow.cfg;
    if (vault.exists && cfg) {
      const id = vault.contract;
      const c = cfg;
      const [ops, ords, acct] = await Promise.all([
        viewCall<ContractOperator[]>(id, "get_operators"),
        viewCall<ContractOrder[]>(id, "get_orders", { limit: 50 }),
        viewCall<Record<string, string>>(id, "get_accounting"),
      ]);
      operators = ops ?? [];
      orders = (ords ?? []).map((o) => mapOrder(o, c));
      // Liquidity and total bonds follow from the accounting and the operators, so they
      // need no calls of their own.
      totalBondedUsd = operators.reduce((sum, op) => sum + units(op.bond, c.token_decimals) + units(op.unbonding, c.token_decimals), 0);
      if (acct) {
        liquidity = [{ symbol: c.token_symbol, amount: String(units(acct.available, c.token_decimals)) }];
        // What the token contract says the vault holds, to check the books against.
        const held = await viewCall<string>(c.token, "ft_balance_of", { account_id: id });
        reserves = {
          ...(Object.fromEntries(Object.entries(acct).map(([k, v]) => [k, units(v, c.token_decimals)])) as NonNullable<TreasuryState["reserves"]>),
          held: held == null ? null : units(held, c.token_decimals),
        };
      }
    }
  }
  // Bonds live in the vault contract; a separate bonds contract is only read if configured.
  if (config.nearBondsContract && config.nearBondsContract !== config.nearVaultContract) {
    const res = await fetchAccount(config.nearBondsContract);
    bonds = res.account;
    latencyMs = Math.max(latencyMs, res.latencyMs);
    if (bonds.exists && totalBondedUsd == null) totalBondedUsd = (await viewCall<{ usd: number }>(bonds.contract, "get_total_bonded"))?.usd ?? null;
  } else {
    bonds = vault;
  }
  return { vault, bonds, liquidity, totalBondedUsd, reserves, operators, orders, cfg, latencyMs };
}

/** Contract operators, enriched with health where an endpoint reports the same NEAR account. */
function mergeOperators(health: Operator[], onChain: ContractOperator[], decimals: number): Operator[] {
  if (!onChain.length) return health;
  const used = new Set<string>();
  const merged = onChain.map((c) => {
    const h = health.find((o) => o.nearAccount === c.account_id || o.name === c.account_id || o.id === c.account_id);
    if (h) used.add(h.id);
    const base = h ?? blankOperator(c.account_id, c.account_id, "", "unmonitored");
    return {
      ...base,
      nearAccount: c.account_id,
      bondUsd: units(c.bond, decimals) + units(c.unbonding, decimals),
      active: c.active,
      equivocations: c.equivocations,
      slashedUsd: units(c.slashed, decimals),
    };
  });
  return [...merged, ...health.filter((o) => !used.has(o.id))];
}

export async function pollNetwork(): Promise<NetworkSnapshot> {
  const prev = store.state.network;

  const health = await Promise.all(config.operators.map(pollOperator));
  if (config.operators.length) {
    const up = health.filter((o) => o.state !== "offline");
    if (up.length) store.succeeded("operators", Math.max(...up.map((o) => o.latencyMs ?? 0)));
    else store.failed("operators", "no operator reachable");
  }

  let coordinator: NetworkSnapshot["coordinator"] = {
    configured: Boolean(config.coordinatorUrl),
    url: config.coordinatorUrl || null,
    online: false,
    latencyMs: null,
  };
  let coordinatorData: Awaited<ReturnType<typeof pollCoordinator>> | null = null;
  if (config.coordinatorUrl) {
    try {
      coordinatorData = await pollCoordinator();
      coordinator = { ...coordinator, online: true, latencyMs: coordinatorData.latencyMs };
      store.succeeded("coordinator", coordinatorData.latencyMs);
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

  const decimals = contracts?.cfg?.token_decimals ?? 6;
  const operators = mergeOperators(health, contracts?.operators ?? [], decimals);
  // The coordinator knows more about each order (deposit detection, signing); without it,
  // the contract's orders are still the authoritative state.
  const orders = coordinatorData?.orders ?? contracts?.orders ?? prev?.orders ?? [];
  const scan = coordinatorData?.treasury ?? null;
  const hasTreasury = scan || contracts;

  return {
    threshold: contracts?.cfg ? { t: contracts.cfg.threshold, n: Math.max(contracts.operators.length, contracts.cfg.threshold) } : config.threshold,
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
          reserves: contracts?.reserves ?? null,
        }
      : (prev?.treasury ?? null),
    orders,
    rounds: coordinatorData?.rounds ?? prev?.rounds ?? [],
    events: coordinatorData?.events ?? prev?.events ?? [],
    vaultToken: contracts?.cfg
      ? {
          assetId: `vault:${contracts.cfg.token}`,
          symbol: contracts.cfg.token_symbol,
          blockchain: "near",
          decimals: contracts.cfg.token_decimals,
          // The vault token is a USD stablecoin (test USDC on testnet).
          price: 1,
          contractAddress: contracts.cfg.token,
        }
      : (prev?.vaultToken ?? null),
    updatedAt: new Date().toISOString(),
  };
}

interface CoordinatorQuote {
  amountIn: string;
  amountOut: string;
  zecUsd: number;
  rate: number;
  spreadBps: number;
  confirmations: number;
}

/** ZEC SSL's own quote, from the coordinator, for settlement through the vault. */
export async function coordinatorQuote(amountZec: string): Promise<Quote> {
  const { data: q } = await fetchJson<CoordinatorQuote>(`${config.coordinatorUrl}/v1/quote?amountZec=${encodeURIComponent(amountZec)}`, { timeoutMs: 10_000 });
  const token = store.state.network?.vaultToken;
  if (!token) throw new HttpError(503, "vault token unknown; is NEAR_VAULT_CONTRACT set?");
  const amountIn = Number(q.amountIn);
  const amountInUsd = amountIn * q.zecUsd;
  return {
    source: "coordinator",
    amountIn: q.amountIn,
    amountInUsd,
    amountOut: q.amountOut,
    amountOutUsd: Number(q.amountOut),
    minAmountOut: q.amountOut,
    destination: token,
    rate: q.rate,
    spreadBps: q.spreadBps,
    operatorFee: String(amountIn * (q.spreadBps / 10_000)),
    // Direction 1 deposits pay the user's own wallet fee; the treasury pays none.
    zcashNetworkFeeZec: "0",
    timeEstimateSec: null,
    quotedAt: new Date().toISOString(),
  };
}

/** The coordinator's view of an order, plus its on-chain record and event history. */
export async function orderDetail(id: string): Promise<OrderDetail> {
  const base = `${config.coordinatorUrl}/v1/orders/${encodeURIComponent(id)}`;
  const [{ data }, events, onChain] = await Promise.all([
    fetchJson<Omit<OrderDetail, "vault" | "events">>(base, { timeoutMs: 10_000 }),
    fetchJson<NetworkEvent[]>(`${base}/events`, { timeoutMs: 10_000 }).then((r) => r.data).catch(() => []),
    config.nearVaultContract
      ? viewCall<ContractOrder & { release_deadline: string }>(config.nearVaultContract, "get_order", { order_id: id }).catch(() => null)
      : Promise.resolve(null),
  ]);
  return {
    ...data,
    events,
    vault: onChain
      ? {
          contract: config.nearVaultContract,
          state: onChain.state,
          approvers: onChain.approvers,
          releaseApprovals: onChain.release_approvals,
          refundApprovals: onChain.refund_approvals,
          releaseDeadline: onChain.release_deadline !== "0" ? nsToIso(onChain.release_deadline) : null,
          claimedTxid: onChain.zcash_txid ?? null,
          claimedAt: onChain.claimed_at && onChain.claimed_at !== "0" ? nsToIso(onChain.claimed_at) : null,
        }
      : null,
  };
}

/** Asks the coordinator for a direction-2 quote and the transfer that locks the tokens. */
export async function createPayout(body: unknown) {
  const { data } = await fetchJson<unknown>(`${config.coordinatorUrl}/v1/payouts`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    timeoutMs: 15_000,
  });
  return data;
}

/** Forwards an order request to the coordinator, which owns quoting and deposit addresses. */
export async function createOrder(body: unknown) {
  const { data } = await fetchJson<unknown>(`${config.coordinatorUrl}/v1/orders`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    // Opening waits for operators to approve on NEAR (up to 30s for a hung operator).
    timeoutMs: 45_000,
  });
  return data;
}
