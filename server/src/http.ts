export class HttpError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

export interface Timed<T> {
  data: T;
  latencyMs: number;
}

export async function fetchJson<T>(url: string, init: RequestInit & { timeoutMs?: number } = {}): Promise<Timed<T>> {
  const { timeoutMs = 12_000, ...rest } = init;
  const started = performance.now();
  const res = await fetch(url, {
    ...rest,
    headers: { accept: "application/json", "user-agent": "zec-ssl/0.1", ...rest.headers },
    signal: AbortSignal.timeout(timeoutMs),
  });
  const latencyMs = Math.round(performance.now() - started);
  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new HttpError(res.status, `${res.status} ${res.statusText} ${body.slice(0, 200)}`.trim());
  }
  return { data: (await res.json()) as T, latencyMs };
}

export function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.name === "TimeoutError" ? "timed out" : err.message;
  return String(err);
}
