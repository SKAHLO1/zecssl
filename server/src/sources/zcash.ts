import { config } from "../config.js";
import { errorMessage, fetchJson, HttpError } from "../http.js";
import type { ZcashBlock, ZcashChain, ZcashTx, ZcashTxKind, ZcashValuePool } from "../types.js";

/*
 * Zcash chain data, from the best provider available:
 *   1. Zebra JSON-RPC (ZEBRA_RPC_URL): the operators' own node; includes value pools.
 *   2. Blockchair: public stats plus a transaction sample we can classify by its
 *      transparent footprint. Its keyless quota is small, so it is polled slowly and
 *      paused for 30 minutes whenever it rate-limits us.
 *   3. 3xpl sandbox: chain head and recent blocks, used when the others are unavailable.
 */


const BLOCK_WINDOW = 8;

// ---------- Zebra ----------

async function zebra<T>(method: string, params: unknown[] = []) {
  const headers: Record<string, string> = { "content-type": "application/json" };
  if (config.zebraRpcAuth) headers.authorization = `Basic ${Buffer.from(config.zebraRpcAuth).toString("base64")}`;
  const { data, latencyMs } = await fetchJson<{ result: T; error: { message: string } | null }>(config.zebraRpcUrl, {
    method: "POST",
    headers,
    body: JSON.stringify({ jsonrpc: "2.0", id: "zec-ssl", method, params }),
  });
  if (data.error) throw new HttpError(502, `zebra ${method}: ${data.error.message}`);
  return { result: data.result, latencyMs };
}

interface ZebraInfo {
  blocks: number;
  bestblockhash: string;
  valuePools?: { id: string; chainValue: number }[];
}

interface ZebraBlock {
  hash: string;
  height: number;
  time: number;
  size?: number;
  tx: string[];
}

const zebraBlocks = new Map<number, ZcashBlock>();

async function fromZebra(): Promise<{ chain: ZcashChain; latencyMs: number }> {
  const { result: info, latencyMs } = await zebra<ZebraInfo>("getblockchaininfo");
  const mempool = await zebra<{ size: number }>("getmempoolinfo").catch(() => null);

  const heights = Array.from({ length: BLOCK_WINDOW }, (_, i) => info.blocks - i).filter((h) => h >= 0);
  await Promise.all(
    heights
      .filter((h) => !zebraBlocks.has(h))
      .map(async (h) => {
        const { result: b } = await zebra<ZebraBlock>("getblock", [String(h), 1]);
        zebraBlocks.set(h, {
          height: b.height,
          hash: b.hash,
          time: new Date(b.time * 1000).toISOString(),
          txCount: b.tx.length,
          size: b.size ?? 0,
        });
      }),
  );
  for (const h of zebraBlocks.keys()) if (h < info.blocks - BLOCK_WINDOW) zebraBlocks.delete(h);

  const recentBlocks = heights.map((h) => zebraBlocks.get(h)).filter((b): b is ZcashBlock => Boolean(b));
  const valuePools: ZcashValuePool[] | null = info.valuePools
    ? info.valuePools.map((p) => ({ id: p.id, valueZec: p.chainValue }))
    : null;

  return {
    latencyMs,
    chain: {
      source: "zebra",
      height: info.blocks,
      bestBlockHash: info.bestblockhash,
      bestBlockTime: recentBlocks[0]?.time ?? new Date().toISOString(),
      blocks24h: null,
      txs24h: null,
      avgBlockTimeSec: avgSpacing(recentBlocks),
      mempoolTxs: mempool?.result.size ?? null,
      hashrate: null,
      nodes: null,
      avgFeeUsd: null,
      blockchainSizeBytes: null,
      valuePools,
      recentBlocks,
      recentTxs: [],
      txMix: null,
      updatedAt: new Date().toISOString(),
    },
  };
}

function avgSpacing(blocks: ZcashBlock[]): number | null {
  if (blocks.length < 2) return null;
  const first = Date.parse(blocks[blocks.length - 1].time);
  const last = Date.parse(blocks[0].time);
  return Math.round((last - first) / 1000 / (blocks.length - 1));
}

// ---------- Blockchair ----------

const BC = "https://api.blockchair.com/zcash";
const BC_STATS_EVERY = 180_000;
const BC_RECENT_EVERY = 300_000;
const BC_PAUSE = 30 * 60_000;

