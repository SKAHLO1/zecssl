function str(name: string, fallback = ""): string {
  return (process.env[name] ?? fallback).trim();
}

function int(name: string, fallback: number): number {
  const raw = process.env[name];
  const n = raw ? Number.parseInt(raw, 10) : Number.NaN;
  return Number.isFinite(n) ? n : fallback;
}

/** Accepts `host:port` (as Render's service references give it) or a full URL. */
function httpUrl(s: string): string {
  const t = s.trim().replace(/\/$/, "");
  return t.includes("://") ? t : `http://${t}`;
}

/**
 * Operator health endpoints: `name=url,name=url` in OPERATOR_ENDPOINTS, or numbered
 * OPERATOR_1_URL / OPERATOR_1_NAME ... OPERATOR_9_URL (one per Render service reference).
 * Name each endpoint after its NEAR account so it stays matched to the contract while down.
 */
function operators(raw: string): { id: string; name: string; url: string }[] {
  const pairs: [string, string][] = raw
    ? raw
        .split(",")
        .map((p) => p.trim())
        .filter(Boolean)
        .map((pair, i) => {
          const eq = pair.indexOf("=");
          return [eq > 0 ? pair.slice(0, eq) : `operator-${i + 1}`, eq > 0 ? pair.slice(eq + 1) : pair];
        })
    : Array.from({ length: 9 }, (_, i) => i + 1)
        .filter((i) => str(`OPERATOR_${i}_URL`))
        .map((i) => [str(`OPERATOR_${i}_NAME`, `operator-${i}`), str(`OPERATOR_${i}_URL`)]);
  return pairs.map(([name, url]) => ({ id: name.toLowerCase(), name, url: httpUrl(url) }));
}

const nearNetwork = str("NEAR_NETWORK", "testnet");

export const config = {
  port: int("PORT", 4000),
  host: str("HOST", "0.0.0.0"),
  corsOrigins: str("CORS_ORIGINS", "http://localhost:3000").split(",").map((s) => s.trim()).filter(Boolean),

  coingeckoKey: str("COINGECKO_API_KEY"),
  blockchairKey: str("BLOCKCHAIR_API_KEY"),
  zebraRpcUrl: str("ZEBRA_RPC_URL"),
  zebraRpcAuth: str("ZEBRA_RPC_AUTH"),

  nearNetwork,
  nearRpcUrl: str("NEAR_RPC_URL", `https://rpc.${nearNetwork}.fastnear.com`),
  nearVaultContract: str("NEAR_VAULT_CONTRACT"),
  nearBondsContract: str("NEAR_BONDS_CONTRACT"),

  coordinatorUrl: str("COORDINATOR_URL") ? httpUrl(str("COORDINATOR_URL")) : "",
  operators: operators(str("OPERATOR_ENDPOINTS")),
  threshold: { t: int("THRESHOLD_T", 2), n: int("THRESHOLD_N", 3) },

  // Testnet faucet signer: account + secret key, or a NEAR CLI credentials file.
  faucetAccount: str("NEAR_FAUCET_ACCOUNT"),
  faucetKey: str("NEAR_FAUCET_KEY"),
  faucetCredentials: str("NEAR_FAUCET_CREDENTIALS"),
  // Behind a proxy (Render, Fly), trust X-Forwarded-For for client addresses.
  trustProxy: str("TRUST_PROXY") === "true",

  spreadBps: int("QUOTE_SPREAD_BPS", 25),
  intentsReferenceRefund: str("INTENTS_REFERENCE_REFUND"),

  /** assets tracked on the markets board; ZEC first */
  marketIds: ["zcash", "near", "usd-coin", "bitcoin", "ethereum", "monero", "solana", "dash"],

  // Poll intervals (ms)
  intervals: {
    market: 60_000,
    zcash: 30_000,
    near: 10_000,
    intents: 60_000,
    network: 15_000,
  },
} as const;

export type Config = typeof config;
