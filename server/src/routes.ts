import type { FastifyInstance } from "fastify";
import { config } from "./config.js";
import { errorMessage, HttpError } from "./http.js";
import { fetchChart } from "./sources/coingecko.js";
import { quote } from "./sources/intents.js";
import { coordinatorQuote, createOrder, orderDetail } from "./sources/network.js";
import { store } from "./store.js";
import type { ChartRange, NetworkSnapshot, Overview, QuoteRequest, StreamTopic } from "./types.js";

const ranges: ChartRange[] = ["1D", "7D", "30D", "1Y"];

function emptyNetwork(): NetworkSnapshot {
  return {
    threshold: config.threshold,
    coordinator: { configured: Boolean(config.coordinatorUrl), url: config.coordinatorUrl || null, online: false, latencyMs: null },
    operators: [],
    vault: null,
    bonds: null,
    treasury: null,
    orders: [],
    rounds: [],
    events: [],
    vaultToken: null,
    updatedAt: new Date().toISOString(),
  };
}

function snapshot(topic: StreamTopic) {
  if (topic === "sources") return store.sourceList();
  if (topic === "network") return store.state.network ?? emptyNetwork();
  return store.state[topic];
}

export async function routes(app: FastifyInstance) {
  app.setErrorHandler((err, _req, reply) => {
    const status = err instanceof HttpError ? err.status : ((err as { statusCode?: number }).statusCode ?? 500);
    reply.status(status >= 400 && status < 600 ? status : 502).send({ error: errorMessage(err) });
  });

  app.get("/v1/health", async () => ({ ok: true, uptimeSec: Math.round(process.uptime()), sources: store.sourceList() }));

  app.get("/v1/overview", async (): Promise<Overview> => ({
    market: store.state.market,
    zcash: store.state.zcash,
    near: store.state.near,
    intents: store.state.intents,
    network: store.state.network ?? emptyNetwork(),
    sources: store.sourceList(),
  }));

  app.get("/v1/market", async () => store.state.market);

  app.get<{ Params: { id: string }; Querystring: { range?: string } }>("/v1/market/:id/chart", async (req) => {
    const { id } = req.params;
    if (!(config.marketIds as readonly string[]).includes(id)) throw new HttpError(404, "unknown asset");
    const range = (req.query.range ?? "1D").toUpperCase() as ChartRange;
    if (!ranges.includes(range)) throw new HttpError(400, `range must be one of ${ranges.join(", ")}`);
    return fetchChart(id, range);
  });

  app.get("/v1/zcash", async () => store.state.zcash);
  app.get("/v1/near", async () => store.state.near);
  app.get("/v1/intents/tokens", async () => store.state.intents);
  app.get("/v1/network", async () => snapshot("network"));

  app.post<{ Body: QuoteRequest }>("/v1/quote", async (req) => {
    const body = req.body ?? ({} as QuoteRequest);
    if (typeof body.amount !== "string" || typeof body.destinationAssetId !== "string") {
      throw new HttpError(400, "amount and destinationAssetId are required");
    }
    if (body.destinationAssetId.startsWith("vault:")) {
      if (!config.coordinatorUrl) throw new HttpError(503, "The ZEC SSL coordinator is not connected.");
      return coordinatorQuote(body.amount);
    }
    return quote(body, store.state.intents);
  });

  app.get<{ Params: { id: string } }>("/v1/orders/:id", async (req) => {
    if (!config.coordinatorUrl) throw new HttpError(503, "The ZEC SSL coordinator is not connected.");
    return orderDetail(req.params.id);
  });

  app.post("/v1/orders", async (req) => {
    if (!config.coordinatorUrl) throw new HttpError(503, "The ZEC SSL coordinator is not connected, so orders cannot be opened yet.");
    return createOrder(req.body);
  });

  // Server-Sent Events: a snapshot of every topic on connect, then each update as it lands.
  app.get("/v1/stream", (req, reply) => {
    const origin = req.headers.origin;
    reply.raw.writeHead(200, {
      "content-type": "text/event-stream",
      "cache-control": "no-cache, no-transform",
      connection: "keep-alive",
      "x-accel-buffering": "no",
      ...(origin && config.corsOrigins.includes(origin) ? { "access-control-allow-origin": origin, vary: "origin" } : {}),
    });
    const send = (topic: StreamTopic, data: unknown) => reply.raw.write(`event: ${topic}\ndata: ${JSON.stringify(data)}\n\n`);

    (["market", "zcash", "near", "intents", "network", "sources"] as StreamTopic[]).forEach((t) => {
      const value = snapshot(t);
      if (value) send(t, value);
    });
    const onUpdate = (topic: StreamTopic, value: unknown) => send(topic, value);
    store.on("update", onUpdate);
    const heartbeat = setInterval(() => reply.raw.write(": ping\n\n"), 15_000);

    req.raw.on("close", () => {
      clearInterval(heartbeat);
      store.off("update", onUpdate);
    });
  });
}
