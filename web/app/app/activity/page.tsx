"use client";

import { ExternalTx, RecentBlocks, TxKindBadge } from "@/components/chain";
import { EventsList, OrdersTable } from "@/components/network";
import { Card } from "@/components/ui";
import { ago, num, token } from "@/lib/format";
import { useLive, useNow } from "@/lib/live";

export default function ActivityPage() {
  const { network, zcash } = useLive();
  const now = useNow();
  const txs = zcash?.recentTxs ?? [];

  return (
    <div className="mx-auto max-w-[1400px] space-y-6">
      <Card title="Orders">
        <OrdersTable orders={network.orders} />
      </Card>

      <div className="grid gap-6 xl:grid-cols-[1fr_420px]">
        <Card title="Zcash mainnet transactions" action={<span className="text-xs text-ink-3">{zcash ? `via ${zcash.source}` : ""}</span>}>
          {txs.length === 0 ? (
            <p className="py-6 text-center text-sm text-ink-3">The transaction sample needs the Blockchair provider; it appears when that source responds.</p>
          ) : (
            <div className="scroll-x">
              <table className="w-full min-w-[620px] text-sm">
                <thead>
                  <tr className="border-b border-line text-left text-xs text-ink-3">
                    <th className="py-2.5 pr-4 font-medium">Transaction</th>
                    <th className="py-2.5 pr-4 font-medium">Type</th>
                    <th className="py-2.5 pr-4 text-right font-medium">Block</th>
                    <th className="py-2.5 pr-4 text-right font-medium">Transparent out</th>
                    <th className="py-2.5 text-right font-medium">Age</th>
                  </tr>
                </thead>
                <tbody>
                  {txs.map((t) => (
                    <tr key={t.hash} className="border-b border-line last:border-0">
                      <td className="py-2.5 pr-4">
                        <ExternalTx hash={t.hash} />
                      </td>
                      <td className="py-2.5 pr-4">
                        <TxKindBadge kind={t.kind} />
                      </td>
                      <td className="tnum py-2.5 pr-4 text-right">{num(t.height)}</td>
                      <td className="tnum py-2.5 pr-4 text-right text-ink-2">{t.outputCount ? `${token(t.transparentOutZat / 1e8, 4)} ZEC` : "—"}</td>
                      <td className="tnum py-2.5 text-right text-ink-3">{ago(t.time, now)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Card>
        <div className="space-y-6">
          <Card title="Network events">
            <EventsList />
          </Card>
          <Card title="Latest Zcash blocks">
            <RecentBlocks />
          </Card>
        </div>
      </div>
    </div>
  );
}
