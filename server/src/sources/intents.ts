import { config } from "../config.js";
import { fetchJson, HttpError } from "../http.js";
import type { IntentsSnapshot, IntentsToken, Quote, QuoteRequest } from "../types.js";

const BASE = "https://1click.chaindefuser.com/v0";
export const ZEC_ASSET_ID = "nep141:zec.omft.near";
const ZEC_DECIMALS = 8;

// ZIP-317: 5,000 zatoshis per logical action with a grace of 2 actions. A shielded
// treasury spend with one payment and one change output is two Ironwood actions.
const ZCASH_FEE_ZAT = 10_000n;

interface OneClickToken {
  assetId: string;
  decimals: number;
  blockchain: string;
  symbol: string;
  price: number;
  contractAddress?: string;
}

export async function fetchIntents(): Promise<{ snapshot: IntentsSnapshot; latencyMs: number }> {
  const { data, latencyMs } = await fetchJson<OneClickToken[]>(`${BASE}/tokens`);
  const tokens: IntentsToken[] = data.map((t) => ({
    assetId: t.assetId,
    symbol: t.symbol,
    blockchain: t.blockchain,
    decimals: t.decimals,
    price: t.price,
    contractAddress: t.contractAddress ?? null,
  }));
  return {
    latencyMs,
    snapshot: {
      tokens,
      chains: [...new Set(tokens.map((t) => t.blockchain))].sort(),
      zec: tokens.find((t) => t.assetId === ZEC_ASSET_ID) ?? null,
      updatedAt: new Date().toISOString(),
    },
  };
}

function toUnits(amount: string, decimals: number): bigint {
  if (!/^\d+(\.\d+)?$/.test(amount)) throw new HttpError(400, "amount must be a positive decimal");
  const [whole, frac = ""] = amount.split(".");
  return BigInt(whole + frac.padEnd(decimals, "0").slice(0, decimals));
}

function fromUnits(units: bigint, decimals: number): string {
  const neg = units < 0n;
  const v = neg ? -units : units;
  const base = 10n ** BigInt(decimals);
  const frac = (v % base).toString().padStart(decimals, "0").replace(/0+$/, "");
  return `${neg ? "-" : ""}${v / base}${frac ? `.${frac}` : ""}`;
}

interface OneClickQuote {
  quote: {
    amountOut: string;
    amountOutUsd: string;
    amountInUsd: string;
    minAmountOut: string;
    timeEstimate: number;
  };
}

/**
 * Prices shielded ZEC -> destination asset. The NEAR leg is priced by a dry-run 1Click quote
 * when a reference refund address is configured, otherwise from 1Click's live oracle prices.
 * The operator spread and the Zcash network fee are applied on top either way.
 */
export async function quote(req: QuoteRequest, snapshot: IntentsSnapshot | null): Promise<Quote> {
  if (!snapshot?.zec) throw new HttpError(503, "NEAR Intents token feed not loaded yet");
  const dest = snapshot.tokens.find((t) => t.assetId === req.destinationAssetId);
  if (!dest) throw new HttpError(400, "unsupported destination asset");

  const grossZat = toUnits(req.amount, ZEC_DECIMALS);
  if (grossZat <= ZCASH_FEE_ZAT) throw new HttpError(400, "amount is below the network fee");
  const feeZat = (grossZat * BigInt(config.spreadBps)) / 10_000n;
  const netZat = grossZat - feeZat - ZCASH_FEE_ZAT;
  const amountInUsd = (Number(grossZat) / 10 ** ZEC_DECIMALS) * snapshot.zec.price;

  const base = {
    amountIn: req.amount,
    amountInUsd,
    destination: dest,
    spreadBps: config.spreadBps,
    operatorFee: fromUnits(feeZat, ZEC_DECIMALS),
    zcashNetworkFeeZec: fromUnits(ZCASH_FEE_ZAT, ZEC_DECIMALS),
    quotedAt: new Date().toISOString(),
  };

  if (config.intentsReferenceRefund) {
    const deadline = new Date(Date.now() + 30 * 60_000).toISOString();
    const { data } = await fetchJson<OneClickQuote>(`${BASE}/quote`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      timeoutMs: 15_000,
      body: JSON.stringify({
        dry: true,
        swapType: "EXACT_INPUT",
        slippageTolerance: 100,
        originAsset: ZEC_ASSET_ID,
        depositType: "ORIGIN_CHAIN",
        destinationAsset: dest.assetId,
        amount: netZat.toString(),
        refundTo: config.intentsReferenceRefund,
        refundType: "ORIGIN_CHAIN",
        recipient: req.recipient || "zec-ssl.near",
        recipientType: "DESTINATION_CHAIN",
        deadline,
        quoteWaitingTimeMs: 3000,
      }),
    });
    const out = BigInt(data.quote.amountOut);
    const outNum = Number(out) / 10 ** dest.decimals;
    return {
      ...base,
      source: "router",
      amountOut: fromUnits(out, dest.decimals),
      amountOutUsd: Number(data.quote.amountOutUsd),
      minAmountOut: fromUnits(BigInt(data.quote.minAmountOut), dest.decimals),
      rate: outNum / (Number(grossZat) / 10 ** ZEC_DECIMALS),
      timeEstimateSec: data.quote.timeEstimate,
    };
  }

  const netZec = Number(netZat) / 10 ** ZEC_DECIMALS;
  const outNum = (netZec * snapshot.zec.price) / dest.price;
  const outUnits = BigInt(Math.floor(outNum * 10 ** Math.min(dest.decimals, 12))) * 10n ** BigInt(Math.max(dest.decimals - 12, 0));
  return {
    ...base,
    source: "oracle",
    amountOut: fromUnits(outUnits, dest.decimals),
    amountOutUsd: outNum * dest.price,
    minAmountOut: null,
    rate: outNum / (Number(grossZat) / 10 ** ZEC_DECIMALS),
    timeEstimateSec: null,
  };
}
