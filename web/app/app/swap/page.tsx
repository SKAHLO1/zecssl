"use client";

import { ArrowDownUp, ArrowLeft, ArrowRight, Check, CircleAlert, Copy, LoaderCircle } from "lucide-react";
import Image from "next/image";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { Suspense, useEffect, useState } from "react";
import { DepositQr } from "@/components/deposit-qr";
import { OrderBadge } from "@/components/network";
import { TokenBadge, TokenSelect } from "@/components/token-select";
import { buttonClass, Card, cx, Mono, Skeleton } from "@/components/ui";
import { api } from "@/lib/api";
import { rememberOrder } from "@/lib/my-orders";
import { chainName, duration, token, usd } from "@/lib/format";
import { useLive } from "@/lib/live";
import type { AccountStatus, IntentsToken, OrderDetail, Quote } from "@/lib/types";
import { useDestinations, useQuote } from "@/lib/use-quote";

const steps = ["Details", "Review", "Deposit", "Settled"] as const;

interface OpenedOrder {
  id: string;
  depositAddress: string;
  memo?: string;
  expiresAt?: string;
  amountZec?: string;
  amountOut?: string;
  symbol?: string;
  /** ZIP 321 payment URI: address, amount and memo for wallets that support it. */
  zip321?: string;
}

const pricingLabel: Record<Quote["source"], string> = {
  coordinator: "ZEC SSL quote, settled by the vault",
  router: "NEAR Intents router quote",
  oracle: "NEAR Intents oracle price",
};

const progress: Record<string, string> = {
  awaiting_deposit: "Waiting for your deposit",
  deposit_detected: "Deposit seen, waiting for it to be mined",
  confirming: "Deposit confirming",
  signing: "Deposit confirmed. Operators are approving the release",
  refunding: "Deposit was short or late. Operators are signing a shielded refund",
  released: "Settled",
  refunded: "Refunded",
  expired: "Expired",
  disputed: "Disputed",
};

function Stepper({ step }: { step: number }) {
  return (
    <ol className="flex items-center gap-2 sm:gap-3">
      {steps.map((s, i) => (
        <li key={s} className="flex items-center gap-2 sm:gap-3">
          <span className="flex items-center gap-2">
            <span
              className={cx(
                "tnum grid size-6 place-items-center rounded-full text-xs font-semibold",
                i < step ? "bg-forest text-[#f5f1e6]" : i === step ? "bg-forest text-[#f5f1e6] ring-4 ring-forest-100" : "bg-sunken text-ink-3",
              )}
            >
              {i < step ? <Check className="size-3.5" /> : i + 1}
            </span>
            <span className={cx("hidden text-sm sm:inline", i <= step ? "font-semibold text-ink" : "text-ink-3")}>{s}</span>
          </span>
          {i < steps.length - 1 && <span className={cx("h-px w-6 sm:w-10", i < step ? "bg-forest" : "bg-line-strong")} />}
        </li>
      ))}
    </ol>
  );
}

function validRecipient(addr: string, chain: string): string | null {
  if (!addr) return "Enter where the funds should arrive.";
  if (chain === "near") {
    const named = /^(([a-z\d]+[-_])*[a-z\d]+\.)*([a-z\d]+[-_])*[a-z\d]+$/.test(addr) && addr.length >= 2 && addr.length <= 64;
    const implicit = /^[0-9a-f]{64}$/.test(addr);
    return named || implicit ? null : "That is not a valid NEAR account ID.";
  }
  if (chain === "eth" || chain === "base" || chain === "arb" || chain === "gnosis" || chain === "pol" || chain === "bsc" || chain === "op" || chain === "avax" || chain === "bera") {
    return /^0x[0-9a-fA-F]{40}$/.test(addr) ? null : `That is not a valid ${chainName(chain)} address.`;
  }
  return addr.length >= 20 ? null : `That does not look like a ${chainName(chain)} address.`;
}

/** Refunds must go back into the shielded pool: unified addresses only, never transparent. */
function validRefund(addr: string): string | null {
  if (!addr) return "A shielded refund address is required.";
  if (/^(t1|t3|tm|t2)/.test(addr)) return "Transparent addresses are not accepted. Use a shielded unified address (u1…).";
  if (!/^(u1|utest1)[0-9a-z]{60,}$/.test(addr)) return "Enter a unified address starting with u1 (or utest1 on testnet).";
  return null;
}

