"use client";

import { ArrowLeft, Check, Copy, ExternalLink, LoaderCircle, TriangleAlert } from "lucide-react";
import Link from "next/link";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";
import { DepositQr } from "@/components/deposit-qr";
import { OrderBadge } from "@/components/network";
import { Card, cx, EmptyState, Mono } from "@/components/ui";
import { api } from "@/lib/api";
import { explorer, netOf } from "@/lib/explorers";
import { token } from "@/lib/format";
import { useLive, useNow } from "@/lib/live";
import type { NetworkEvent, OrderDetail } from "@/lib/types";

const FINAL = ["released", "refunded", "expired", "compensated"];

/** The milestones of a shielded-ZEC-in order, and the event title that marks each. */
const milestones: { label: string; match: RegExp }[] = [
  { label: "Order opened on the vault", match: / opened$/ },
  { label: "Deposit seen on Zcash", match: /^Deposit (seen|confirming)/ },
  { label: "Deposit confirmed by operators", match: /^Deposit confirmed/ },
  { label: "Released on NEAR", match: / settled$/ },
];

function base64url(s: string): string {
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function CopyButton({ text, label }: { text: string; label: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      className="shrink-0 rounded-lg p-1.5 hover:bg-surface"
      onClick={() => {
        void navigator.clipboard.writeText(text);
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      }}
      aria-label={label}
    >
      {copied ? <Check className="size-4 text-up" /> : <Copy className="size-4" />}
    </button>
  );
}

function Timeline({ events, state }: { events: NetworkEvent[]; state: string }) {
  const at = (re: RegExp) => events.find((e) => re.test(e.title))?.at ?? null;
  const reached = milestones.map((m) => ({ ...m, at: at(m.match) }));
  const failed = state === "refunded" || state === "expired" || state === "disputed";
  return (
    <ol className="relative space-y-5 border-l border-line pl-6">
      {reached.map((m, i) => {
        const done = Boolean(m.at) || (state === "released" && i < reached.length);
        const current = !done && (i === 0 || Boolean(reached[i - 1].at));
        return (
          <li key={m.label} className="relative">
            <span
              className={cx(
                "absolute -left-[31px] grid size-4 place-items-center rounded-full border-2",
                done ? "border-forest bg-forest" : current && !failed ? "border-forest bg-surface" : "border-line-strong bg-surface",
              )}
            >
              {done && <Check className="size-2.5 text-white" strokeWidth={4} />}
            </span>
            <div className={cx("text-sm", done ? "font-semibold" : "text-ink-3")}>{m.label}</div>
            {m.at && <div className="tnum text-xs text-ink-3">{new Date(m.at).toLocaleString()}</div>}
            {current && !failed && !FINAL.includes(state) && (
              <div className="mt-1 inline-flex items-center gap-1.5 text-xs text-forest">
                <LoaderCircle className="size-3 animate-spin" /> In progress
              </div>
            )}
          </li>
        );
      })}
      {failed && (
        <li className="relative">
          <span className="absolute -left-[31px] grid size-4 place-items-center rounded-full border-2 border-warn bg-warn" />
          <div className="text-sm font-semibold text-warn">Order {state}</div>
        </li>
      )}
    </ol>
  );
}

function Approvals({ title, accounts, threshold, net }: { title: string; accounts: string[]; threshold: number; net: ReturnType<typeof netOf> }) {
  return (
    <div>
      <div className="flex items-baseline justify-between">
        <span className="text-sm font-medium">{title}</span>
        <span className="tnum text-xs text-ink-3">
          {accounts.length} / {threshold}
        </span>
      </div>
      <ul className="mt-2 space-y-1.5">
        {accounts.length === 0 && <li className="text-xs text-ink-3">None yet</li>}
        {accounts.map((a) => (
          <li key={a} className="flex items-center gap-2 text-sm">
            <Check className="size-3.5 text-up" />
            <a href={explorer.nearAccount(net, a)} target="_blank" rel="noreferrer" className="truncate hover:underline">
              {a}
            </a>
          </li>
        ))}
      </ul>
    </div>
  );
}

export default function OrderPage() {
  const { id } = useParams<{ id: string }>();
  const { near } = useLive();
  const net = netOf(near?.network);
  const now = useNow(1000);
  const [order, setOrder] = useState<OrderDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const o = await api.order(id);
        if (cancelled) return;
        setOrder(o);
        setError(null);
        if (FINAL.includes(o.state)) return;
      } catch (e) {
        if (!cancelled) setError((e as Error).message);
      }
      timer = setTimeout(poll, 8_000);
    };
    void poll();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [id]);

  if (error && !order) {
    return (
      <div className="mx-auto max-w-3xl">
        <EmptyState icon={<TriangleAlert className="size-5" />} title={`Order ${id} not found`}>
          {error}
        </EmptyState>
      </div>
    );
  }
  if (!order) {
    return <div className="mx-auto grid h-64 max-w-3xl place-items-center text-sm text-ink-3">Loading order…</div>;
  }

  const awaiting = ["quoted", "awaiting_deposit", "deposit_detected", "confirming"].includes(order.state);
  const uri = order.depositAddress ? `zcash:${order.depositAddress}?amount=${order.amountInZec}&memo=${base64url(order.id)}` : null;
  const secondsLeft = Math.max(0, Math.round((Date.parse(order.expiresAt) - now) / 1000));
  const shareUrl = typeof window !== "undefined" ? window.location.href : "";

  return (
    <div className="mx-auto max-w-[1100px] space-y-6">
      <Link href="/app/activity" className="inline-flex items-center gap-1.5 text-sm text-ink-3 hover:text-ink">
        <ArrowLeft className="size-4" /> All orders
      </Link>

      <div className="card flex flex-wrap items-center justify-between gap-4 p-5">
        <div>
          <div className="flex items-center gap-3">
            <h2 className="font-mono text-xl font-semibold">{order.id}</h2>
            <OrderBadge state={order.state} />
          </div>
          <p className="tnum mt-1 text-sm text-ink-2">
            {order.amountInZec} shielded ZEC → {order.amountOut} {order.destinationSymbol} on NEAR {net}
          </p>
        </div>
        <div className="flex items-center gap-2 rounded-xl bg-sunken/70 px-3 py-2 text-xs text-ink-2">
          Share this order
          <CopyButton text={shareUrl} label="Copy the order link" />
        </div>
      </div>

      <div className="grid gap-6 lg:grid-cols-[1fr_340px]">
        <div className="space-y-6">
          <Card title="Deposit">
            {awaiting && order.depositAddress && uri ? (
              <div className="flex flex-col gap-5 sm:flex-row">
                <DepositQr uri={uri} />
                <div className="min-w-0 flex-1 space-y-3 text-sm">
                  <div>
                    <div className="text-xs text-ink-3">Send exactly</div>
                    <div className="tnum text-lg font-semibold">{order.amountInZec} ZEC</div>
                  </div>
                  <div>
                    <div className="text-xs text-ink-3">to this shielded address</div>
                    <div className="mt-1 flex items-start gap-2 rounded-xl bg-sunken/70 p-2.5">
                      <Mono className="break-all">{order.depositAddress}</Mono>
                      <CopyButton text={order.depositAddress} label="Copy deposit address" />
                    </div>
                  </div>
                  <div className="flex flex-wrap gap-x-6 gap-y-1 text-ink-2">
                    <span>
                      Memo <Mono>{order.id}</Mono>
                    </span>
                    <span className="tnum">
                      {secondsLeft > 0 ? `${Math.floor(secondsLeft / 60)}:${String(secondsLeft % 60).padStart(2, "0")} left to deposit` : "Deposit window closed"}
                    </span>
                    <a href={uri} className="font-medium text-forest hover:underline">
                      Open in wallet
                    </a>
                  </div>
                </div>
              </div>
            ) : (
              <dl className="grid gap-4 text-sm sm:grid-cols-3">
                <div>
                  <dt className="text-xs text-ink-3">Received</dt>
                  <dd className="tnum font-medium">
                    {token(order.receivedZec)} / {order.amountInZec} ZEC
                  </dd>
                </div>
                <div className="sm:col-span-2">
                  <dt className="text-xs text-ink-3">Deposit address</dt>
                  <dd>
                    <Mono className="break-all">{order.depositAddress ?? "—"}</Mono>
                  </dd>
                </div>
              </dl>
            )}
            {order.depositTxids.length > 0 && (
              <div className="mt-4 border-t border-line pt-3 text-sm">
                <div className="text-xs text-ink-3">Deposit transactions (shielded)</div>
                {order.depositTxids.map((t) => (
                  <a key={t} href={explorer.zcashTx(net, t)} target="_blank" rel="noreferrer" className="mt-1 flex items-center gap-1.5 text-forest hover:underline">
                    <Mono>{`${t.slice(0, 16)}…${t.slice(-8)}`}</Mono>
                    <ExternalLink className="size-3" />
                  </a>
                ))}
              </div>
            )}
          </Card>

          <Card title="Event log">
            {order.events.length === 0 ? (
              <p className="text-sm text-ink-3">No events yet.</p>
            ) : (
              <ul className="divide-y divide-line">
                {order.events.map((e) => (
                  <li key={e.id} className="flex items-start justify-between gap-4 py-2.5 text-sm">
                    <span>
                      <span className="font-medium">{e.title}</span>
                      <span className="block text-xs text-ink-3">{e.detail}</span>
                    </span>
                    <span className="tnum shrink-0 text-xs text-ink-3">{new Date(e.at).toLocaleTimeString()}</span>
                  </li>
                ))}
              </ul>
            )}
          </Card>
        </div>

        <div className="space-y-6">
          <Card title="Progress">
            <Timeline events={order.events} state={order.state} />
          </Card>
          <Card title="On-chain approvals">
            {order.vault ? (
              <div className="space-y-5">
                <Approvals title="Opened by" accounts={order.vault.approvers} threshold={order.threshold} net={net} />
                <Approvals title="Released by" accounts={order.vault.releaseApprovals} threshold={order.threshold} net={net} />
                <a href={explorer.nearAccount(net, order.vault.contract)} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1.5 text-xs text-forest hover:underline">
                  Vault contract {order.vault.contract} <ExternalLink className="size-3" />
                </a>
              </div>
            ) : (
              <p className="text-sm text-ink-3">Vault contract not configured.</p>
            )}
          </Card>
          <Card title="Payout">
            <dl className="space-y-2 text-sm">
              <div className="flex justify-between gap-3">
                <dt className="text-ink-3">Recipient</dt>
                <dd className="truncate">
                  <a href={explorer.nearAccount(net, order.recipient)} target="_blank" rel="noreferrer" className="hover:underline">
                    {order.recipient}
                  </a>
                </dd>
              </div>
              <div className="flex justify-between gap-3">
                <dt className="text-ink-3">Amount</dt>
                <dd className="tnum font-medium">
                  {order.amountOut} {order.destinationSymbol}
                </dd>
              </div>
            </dl>
          </Card>
        </div>
      </div>
    </div>
  );
}
