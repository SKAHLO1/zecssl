import { config } from "./config.js";
import { errorMessage } from "./http.js";
import { fetchMarkets } from "./sources/coingecko.js";
import { fetchIntents } from "./sources/intents.js";
import { fetchNearChain } from "./sources/near.js";
import { pollNetwork } from "./sources/network.js";
import { pollZcash } from "./sources/zcash.js";
import { store } from "./store.js";

type Task = () => Promise<void>;

/** Runs a task now and then on a fixed interval, never overlapping itself. */
function every(ms: number, task: Task): () => void {
  let running = false;
  const run = async () => {
    if (running) return;
    running = true;
    try {
      await task();
    } finally {
      running = false;
    }
  };
  void run();
  const timer = setInterval(run, ms);
  return () => clearInterval(timer);
}

export function startPollers(): () => void {
  store.declareSource("coingecko", true);
  store.declareSource("zcash", true);
  store.declareSource("near", true);
  store.declareSource("intents", true);
  store.declareSource("coordinator", Boolean(config.coordinatorUrl));
  store.declareSource("operators", config.operators.length > 0);
  store.declareSource("vault", Boolean(config.nearVaultContract || config.nearBondsContract));

  const stops = [
    every(config.intervals.market, async () => {
      try {
        const { assets, latencyMs } = await fetchMarkets();
        store.set("market", assets);
        store.succeeded("coingecko", latencyMs);
      } catch (err) {
        store.failed("coingecko", errorMessage(err));
      }
    }),
    every(config.intervals.zcash, async () => {
      try {
        const { chain, latencyMs } = await pollZcash();
        store.set("zcash", chain);
        store.succeeded("zcash", latencyMs);
      } catch (err) {
        store.failed("zcash", errorMessage(err));
      }
    }),
    every(config.intervals.near, async () => {
      try {
        const { chain, latencyMs } = await fetchNearChain();
        store.set("near", chain);
        store.succeeded("near", latencyMs);
      } catch (err) {
        store.failed("near", errorMessage(err));
      }
    }),
    every(config.intervals.intents, async () => {
      try {
        const { snapshot, latencyMs } = await fetchIntents();
        store.set("intents", snapshot);
        store.succeeded("intents", latencyMs);
      } catch (err) {
        store.failed("intents", errorMessage(err));
      }
    }),
    every(config.intervals.network, async () => {
      store.set("network", await pollNetwork());
    }),
  ];
  return () => stops.forEach((stop) => stop());
}