interface BcStats {
  best_block_height: number;
  best_block_hash: string;
  best_block_time: string;
  blocks_24h: number;
  transactions_24h: number;
  mempool_transactions: number;
  hashrate_24h: string;
  nodes: number;
  average_transaction_fee_usd_24h: number;
  median_transaction_fee_usd_24h: number;
  blockchain_size: number;
}

interface BcBlock {
  id: number;
  hash: string;
  time: string;
  transaction_count: number;
  size: number;
}

interface BcTx {
  block_id: number;
  hash: string;
  time: string;
  version: number;
  is_coinbase: boolean;
  input_count: number;
  output_count: number;
  output_total: number;
  fee: number;
}

interface BcEnvelope<T> {
  data: T | null;
  context: { code: number; error?: string };
}

const bc = {
  stats: null as { value: BcStats; at: number } | null,
  recent: null as { blocks: ZcashBlock[]; txs: ZcashTx[]; at: number } | null,
  pausedUntil: 0,
};

// Blockchair reports UTC times without a zone marker.
const utc = (t: string) => `${t.replace(" ", "T")}Z`;

async function bcGet<T>(path: string) {
  const url = `${BC}${path}${config.blockchairKey ? `${path.includes("?") ? "&" : "?"}key=${config.blockchairKey}` : ""}`;
  try {
    const { data, latencyMs } = await fetchJson<BcEnvelope<T>>(url);
    if (!data.data) throw new HttpError(data.context.code, data.context.error ?? "empty response");
    return { value: data.data, latencyMs };
  } catch (err) {
    if (err instanceof HttpError && [402, 429, 430, 435].includes(err.status)) bc.pausedUntil = Date.now() + BC_PAUSE;
    throw err;
  }
}

/**
 * Classifies a transaction by its transparent footprint. With no transparent inputs or
 * outputs it is fully shielded; transparent in with nothing transparent out is shielding;
 * the reverse is deshielding, which is the public hop ZEC SSL removes.
 */
function kindOf(tx: BcTx): ZcashTxKind {
  if (tx.is_coinbase) return "coinbase";
  if (tx.input_count === 0 && tx.output_count === 0) return "shielded";
  if (tx.input_count > 0 && tx.output_count === 0) return "shielding";
  if (tx.input_count === 0 && tx.output_count > 0) return "deshielding";
  return "transparent";
}

async function refreshBlockchair(): Promise<number | null> {
  if (Date.now() < bc.pausedUntil) return null;
  let latency: number | null = null;
  if (!bc.stats || Date.now() - bc.stats.at > BC_STATS_EVERY) {
    const { value, latencyMs } = await bcGet<BcStats>("/stats");
    bc.stats = { value, at: Date.now() };
    latency = latencyMs;
  }
  if (!bc.recent || Date.now() - bc.recent.at > BC_RECENT_EVERY) {
    const blocks = await bcGet<BcBlock[]>(`/blocks?limit=${BLOCK_WINDOW}`);
    const txs = await bcGet<BcTx[]>("/transactions?limit=50");
    bc.recent = {
      at: Date.now(),
      blocks: blocks.value.map((b) => ({ height: b.id, hash: b.hash, time: utc(b.time), txCount: b.transaction_count, size: b.size })),
      txs: txs.value.map((t) => ({
        hash: t.hash,
        height: t.block_id,
        time: utc(t.time),
        version: t.version,
        kind: kindOf(t),
        inputCount: t.input_count,
        outputCount: t.output_count,
        transparentOutZat: t.output_total,
        feeZat: t.fee,
      })),
    };
    latency = Math.max(blocks.latencyMs, txs.latencyMs);
  }
  return latency;
}

function mixOf(txs: ZcashTx[]): ZcashChain["txMix"] {
  const sample = txs.filter((t) => t.kind !== "coinbase");
  if (!sample.length) return null;
  const share = (k: ZcashTxKind) => sample.filter((t) => t.kind === k).length / sample.length;
  return { shielded: share("shielded"), shielding: share("shielding"), deshielding: share("deshielding"), transparent: share("transparent") };
}

// ---------- 3xpl ----------

const XPL = "https://sandbox-api.3xpl.com";

interface XplStats {
  best_block: number;
  best_block_hash: string;
  best_block_time: string;
  average_fee_24h?: { usd: string };
  mempool_events?: Record<string, number>;
}

