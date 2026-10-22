import cors from "@fastify/cors";
import Fastify from "fastify";
import { config } from "./config.js";
import { startPollers } from "./poller.js";
import { routes } from "./routes.js";

const app = Fastify({ logger: { level: process.env.LOG_LEVEL ?? "info" }, trustProxy: config.trustProxy });

await app.register(cors, { origin: config.corsOrigins });
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
