import cors from "@fastify/cors";
import rateLimit from "@fastify/rate-limit";
import Fastify from "fastify";
import { config } from "./config.js";
import { startPollers } from "./poller.js";
import { routes } from "./routes.js";

// Request bodies here are a few small JSON fields; anything bigger is not ours.
const app = Fastify({ logger: { level: process.env.LOG_LEVEL ?? "info" }, trustProxy: config.trustProxy, bodyLimit: 32 * 1024 });

await app.register(cors, { origin: config.corsOrigins });
// Per client IP (the real one, via the proxy when TRUST_PROXY is set). Routes that cost
// money or upstream quota set tighter limits of their own.
await app.register(rateLimit, { global: true, max: 300, timeWindow: "1 minute" });
await app.register(routes);

const stopPollers = startPollers();

async function shutdown(signal: string) {
  app.log.info({ signal }, "shutting down");
  stopPollers();
  await app.close();
  process.exit(0);
}
process.on("SIGINT", () => void shutdown("SIGINT"));
process.on("SIGTERM", () => void shutdown("SIGTERM"));

await app.listen({ port: config.port, host: config.host });
