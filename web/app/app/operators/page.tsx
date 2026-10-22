"use client";

import { KeyRound, Landmark, Server, ShieldCheck } from "lucide-react";
import { CoordinatorEmpty, EventsList, OperatorsTable, operatorSummary, SigMeter } from "@/components/network";
import { Badge, Card, Mono, Stat } from "@/components/ui";
import { ago, usd } from "@/lib/format";
import { useLive, useNow } from "@/lib/live";

/** Architecture from the PRD: t-of-n operators sign for both the Zcash treasury and the NEAR vault. */
function FlowDiagram({ t, n }: { t: number; n: number }) {
  const ops = Array.from({ length: n }, (_, i) => i);
  const w = 360;
  const span = Math.min(80, 260 / Math.max(n - 1, 1));
  const x0 = w / 2 - (span * (n - 1)) / 2;
  return (
    <svg viewBox={`0 0 ${w} 230`} className="w-full" role="img" aria-label={`${n} operators, ${t} of whom must sign, control the Zcash treasury and the NEAR vault`}>
      <text x={w / 2} y="16" textAnchor="middle" className="fill-ink-3 text-[11px]">
        Operators ({t}-of-{n})
      </text>
      {ops.map((i) => (
        <g key={i}>
          <line x1={x0 + i * span} y1="48" x2="70" y2="168" stroke="var(--color-line-strong)" strokeDasharray="3 3" />
          <line x1={x0 + i * span} y1="48" x2="290" y2="168" stroke="var(--color-line-strong)" strokeDasharray="3 3" />
        </g>
      ))}
      {ops.map((i) => (
        <g key={i} transform={`translate(${x0 + i * span}, 40)`}>
          <circle r="16" fill="var(--color-forest-50)" stroke="var(--color-forest)" strokeWidth="1.5" />
          <text textAnchor="middle" y="4" className="fill-forest text-[11px] font-semibold">
            {i + 1}
          </text>
        </g>
      ))}
      <line x1="96" y1="186" x2="264" y2="186" stroke="var(--color-ink-3)" markerEnd="url(#arrow)" />
      <defs>
        <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
          <path d="M0,0 L10,5 L0,10 z" fill="var(--color-ink-3)" />
        </marker>
      </defs>
      <g transform="translate(70, 186)">
        <circle r="22" fill="var(--color-gold-50)" stroke="var(--color-gold)" strokeWidth="1.5" />
        <text textAnchor="middle" y="4" className="fill-gold-600 text-[11px] font-bold">
          ZEC
        </text>
        <text textAnchor="middle" y="40" className="fill-ink text-[11px] font-medium">
          Ironwood treasury
        </text>
      </g>
      <g transform="translate(290, 186)">
        <circle r="22" fill="var(--color-sunken)" stroke="var(--color-ink-2)" strokeWidth="1.5" />
        <text textAnchor="middle" y="4" className="fill-ink text-[11px] font-bold">
          NEAR
        </text>
        <text textAnchor="middle" y="40" className="fill-ink text-[11px] font-medium">
          Vault + bonds
        </text>
      </g>
    </svg>
  );
}

export default function OperatorsPage() {
  const { network } = useLive();
  const now = useNow();
  const health = operatorSummary(network.operators);
  const bonded = network.treasury?.totalBondedUsd ?? (network.operators.some((o) => o.bondUsd != null) ? network.operators.reduce((s, o) => s + (o.bondUsd ?? 0), 0) : null);
  const { t, n } = network.threshold;

  return (
    <div className="mx-auto max-w-[1400px] space-y-6">
      <div>
        <h2 className="text-xl font-semibold tracking-tight">A bonded, threshold-signed network</h2>
        <p className="mt-1 max-w-2xl text-sm text-ink-3">
          Every settlement needs {t} of {n} operators to sign. The coordinator assembles spends but holds no key share.
        </p>
      </div>

      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <Stat icon={<Server className="size-5" />} label="Operators online" value={health.value} sub={health.note ?? undefined} />
        <Stat icon={<KeyRound className="size-5" />} label="Signing threshold" value={`${t} of ${n}`} sub={`Settles with up to ${n - t} offline`} />
        <Stat icon={<Landmark className="size-5" />} label="Total bonded" value={usd(bonded)} sub={network.bonds ? network.bonds.contract : "Bond contract not configured"} />
        <Stat
          icon={<ShieldCheck className="size-5" />}
          label="Coordinator"
          value={network.coordinator.online ? "Online" : network.coordinator.configured ? "Unreachable" : "Not connected"}
          sub={network.coordinator.online ? `${network.coordinator.latencyMs} ms` : undefined}
        />
      </div>

      <div className="grid gap-6 xl:grid-cols-[1fr_400px]">
        <Card title="Operators">
          <OperatorsTable />
        </Card>
        <Card title="Network flow">
          <FlowDiagram t={t} n={n} />
          <p className="mt-3 text-xs leading-relaxed text-ink-3">
            Each operator re-derives a spend&apos;s outputs from the approved order before signing. Signing anything that doesn&apos;t match is slashable.
          </p>
        </Card>
      </div>

      <div className="grid gap-6 xl:grid-cols-[1fr_400px]">
        <Card title="Signing rounds">
          {!network.coordinator.online ? (
            <CoordinatorEmpty what="Signing rounds" />
          ) : network.rounds.length === 0 ? (
            <p className="py-6 text-center text-sm text-ink-3">No signing rounds yet.</p>
          ) : (
            <div className="scroll-x">
              <table className="w-full min-w-[600px] text-sm">
                <thead>
                  <tr className="border-b border-line text-left text-xs text-ink-3">
                    <th className="py-2.5 pr-4 font-medium">Round</th>
                    <th className="py-2.5 pr-4 font-medium">Kind</th>
                    <th className="py-2.5 pr-4 font-medium">Signers</th>
                    <th className="py-2.5 pr-4 font-medium">Progress</th>
                    <th className="py-2.5 text-right font-medium">Started</th>
                  </tr>
                </thead>
                <tbody>
                  {network.rounds.map((r) => (
                    <tr key={r.id} className="border-b border-line last:border-0">
                      <td className="py-3 pr-4">
                        <Mono>{r.id.slice(0, 10)}</Mono>
                      </td>
                      <td className="py-3 pr-4">
                        <Badge tone={r.kind === "dkg" ? "gold" : r.kind === "refund" ? "warn" : "brand"}>{r.kind.replace("_", " ")}</Badge>
                      </td>
                      <td className="py-3 pr-4 text-ink-2">{r.signers.join(", ") || "—"}</td>
                      <td className="py-3 pr-4">
                        <SigMeter collected={r.collected} threshold={r.threshold} />
                      </td>
                      <td className="tnum py-3 text-right text-ink-3">{ago(r.startedAt, now)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Card>
        <Card title="Recent events">
          <EventsList limit={8} />
        </Card>
      </div>
    </div>
  );
}
