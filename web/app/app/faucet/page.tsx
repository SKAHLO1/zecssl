"use client";

import { CircleAlert, ExternalLink, LoaderCircle } from "lucide-react";
import { useEffect, useState } from "react";
import { buttonClass, Card, cx, EmptyState, Mono } from "@/components/ui";
import { api } from "@/lib/api";
import { explorer } from "@/lib/explorers";
import { useLive } from "@/lib/live";
import type { AccountStatus, FaucetInfo } from "@/lib/types";

const ZEC_FAUCETS = [
  { name: "Valar Group", url: "https://faucet.testnet.valargroup.dev/", note: "0.125 TAZ per day; accepts unified addresses" },
  { name: "Jino Labs", url: "https://zcashfaucet.jinolabs.xyz/", note: "0.1 TAZ, sent shielded; a short in-browser puzzle instead of a captcha" },
];

// The same wallets the site's NEAR wallet picker offers. MyNearWallet is being sunset.
const NEAR_WALLETS = [
  { name: "Meteor Wallet", url: "https://wallet.meteorwallet.app/", note: "web, extension and mobile; switch the network to testnet" },
  { name: "HERE Wallet", url: "https://www.herewallet.app/", note: "mobile" },
  { name: "Intear Wallet", url: "https://wallet.intear.tech/", note: "web" },
  { name: "Sender", url: "https://sender.org/", note: "browser extension" },
  { name: "MyNearWallet (testnet)", url: "https://testnet.mynearwallet.com/", note: "being sunset; deprecated 31 October 2026" },
];

function Step({ n, title, children }: { n: number; title: string; children: React.ReactNode }) {
  return (
    <Card>
      <div className="flex gap-4">
        <span className="tnum grid size-8 shrink-0 place-items-center rounded-full bg-forest text-sm font-semibold text-[#f5f1e6]">{n}</span>
        <div className="min-w-0 flex-1">
          <h2 className="font-semibold">{title}</h2>
          <div className="mt-2 text-sm text-ink-2">{children}</div>
        </div>
      </div>
    </Card>
  );
}

