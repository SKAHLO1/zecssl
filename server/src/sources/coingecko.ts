import { config } from "../config.js";
import { fetchJson } from "../http.js";
import type { ChartRange, MarketAsset, PriceChart } from "../types.js";

const BASE = config.coingeckoKey ? "https://pro-api.coingecko.com/api/v3" : "https://api.coingecko.com/api/v3";

function headers(): Record<string, string> {
  return config.coingeckoKey ? { "x-cg-pro-api-key": config.coingeckoKey } : {};
}

interface CgMarket {
  id: string;
  symbol: string;
  name: string;
  image: string;
  market_cap_rank: number | null;
  current_price: number;
  market_cap: number;
  total_volume: number;
  high_24h: number | null;
  low_24h: number | null;
  circulating_supply: number | null;
  ath: number | null;
  last_updated: string;
  price_change_percentage_1h_in_currency?: number | null;
  price_change_percentage_24h_in_currency?: number | null;
  price_change_percentage_7d_in_currency?: number | null;
  sparkline_in_7d?: { price: number[] };
}

export async function fetchMarkets() {
  const url =
    `${BASE}/coins/markets?vs_currency=usd&ids=${config.marketIds.join(",")}` +
    `&sparkline=true&price_change_percentage=1h,24h,7d`;
  const { data, latencyMs } = await fetchJson<CgMarket[]>(url, { headers: headers() });
  const order = new Map<string, number>(config.marketIds.map((id, i) => [id, i]));
  const assets: MarketAsset[] = data
    .map((m) => ({
      id: m.id,
      symbol: m.symbol.toUpperCase(),
      name: m.name,
      image: m.image,
      rank: m.market_cap_rank,
      price: m.current_price,
      change1h: m.price_change_percentage_1h_in_currency ?? null,
      change24h: m.price_change_percentage_24h_in_currency ?? null,
      change7d: m.price_change_percentage_7d_in_currency ?? null,
      marketCap: m.market_cap,
      volume24h: m.total_volume,
      high24h: m.high_24h,
      low24h: m.low_24h,
      circulatingSupply: m.circulating_supply,
      ath: m.ath,
      // hourly points over 7 days; thin to ~42 points for sparklines
      sparkline7d: (m.sparkline_in_7d?.price ?? []).filter((_, i) => i % 4 === 0),
      updatedAt: m.last_updated,
    }))
    .sort((a, b) => (order.get(a.id) ?? 99) - (order.get(b.id) ?? 99));
  return { assets, latencyMs };
}

const rangeDays: Record<ChartRange, string> = { "1D": "1", "7D": "7", "30D": "30", "1Y": "365" };
const rangeTtl: Record<ChartRange, number> = { "1D": 120_000, "7D": 600_000, "30D": 1_800_000, "1Y": 3_600_000 };
const chartCache = new Map<string, { chart: PriceChart; expires: number; pending?: Promise<PriceChart> }>();

/** Price history, cached per id and range so the public rate limit is never hit by page views. */
export async function fetchChart(id: string, range: ChartRange): Promise<PriceChart> {
  const key = `${id}:${range}`;
  const hit = chartCache.get(key);
  if (hit && hit.expires > Date.now()) return hit.chart;
  if (hit?.pending) return hit.pending;

  const pending = (async () => {
    const url = `${BASE}/coins/${id}/market_chart?vs_currency=usd&days=${rangeDays[range]}`;
    const { data } = await fetchJson<{ prices: [number, number][] }>(url, { headers: headers() });
    const chart: PriceChart = {
      id,
      range,
      points: data.prices.map(([t, price]) => ({ t, price })),
      updatedAt: new Date().toISOString(),
    };
    chartCache.set(key, { chart, expires: Date.now() + rangeTtl[range] });
    return chart;
  })();

  chartCache.set(key, { chart: hit?.chart as PriceChart, expires: 0, pending });
  try {
    return await pending;
  } catch (err) {
    // serve the stale chart if we have one
    if (hit?.chart) {
      chartCache.set(key, { chart: hit.chart, expires: Date.now() + 60_000 });
      return hit.chart;
    }
    chartCache.delete(key);
    throw err;
  }
}
