"use client";

import { sourceState, SourcesList } from "@/components/chain";
import { Card, Mono } from "@/components/ui";
import { bytes, hashrate, num, shortHash } from "@/lib/format";
import { useLive } from "@/lib/live";

export default function StatusPage() {
  const { sources, zcash, near, intents } = useLive();
  const configured = sources.filter((s) => s.configured);
  const healthy = configured.filter((s) => sourceState(s) === "ok").length;
  const allGood = configured.length > 0 && healthy === configured.length;

  return (
    <div className="mx-auto max-w-[1100px] space-y-6">
      <div className={`card flex items-center gap-4 p-5 ${allGood ? "border-up/30 bg-up-50" : "border-warn/30 bg-warn-50"}`}>
        <span className={`size-3 rounded-full ${allGood ? "bg-up live-dot" : "bg-warn"}`} />
        <div>
          <div className="font-semibold">{allGood ? "All configured sources are operational" : `${healthy} of ${configured.length} configured sources are healthy`}</div>
          <div className="text-sm text-ink-2">Unconfigured sources are ZEC SSL services not yet deployed; they light up when their endpoints are set on the API server.</div>
        </div>
      </div>

      <Card title="Data sources">
        <SourcesList />
      </Card>

      <div className="grid gap-6 md:grid-cols-3">
        <Card title="Zcash">
          <dl className="space-y-2 text-sm">
            <div className="flex justify-between"><dt className="text-ink-3">Provider</dt><dd className="font-medium capitalize">{zcash?.source ?? "—"}</dd></div>
            <div className="flex justify-between"><dt className="text-ink-3">Height</dt><dd className="tnum font-medium">{num(zcash?.height)}</dd></div>
            <div className="flex justify-between"><dt className="text-ink-3">Best block</dt><dd><Mono>{shortHash(zcash?.bestBlockHash, 8, 6)}</Mono></dd></div>
            <div className="flex justify-between"><dt className="text-ink-3">Hashrate</dt><dd className="tnum font-medium">{hashrate(zcash?.hashrate)}</dd></div>
            <div className="flex justify-between"><dt className="text-ink-3">Nodes</dt><dd className="tnum font-medium">{num(zcash?.nodes)}</dd></div>
            <div className="flex justify-between"><dt className="text-ink-3">Chain size</dt><dd className="tnum font-medium">{bytes(zcash?.blockchainSizeBytes)}</dd></div>
          </dl>
        </Card>
        <Card title="NEAR">
          <dl className="space-y-2 text-sm">
            <div className="flex justify-between"><dt className="text-ink-3">Network</dt><dd className="font-medium capitalize">{near?.network ?? "—"}</dd></div>
            <div className="flex justify-between"><dt className="text-ink-3">Height</dt><dd className="tnum font-medium">{num(near?.height)}</dd></div>
            <div className="flex justify-between"><dt className="text-ink-3">Block hash</dt><dd><Mono>{shortHash(near?.blockHash, 6, 6)}</Mono></dd></div>
            <div className="flex justify-between"><dt className="text-ink-3">Protocol</dt><dd className="tnum font-medium">{near?.protocolVersion ?? "—"}</dd></div>
          </dl>
        </Card>
        <Card title="NEAR Intents">
          <dl className="space-y-2 text-sm">
            <div className="flex justify-between"><dt className="text-ink-3">Assets</dt><dd className="tnum font-medium">{num(intents?.tokens.length)}</dd></div>
            <div className="flex justify-between"><dt className="text-ink-3">Chains</dt><dd className="tnum font-medium">{num(intents?.chains.length)}</dd></div>
            <div className="flex justify-between"><dt className="text-ink-3">ZEC asset</dt><dd><Mono>{intents?.zec?.assetId ?? "—"}</Mono></dd></div>
          </dl>
        </Card>
      </div>
    </div>
  );
}
