"use client";

import { ArrowRight, Menu, X } from "lucide-react";
import Link from "next/link";
import { useState } from "react";
import { num, price } from "@/lib/format";
import { useLive } from "@/lib/live";
import { buttonClass, Change, cx, Logo, StatusDot } from "./ui";

/** Thin market strip above the header, like the global stats bar on CoinGecko. */
function TickerStrip() {
  const { market, zcash, near, intents, connection } = useLive();
  const zec = market.find((a) => a.id === "zcash");
  const nearAsset = market.find((a) => a.id === "near");
  return (
    <div className="border-b border-line bg-surface">
      <div className="mx-auto flex max-w-[1240px] items-center gap-6 overflow-x-auto px-4 py-2 text-xs whitespace-nowrap text-ink-2 sm:px-6">
        <span className="flex items-center gap-1.5">
          ZEC <span className="tnum font-semibold text-ink">{price(zec?.price)}</span>
          <Change value={zec?.change24h} />
        </span>
        <span className="flex items-center gap-1.5">
          NEAR <span className="tnum font-semibold text-ink">{price(nearAsset?.price)}</span>
          <Change value={nearAsset?.change24h} />
        </span>
        <span>
          Zcash block <span className="tnum font-semibold text-ink">{num(zcash?.height)}</span>
        </span>
        <span>
          NEAR {near?.network ?? ""} block <span className="tnum font-semibold text-ink">{num(near?.height)}</span>
        </span>
        <span>
          Intents assets <span className="tnum font-semibold text-ink">{num(intents?.tokens.length)}</span> on{" "}
          <span className="tnum font-semibold text-ink">{num(intents?.chains.length)}</span> chains
        </span>
        <span className="ml-auto flex items-center gap-1.5">
          <StatusDot state={connection === "live" ? "ok" : connection === "connecting" ? "warn" : "down"} />
          {connection === "live" ? "Live data" : connection === "connecting" ? "Connecting" : "Offline"}
        </span>
      </div>
    </div>
  );
}

const links = [
  { href: "/app/swap", label: "Swap" },
  { href: "/app/markets", label: "Markets" },
  { href: "/app/operators", label: "Network" },
  { href: "/#how-it-works", label: "How it works" },
  { href: "/#security", label: "Security" },
];

export function SiteHeader() {
  const [open, setOpen] = useState(false);
  return (
    <header className="sticky top-0 z-40">
      <TickerStrip />
      <div className="border-b border-line bg-paper/90 backdrop-blur">
        <div className="mx-auto flex h-16 max-w-[1240px] items-center gap-4 px-4 sm:px-6 md:gap-8">
          <Link href="/" aria-label="ZEC SSL home" className="shrink-0">
            <Logo />
          </Link>
          <nav className="hidden items-center gap-6 text-sm font-medium text-ink-2 md:flex">
            {links.map((l) => (
              <Link key={l.href} href={l.href} className="transition hover:text-ink">
                {l.label}
              </Link>
            ))}
          </nav>
          <div className="ml-auto flex items-center gap-2">
            <span className="hidden sm:block">
              <Link href="/app" className={buttonClass.secondary}>
                Dashboard
              </Link>
            </span>
            <Link href="/app/swap" className={cx(buttonClass.primary, "px-4 whitespace-nowrap sm:px-5")}>
              Launch app <ArrowRight className="hidden size-4 sm:block" />
            </Link>
            <button className="grid size-10 place-items-center rounded-full hover:bg-sunken md:hidden" onClick={() => setOpen((o) => !o)} aria-label="Menu" aria-expanded={open}>
              {open ? <X className="size-5" /> : <Menu className="size-5" />}
            </button>
          </div>
        </div>
        {open && (
          <nav className="border-t border-line px-4 py-3 md:hidden">
            {[...links, { href: "/app", label: "Dashboard" }].map((l) => (
              <Link key={l.href} href={l.href} onClick={() => setOpen(false)} className="block rounded-lg px-2 py-2.5 text-sm font-medium hover:bg-sunken">
                {l.label}
              </Link>
            ))}
          </nav>
        )}
      </div>
    </header>
  );
}

export function SiteFooter() {
  const cols = [
    { title: "Product", links: [["Swap", "/app/swap"], ["Dashboard", "/app"], ["Markets", "/app/markets"], ["Activity", "/app/activity"]] },
    { title: "Network", links: [["Operators", "/app/operators"], ["Treasury", "/app/treasury"], ["System status", "/app/status"]] },
    {
      title: "Ecosystem",
      links: [
        ["Zcash FROST", "https://github.com/ZcashFoundation/frost"],
        ["NEAR Intents", "https://docs.near-intents.org"],
        ["Zebra", "https://github.com/ZcashFoundation/zebra"],
        ["librustzcash", "https://github.com/zcash/librustzcash"],
      ],
    },
  ];
  return (
    <footer className="border-t border-line bg-surface">
      <div className="mx-auto grid max-w-[1240px] gap-10 px-4 py-12 sm:px-6 md:grid-cols-[1.4fr_repeat(3,1fr)]">
        <div>
          <Logo />
          <p className="mt-4 max-w-xs text-sm leading-relaxed text-ink-3">
            A bonded, threshold-signed operator network that settles cross-chain swaps directly into and out of Zcash&apos;s shielded pool.
          </p>
        </div>
        {cols.map((c) => (
          <div key={c.title}>
            <div className="text-sm font-semibold">{c.title}</div>
            <ul className="mt-3 space-y-2 text-sm text-ink-3">
              {c.links.map(([label, href]) => (
                <li key={href}>
                  {href.startsWith("http") ? (
                    <a href={href} target="_blank" rel="noreferrer" className="hover:text-ink">
                      {label}
                    </a>
                  ) : (
                    <Link href={href} className="hover:text-ink">
                      {label}
                    </Link>
                  )}
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
      <div className="border-t border-line">
        <div className="mx-auto flex max-w-[1240px] flex-wrap items-center justify-between gap-3 px-4 py-5 text-xs text-ink-3 sm:px-6">
          <span>© {new Date().getFullYear()} ZEC SSL. Open source. Testnet software; not financial advice.</span>
          <span>Market data: CoinGecko · Zcash data: Blockchair, 3xpl or your Zebra node · Routing: NEAR Intents 1Click</span>
        </div>
      </div>
    </footer>
  );
}
