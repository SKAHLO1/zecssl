"use client";

import { useEffect, useMemo, useState } from "react";
import { api } from "./api";
import { useLive } from "./live";
import type { IntentsToken, Quote } from "./types";

// Destinations offered first, in this order; the rest of the NEAR Intents list follows.
const preferred: [symbol: string, chain: string][] = [
  ["USDC", "near"],
  ["USDT", "near"],
  ["wNEAR", "near"],
  ["USDC", "eth"],
  ["USDC", "base"],
  ["USDC", "sol"],
  ["USDC", "arb"],
  ["ETH", "eth"],
  ["BTC", "btc"],
  ["SOL", "sol"],
];

export function useDestinations(): IntentsToken[] {
  const { intents } = useLive();
  return useMemo(() => {
    const tokens = (intents?.tokens ?? []).filter((t) => t.blockchain !== "zec" && t.price > 0);
    const picked: IntentsToken[] = [];
    for (const [sym, chain] of preferred) {
      const hit = tokens.find((t) => t.symbol === sym && t.blockchain === chain);
      if (hit) picked.push(hit);
    }
    const rest = tokens.filter((t) => !picked.includes(t)).sort((a, b) => a.symbol.localeCompare(b.symbol));
    return [...picked, ...rest];
  }, [intents]);
}

/** Debounced, auto-refreshing quote for shielded ZEC -> destination. */
export function useQuote(amount: string, destinationAssetId: string | null, recipient?: string) {
  const [quote, setQuote] = useState<Quote | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    const valid = /^\d*\.?\d+$/.test(amount) && Number(amount) > 0;
    if (!valid || !destinationAssetId) {
      setQuote(null);
      setError(null);
      return;
    }
    let cancelled = false;
    const run = () => {
      setLoading(true);
      api
        .quote({ amount, destinationAssetId, recipient })
        .then((q) => {
          if (!cancelled) {
            setQuote(q);
            setError(null);
          }
        })
        .catch((e: Error) => !cancelled && setError(e.message))
        .finally(() => !cancelled && setLoading(false));
    };
    const debounce = setTimeout(run, 350);
    const refresh = setInterval(run, 20_000);
    return () => {
      cancelled = true;
      clearTimeout(debounce);
      clearInterval(refresh);
    };
  }, [amount, destinationAssetId, recipient]);

  return { quote, error, loading };
}