export default function FaucetPage() {
  const { near } = useLive();
  const [info, setInfo] = useState<FaucetInfo | null>(null);
  const [account, setAccount] = useState("");
  const [status, setStatus] = useState<AccountStatus | null>(null);
  const [busy, setBusy] = useState<null | "check" | "register" | "drip">(null);
  const [message, setMessage] = useState<{ ok: boolean; text: string; tx?: string } | null>(null);

  useEffect(() => {
    api.faucet().then(setInfo).catch(() => setInfo(null));
  }, []);

  async function run(kind: "check" | "register" | "drip") {
    const id = account.trim();
    if (!id) return;
    setBusy(kind);
    setMessage(null);
    try {
      if (kind === "check") {
        setStatus(await api.account(id));
      } else if (kind === "register") {
        const r = await api.faucetRegister(id);
        setStatus(r.status);
        setMessage({ ok: true, text: r.txHash ? "Registered." : "Already registered.", tx: r.txHash ?? undefined });
      } else {
        const r = await api.faucetUsdc(id);
        setStatus(r.status);
        setMessage({ ok: true, text: `Sent ${r.amount} test ${r.status.symbol}.`, tx: r.txHash });
      }
    } catch (e) {
      setMessage({ ok: false, text: (e as Error).message });
    } finally {
      setBusy(null);
    }
  }

  if (near && near.network !== "testnet") {
    return (
      <div className="mx-auto max-w-2xl">
        <EmptyState icon={<CircleAlert className="size-5" />} title="The faucet only exists on testnet" />
      </div>
    );
  }

  const symbol = info?.symbol ?? "USDC";
  return (
    <div className="mx-auto max-w-[860px] space-y-6">
      <div>
        <h2 className="text-xl font-semibold tracking-tight">Testnet faucet</h2>
        <p className="mt-1 text-sm text-ink-3">
          Everything you need to try a swap on testnet. Test ZEC (TAZ) and test {symbol} have no value.
        </p>
      </div>

      <Step n={1} title="Get a NEAR testnet account">
        <p>Your {symbol} payout goes to a NEAR testnet account. Create one in any testnet wallet:</p>
        <ul className="mt-2 flex flex-wrap gap-3">
          {NEAR_WALLETS.map((w) => (
            <li key={w.url}>
              <a href={w.url} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1.5 font-medium text-forest hover:underline">
                {w.name} <ExternalLink className="size-3.5" />
              </a>
              <span className="block text-xs text-ink-3">{w.note}</span>
            </li>
          ))}
        </ul>
      </Step>

      <Step n={2} title={`Register it for test ${symbol}`}>
        <p>Payouts only reach accounts registered with the vault&apos;s token. The faucet pays the small registration deposit, and can send you 100 test {symbol} once a day.</p>
        {info && !info.enabled ? (
          <p className="mt-3 rounded-xl bg-warn-50 px-3 py-2 text-warn">The faucet is not configured on this deployment.</p>
        ) : (
          <div className="mt-3 space-y-3">
            <div className="flex flex-col gap-2 sm:flex-row">
              <input
                value={account}
                onChange={(e) => {
                  setAccount(e.target.value);
                  setStatus(null);
                }}
                onKeyDown={(e) => e.key === "Enter" && void run("check")}
                placeholder="you.testnet"
                className="min-w-0 flex-1 rounded-xl border border-line bg-surface px-3.5 py-2.5 font-mono text-sm outline-none focus:border-forest focus:ring-2 focus:ring-forest-100"
                aria-label="NEAR testnet account"
              />
              <button className={buttonClass.secondary} disabled={!account.trim() || busy !== null} onClick={() => void run("check")}>
                {busy === "check" ? <LoaderCircle className="size-4 animate-spin" /> : null} Check
              </button>
            </div>
            {status && (
              <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl bg-sunken/60 px-3.5 py-3">
                <span>
                  {!status.exists ? (
                    <span className="text-down">That account doesn&apos;t exist on NEAR testnet.</span>
                  ) : (
                    <>
                      <span className={status.registered ? "text-up" : "text-warn"}>{status.registered ? "Registered" : "Not registered"}</span>
                      <span className="tnum ml-3 text-ink-3">
                        Balance {status.balance ?? "0"} {status.symbol}
                      </span>
                    </>
                  )}
                </span>
                {status.exists && (
                  <span className="flex gap-2">
                    {!status.registered && (
                      <button className={cx(buttonClass.secondary, "px-4 py-2")} disabled={busy !== null} onClick={() => void run("register")}>
                        {busy === "register" ? <LoaderCircle className="size-4 animate-spin" /> : null} Register
                      </button>
                    )}
                    <button className={cx(buttonClass.primary, "px-4 py-2")} disabled={busy !== null} onClick={() => void run("drip")}>
                      {busy === "drip" ? <LoaderCircle className="size-4 animate-spin" /> : null} Send me 100 {status.symbol}
                    </button>
                  </span>
                )}
              </div>
            )}
            {message && (
              <p className={cx("text-sm", message.ok ? "text-up" : "text-down")}>
                {message.text}{" "}
                {message.tx && (
                  <a href={explorer.nearTx("testnet", message.tx)} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 text-forest hover:underline">
                    <Mono>{message.tx.slice(0, 10)}…</Mono> <ExternalLink className="size-3" />
                  </a>
                )}
              </p>
            )}
          </div>
        )}
      </Step>

      <Step n={3} title="Get testnet ZEC">
        <p>Deposits come from a shielded Zcash testnet wallet. These community faucets send TAZ to a unified (utest1…) address:</p>
        <ul className="mt-2 space-y-2">
          {ZEC_FAUCETS.map((f) => (
            <li key={f.url}>
              <a href={f.url} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1.5 font-medium text-forest hover:underline">
                {f.name} <ExternalLink className="size-3.5" />
              </a>
              <span className="block text-xs text-ink-3">{f.note}</span>
            </li>
          ))}
        </ul>
        <p className="mt-3 text-xs text-ink-3">
          Your wallet needs testnet support with the current Zcash network upgrades. For developers, <Mono>zcash-devtool</Mono> works (see protocol/run/user-wallet.sh).
        </p>
      </Step>
    </div>
  );
}
