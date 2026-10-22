"use client";

import { ExternalLink } from "lucide-react";
import { ago, bytes, num, share, shortHash } from "@/lib/format";
import { useLive, useNow } from "@/lib/live";
import type { SourceStatus, ZcashChain, ZcashTxKind } from "@/lib/types";
import { Badge, cx, Mono, Skeleton, StatusDot } from "./ui";

export const explorerTx = (hash: string) => `https://blockchair.com/zcash/transaction/${hash}`;
export const explorerBlock = (height: number) => `https://blockchair.com/zcash/block/${height}`;

export function RecentBlocks({ limit = 8 }: { limit?: number }) {
  const { zcash, loaded } = useLive();
  const now = useNow();
  const blocks = zcash?.recentBlocks.slice(0, limit) ?? [];
  return (
    <div className="scroll-x">
      <table className="w-full min-w-[420px] text-sm">
        <thead>
          <tr className="border-b border-line text-left text-xs text-ink-3">
            <th className="py-2.5 pr-4 font-medium">Block</th>
            <th className="py-2.5 pr-4 font-medium">Hash</th>
            <th className="py-2.5 pr-4 text-right font-medium">Txs</th>
            <th className="hidden py-2.5 pr-4 text-right font-medium sm:table-cell">Size</th>
            <th className="py-2.5 text-right font-medium">Age</th>
          </tr>
        </thead>
        <tbody>
          {blocks.length === 0 && (
            <tr>
              <td colSpan={5} className="py-6 text-center text-ink-3">
                {loaded ? "Waiting for the Zcash data provider…" : <Skeleton className="h-5 w-full" />}
              </td>
            </tr>
          )}
          {blocks.map((b) => (
            <tr key={b.height} className="border-b border-line last:border-0">
              <td className="py-2.5 pr-4">
                <a href={explorerBlock(b.height)} target="_blank" rel="noreferrer" className="tnum font-medium text-forest hover:underline">
                  {num(b.height)}
                </a>
              </td>
              <td className="py-2.5 pr-4 text-ink-2">
                <Mono>{shortHash(b.hash, 8, 6)}</Mono>
              </td>
              <td className="tnum py-2.5 pr-4 text-right">{b.txCount}</td>
              <td className="tnum hidden py-2.5 pr-4 text-right text-ink-2 sm:table-cell">{b.size ? bytes(b.size) : "—"}</td>
              <td className="tnum py-2.5 text-right text-ink-3">{ago(b.time, now)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

const kindMeta: Record<Exclude<ZcashTxKind, "coinbase">, { label: string; color: string; note: string }> = {
  shielded: { label: "Fully shielded", color: "var(--color-forest)", note: "No transparent inputs or outputs" },
  shielding: { label: "Shielding", color: "#5f8f6f", note: "Transparent in, shielded out" },
  deshielding: { label: "Deshielding", color: "var(--color-gold)", note: "Shielded in, transparent out: the public hop" },
  transparent: { label: "Transparent / mixed", color: "#b9b2a3", note: "Has transparent inputs and outputs" },
};

const kindOrder = ["shielded", "shielding", "deshielding", "transparent"] as const;

/** Stacked bar of the recent transaction sample by transparent footprint, with a legend. */
export function TxMix({ zcash }: { zcash: ZcashChain | null }) {
  const mix = zcash?.txMix;
  const sample = zcash?.recentTxs.filter((t) => t.kind !== "coinbase").length ?? 0;
  if (!mix) {
    return <div className="rounded-xl bg-sunken/60 px-4 py-6 text-center text-sm text-ink-3">The transaction sample needs the Blockchair provider; it will appear when that source is available.</div>;
  }
  return (
    <div>
      <div className="flex h-3 w-full gap-[2px] overflow-hidden rounded-full" role="img" aria-label={kindOrder.map((k) => `${kindMeta[k].label} ${share(mix[k])}`).join(", ")}>
        {kindOrder.map((k) => (mix[k] > 0 ? <span key={k} style={{ width: `${mix[k] * 100}%`, background: kindMeta[k].color }} /> : null))}
      </div>
      <ul className="mt-4 grid gap-2.5 sm:grid-cols-2">
        {kindOrder.map((k) => (
          <li key={k} className="flex items-start gap-2.5">
            <span className="mt-1 size-2.5 shrink-0 rounded-sm" style={{ background: kindMeta[k].color }} />
            <span className="min-w-0 flex-1">
              <span className="flex items-baseline justify-between gap-2 text-sm">
                <span className="font-medium">{kindMeta[k].label}</span>
                <span className="tnum font-semibold">{share(mix[k])}</span>
              </span>
              <span className="block text-xs text-ink-3">{kindMeta[k].note}</span>
            </span>
          </li>
        ))}
      </ul>
      <p className="mt-3 text-xs text-ink-3">Sample: the latest {sample} non-coinbase transactions on Zcash mainnet, classified by their transparent inputs and outputs.</p>
    </div>
  );
}

export function TxKindBadge({ kind }: { kind: ZcashTxKind }) {
  if (kind === "coinbase") return <Badge>Coinbase</Badge>;
  if (kind === "shielded") return <Badge tone="brand">Shielded</Badge>;
  if (kind === "shielding") return <Badge tone="up">Shielding</Badge>;
  if (kind === "deshielding") return <Badge tone="gold">Deshielding</Badge>;
  return <Badge>Transparent</Badge>;
}

export function sourceState(s: SourceStatus): "ok" | "warn" | "down" | "off" {
  if (!s.configured) return "off";
  if (s.ok) return "ok";
  return s.lastSuccess ? "warn" : "down";
}

export function SourcesList({ compact }: { compact?: boolean }) {
  const { sources, connection } = useLive();
  const now = useNow();
  return (
    <ul className="divide-y divide-line">
      <li className="flex items-center gap-3 py-2.5">
        <StatusDot state={connection === "live" ? "ok" : connection === "connecting" ? "warn" : "down"} />
        <span className="flex-1 text-sm font-medium">Live stream</span>
        <span className="text-xs text-ink-3">{connection === "live" ? "Connected" : connection === "connecting" ? "Connecting…" : "Disconnected"}</span>
      </li>
      {sources.map((s) => {
        const state = sourceState(s);
        return (
          <li key={s.name} className="flex items-center gap-3 py-2.5">
            <StatusDot state={state} />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-sm font-medium">{s.label}</span>
              {!compact && s.lastError && s.configured && <span className="block truncate text-xs text-down">{s.lastError}</span>}
            </span>
            <span className={cx("tnum shrink-0 text-xs", state === "off" ? "text-ink-3" : "text-ink-2")}>
              {state === "off" ? "Not configured" : s.lastSuccess ? `${s.latencyMs ?? "—"} ms · ${ago(s.lastSuccess, now)}` : "No response"}
            </span>
          </li>
        );
      })}
    </ul>
  );
}

export function ExternalTx({ hash }: { hash: string }) {
  return (
    <a href={explorerTx(hash)} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 text-forest hover:underline">
      <Mono>{shortHash(hash, 8, 6)}</Mono>
      <ExternalLink className="size-3" />
    </a>
  );
}
