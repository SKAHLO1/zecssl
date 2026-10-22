"use client";

import { Blocks, ChartLine, Network, Route } from "lucide-react";
import Link from "next/link";
import { RecentBlocks, SourcesList, TxMix } from "@/components/chain";
import { EventsList, OrdersTable } from "@/components/network";
import { PriceChart } from "@/components/price-chart";
import { QuickSwap } from "@/components/quick-swap";
import { Card, Change, Sparkline, Stat } from "@/components/ui";
import { duration, num, price } from "@/lib/format";
import { useLive } from "@/lib/live";

export default function DashboardPage() {
  const { market, zcash, near, intents, network } = useLive();
  const zec = market.find((a) => a.id === "zcash");
  const online = network.operators.filter((o) => o.state !== "offline").length;

  return (
    <div className="mx-auto max-w-[1400px] space-y-6">
      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <div className="card flex items-center justify-between gap-3 p-4">
          <div>
            <div className="text-[13px] text-ink-3">ZEC / USD</div>
            <div className="tnum mt-0.5 text-xl font-semibold tracking-tight">{price(zec?.price)}</div>
            <div className="mt-0.5 flex items-center gap-1.5 text-xs">
              <Change value={zec?.change24h} /> <span className="text-ink-3">24h</span>
            </div>
          </div>
          {zec && <Sparkline data={zec.sparkline7d} up={(zec.change7d ?? 0) >= 0} width={110} height={40} />}
        </div>
        <Stat icon={<Blocks className="size-5" />} label="Zcash height" value={num(zcash?.height)} sub={`~${duration(zcash?.avgBlockTimeSec)} blocks · ${num(zcash?.mempoolTxs)} in mempool`} />
        <Stat icon={<ChartLine className="size-5" />} label={`NEAR ${near?.network ?? ""} height`} value={num(near?.height)} sub={near ? `Protocol v${near.protocolVersion ?? "—"}` : undefined} />
        <Stat
          icon={<Network className="size-5" />}
          label="Operators online"
          value={network.operators.length ? `${online} / ${network.operators.length}` : "—"}
          sub={`${network.threshold.t}-of-${network.threshold.n} threshold${network.operators.length ? "" : " · not connected"}`}
        />
      </div>

      <div className="grid gap-6 xl:grid-cols-[1fr_380px]">
        <Card>
          <PriceChart id="zcash" name="Zcash" livePrice={zec?.price} />
        </Card>
        <QuickSwap />
      </div>

      <div className="grid gap-6 xl:grid-cols-[1fr_380px]">
        <Card
          title="Recent orders"
          action={
            <Link href="/app/activity" className="text-sm font-medium text-forest hover:underline">
              View all
            </Link>
          }
        >
          <OrdersTable orders={network.orders} limit={6} />
        </Card>
        <Card
          title="Data sources"
          action={
            <Link href="/app/status" className="text-sm font-medium text-forest hover:underline">
              Status
            </Link>
          }
        >
          <SourcesList compact />
        </Card>
      </div>

      <div className="grid gap-6 lg:grid-cols-2 xl:grid-cols-3">
        <Card title="Latest Zcash blocks" className="xl:col-span-1">
          <RecentBlocks limit={6} />
        </Card>
        <Card title="Zcash transaction mix" action={<span className="text-xs text-ink-3">mainnet sample</span>}>
          <TxMix zcash={zcash} />
        </Card>
        <Card
          title="Network events"
          className="lg:col-span-2 xl:col-span-1"
          action={
            <span className="inline-flex items-center gap-1.5 text-xs text-ink-3">
              <Route className="size-3.5" /> {num(intents?.tokens.length)} routable assets
            </span>
          }
        >
          <EventsList limit={5} />
        </Card>
      </div>
    </div>
  );
}
