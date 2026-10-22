const usd0 = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 });
const usd2 = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", minimumFractionDigits: 2, maximumFractionDigits: 2 });
const int = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 });

/** Price with precision that suits its size, like exchange tickers. */
export function price(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return "—";
  if (v >= 1000) return usd2.format(v);
  if (v >= 1) return usd2.format(v);
  if (v >= 0.01) return `$${v.toFixed(4)}`;
  return `$${v.toPrecision(3)}`;
}

// Compact notation is formatted by hand: Node's and browsers' ICU data disagree on
// trailing zeros ("1.10B" vs "1.1B"), which breaks hydration.
function compactNumber(v: number): string {
  const abs = Math.abs(v);
  const units: [number, string][] = [[1e12, "T"], [1e9, "B"], [1e6, "M"], [1e3, "K"]];
  for (const [size, suffix] of units) {
    if (abs >= size) return `${(v / size).toFixed(2)}${suffix}`;
  }
  return v.toFixed(abs >= 100 ? 0 : 2);
}

export function usd(v: number | null | undefined, opts: { compact?: boolean; cents?: boolean } = {}): string {
  if (v == null || !Number.isFinite(v)) return "—";
  if (opts.compact) return `${v < 0 ? "-" : ""}$${compactNumber(Math.abs(v))}`;
  return opts.cents ? usd2.format(v) : usd0.format(v);
}

export function num(v: number | null | undefined, opts: { compact?: boolean; digits?: number } = {}): string {
  if (v == null || !Number.isFinite(v)) return "—";
  if (opts.compact) return compactNumber(v);
  if (opts.digits != null) return v.toLocaleString("en-US", { minimumFractionDigits: opts.digits, maximumFractionDigits: opts.digits });
  return int.format(v);
}

export function token(v: string | number | null | undefined, maxDigits = 6): string {
  if (v == null || v === "") return "—";
  const n = typeof v === "string" ? Number(v) : v;
  if (!Number.isFinite(n)) return String(v);
  return n.toLocaleString("en-US", { maximumFractionDigits: n >= 1000 ? 2 : maxDigits });
}

export function pct(v: number | null | undefined, digits = 2): string {
  if (v == null || !Number.isFinite(v)) return "—";
  return `${Math.abs(v).toFixed(digits)}%`;
}

export function share(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return "—";
  return `${Math.round(v * 100)}%`;
}

export function bytes(v: number | null | undefined): string {
  if (v == null) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let n = v;
  while (n >= 1000 && i < units.length - 1) {
    n /= 1000;
    i++;
  }
  return `${n.toFixed(n >= 100 ? 0 : 1)} ${units[i]}`;
}

export function hashrate(v: number | null | undefined): string {
  if (v == null) return "—";
  const units = ["Sol/s", "kSol/s", "MSol/s", "GSol/s", "TSol/s"];
  let i = 0;
  let n = v;
  while (n >= 1000 && i < units.length - 1) {
    n /= 1000;
    i++;
  }
  return `${n.toFixed(2)} ${units[i]}`;
}

export function shortHash(h: string | null | undefined, head = 6, tail = 4): string {
  if (!h) return "—";
  return h.length <= head + tail + 1 ? h : `${h.slice(0, head)}…${h.slice(-tail)}`;
}

export function ago(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return "—";
  // useNow() reports 0 until mount, so the server render leaves relative times blank
  if (!now) return "";
  const s = Math.max(0, Math.round((now - Date.parse(iso)) / 1000));
  if (s < 5) return "just now";
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h}h ago`;
  return `${Math.round(h / 24)}d ago`;
}

export function duration(sec: number | null | undefined): string {
  if (sec == null) return "—";
  if (sec < 90) return `${Math.round(sec)}s`;
  return `${Math.round(sec / 60)} min`;
}

export const chainNames: Record<string, string> = {
  near: "NEAR",
  eth: "Ethereum",
  arb: "Arbitrum",
  base: "Base",
  sol: "Solana",
  btc: "Bitcoin",
  zec: "Zcash",
  gnosis: "Gnosis",
  pol: "Polygon",
  bsc: "BNB Chain",
  aptos: "Aptos",
  tron: "Tron",
  ton: "TON",
  sui: "Sui",
  op: "Optimism",
  avax: "Avalanche",
  doge: "Dogecoin",
  xrp: "XRP Ledger",
  stellar: "Stellar",
  bera: "Berachain",
  cardano: "Cardano",
  ltc: "Litecoin",
  bch: "Bitcoin Cash",
};

export const chainName = (id: string) => chainNames[id] ?? id.toUpperCase();
