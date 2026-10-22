"use client";

import { ArrowRight, Check, EyeOff, KeyRound, Landmark, Scale, ShieldCheck, X } from "lucide-react";
import Image from "next/image";
import Link from "next/link";
import { duration, num, price, share } from "@/lib/format";
import { useLive } from "@/lib/live";
import { RecentBlocks, TxMix } from "./chain";
import { MarketsTable } from "./markets-table";
import { QuickSwap } from "./quick-swap";
import { buttonClass, Change, cx, Skeleton } from "./ui";

function Section({ id, eyebrow, title, intro, children, className }: { id?: string; eyebrow?: string; title: string; intro?: string; children: React.ReactNode; className?: string }) {
  return (
    <section id={id} className={cx("mx-auto max-w-[1240px] scroll-mt-28 px-4 sm:px-6", className)}>
      <div className="max-w-2xl">
        {eyebrow && <div className="text-xs font-semibold tracking-[0.08em] text-gold-600 uppercase">{eyebrow}</div>}
        <h2 className="mt-2 text-[28px] leading-tight font-semibold tracking-tight sm:text-[34px]">{title}</h2>
        {intro && <p className="mt-3 text-[15px] leading-relaxed text-ink-2">{intro}</p>}
      </div>
      <div className="mt-8">{children}</div>
    </section>
  );
}

export function Hero() {
  const { market } = useLive();
  const zec = market.find((a) => a.id === "zcash");
  return (
    <section className="mx-auto grid max-w-[1240px] items-center gap-12 px-4 pt-12 pb-16 sm:px-6 lg:grid-cols-[1.05fr_1fr] lg:pt-16 lg:pb-20">
      <div>
        <span className="inline-flex items-center gap-2 rounded-full border border-line bg-surface px-3 py-1 text-xs font-medium text-ink-2">
          <span className="size-1.5 rounded-full bg-gold" />
          Zcash Ironwood · NEAR Intents · Testnet
        </span>
        <h1 className="mt-5 text-[40px] leading-[1.05] font-semibold tracking-[-0.025em] sm:text-[56px]">
          Private swaps.
          <br />
          <span className="text-forest">No public hop.</span>
        </h1>
        <p className="mt-5 max-w-xl text-[17px] leading-relaxed text-ink-2">
          ZEC SSL settles cross-chain swaps directly into and out of Zcash&apos;s shielded pool. Your ZEC never touches a transparent address, and no single
          operator can move your funds.
        </p>
        <div className="mt-8 flex flex-wrap gap-3">
          <Link href="/app/swap" className={buttonClass.primary}>
            Start a swap <ArrowRight className="size-4" />
          </Link>
          <Link href="/app/operators" className={buttonClass.secondary}>
            View the network
          </Link>
        </div>
        <dl className="mt-10 grid max-w-xl grid-cols-2 gap-x-6 gap-y-5 border-t border-line pt-6 sm:grid-cols-4">
          {[
            ["Settlement", "Shielded only"],
            ["Signing", "2-of-3 FROST"],
            ["Operators", "Bonded on NEAR"],
            ["Routing", "NEAR Intents"],
          ].map(([k, v]) => (
            <div key={k}>
              <dt className="text-xs text-ink-3">{k}</dt>
              <dd className="mt-1 text-sm font-semibold">{v}</dd>
            </div>
          ))}
        </dl>
      </div>

      <div className="relative">
        <div className="relative aspect-[4/3.4] overflow-hidden rounded-[28px] bg-forest-900 sm:aspect-[4/3]">
          <Image src="/images/control-room.jpg" alt="An operations control room with a lit network diagram wall" fill priority sizes="(min-width: 1024px) 600px, 100vw" className="object-cover" />
          <div className="absolute inset-0 bg-gradient-to-t from-forest-900/70 via-forest-900/10 to-transparent" />
          <div className="absolute top-4 right-4 flex items-center gap-3 rounded-2xl bg-surface/95 px-3.5 py-2.5 shadow-[var(--shadow-float)] backdrop-blur">
            {zec?.image && (
              // eslint-disable-next-line @next/next/no-img-element
              <img src={zec.image} alt="" width={28} height={28} className="rounded-full" />
            )}
            <div>
              <div className="text-[11px] text-ink-3">ZEC / USD</div>
              <div className="tnum flex items-center gap-2 text-sm font-semibold">
                {zec ? price(zec.price) : <Skeleton className="h-4 w-16" />}
                <Change value={zec?.change24h} className="text-xs" />
              </div>
            </div>
          </div>
        </div>
        <QuickSwap floating className="relative z-10 mx-auto -mt-24 w-[calc(100%-2rem)] max-w-[380px] sm:-mt-40 lg:absolute lg:-bottom-10 lg:-left-12 lg:mt-0 lg:w-[360px]" />
      </div>
    </section>
  );
}

