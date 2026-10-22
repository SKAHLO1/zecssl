"use client";

import { CircleAlert, Repeat, Server, Signature, TriangleAlert } from "lucide-react";
import { ago, num, shortHash, token, usd } from "@/lib/format";
import { useLive, useNow } from "@/lib/live";
import type { NetworkEvent, Order, OrderState } from "@/lib/types";
import { Badge, EmptyState, Mono, StatusDot } from "./ui";

const orderTone: Record<OrderState, Parameters<typeof Badge>[0]["tone"]> = {
  quoted: "neutral",
  awaiting_deposit: "neutral",
  deposit_detected: "brand",
  confirming: "brand",
  signing: "gold",
  released: "up",
  refunded: "warn",
  expired: "neutral",
  disputed: "down",
  compensated: "warn",
  locked: "neutral",
  claimed: "gold",
  settled: "up",
  reimbursed: "warn",
};

const orderLabel: Record<OrderState, string> = {
  quoted: "Quoted",
  awaiting_deposit: "Awaiting deposit",
  deposit_detected: "Deposit detected",
  confirming: "Confirming",
  signing: "Signing",
  released: "Settled",
  refunded: "Refunded",
  expired: "Expired",
  disputed: "Disputed",
  compensated: "Compensated",
  locked: "Escrowed",
  claimed: "Claimed",
  settled: "Settled",
  reimbursed: "Reimbursed",
};

export function OrderBadge({ state }: { state: OrderState }) {
  return (
    <Badge tone={orderTone[state]} dot>
      {orderLabel[state]}
    </Badge>
  );
}

/** Shown wherever coordinator data would go but no coordinator is configured or reachable. */
export function CoordinatorEmpty({ what }: { what: string }) {
  const { network } = useLive();
  if (network.coordinator.configured && !network.coordinator.online) {
    return (
      <EmptyState icon={<TriangleAlert className="size-5" />} title="Coordinator unreachable">
        The API can&apos;t reach the coordinator at <Mono>{network.coordinator.url}</Mono>. {what} will appear when it responds.
      </EmptyState>
    );
  }
  if (!network.coordinator.configured) {
    return (
      <EmptyState icon={<Server className="size-5" />} title="Coordinator not connected">
        {what} come from the ZEC SSL coordinator. Set <Mono>COORDINATOR_URL</Mono> on the API server to stream them here.
      </EmptyState>
    );
  }
  return null;
}

