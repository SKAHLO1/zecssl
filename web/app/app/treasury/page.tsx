"use client";

import { FileCheck, Landmark, Scale, Vault } from "lucide-react";
import { CoordinatorEmpty } from "@/components/network";
import { Badge, Card, EmptyState, Mono, Stat } from "@/components/ui";
import { num, token, usd } from "@/lib/format";
import { useLive } from "@/lib/live";
import type { VaultAccount } from "@/lib/types";

function ContractCard({ title, account, envVar }: { title: string; account: VaultAccount | null; envVar: string }) {
  return (
    <Card title={title} action={account ? <Badge tone={account.exists ? "up" : "warn"} dot>{account.exists ? "Deployed" : "Not found"}</Badge> : null}>
      {!account ? (
        <EmptyState icon={<Vault className="size-5" />} title="Contract not configured">
          Set <Mono>{envVar}</Mono> on the API server to read this contract from NEAR.
        </EmptyState>
      ) : (
        <dl className="divide-y divide-line text-sm">
          {[
            ["Account", <Mono key="a">{account.contract}</Mono>],
            ["Network", account.network],
            ["Balance", account.balanceNear ? `${token(account.balanceNear, 4)} NEAR` : "—"],
            ["Storage", account.storageBytes != null ? `${num(account.storageBytes)} bytes` : "—"],
            ["Code hash", <Mono key="c">{account.codeHash ? `${account.codeHash.slice(0, 12)}…` : "—"}</Mono>],
          ].map(([k, v]) => (
            <div key={String(k)} className="flex justify-between gap-4 py-2.5">
              <dt className="text-ink-3">{k}</dt>
              <dd className="text-right font-medium">{v}</dd>
            </div>
          ))}
        </dl>
      )}
    </Card>
  );
}

export default function TreasuryPage() {
  const { network, zcash, market } = useLive();
  const tr = network.treasury;
  const zecPrice = market.find((a) => a.id === "zcash")?.price ?? null;
  const shieldedZec = tr?.shieldedBalanceZat != null ? Number(tr.shieldedBalanceZat) / 1e8 : null;
  const scanLag = tr?.lastScanHeight != null && zcash ? zcash.height - tr.lastScanHeight : null;

  return (
    <div className="mx-auto max-w-[1400px] space-y-6">
      <div>
        <h2 className="text-xl font-semibold tracking-tight">Treasury and reserves</h2>
        <p className="mt-1 max-w-2xl text-sm text-ink-3">
          One Ironwood account derived from the operators&apos; group key holds deposits; a NEAR vault holds release liquidity. Bonds must always exceed the value in flight.
        </p>
      </div>

      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <Stat icon={<Vault className="size-5" />} label="Shielded treasury" value={shieldedZec != null ? `${token(shieldedZec, 4)} ZEC` : "—"} sub={shieldedZec != null && zecPrice ? usd(shieldedZec * zecPrice) : "Reported by the coordinator"} />
        <Stat icon={<Landmark className="size-5" />} label="Vault liquidity" value={tr?.vaultLiquidity.length ? tr.vaultLiquidity.map((l) => `${token(l.amount, 2)} ${l.symbol}`).join(" · ") : "—"} sub={network.vault ? network.vault.contract : "Vault not configured"} />
        <Stat icon={<Scale className="size-5" />} label="Total bonded" value={usd(tr?.totalBondedUsd)} sub="Slashable on NEAR" />
        <Stat icon={<FileCheck className="size-5" />} label="Deposit addresses issued" value={num(tr?.addressesIssued)} sub={scanLag != null ? `Scanner ${scanLag <= 1 ? "in sync" : `${scanLag} blocks behind`}` : "One fresh address per order"} />
      </div>

      {!network.coordinator.online && (
        <Card title="Shielded treasury">
          <CoordinatorEmpty what="Treasury balance and scan height" />
        </Card>
      )}

      <div className="grid gap-6 lg:grid-cols-2">
        <ContractCard title="NEAR vault contract" account={network.vault} envVar="NEAR_VAULT_CONTRACT" />
        <ContractCard title="Bond contract" account={network.bonds} envVar="NEAR_BONDS_CONTRACT" />
      </div>

      <Card title="Reserve attestations">
        <EmptyState icon={<FileCheck className="size-5" />} title="Attestations start at mainnet beta">
          Operators will publish periodic reserve attestations so anyone can verify solvency without seeing individual flows (FR-17).
        </EmptyState>
      </Card>
    </div>
  );
}
