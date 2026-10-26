import type { FastifyInstance } from "fastify";
import { config } from "./config.js";
import { errorMessage, HttpError } from "./http.js";
import { fetchChart } from "./sources/coingecko.js";
import { quote } from "./sources/intents.js";
import { accountStatus, drip, faucetEnabled, register } from "./sources/faucet.js";
import { accountOrders, coordinatorQuote, createOrder, createPayout, orderDetail } from "./sources/network.js";
import { store } from "./store.js";
import type { ChartRange, FaucetInfo, NetworkSnapshot, Overview, QuoteRequest, StreamTopic } from "./types.js";

const ranges: ChartRange[] = ["1D", "7D", "30D", "1Y"];

/** NEAR account ids: named (alice.testnet) or implicit (64 hex characters). */
const NEAR_ACCOUNT_ID = /^(?:[a-z0-9_-]+\.)*[a-z0-9_-]+$|^[0-9a-f]{64}$/;

/** Order ids are what the coordinator issues: letters, digits, dash and underscore. */
const ORDER_ID = /^[A-Za-z0-9_-]{1,64}$/;

/** Per-IP limits for routes that cost gas, reserve treasury funds or spend upstream quota. */
const limit = (max: number, timeWindow: string) => ({ config: { rateLimit: { max, timeWindow } } });

/** Live streams held open per client IP, and in total. */
const STREAMS_PER_IP = 4;
const STREAMS_TOTAL = 500;
const openStreams = new Map<string, number>();
let streamCount = 0;

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

  app.get<{ Params: { id: string }; Querystring: { range?: string } }>("/v1/market/:id/chart", limit(60, "1 minute"), async (req) => {
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

  app.post<{ Body: QuoteRequest }>("/v1/quote", limit(60, "1 minute"), async (req) => {
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

  app.get<{ Params: { id: string } }>("/v1/accounts/:id", limit(60, "1 minute"), async (req) => accountStatus(req.params.id));

  // One account's orders, for "Your orders". The public snapshot masks order ids.
  app.get<{ Params: { id: string } }>("/v1/accounts/:id/orders", limit(30, "1 minute"), async (req) => {
    if (!config.coordinatorUrl) throw new HttpError(503, "The ZEC SSL coordinator is not connected.");
    if (!NEAR_ACCOUNT_ID.test(req.params.id)) throw new HttpError(400, "not a valid NEAR account id");
    return accountOrders(req.params.id);
  });

  app.get("/v1/faucet", async (): Promise<FaucetInfo> => {
    const token = store.state.network?.vaultToken;
    return {
      enabled: faucetEnabled(),
      network: config.nearNetwork,
      dripAmount: 100,
      symbol: token?.symbol ?? null,
      token: token?.contractAddress ?? null,
    };
  });

  app.post<{ Body: { accountId?: string } }>("/v1/faucet/register", limit(10, "1 hour"), async (req) => {
    if (typeof req.body?.accountId !== "string") throw new HttpError(400, "accountId is required");
    return register(req.body.accountId.trim());
  });

  app.post<{ Body: { accountId?: string } }>("/v1/faucet/usdc", limit(10, "1 hour"), async (req) => {
    if (typeof req.body?.accountId !== "string") throw new HttpError(400, "accountId is required");
    return drip(req.body.accountId.trim(), req.ip);
  });

  app.get<{ Params: { id: string } }>("/v1/orders/:id", limit(120, "1 minute"), async (req) => {
    if (!config.coordinatorUrl) throw new HttpError(503, "The ZEC SSL coordinator is not connected.");
    if (!ORDER_ID.test(req.params.id)) throw new HttpError(400, "not a valid order id");
    return orderDetail(req.params.id);
  });

  // Opening an order makes every operator send a NEAR transaction (gas), so it is limited
  // hard per client; the coordinator also caps unfunded orders in total.
  app.post("/v1/orders", limit(5, "10 minutes"), async (req) => {
    if (!config.coordinatorUrl) throw new HttpError(503, "The ZEC SSL coordinator is not connected, so orders cannot be opened yet.");
    return createOrder(req.body);
  });

  // Direction 2: quote vault tokens → shielded ZEC. The user then locks the tokens with the
  // returned ft_transfer_call from their own NEAR wallet.
  app.post("/v1/payouts", limit(5, "10 minutes"), async (req) => {
    if (!config.coordinatorUrl) throw new HttpError(503, "The ZEC SSL coordinator is not connected, so payouts cannot be quoted yet.");
    return createPayout(req.body);
  });

  // Server-Sent Events: a snapshot of every topic on connect, then each update as it lands.
  app.get("/v1/stream", (req, reply) => {
    const held = openStreams.get(req.ip) ?? 0;
    if (held >= STREAMS_PER_IP || streamCount >= STREAMS_TOTAL) {
      reply.status(429).send({ error: "too many open live streams" });
      return;
    }
    openStreams.set(req.ip, held + 1);
    streamCount += 1;
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
      streamCount -= 1;
      const left = (openStreams.get(req.ip) ?? 1) - 1;
      if (left > 0) openStreams.set(req.ip, left);
      else openStreams.delete(req.ip);
    });
  });
}