export function OrdersTable({ orders, limit }: { orders: Order[]; limit?: number }) {
  const now = useNow();
  const { network } = useLive();
  const rows = limit ? orders.slice(0, limit) : orders;
  // Orders come from the coordinator, or straight from the vault contract without it.
  if (!network.coordinator.online && !network.vault?.exists) return <CoordinatorEmpty what="Orders" />;
  if (!rows.length) {
    return (
      <EmptyState icon={<Repeat className="size-5" />} title="No orders yet">
        New swaps will show up here as soon as operators approve them.
      </EmptyState>
    );
  }
  return (
    <div className="scroll-x">
      <table className="w-full min-w-[640px] text-sm">
        <thead>
          <tr className="border-b border-line text-left text-xs text-ink-3">
            <th className="py-2.5 pr-4 font-medium">Order</th>
            <th className="py-2.5 pr-4 font-medium">Route</th>
            <th className="py-2.5 pr-4 text-right font-medium">Amount</th>
            <th className="py-2.5 pr-4 font-medium">Signatures</th>
            <th className="py-2.5 pr-4 font-medium">Status</th>
            <th className="py-2.5 text-right font-medium">Updated</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((o) => (
            <tr key={o.id} className="border-b border-line last:border-0">
              <td className="py-3 pr-4">
                <Mono>{shortHash(o.id, 8, 4)}</Mono>
              </td>
              <td className="py-3 pr-4">{o.direction === "zec_to_near" ? `ZEC → ${o.destinationSymbol}` : `${o.destinationSymbol} → ZEC`}</td>
              <td className="tnum py-3 pr-4 text-right">
                {token(o.amountInZec, 4)} ZEC
                <div className="text-xs text-ink-3">
                  {token(o.amountOut, 2)} {o.destinationSymbol}
                </div>
              </td>
              <td className="py-3 pr-4">
                <SigMeter collected={o.signatures} threshold={o.threshold} />
              </td>
              <td className="py-3 pr-4">
                <OrderBadge state={o.state} />
              </td>
              <td className="tnum py-3 text-right text-ink-3">{ago(o.updatedAt, now)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function SigMeter({ collected, threshold }: { collected: number; threshold: number }) {
  return (
    <span className="inline-flex items-center gap-2">
      <span className="flex gap-1" aria-hidden>
        {Array.from({ length: threshold }).map((_, i) => (
          <span key={i} className={i < collected ? "h-1.5 w-4 rounded-full bg-forest" : "h-1.5 w-4 rounded-full bg-line-strong"} />
        ))}
      </span>
      <span className="tnum text-xs text-ink-2">
        {collected}/{threshold}
      </span>
    </span>
  );
}

const eventIcon: Record<NetworkEvent["kind"], { tone: string; icon: typeof Repeat }> = {
  order: { tone: "bg-forest-50 text-forest", icon: Repeat },
  signing: { tone: "bg-gold-50 text-gold-600", icon: Signature },
  deposit: { tone: "bg-up-50 text-up", icon: Repeat },
  release: { tone: "bg-up-50 text-up", icon: Repeat },
  refund: { tone: "bg-warn-50 text-warn", icon: Repeat },
  slash: { tone: "bg-down-50 text-down", icon: TriangleAlert },
  operator: { tone: "bg-sunken text-ink-2", icon: Server },
};

export function EventsList({ limit }: { limit?: number }) {
  const { network } = useLive();
  const now = useNow();
  if (!network.coordinator.online) return <CoordinatorEmpty what="Network events" />;
  const events = limit ? network.events.slice(0, limit) : network.events;
  if (!events.length) return <EmptyState icon={<CircleAlert className="size-5" />} title="No events yet" />;
  return (
    <ul className="divide-y divide-line">
      {events.map((e) => {
        const { tone, icon: Icon } = eventIcon[e.kind];
        return (
          <li key={e.id} className="flex items-start gap-3 py-3">
            <span className={`grid size-9 shrink-0 place-items-center rounded-full ${tone}`}>
              <Icon className="size-4" />
            </span>
            <span className="min-w-0 flex-1">
              <span className="block text-sm font-medium">{e.title}</span>
              <span className="block truncate text-xs text-ink-3">{e.detail}</span>
            </span>
            <span className="tnum shrink-0 text-xs text-ink-3">{ago(e.at, now)}</span>
          </li>
        );
      })}
    </ul>
  );
}

export function OperatorsTable() {
  const { network } = useLive();
  const now = useNow();
  if (!network.operators.length) {
    return (
      <EmptyState icon={<Server className="size-5" />} title="No operator endpoints configured">
        List each operator node in <Mono>OPERATOR_ENDPOINTS</Mono> on the API server (for example <Mono>op-1=https://op1.example:7000</Mono>). The API polls each
        node&apos;s <Mono>/health</Mono> every 10 seconds.
      </EmptyState>
    );
  }
  return (
    <div className="scroll-x">
      <table className="w-full min-w-[720px] text-sm">
        <thead>
          <tr className="border-b border-line text-left text-xs text-ink-3">
            <th className="py-2.5 pr-4 font-medium">Operator</th>
            <th className="py-2.5 pr-4 font-medium">Status</th>
            <th className="py-2.5 pr-4 text-right font-medium">Bonded</th>
            <th className="py-2.5 pr-4 text-right font-medium">Signatures</th>
            <th className="py-2.5 pr-4 text-right font-medium">Latency</th>
            <th className="py-2.5 pr-4 font-medium">Version</th>
            <th className="py-2.5 text-right font-medium">Last seen</th>
          </tr>
        </thead>
        <tbody>
          {network.operators.map((o) => (
            <tr key={o.id} className="border-b border-line last:border-0">
              <td className="py-3 pr-4">
                <div className="font-medium">{o.name}</div>
                <div className="text-xs text-ink-3">{o.nearAccount ?? o.endpoint}</div>
              </td>
              <td className="py-3 pr-4">
                <span className="inline-flex items-center gap-2">
                  <StatusDot state={o.state === "online" ? "ok" : o.state === "degraded" ? "warn" : o.state === "unmonitored" ? "off" : "down"} />
                  <span className="capitalize">{o.state}</span>
                  {o.active === false && <Badge tone="warn">Removed</Badge>}
                  {(o.equivocations ?? 0) > 0 && <Badge tone="down">{o.equivocations} equivocation{o.equivocations === 1 ? "" : "s"}</Badge>}
                </span>
              </td>
              <td className="tnum py-3 pr-4 text-right">{usd(o.bondUsd)}</td>
              <td className="tnum py-3 pr-4 text-right">{num(o.signaturesTotal)}</td>
              <td className="tnum py-3 pr-4 text-right">{o.latencyMs != null ? `${o.latencyMs} ms` : "—"}</td>
              <td className="py-3 pr-4">
                <Mono>{o.version ?? "—"}</Mono>
              </td>
              <td className="tnum py-3 text-right text-ink-3">{ago(o.lastSeen, now) || "—"}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
