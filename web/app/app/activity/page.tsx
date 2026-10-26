"use client";

import { LoaderCircle, Wallet } from "lucide-react";
import { useEffect, useState } from "react";
import { ExternalTx, RecentBlocks, TxKindBadge } from "@/components/chain";
import { EventsList, OrdersTable } from "@/components/network";
import { buttonClass, Card, EmptyState, Mono } from "@/components/ui";
import { api } from "@/lib/api";
import { ago, num, token } from "@/lib/format";
import { useLive, useNow } from "@/lib/live";
import { myOrderIds } from "@/lib/my-orders";
import { useNearWallet } from "@/lib/near-wallet";
import type { Order } from "@/lib/types";

/**
 * The visitor's own orders: those opened in this browser, plus, with a NEAR wallet connected,
 * every order paid to or locked by that account. Only these link to their order pages.
 */
function YourOrders() {
  const { near } = useLive();
  const wallet = useNearWallet(near?.network === "mainnet" ? "mainnet" : "testnet");
  const [orders, setOrders] = useState<Order[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      const local = await Promise.all(myOrderIds().map((id) => api.order(id).catch(() => null)));
      const mine = wallet.accountId ? await api.accountOrders(wallet.accountId).catch(() => []) : [];
      const byId = new Map<string, Order>();
      for (const o of [...mine, ...local]) if (o) byId.set(o.id, o);
      if (!cancelled) setOrders([...byId.values()].sort((a, b) => b.createdAt.localeCompare(a.createdAt)));
    };
    void load();
    const timer = setInterval(load, 20_000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [wallet.accountId]);

  return (
    <Card
      title="Your orders"
      action={
        wallet.accountId ? (
          <button className="text-xs text-ink-3 hover:text-ink" onClick={() => void wallet.disconnect()}>
            <Mono>{wallet.accountId}</Mono> · Disconnect
          </button>
        ) : (
          <button className={buttonClass.ghost} onClick={() => void wallet.connect()} disabled={!wallet.ready}>
            <Wallet className="size-4" /> Connect NEAR wallet
          </button>
        )
      }
    >
      {orders === null ? (
        <div className="flex items-center gap-2 py-6 text-sm text-ink-3">
          <LoaderCircle className="size-4 animate-spin" /> Loading your orders…
        </div>
      ) : orders.length === 0 ? (
        <EmptyState title="No orders yet">
          Orders you open in this browser show up here. Connect a NEAR wallet to also see every order paid to or locked by your account.
        </EmptyState>
      ) : (
        <OrdersTable orders={orders} linkable />
      )}
    </Card>
  );
}

export default function ActivityPage() {
  const { network, zcash } = useLive();
  const now = useNow();
  const txs = zcash?.recentTxs ?? [];

  return (
    <div className="mx-auto max-w-[1400px] space-y-6">
      <YourOrders />

      <Card title="Network activity" action={<span className="text-xs text-ink-3">Recent swaps; details stay private to each order</span>}>
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