/**
 * Payouts in the vault's token reach an account only if it is registered with the token;
 * otherwise they wait as claimable. Checks as the user types and offers the faucet's
 * one-click registration on testnet.
 */
function RecipientCheck({ account }: { account: string }) {
  const [status, setStatus] = useState<AccountStatus | null>(null);
  const [faucet, setFaucet] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setStatus(null);
    setError(null);
    if (!account) return;
    const t = setTimeout(() => {
      api.account(account).then(setStatus).catch((e: Error) => setError(e.message));
    }, 400);
    return () => clearTimeout(t);
  }, [account]);
  useEffect(() => {
    api.faucet().then((f) => setFaucet(f.enabled)).catch(() => setFaucet(false));
  }, []);
  if (error) return <span className="mt-1 block text-xs text-down">{error}</span>;
  if (!status) return null;
  if (!status.exists) return <span className="mt-1 block text-xs text-down">This NEAR account does not exist.</span>;
  if (status.registered) return <span className="mt-1 block text-xs text-up">Ready to receive {status.symbol}.</span>;
  return (
    <span className="mt-1 flex flex-wrap items-center gap-2 text-xs text-warn">
      Not registered with {status.symbol} yet, so the payout would wait until you claim it.
      {faucet && (
        <button
          type="button"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            try {
              setStatus((await api.faucetRegister(account)).status);
            } catch (e) {
              setError((e as Error).message);
            } finally {
              setBusy(false);
            }
          }}
          className="rounded-full bg-forest px-2.5 py-0.5 font-semibold text-[#f5f1e6] disabled:opacity-50"
        >
          {busy ? "Registering…" : "Register it"}
        </button>
      )}
    </span>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-start justify-between gap-4 py-2.5 text-sm">
      <span className="text-ink-3">{label}</span>
      <span className="tnum text-right font-medium">{children}</span>
    </div>
  );
}

