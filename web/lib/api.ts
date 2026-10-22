import type { ChartRange, Overview, PriceChart, Quote, QuoteRequest } from "./types";

/** Base URL the browser uses. */
export const publicApiUrl = (process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:4000").replace(/\/$/, "");

/** Base URL the Next.js server uses for the first render. */
const serverApiUrl = (process.env.API_URL ?? publicApiUrl).replace(/\/$/, "");

/** First-render snapshot. Returns null if the API is down so pages can still render. */
export async function getOverview(): Promise<Overview | null> {
  try {
    const res = await fetch(`${serverApiUrl}/v1/overview`, { cache: "no-store", signal: AbortSignal.timeout(4000) });
    if (!res.ok) return null;
    return (await res.json()) as Overview;
  } catch {
    return null;
  }
}

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${publicApiUrl}${path}`, init);
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error((body as { error?: string }).error ?? `Request failed (${res.status})`);
  return body as T;
}

export const api = {
  chart: (id: string, range: ChartRange) => call<PriceChart>(`/v1/market/${id}/chart?range=${range}`),
  quote: (req: QuoteRequest) =>
    call<Quote>("/v1/quote", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(req) }),
  createOrder: (body: unknown) =>
    call<unknown>("/v1/orders", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) }),
};