interface XplBlock {
  block: { block: number; hash: string; time: string };
  events: { "zcash-main"?: { transaction: string }[] };
}

const xplBlocks = new Map<number, ZcashBlock>();

async function from3xpl(): Promise<{ head: XplStats; blocks: ZcashBlock[]; latencyMs: number }> {
  const { data, latencyMs } = await fetchJson<{ data: { blockchains: Record<string, XplStats> } }>(`${XPL}/?data=stats`);
  const head = data.data.blockchains.zcash;
  if (!head) throw new HttpError(502, "3xpl has no zcash stats");

  const heights = Array.from({ length: BLOCK_WINDOW }, (_, i) => head.best_block - i);
  for (const h of heights.filter((h) => !xplBlocks.has(h))) {
    try {
      const { data: b } = await fetchJson<{ data: XplBlock }>(`${XPL}/zcash/block/${h}?data=block,events&limit=1000`);
      const txs = new Set((b.data.events["zcash-main"] ?? []).map((e) => e.transaction));
      xplBlocks.set(h, { height: h, hash: b.data.block.hash, time: b.data.block.time.replace(".000000Z", "Z"), txCount: txs.size, size: 0 });
    } catch {
      break; // the head may not be indexed yet; try again next tick
    }
  }
  for (const h of xplBlocks.keys()) if (h < head.best_block - BLOCK_WINDOW) xplBlocks.delete(h);
  const blocks = heights.map((h) => xplBlocks.get(h)).filter((b): b is ZcashBlock => Boolean(b));
  return { head, blocks, latencyMs };
}

// ---------- combined ----------

export async function pollZcash(): Promise<{ chain: ZcashChain; latencyMs: number }> {
  const errors: string[] = [];

  if (config.zebraRpcUrl) {
    try {
      return await fromZebra();
    } catch (err) {
      errors.push(`zebra: ${errorMessage(err)}`);
    }
  }

  let bcLatency: number | null = null;
  try {
    bcLatency = await refreshBlockchair();
  } catch (err) {
    errors.push(`blockchair: ${errorMessage(err)}`);
  }

  // Blockchair stats are trusted for 15 minutes; the head itself comes from 3xpl when fresher.
  const stats = bc.stats && Date.now() - bc.stats.at < 15 * 60_000 ? bc.stats.value : null;
  let xpl: Awaited<ReturnType<typeof from3xpl>> | null = null;
  try {
    xpl = await from3xpl();
  } catch (err) {
    errors.push(`3xpl: ${errorMessage(err)}`);
  }

  if (!stats && !xpl) throw new Error(errors.join("; ") || "no Zcash provider available");

  const headHeight = Math.max(stats?.best_block_height ?? 0, xpl?.head.best_block ?? 0);
  const useXplHead = xpl && xpl.head.best_block >= (stats?.best_block_height ?? 0);
  const recentBlocks = xpl?.blocks.length ? xpl.blocks : (bc.recent?.blocks ?? []);
  const recentTxs = bc.recent?.txs ?? [];

  return {
    latencyMs: xpl?.latencyMs ?? bcLatency ?? 0,
    chain: {
      source: stats ? "blockchair" : "3xpl",
      height: headHeight,
      bestBlockHash: useXplHead ? xpl!.head.best_block_hash : stats!.best_block_hash,
      bestBlockTime: useXplHead ? xpl!.head.best_block_time.replace(".000000Z", "Z") : utc(stats!.best_block_time),
      blocks24h: stats?.blocks_24h ?? null,
      txs24h: stats?.transactions_24h ?? null,
      avgBlockTimeSec: stats?.blocks_24h ? Math.round(86_400 / stats.blocks_24h) : avgSpacing(recentBlocks),
      mempoolTxs: stats?.mempool_transactions ?? xpl?.head.mempool_events?.["zcash-main"] ?? null,
      hashrate: stats ? Number(stats.hashrate_24h) : null,
      nodes: stats?.nodes ?? null,
      avgFeeUsd: xpl?.head.average_fee_24h ? Number(xpl.head.average_fee_24h.usd) : (stats?.average_transaction_fee_usd_24h ?? null),
      blockchainSizeBytes: stats?.blockchain_size ?? null,
      valuePools: null,
      recentBlocks,
      recentTxs: recentTxs.slice(0, 25),
      txMix: mixOf(recentTxs),
      updatedAt: new Date().toISOString(),
    },
  };
}