function SwapFlow() {
  const params = useSearchParams();
  const destinations = useDestinations();
  const { network } = useLive();
  const [step, setStep] = useState(0);
  const [amount, setAmount] = useState(params.get("amount") ?? "10");
  const [dest, setDest] = useState<IntentsToken | null>(null);
  const [recipient, setRecipient] = useState("");
  const [refund, setRefund] = useState("");
  const [touched, setTouched] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [order, setOrder] = useState<OpenedOrder | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (dest || !destinations.length) return;
    const wanted = params.get("to");
    setDest(destinations.find((d) => d.assetId === wanted) ?? destinations[0]);
  }, [dest, destinations, params]);

  const { quote, error, loading } = useQuote(amount, dest?.assetId ?? null, step === 0 ? undefined : recipient);
  const recipientError = dest ? validRecipient(recipient.trim(), dest.blockchain) : null;
  const refundError = validRefund(refund.trim());
  const canReview = Boolean(quote) && !recipientError && !refundError;

  // Once the order is open, follow it on the coordinator, which sees deposits as they land.
  const [live, setLive] = useState<OrderDetail | null>(null);
  useEffect(() => {
    if (!order) return;
    let cancelled = false;
    const poll = () =>
      api
        .order(order.id)
        .then((o) => !cancelled && setLive(o))
        .catch(() => undefined);
    poll();
    const t = setInterval(poll, 8_000);
    return () => {
      cancelled = true;
      clearInterval(t);
    };
  }, [order]);
  useEffect(() => {
    if (live?.state === "released") setStep(3);
  }, [live?.state]);

  async function openOrder() {
    if (!quote || !dest) return;
    setSubmitting(true);
    setSubmitError(null);
    try {
      const res = (await api.createOrder({
        direction: "zec_to_near",
        amountZec: quote.amountIn,
        destinationAssetId: dest.assetId,
        recipient: recipient.trim(),
        refundAddress: refund.trim(),
        quotedAmountOut: quote.amountOut,
      })) as OpenedOrder;
      setOrder(res);
      rememberOrder(res.id);
      setStep(2);
    } catch (e) {
      setSubmitError((e as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="mx-auto grid max-w-[1200px] gap-6 lg:grid-cols-[1fr_340px]">
      <Card className="overflow-visible">
        <div className="flex flex-wrap items-center justify-between gap-4 border-b border-line pb-5">
          <div>
            <h2 className="text-xl font-semibold tracking-tight">Swap shielded ZEC</h2>
            <p className="mt-0.5 text-sm text-ink-3">Choose your destination, enter the amount, and let the operator network handle the rest.</p>
          </div>
          <Stepper step={step} />
        </div>

        {step === 0 && (
          <div className="pt-6">
            <div className="grid items-stretch gap-3 md:grid-cols-[1fr_auto_1fr]">
              <div className="rounded-2xl border border-line p-4">
                <div className="text-xs font-medium text-ink-3">From</div>
                <div className="mt-2 flex items-center gap-2.5">
                  <TokenBadge token={{ symbol: "ZEC", blockchain: "zec" }} size={32} />
                  <div>
                    <div className="text-sm font-semibold">Shielded ZEC</div>
                    <div className="text-xs text-ink-3">Ironwood pool</div>
                  </div>
                </div>
                <div className="mt-4 flex items-center rounded-xl bg-sunken/70 px-3.5 py-3">
                  <input
                    inputMode="decimal"
                    value={amount}
                    onChange={(e) => setAmount(e.target.value.replace(/[^\d.]/g, ""))}
                    className="tnum w-full min-w-0 bg-transparent text-2xl font-semibold outline-none"
                    aria-label="Amount of ZEC"
                  />
                  <span className="text-sm font-semibold text-ink-3">ZEC</span>
                </div>
                <div className="tnum mt-2 text-xs text-ink-3">{quote ? `≈ ${usd(quote.amountInUsd, { cents: true })}` : " "}</div>
              </div>

              <div className="flex items-center justify-center">
                <Link
                  href="/app/swap/to-zec"
                  className="grid size-10 place-items-center rounded-full border border-line bg-surface text-ink-3 transition hover:bg-sunken hover:text-ink"
                  title="Switch direction: USDC to shielded ZEC"
                  aria-label="Switch direction: USDC to shielded ZEC"
                >
                  <ArrowDownUp className="size-4 md:rotate-90" />
                </Link>
              </div>

              <div className="rounded-2xl border border-line p-4">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-medium text-ink-3">To</span>
                  {dest && <span className="text-xs text-ink-3">on {chainName(dest.blockchain)}</span>}
                </div>
                <div className="mt-2">
                  <TokenSelect tokens={destinations} value={dest} onChange={setDest} />
                </div>
                <div className="mt-4 flex items-center rounded-xl bg-sunken/70 px-3.5 py-3">
                  <span className="tnum w-full truncate text-2xl font-semibold">
                    {loading && !quote ? <Skeleton className="h-7 w-32" /> : quote ? token(quote.amountOut, 4) : "0"}
                  </span>
                  <span className="text-sm font-semibold text-ink-3">{dest?.symbol}</span>
                </div>
                <div className="tnum mt-2 text-xs text-ink-3">{quote ? `≈ ${usd(quote.amountOutUsd, { cents: true })}` : " "}</div>
              </div>
            </div>

            <div className="mt-6 grid gap-4 md:grid-cols-2">
              <label className="block">
                <span className="text-sm font-medium">Recipient {dest ? `on ${chainName(dest.blockchain)}` : ""}</span>
                <input
                  value={recipient}
                  onChange={(e) => setRecipient(e.target.value)}
                  placeholder={dest?.blockchain === "near" ? "alice.near" : "Destination address"}
                  className="mt-1.5 w-full rounded-xl border border-line bg-surface px-3.5 py-2.5 font-mono text-sm outline-none focus:border-forest focus:ring-2 focus:ring-forest-100"
                />
                {touched && recipientError ? (
                  <span className="mt-1 block text-xs text-down">{recipientError}</span>
                ) : (
                  dest?.assetId.startsWith("vault:") && !recipientError && <RecipientCheck account={recipient.trim()} />
                )}
              </label>
              <label className="block">
                <span className="text-sm font-medium">Shielded refund address</span>
                <input
                  value={refund}
                  onChange={(e) => setRefund(e.target.value)}
                  placeholder="u1…"
                  className="mt-1.5 w-full rounded-xl border border-line bg-surface px-3.5 py-2.5 font-mono text-sm outline-none focus:border-forest focus:ring-2 focus:ring-forest-100"
                />
                {touched && refundError ? (
                  <span className="mt-1 block text-xs text-down">{refundError}</span>
                ) : (
                  <span className="mt-1 block text-xs text-ink-3">If the swap fails, your ZEC returns here, still shielded.</span>
                )}
              </label>
            </div>

            <div className="mt-6 grid gap-6 md:grid-cols-[1fr_auto] md:items-end">
              <div className="rounded-2xl bg-sunken/60 px-4 py-2">
                <div className="divide-y divide-line">
                  <Row label="Rate">{quote ? `1 ZEC ≈ ${token(quote.rate, 4)} ${quote.destination.symbol}` : "—"}</Row>
                  <Row label="Operator fee">{quote ? `${token(quote.operatorFee, 6)} ZEC (${quote.spreadBps / 100}%)` : "—"}</Row>
                  <Row label="Zcash network fee">{quote ? `${quote.zcashNetworkFeeZec} ZEC` : "—"}</Row>
                  <Row label="Pricing">{quote ? pricingLabel[quote.source] : "—"}</Row>
                </div>
              </div>
              <button
                className={cx(buttonClass.primary, "md:min-w-44")}
                onClick={() => {
                  setTouched(true);
                  if (canReview) setStep(1);
                }}
                disabled={!quote}
              >
                Continue <ArrowRight className="size-4" />
              </button>
            </div>
            {error && <p className="mt-3 text-sm text-down">{error}</p>}
          </div>
        )}

        {step === 1 && quote && dest && (
          <div className="pt-6">
            <div className="grid gap-6 md:grid-cols-2">
              <div className="rounded-2xl border border-line p-5">
                <div className="text-xs text-ink-3">You send</div>
                <div className="tnum mt-1 text-2xl font-semibold">{token(quote.amountIn)} ZEC</div>
                <div className="text-xs text-ink-3">from the shielded pool</div>
                <div className="my-4 h-px bg-line" />
                <div className="text-xs text-ink-3">You receive</div>
                <div className="tnum mt-1 text-2xl font-semibold">
                  {token(quote.amountOut, 4)} {dest.symbol}
                </div>
                <div className="text-xs text-ink-3">on {chainName(dest.blockchain)}</div>
              </div>
              <div className="divide-y divide-line">
                <Row label="Recipient">
                  <Mono>{recipient}</Mono>
                </Row>
                <Row label="Refund to">
                  <Mono>{`${refund.slice(0, 10)}…${refund.slice(-6)}`}</Mono>
                </Row>
                <Row label="Minimum received">{quote.minAmountOut ? `${token(quote.minAmountOut, 4)} ${dest.symbol}` : "Set by the coordinator"}</Row>
                <Row label="Estimated time">{quote.timeEstimateSec ? duration(quote.timeEstimateSec) : "~5 min after confirmation"}</Row>
                <Row label="Signing">{`${network.threshold.t}-of-${network.threshold.n} operators`}</Row>
              </div>
            </div>
            {submitError && (
              <div className="mt-5 flex gap-2.5 rounded-xl border border-down/20 bg-down-50 p-3.5 text-sm text-down">
                <CircleAlert className="mt-0.5 size-4 shrink-0" />
                <span>{submitError}</span>
              </div>
            )}
            <div className="mt-6 flex flex-wrap justify-between gap-3">
              <button className={buttonClass.secondary} onClick={() => setStep(0)}>
                <ArrowLeft className="size-4" /> Back
              </button>
              <button className={buttonClass.primary} onClick={openOrder} disabled={submitting}>
                {submitting ? <LoaderCircle className="size-4 animate-spin" /> : null}
                Open order
              </button>
            </div>
          </div>
        )}

        {step >= 2 && order && (
          <div className="pt-6">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-sm text-ink-3">Order</div>
                <Mono>{order.id}</Mono>
              </div>
              {live && <OrderBadge state={live.state} />}
            </div>
            {step === 2 && (
              <div className="mt-5 space-y-4">
                <div className="flex flex-col gap-5 rounded-2xl border border-line p-5 sm:flex-row">
                  {order.zip321 && <DepositQr uri={order.zip321} />}
                  <div className="min-w-0 flex-1">
                  <div className="text-sm font-semibold">
                    Send exactly {order.amountZec ?? token(quote?.amountIn)} ZEC from a shielded wallet to
                  </div>
                  <div className="mt-3 flex items-start gap-2 rounded-xl bg-sunken/70 p-3">
                    <Mono className="break-all">{order.depositAddress}</Mono>
                    <button
                      className="shrink-0 rounded-lg p-1.5 hover:bg-surface"
                      onClick={() => {
                        void navigator.clipboard.writeText(order.depositAddress);
                        setCopied(true);
                        setTimeout(() => setCopied(false), 1500);
                      }}
                      aria-label="Copy deposit address"
                    >
                      {copied ? <Check className="size-4 text-up" /> : <Copy className="size-4" />}
                    </button>
                  </div>
                  <div className="mt-3 flex flex-wrap items-center gap-x-6 gap-y-2 text-sm text-ink-2">
                    {order.memo && (
                      <span>
                        Memo <Mono>{order.memo}</Mono>
                      </span>
                    )}
                    {order.expiresAt && <span>Deposit by {new Date(order.expiresAt).toLocaleTimeString()}</span>}
                    {order.zip321 && (
                      <a href={order.zip321} className="font-medium text-forest hover:underline">
                        Open in wallet
                      </a>
                    )}
                  </div>
                  <p className="mt-3 text-xs text-ink-3">
                    The address is unique to this order. Send from a shielded (u…) address only; this deposit never touches a transparent address.
                  </p>
                  <Link href={`/app/orders/${order.id}`} className="mt-3 inline-flex text-sm font-medium text-forest hover:underline">
                    Order page: track it or share it
                  </Link>
                  </div>
                </div>
                <div className="rounded-2xl bg-sunken/60 p-5">
                  <div className="flex items-center gap-2 text-sm font-semibold">
                    {live && !["released", "refunded", "expired"].includes(live.state) && <LoaderCircle className="size-4 animate-spin text-forest" />}
                    {live ? (progress[live.state] ?? live.state) : "Checking order…"}
                  </div>
                  {live && (
                    <dl className="mt-3 grid gap-2 text-sm sm:grid-cols-3">
                      <div>
                        <dt className="text-xs text-ink-3">Received</dt>
                        <dd className="tnum font-medium">
                          {token(live.receivedZec)} / {live.amountInZec} ZEC
                        </dd>
                      </div>
                      <div>
                        <dt className="text-xs text-ink-3">Release approvals</dt>
                        <dd className="tnum font-medium">
                          {live.state === "signing" || live.state === "released" ? `${live.signatures} / ${live.threshold}` : "—"}
                        </dd>
                      </div>
                      <div>
                        <dt className="text-xs text-ink-3">You receive</dt>
                        <dd className="tnum font-medium">
                          {live.amountOut} {live.destinationSymbol}
                        </dd>
                      </div>
                    </dl>
                  )}
                  {live?.depositTxids.length ? (
                    <div className="mt-3 text-xs text-ink-3">
                      Deposit {live.depositTxids.map((t) => <Mono key={t}>{`${t.slice(0, 10)}…`}</Mono>)}
                    </div>
                  ) : null}
                </div>
              </div>
            )}
            {step === 3 && live && (
              <div className="mt-5 rounded-2xl border border-up/30 bg-up-50 p-5 text-sm">
                <div className="font-semibold text-up">Settled</div>
                <p className="mt-1 text-ink-2">
                  {live.amountOut} {live.destinationSymbol} was paid to <Mono>{live.recipient}</Mono> after {live.threshold} operators each confirmed your deposit on
                  Zcash.
                </p>
                <div className="mt-4 flex flex-wrap gap-3">
                  <Link href={`/app/orders/${order.id}`} className={buttonClass.primary}>
                    View order details <ArrowRight className="size-4" />
                  </Link>
                  <button
                    className={buttonClass.secondary}
                    onClick={() => {
                      setOrder(null);
                      setStep(0);
                    }}
                  >
                    New swap
                  </button>
                </div>
              </div>
            )}
          </div>
        )}
      </Card>

      <aside className="relative min-h-[420px] overflow-hidden rounded-[var(--radius-card)] bg-forest-900 text-[#f5f1e6]">
        <Image src="/images/control-room.jpg" alt="" fill sizes="340px" className="object-cover opacity-45" />
        <div className="absolute inset-0 bg-gradient-to-t from-forest-900 via-forest-900/70 to-forest-900/20" />
        <div className="relative flex h-full flex-col justify-end p-6">
          <h3 className="text-xl leading-snug font-semibold">
            Your funds stay
            <br />
            in the shielded pool.
          </h3>
          <ul className="mt-5 space-y-3 text-sm text-[#f5f1e6]/85">
            {["No transparent-address hop", `${network.threshold.t}-of-${network.threshold.n} threshold signed`, "Bonded, slashable operators", "Shielded refund on any failure"].map((t) => (
              <li key={t} className="flex items-center gap-2.5">
                <span className="grid size-5 place-items-center rounded-full bg-gold/90 text-forest-900">
                  <Check className="size-3" />
                </span>
                {t}
              </li>
            ))}
          </ul>
        </div>
      </aside>
    </div>
  );
}

export default function SwapPage() {
  return (
    <Suspense>
      <SwapFlow />
    </Suspense>
  );
}
