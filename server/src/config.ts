function str(name: string, fallback = ""): string {
  return (process.env[name] ?? fallback).trim();
}

function int(name: string, fallback: number): number {
  const raw = process.env[name];
  const n = raw ? Number.parseInt(raw, 10) : Number.NaN;
  return Number.isFinite(n) ? n : fallback;
}

function operators(raw: string): { id: string; name: string; url: string }[] {
  return raw
    .split(",")
    .map((pair) => pair.trim())
    .filter(Boolean)
    .map((pair, i) => {
      const eq = pair.indexOf("=");
      const name = eq > 0 ? pair.slice(0, eq) : `operator-${i + 1}`;
      const url = (eq > 0 ? pair.slice(eq + 1) : pair).replace(/\/$/, "");
      return { id: name.toLowerCase(), name, url };
    });
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

  coordinatorUrl: str("COORDINATOR_URL").replace(/\/$/, ""),
  operators: operators(str("OPERATOR_ENDPOINTS")),
  threshold: { t: int("THRESHOLD_T", 2), n: int("THRESHOLD_N", 3) },

  spreadBps: int("QUOTE_SPREAD_BPS", 25),
  intentsReferenceRefund: str("INTENTS_REFERENCE_REFUND"),

  /** assets tracked on the markets board; ZEC first */
  marketIds: ["zcash", "near", "usd-coin", "bitcoin", "ethereum", "monero", "solana", "dash"],

  // Poll intervals (ms)
  intervals: {
    market: 60_000,
    zcash: 30_000,
    near: 6_000,
    intents: 60_000,
    network: 10_000,
  },
} as const;

export type Config = typeof config;
