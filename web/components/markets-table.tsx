"use client";

import Link from "next/link";
import { useEffect, useRef, useState } from "react";
import { price, usd } from "@/lib/format";
import { useLive } from "@/lib/live";
import type { MarketAsset } from "@/lib/types";
import { Change, CoinIcon, cx, Skeleton, Sparkline } from "./ui";

/** Briefly tints a cell when its value ticks up or down, like exchange order books. */
function FlashPrice({ value }: { value: number }) {
  const prev = useRef(value);
  const [flash, setFlash] = useState<"up" | "down" | null>(null);
  useEffect(() => {
    if (value !== prev.current) {
      setFlash(value > prev.current ? "up" : "down");
      prev.current = value;
      const t = setTimeout(() => setFlash(null), 1200);
      return () => clearTimeout(t);
    }
  }, [value]);
  return <span className={cx("rounded px-1 -mx-1", flash === "up" && "flash-up", flash === "down" && "flash-down")}>{price(value)}</span>;
}

export function MarketsTable({ limit, onSelect, selected }: { limit?: number; onSelect?: (a: MarketAsset) => void; selected?: string }) {
  const { market, loaded } = useLive();
  const rows = limit ? market.slice(0, limit) : market;

  return (
    <div className="scroll-x">
      <table className="w-full min-w-[640px] text-sm">
        <thead>
          <tr className="border-b border-line text-left text-xs font-medium text-ink-3">
            <th className="w-10 py-3 pr-2 pl-5 font-medium">#</th>
            <th className="py-3 pr-4 font-medium">Asset</th>
            <th className="py-3 pr-4 text-right font-medium">Price</th>
            <th className="hidden py-3 pr-4 text-right font-medium md:table-cell">1h</th>
            <th className="py-3 pr-4 text-right font-medium">24h</th>
            <th className="hidden py-3 pr-4 text-right font-medium md:table-cell">7d</th>
            <th className="hidden py-3 pr-4 text-right font-medium lg:table-cell">24h volume</th>
            <th className="hidden py-3 pr-4 text-right font-medium sm:table-cell">Market cap</th>
            <th className="hidden py-3 pr-5 text-right font-medium lg:table-cell">Last 7 days</th>
          </tr>
        </thead>
        <tbody>
          {rows.length === 0 &&
            Array.from({ length: limit ?? 6 }).map((_, i) => (
              <tr key={i} className="border-b border-line last:border-0">
                <td colSpan={9} className="px-5 py-4">
                  {loaded ? <span className="text-ink-3">Market data unavailable right now.</span> : <Skeleton className="h-5 w-full" />}
                </td>
              </tr>
            ))}
          {rows.map((a) => (
            <tr
              key={a.id}
              onClick={onSelect ? () => onSelect(a) : undefined}
              className={cx(
                "border-b border-line transition-colors last:border-0 hover:bg-paper",
                onSelect && "cursor-pointer",
                selected === a.id && "bg-forest-50/60 hover:bg-forest-50/60",
              )}
            >
              <td className="tnum py-3.5 pr-2 pl-5 text-ink-3">{a.rank ?? "—"}</td>
              <td className="py-3.5 pr-4">
                <span className="flex items-center gap-3">
                  <CoinIcon src={a.image} alt={a.symbol} />
                  <span className="min-w-0">
                    <span className="block font-semibold">{a.name}</span>
                    <span className="block text-xs text-ink-3">{a.symbol}</span>
                  </span>
                  {a.id === "zcash" && <span className="rounded bg-gold-50 px-1.5 py-0.5 text-[10px] font-semibold text-gold-600">SETTLED</span>}
                </span>
              </td>
              <td className="tnum py-3.5 pr-4 text-right font-medium">
                <FlashPrice value={a.price} />
              </td>
              <td className="hidden py-3.5 pr-4 text-right md:table-cell">
                <Change value={a.change1h} />
              </td>
              <td className="py-3.5 pr-4 text-right">
                <Change value={a.change24h} />
              </td>
              <td className="hidden py-3.5 pr-4 text-right md:table-cell">
                <Change value={a.change7d} />
              </td>
              <td className="tnum hidden py-3.5 pr-4 text-right text-ink-2 lg:table-cell">{usd(a.volume24h, { compact: true })}</td>
              <td className="tnum hidden py-3.5 pr-4 text-right text-ink-2 sm:table-cell">{usd(a.marketCap, { compact: true })}</td>
              <td className="hidden py-2 pr-5 lg:table-cell">
                <span className="flex justify-end">
                  <Sparkline data={a.sparkline7d} up={(a.change7d ?? 0) >= 0} />
                </span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {limit && market.length > limit && (
        <div className="border-t border-line px-5 py-3 text-right">
          <Link href="/app/markets" className="text-sm font-medium text-forest hover:underline underline-offset-4">
            View all markets
          </Link>
        </div>
      )}
    </div>
  );
}
