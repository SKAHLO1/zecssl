"use client";

import { ArrowDown, ArrowRight } from "lucide-react";
import Link from "next/link";
import { useEffect, useState } from "react";
import { token, usd } from "@/lib/format";
import { useDestinations, useQuote } from "@/lib/use-quote";
import type { IntentsToken } from "@/lib/types";
import { TokenBadge, TokenSelect } from "./token-select";
import { buttonClass, cx, Skeleton } from "./ui";

/** Compact quote card: live price for a shielded ZEC exit, then hands off to the swap flow. */
export function QuickSwap({ className, floating }: { className?: string; floating?: boolean }) {
  const destinations = useDestinations();
  const [amount, setAmount] = useState("10");
  const [dest, setDest] = useState<IntentsToken | null>(null);
  useEffect(() => {
    if (!dest && destinations.length) setDest(destinations[0]);
  }, [dest, destinations]);
  const { quote, error, loading } = useQuote(amount, dest?.assetId ?? null);

  const href = `/app/swap?amount=${encodeURIComponent(amount)}${dest ? `&to=${encodeURIComponent(dest.assetId)}` : ""}`;

  return (
    <div className={cx("rounded-2xl border border-line bg-surface p-4", floating ? "shadow-[var(--shadow-float)]" : "shadow-[var(--shadow-card)]", className)}>
      <div className="mb-3 flex items-center justify-between">
        <span className="text-sm font-semibold">Swap</span>
        <span className="text-xs text-ink-3">Shielded ZEC out</span>
      </div>

      <label className="block rounded-xl bg-sunken/70 px-3.5 py-3">
        <span className="text-xs text-ink-3">You send</span>
        <span className="mt-1 flex items-center gap-2">
          <input
            inputMode="decimal"
            value={amount}
            onChange={(e) => setAmount(e.target.value.replace(/[^\d.]/g, ""))}
            className="tnum w-full min-w-0 bg-transparent text-2xl font-semibold outline-none"
            aria-label="Amount of ZEC to send"
          />
          <span className="flex shrink-0 items-center gap-2 rounded-full border border-line bg-surface py-1.5 pr-3 pl-1.5 text-sm font-semibold">
            <TokenBadge token={{ symbol: "ZEC", blockchain: "zec" }} size={24} />
            ZEC
          </span>
        </span>
        <span className="tnum mt-1 block text-xs text-ink-3">{quote ? usd(quote.amountInUsd, { cents: true }) : "Shielded · Ironwood pool"}</span>
      </label>

      <div className="relative z-10 -my-2.5 flex justify-center">
        <span className="grid size-8 place-items-center rounded-full border-4 border-surface bg-sunken text-ink-2">
          <ArrowDown className="size-4" />
        </span>
      </div>

      <div className="rounded-xl bg-sunken/70 px-3.5 py-3">
        <span className="text-xs text-ink-3">You receive</span>
        <span className="mt-1 flex items-center gap-2">
          <span className="tnum min-w-0 flex-1 truncate text-2xl font-semibold">
            {loading && !quote ? <Skeleton className="h-7 w-32" /> : quote ? token(quote.amountOut, 4) : "0"}
          </span>
          <TokenSelect tokens={destinations} value={dest} onChange={setDest} />
        </span>
        <span className="tnum mt-1 block text-xs text-ink-3">{quote ? usd(quote.amountOutUsd, { cents: true }) : " "}</span>
      </div>

      <div className="mt-3 min-h-5 text-xs text-ink-3">
        {error ? (
          <span className="text-down">{error}</span>
        ) : quote ? (
          <span className="tnum">
            1 ZEC ≈ {token(quote.rate, 4)} {quote.destination.symbol} · {quote.spreadBps / 100}% operator fee
          </span>
        ) : null}
      </div>

      <Link href={href} className={cx(buttonClass.primary, "mt-3 w-full")}>
        Continue <ArrowRight className="size-4" />
      </Link>
    </div>
  );
}
