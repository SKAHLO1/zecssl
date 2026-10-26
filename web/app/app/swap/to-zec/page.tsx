"use client";

import { ArrowDownUp, ArrowRight, CircleAlert, LoaderCircle, Wallet } from "lucide-react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { Suspense, useEffect, useState } from "react";
import { TokenBadge } from "@/components/token-select";
import { buttonClass, Card, cx, Mono } from "@/components/ui";
import { api } from "@/lib/api";
import { rememberOrder } from "@/lib/my-orders";
import { token } from "@/lib/format";
import { useLive } from "@/lib/live";
import { useNearWallet } from "@/lib/near-wallet";
import type { AccountStatus, PayoutQuote } from "@/lib/types";

/** Shielded unified addresses only: the payout never touches a transparent address. */
function validZecAddress(a: string, testnet: boolean): boolean {
  return a.startsWith(testnet ? "utest1" : "u1") && a.length >= 60 && /^[a-z0-9]+$/.test(a);
}

function ToZecFlow() {
  const params = useSearchParams();
  const router = useRouter();
  const { near, network } = useLive();
  const availableZat = network?.treasury?.availableForPayoutsZat;
  const availableZec = availableZat != null ? Number(availableZat) / 1e8 : null;
  const testnet = (near?.network ?? "testnet") !== "mainnet";
  const wallet = useNearWallet(testnet ? "testnet" : "mainnet");
  const [amount, setAmount] = useState("10");
  const [zecAddress, setZecAddress] = useState("");
  const [balance, setBalance] = useState<AccountStatus | null>(null);
  const [quote, setQuote] = useState<PayoutQuote | null>(null);
  const [busy, setBusy] = useState<"quote" | "lock" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const returned = params.get("order");

  useEffect(() => {
    if (!wallet.accountId) return setBalance(null);
    api.account(wallet.accountId).then(setBalance).catch(() => setBalance(null));
  }, [wallet.accountId]);

  const addressOk = validZecAddress(zecAddress.trim(), testnet);
  const amountOk = Number(amount) > 0;

  async function getQuote() {
    if (!wallet.accountId) return;
    setBusy("quote");
    setError(null);
    try {
      const q = await api.createPayout({ amountIn: amount, account: wallet.accountId, zecAddress: zecAddress.trim() });
      rememberOrder(q.id);
      setQuote(q);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(null);
    }
  }

  async function lock() {
    if (!quote) return;
    setBusy("lock");
    setError(null);
    // Browser wallets leave the page and come back here; remember which order this was.
    router.replace(`/app/swap/to-zec?order=${encodeURIComponent(quote.id)}`);
    try {
      await wallet.lock(quote);
      router.push(`/app/orders/${encodeURIComponent(quote.id)}`);
    } catch (e) {
      setError((e as Error).message);
      setBusy(null);
    }
  }

  if (returned) {
    return (
      <Card title="Tokens sent to the vault">
        <p className="text-sm text-ink-2">
          Once the vault shows order <Mono>{returned}</Mono> locked, the operators pay the ZEC to your shielded address and claim settlement. Follow it on the order page.
        </p>
        <div className="mt-4 flex flex-wrap gap-3">
          <Link href={`/app/orders/${encodeURIComponent(returned)}`} className={buttonClass.primary}>
            Track order <ArrowRight className="size-4" />
          </Link>
          <Link href="/app/swap/to-zec" className={buttonClass.secondary}>
            New swap
          </Link>
        </div>
      </Card>
    );
  }

  return (
    <div className="space-y-6">
      <Card
        title="Swap USDC for shielded ZEC"
        action={
          wallet.accountId ? (
            <button className="text-xs text-ink-3 hover:text-ink" onClick={() => void wallet.disconnect()}>
              {wallet.accountId} · Disconnect
            </button>
          ) : null
        }
      >
        <div className="grid items-stretch gap-3 md:grid-cols-[1fr_auto_1fr]">
          <div className="rounded-2xl border border-line p-4">
            <div className="flex items-center justify-between text-xs text-ink-3">
              <span className="font-medium">From</span>
              {balance?.balance != null && <span className="tnum">Balance {token(balance.balance)} {balance.symbol}</span>}
            </div>
            <div className="mt-2 flex items-center gap-2.5">
              <TokenBadge token={{ symbol: "USDC", blockchain: "near" }} size={32} />
              <div>
                <div className="text-sm font-semibold">USDC</div>
                <div className="text-xs text-ink-3">on NEAR {testnet ? "testnet" : ""}</div>
              </div>
            </div>
            <div className="mt-4 flex items-center rounded-xl bg-sunken/70 px-3.5 py-3">
              <input
                inputMode="decimal"
                value={amount}
                onChange={(e) => {
                  setAmount(e.target.value.replace(/[^\d.]/g, ""));
                  setQuote(null);
                }}
                className="tnum w-full min-w-0 bg-transparent text-2xl font-semibold outline-none"
                aria-label="Amount of USDC"
              />
              <span className="text-sm font-semibold text-ink-3">USDC</span>
            </div>
          </div>

          <div className="flex items-center justify-center">
            <Link
              href="/app/swap"
              className="grid size-10 place-items-center rounded-full border border-line bg-surface text-ink-3 transition hover:bg-sunken hover:text-ink"
              title="Switch direction: shielded ZEC to USDC"
              aria-label="Switch direction: shielded ZEC to USDC"
            >
              <ArrowDownUp className="size-4 md:rotate-90" />
            </Link>
          </div>

          <div className="rounded-2xl border border-line p-4">
            <div className="text-xs font-medium text-ink-3">To</div>
            <div className="mt-2 flex items-center gap-2.5">
              <TokenBadge token={{ symbol: "ZEC", blockchain: "zec" }} size={32} />
              <div>
                <div className="text-sm font-semibold">Shielded ZEC</div>
                <div className="text-xs text-ink-3">
                  {availableZec != null ? `Up to ${token(String(availableZec))} ZEC available` : "paid to a unified address"}
                </div>
              </div>
            </div>
            <div className="mt-4 rounded-xl bg-sunken/70 px-3.5 py-3">
              <span className="tnum block truncate text-2xl font-semibold">{quote ? token(quote.zecAmount) : "—"}</span>
            </div>
          </div>
        </div>

        <label className="mt-5 block text-sm font-medium" htmlFor="zec-address">
          Your shielded ZEC address
        </label>
        <input
          id="zec-address"
          value={zecAddress}
          onChange={(e) => {
            setZecAddress(e.target.value);
            setQuote(null);
          }}
          placeholder={testnet ? "utest1…" : "u1…"}
          className={cx(
            "mt-1.5 w-full rounded-xl border bg-surface px-3.5 py-2.5 font-mono text-sm outline-none focus:border-forest",
            zecAddress && !addressOk ? "border-down" : "border-line",
          )}
          spellCheck={false}
        />
        <p className="mt-1.5 text-xs text-ink-3">
          {zecAddress && !addressOk
            ? `Use a unified address starting with ${testnet ? "utest1" : "u1"}. Transparent addresses are not supported.`
            : "The ZEC goes straight to this address in a fully shielded transaction."}
        </p>

        {error && (
          <div className="mt-4 flex items-start gap-2 rounded-xl border border-down/30 bg-down/5 p-3 text-sm text-down">
            <CircleAlert className="mt-0.5 size-4 shrink-0" /> {error}
          </div>
        )}

        <div className="mt-5">
          {!wallet.accountId ? (
            <button className={buttonClass.primary} onClick={() => void wallet.connect()} disabled={!wallet.ready}>
              <Wallet className="size-4" /> Connect NEAR wallet
            </button>
          ) : !quote ? (
            <button className={buttonClass.primary} onClick={() => void getQuote()} disabled={!addressOk || !amountOk || busy !== null}>
              {busy === "quote" && <LoaderCircle className="size-4 animate-spin" />} Get quote
            </button>
          ) : null}
        </div>
      </Card>

      {quote && (
        <Card title="Review and lock">
          <dl className="grid gap-4 text-sm sm:grid-cols-3">
            <div>
              <dt className="text-xs text-ink-3">You lock</dt>
              <dd className="tnum font-semibold">
                {quote.amountIn} {quote.symbol}
              </dd>
            </div>
            <div>
              <dt className="text-xs text-ink-3">You receive</dt>
              <dd className="tnum font-semibold">{quote.zecAmount} shielded ZEC</dd>
            </div>
            <div>
              <dt className="text-xs text-ink-3">Lock before</dt>
              <dd className="tnum font-semibold">{new Date(quote.expiresAt).toLocaleTimeString()}</dd>
            </div>
          </dl>
          <p className="mt-4 text-xs text-ink-3">
            Your tokens stay in the vault contract <Mono>{quote.vault}</Mono> until the operators pay the ZEC and claim settlement. If they don&apos;t pay before the deadline, you can
            reclaim the tokens; if they claim without paying, you can challenge the claim.
          </p>
          <div className="mt-5 flex flex-wrap gap-3">
            <button className={buttonClass.primary} onClick={() => void lock()} disabled={busy !== null}>
              {busy === "lock" && <LoaderCircle className="size-4 animate-spin" />} Lock {quote.amountIn} {quote.symbol}
            </button>
            <button className={buttonClass.secondary} onClick={() => setQuote(null)} disabled={busy !== null}>
              Change
            </button>
          </div>
        </Card>
      )}
    </div>
  );
}

export default function ToZecPage() {
  return (
    <Suspense>
      <ToZecFlow />
    </Suspense>
  );
}
