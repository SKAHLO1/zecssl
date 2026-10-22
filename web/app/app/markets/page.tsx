"use client";

import { useState } from "react";
import { MarketsTable } from "@/components/markets-table";
import { PriceChart } from "@/components/price-chart";
import { Card, CoinIcon } from "@/components/ui";
import { num, price, usd } from "@/lib/format";
import { useLive } from "@/lib/live";

export default function MarketsPage() {
  const { market } = useLive();
  const [selected, setSelected] = useState("zcash");
  const asset = market.find((a) => a.id === selected) ?? market[0];

  const stats: [string, string][] = asset
    ? [
        ["Market cap", usd(asset.marketCap, { compact: true })],
        ["24h volume", usd(asset.volume24h, { compact: true })],
        ["24h low / high", `${price(asset.low24h)} / ${price(asset.high24h)}`],
        ["All-time high", price(asset.ath)],
        ["Circulating supply", `${num(asset.circulatingSupply, { compact: true })} ${asset.symbol}`],
        ["Market cap rank", asset.rank ? `#${asset.rank}` : "—"],
      ]
    : [];

  return (
    <div className="mx-auto max-w-[1400px] space-y-6">
      <div className="grid gap-6 xl:grid-cols-[1fr_340px]">
        <Card>
          {asset ? (
            <>
              <div className="mb-4 flex items-center gap-3">
                <CoinIcon src={asset.image} alt={asset.symbol} size={36} />
                <div>
                  <div className="text-lg font-semibold">{asset.name}</div>
                  <div className="text-xs text-ink-3">{asset.symbol} / USD</div>
                </div>
              </div>
              <PriceChart key={asset.id} id={asset.id} name={asset.name} livePrice={asset.price} />
            </>
          ) : (
            <div className="grid h-80 place-items-center text-sm text-ink-3">Waiting for market data…</div>
          )}
        </Card>
        <Card title="Market stats">
          <dl className="divide-y divide-line text-sm">
            {stats.map(([k, v]) => (
              <div key={k} className="flex justify-between gap-4 py-3">
                <dt className="text-ink-3">{k}</dt>
                <dd className="tnum text-right font-medium">{v}</dd>
              </div>
            ))}
          </dl>
          <p className="mt-3 text-xs text-ink-3">Data from CoinGecko, refreshed every minute by the ZEC SSL API.</p>
        </Card>
      </div>
      <div className="card overflow-hidden">
        <MarketsTable onSelect={(a) => setSelected(a.id)} selected={asset?.id} />
      </div>
    </div>
  );
}