export function LiveStats() {
  const { market, zcash, intents, loaded } = useLive();
  const zec = market.find((a) => a.id === "zcash");
  const items = [
    { label: "ZEC price", value: price(zec?.price), sub: <Change value={zec?.change24h} className="text-xs" />, subLabel: "24h" },
    {
      label: "Zcash block height",
      value: num(zcash?.height),
      sub: <span className="text-xs text-ink-3">~{duration(zcash?.avgBlockTimeSec)} per block</span>,
    },
    {
      label: "Recent txs that deshield",
      value: share(zcash?.txMix?.deshielding),
      sub: <span className="text-xs text-ink-3">exits to a public address</span>,
    },
    {
      label: "Routable assets",
      value: num(intents?.tokens.length),
      sub: <span className="text-xs text-ink-3">across {num(intents?.chains.length)} chains via NEAR Intents</span>,
    },
  ];
  return (
    <div className="mx-auto max-w-[1240px] px-4 sm:px-6">
      <div className="card grid grid-cols-2 divide-line lg:grid-cols-4 lg:divide-x [&>*:nth-child(-n+2)]:border-b [&>*:nth-child(-n+2)]:border-line lg:[&>*:nth-child(-n+2)]:border-b-0 [&>*:nth-child(odd)]:border-r [&>*:nth-child(odd)]:border-line lg:[&>*:nth-child(odd)]:border-r-0">
        {items.map((it) => (
          <div key={it.label} className="px-5 py-5">
            <div className="text-[13px] text-ink-3">{it.label}</div>
            <div className="tnum mt-1 text-2xl font-semibold tracking-tight">{loaded || it.value !== "—" ? it.value : <Skeleton className="h-7 w-24" />}</div>
            <div className="mt-1 flex items-center gap-1.5">
              {it.sub}
              {it.subLabel && <span className="text-xs text-ink-3">{it.subLabel}</span>}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export function MarketsSection() {
  return (
    <Section eyebrow="Markets" title="Privacy and settlement assets, live" intro="Prices update every minute. ZEC is the asset ZEC SSL settles; NEAR and USDC are the most common destinations.">
      <div className="card overflow-hidden">
        <MarketsTable limit={6} />
      </div>
    </Section>
  );
}

export function Comparison() {
  const rows: [string, boolean, boolean][] = [
    ["ZEC leaves from a shielded address", false, true],
    ["Amount and timing hidden on Zcash", false, true],
    ["No transparent refund address needed", false, true],
    ["No single party can move funds", false, true],
    ["Refund path when a swap fails", true, true],
  ];
  return (
    <Section
      id="how-it-works"
      eyebrow="The problem"
      title="Today, every shielded exit goes public first"
      intro="Cross-chain routes can pay into a shielded address, but a swap out of ZEC must start from a transparent one. Unshielding puts the amount, timing and destination on a public address, exactly where chain analysis links identities."
    >
      <div className="grid gap-6 lg:grid-cols-[1.1fr_1fr]">
        <div className="card overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-line bg-paper text-left">
                <th className="px-5 py-3 font-medium text-ink-3">Swapping ZEC out</th>
                <th className="w-28 px-3 py-3 text-center font-medium text-ink-3">Transparent hop</th>
                <th className="w-28 px-3 py-3 text-center font-semibold text-forest">ZEC SSL</th>
              </tr>
            </thead>
            <tbody>
              {rows.map(([label, today, ssl]) => (
                <tr key={label} className="border-b border-line last:border-0">
                  <td className="px-5 py-3.5">{label}</td>
                  {[today, ssl].map((v, i) => (
                    <td key={i} className="px-3 py-3.5 text-center">
                      {v ? <Check className="mx-auto size-4 text-up" aria-label="Yes" /> : <X className="mx-auto size-4 text-down" aria-label="No" />}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <ol className="space-y-3">
          {[
            ["Quote", "Request a quote with the amount, a NEAR recipient and a shielded refund address. Two of three operators approve it."],
            ["Deposit", "Send shielded ZEC to a fresh treasury address with the order ID in the encrypted memo."],
            ["Verify", "Each operator detects the deposit with the viewing key and checks it against the quote, independently."],
            ["Release", "Two of three operators sign the release; the NEAR vault pays the recipient. The order closes with a full trail."],
          ].map(([t, d], i) => (
            <li key={t} className="card flex gap-4 p-4">
              <span className="tnum grid size-8 shrink-0 place-items-center rounded-full bg-forest text-sm font-semibold text-[#f5f1e6]">{i + 1}</span>
              <span>
                <span className="block font-semibold">{t}</span>
                <span className="mt-0.5 block text-sm leading-relaxed text-ink-2">{d}</span>
              </span>
            </li>
          ))}
        </ol>
      </div>
    </Section>
  );
}

export function ChainSection() {
  const { zcash } = useLive();
  return (
    <Section eyebrow="Zcash mainnet" title="What the public chain sees right now" intro="A live sample of Zcash mainnet. Deshielding transactions are the public hop that ZEC SSL removes from cross-chain swaps.">
      <div className="grid gap-6 lg:grid-cols-2">
        <div className="card p-5">
          <div className="mb-4 flex items-baseline justify-between">
            <h3 className="font-semibold">Transaction mix</h3>
            <span className="text-xs text-ink-3">{zcash?.source ? `via ${zcash.source}` : ""}</span>
          </div>
          <TxMix zcash={zcash} />
        </div>
        <div className="card p-5">
          <div className="mb-2 flex items-baseline justify-between">
            <h3 className="font-semibold">Latest blocks</h3>
            <Link href="/app/activity" className="text-sm font-medium text-forest hover:underline">
              Activity
            </Link>
          </div>
          <RecentBlocks limit={6} />
        </div>
      </div>
    </Section>
  );
}

export function Security() {
  const pillars = [
    { icon: KeyRound, title: "Threshold signing", body: "Every treasury spend needs t-of-n rerandomized FROST signatures over RedPallas. No single operator holds a spending key." },
    { icon: Scale, title: "Bonded operators", body: "Operators bond on NEAR. Bonds exceed the value at risk, and signing anything that does not match an approved order is slashable." },
    { icon: EyeOff, title: "Shielded only", body: "Treasury transactions are checked for zero transparent inputs or outputs before anyone signs. The coordinator holds no key share." },
    { icon: Landmark, title: "Auditable reserves", body: "Anyone can check treasury solvency through published reserve attestations, without exposing individual user flows." },
  ];
  return (
    <section id="security" className="scroll-mt-28 px-4 sm:px-6">
      <div className="relative mx-auto max-w-[1240px] overflow-hidden rounded-[28px] bg-forest-900 text-[#f5f1e6]">
        <Image src="/images/hardware.jpg" alt="" fill sizes="1240px" className="object-cover opacity-30" />
        <div className="absolute inset-0 bg-gradient-to-r from-forest-900 via-forest-900/90 to-forest-900/60" />
        <div className="relative grid gap-10 p-8 sm:p-12 lg:grid-cols-[1fr_1.4fr]">
          <div>
            <div className="text-xs font-semibold tracking-[0.08em] text-gold uppercase">Trust model</div>
            <h2 className="mt-2 text-[28px] leading-tight font-semibold tracking-tight sm:text-[34px]">Cryptography where it can be. Bonds where it cannot.</h2>
            <p className="mt-4 text-[15px] leading-relaxed text-[#f5f1e6]/75">
              Zcash has no scripts or hash-locks, so the ZEC leg is not atomic. ZEC SSL is explicit about it: no single operator can spend, and colluding operators
              lose more in bonds than they could take.
            </p>
            <Link href="/app/operators" className="mt-6 inline-flex items-center gap-2 rounded-full bg-[#f5f1e6] px-5 py-2.5 text-sm font-semibold text-forest-900 hover:bg-white">
              Inspect the network <ArrowRight className="size-4" />
            </Link>
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            {pillars.map((p) => (
              <div key={p.title} className="rounded-2xl border border-white/10 bg-white/[0.06] p-5 backdrop-blur-sm">
                <p.icon className="size-5 text-gold" />
                <div className="mt-3 font-semibold">{p.title}</div>
                <p className="mt-1.5 text-sm leading-relaxed text-[#f5f1e6]/70">{p.body}</p>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

export function Roadmap() {
  const phases = [
    { name: "Zecathon MVP", when: "Testnet · Oct 28", current: true, items: ["3 operators, 2-of-3 FROST key by DKG", "Shielded ZEC → USDC on NEAR", "NEAR vault and operator bonds", "Live order and signing dashboard"] },
    { name: "Stretch", when: "Testnet", items: ["USDC → shielded ZEC", "Optimistic settlement with a challenge window", "Automated refunds on timeout"] },
    { name: "v1", when: "Testnet, post-hackathon", items: ["Solver on NEAR Intents", "3-of-5 with key resharing", "Batched payouts, randomized timing", "TypeScript and Flutter SDKs"] },
    { name: "Mainnet beta", when: "After audits", items: ["Independent operators", "Audited contracts and signing code", "Value caps tied to total bonds", "Published reserve attestations"] },
  ];
  return (
    <Section eyebrow="Roadmap" title="Shipping the hard direction first" intro="The MVP proves shielded ZEC out to NEAR on testnet. Everything else stages after it.">
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {phases.map((p) => (
          <div key={p.name} className={cx("card p-5", p.current && "border-forest ring-1 ring-forest")}>
            <div className="flex items-center justify-between">
              <span className="font-semibold">{p.name}</span>
              {p.current && <span className="rounded-full bg-forest px-2 py-0.5 text-[10px] font-semibold text-[#f5f1e6]">IN PROGRESS</span>}
            </div>
            <div className="mt-0.5 text-xs text-ink-3">{p.when}</div>
            <ul className="mt-4 space-y-2 text-sm text-ink-2">
              {p.items.map((it) => (
                <li key={it} className="flex gap-2">
                  <ShieldCheck className={cx("mt-0.5 size-4 shrink-0", p.current ? "text-forest" : "text-line-strong")} />
                  {it}
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
    </Section>
  );
}

export function FinalCta() {
  return (
    <section className="mx-auto max-w-[1240px] px-4 sm:px-6">
      <div className="card flex flex-col items-start justify-between gap-6 p-8 sm:flex-row sm:items-center sm:p-10">
        <div>
          <h2 className="text-2xl font-semibold tracking-tight">Building a Zcash wallet?</h2>
          <p className="mt-2 max-w-xl text-[15px] text-ink-2">Call the ZEC SSL API instead of unshielding to an ephemeral address. Quotes, order state and settlement trails are all exposed over REST and a live stream.</p>
        </div>
        <div className="flex shrink-0 gap-3">
          <Link href="/app" className={buttonClass.secondary}>
            Open dashboard
          </Link>
          <Link href="/app/swap" className={buttonClass.primary}>
            Try a swap <ArrowRight className="size-4" />
          </Link>
        </div>
      </div>
    </section>
  );
}
