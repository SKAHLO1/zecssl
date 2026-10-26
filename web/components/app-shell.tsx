"use client";

import { Activity, ArrowRight, ChartCandlestick, Droplets, Gauge, LayoutDashboard, LogOut, Menu, Network, Repeat, Vault, Wallet, X } from "lucide-react";
import Image from "next/image";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState, type ReactNode } from "react";
import { num } from "@/lib/format";
import { useLive } from "@/lib/live";
import { useNearWallet } from "@/lib/near-wallet";
import { buttonClass, cx, Logo, StatusDot } from "./ui";

const nav = [
  { href: "/app", label: "Dashboard", icon: LayoutDashboard },
  { href: "/app/swap", label: "Swap", icon: Repeat },
  { href: "/app/operators", label: "Operator network", icon: Network },
  { href: "/app/treasury", label: "Treasury", icon: Vault },
  { href: "/app/markets", label: "Markets", icon: ChartCandlestick },
  { href: "/app/activity", label: "Activity", icon: Activity },
  { href: "/app/status", label: "System status", icon: Gauge },
  { href: "/app/faucet", label: "Testnet faucet", icon: Droplets },
];

function Sidebar({ onNavigate }: { onNavigate?: () => void }) {
  const path = usePathname();
  return (
    <div className="flex h-full flex-col bg-forest-900 px-3 py-5 text-[#f5f1e6]">
      <Link href="/" className="px-2" onClick={onNavigate}>
        <Logo tone="light" />
      </Link>
      <nav className="mt-8 space-y-0.5">
        {nav.map(({ href, label, icon: Icon }) => {
          const active = href === "/app" ? path === "/app" : path.startsWith(href);
          return (
            <Link
              key={href}
              href={href}
              onClick={onNavigate}
              aria-current={active ? "page" : undefined}
              className={cx(
                "flex items-center gap-3 rounded-xl px-3 py-2.5 text-sm font-medium transition",
                active ? "bg-white/10 text-white" : "text-[#f5f1e6]/65 hover:bg-white/5 hover:text-white",
              )}
            >
              <Icon className="size-[18px]" />
              {label}
            </Link>
          );
        })}
      </nav>
      <div className="relative mt-auto overflow-hidden rounded-2xl">
        <Image src="/images/control-room.jpg" alt="" width={480} height={320} className="h-28 w-full object-cover opacity-50" />
        <div className="absolute inset-0 bg-gradient-to-t from-forest-900 via-forest-900/50 to-transparent" />
        <div className="absolute inset-x-0 bottom-0 p-3.5 text-xs leading-relaxed">
          <div className="font-semibold">Shielded.</div>
          <div className="text-[#f5f1e6]/70">Cross-chain. No public hops.</div>
        </div>
      </div>
    </div>
  );
}

/** The connected NEAR wallet, on every app page: connect, see which account, disconnect. */
function WalletButton() {
  const { near } = useLive();
  const wallet = useNearWallet(near?.network === "mainnet" ? "mainnet" : "testnet");
  if (!wallet.accountId) {
    return (
      <button className={cx(buttonClass.secondary, "px-3 py-2 sm:px-4")} onClick={() => void wallet.connect()} disabled={!wallet.ready} aria-label="Connect NEAR wallet">
        <Wallet className="size-4" />
        <span className="hidden sm:inline">Connect wallet</span>
      </button>
    );
  }
  return (
    <div className="flex items-center gap-1 rounded-full border border-line bg-surface py-1 pr-1 pl-3 text-xs">
      <StatusDot state="ok" />
      <span className="max-w-[8rem] truncate font-medium sm:max-w-[12rem]" title={wallet.accountId}>
        {wallet.accountId}
      </span>
      <button
        className="inline-flex items-center gap-1 rounded-full px-2 py-1.5 text-ink-2 hover:bg-sunken hover:text-ink"
        onClick={() => void wallet.disconnect()}
        aria-label="Disconnect wallet"
      >
        <LogOut className="size-3.5" />
        <span className="hidden sm:inline">Disconnect</span>
      </button>
    </div>
  );
}

function TopBar({ title, onMenu }: { title: string; onMenu: () => void }) {
  const { near, zcash, connection } = useLive();
  return (
    <header className="sticky top-0 z-30 border-b border-line bg-paper/90 backdrop-blur">
      <div className="flex h-16 items-center gap-3 px-4 sm:px-6 lg:px-8">
        <button className="grid size-10 place-items-center rounded-full hover:bg-sunken lg:hidden" onClick={onMenu} aria-label="Open navigation">
          <Menu className="size-5" />
        </button>
        <h1 className="text-lg font-semibold tracking-tight">{title}</h1>
        <div className="ml-auto flex items-center gap-2">
          <span className="hidden items-center gap-2 rounded-full border border-line bg-surface px-3 py-1.5 text-xs text-ink-2 md:flex">
            <StatusDot state={connection === "live" ? "ok" : connection === "connecting" ? "warn" : "down"} />
            {connection === "live" ? "Live" : connection === "connecting" ? "Connecting" : "Offline"}
            <span className="text-line-strong">|</span>
            {near?.network === "testnet" ? "Zcash mainnet" : "Zcash"} <span className="tnum font-semibold text-ink">{num(zcash?.height)}</span>
            <span className="text-line-strong">|</span>
            NEAR {near?.network} <span className="tnum font-semibold text-ink">{num(near?.height)}</span>
          </span>
          <WalletButton />
          <Link href="/app/swap" className={cx(buttonClass.primary, "px-4 py-2")}>
            Swap <ArrowRight className="size-4" />
          </Link>
        </div>
      </div>
    </header>
  );
}

const titles: [string, string][] = [
  ["/app/swap", "Swap"],
  ["/app/operators", "Operator network"],
  ["/app/treasury", "Treasury"],
  ["/app/markets", "Markets"],
  ["/app/activity", "Activity"],
  ["/app/status", "System status"],
  ["/app/faucet", "Testnet faucet"],
  ["/app/orders", "Order"],
];

export function AppShell({ children }: { children: ReactNode }) {
  const path = usePathname();
  const [open, setOpen] = useState(false);
  const { loaded, connection, near } = useLive();
  useEffect(() => setOpen(false), [path]);
  const title = titles.find(([p]) => path.startsWith(p))?.[1] ?? "Dashboard";

  return (
    <div className="min-h-dvh lg:pl-64">
      <aside className="fixed inset-y-0 left-0 z-40 hidden w-64 lg:block">
        <Sidebar />
      </aside>
      {open && (
        <div className="fixed inset-0 z-50 lg:hidden">
          <button className="absolute inset-0 bg-forest-900/50" onClick={() => setOpen(false)} aria-label="Close navigation" />
          <aside className="relative h-full w-72">
            <Sidebar onNavigate={() => setOpen(false)} />
            <button className="absolute top-5 right-3 grid size-9 place-items-center rounded-full text-white hover:bg-white/10" onClick={() => setOpen(false)} aria-label="Close">
              <X className="size-5" />
            </button>
          </aside>
        </div>
      )}
      <TopBar title={title} onMenu={() => setOpen(true)} />
      {near?.network === "testnet" && (
        <div className="border-b border-gold/30 bg-gold-50 px-4 py-2 text-xs text-gold-600 sm:px-6 lg:px-8">
          <span className="font-semibold">Testnet.</span> Swaps settle on Zcash testnet and NEAR testnet; TAZ and test USDC have no value.{" "}
          <Link href="/app/faucet" className="font-semibold underline underline-offset-2">
            Get test funds
          </Link>
        </div>
      )}
      {!loaded && connection !== "live" && (
        <div className="border-b border-warn/30 bg-warn-50 px-4 py-2.5 text-sm text-warn sm:px-6 lg:px-8">
          Can&apos;t reach the ZEC SSL API yet. Live data will appear as soon as it responds.
        </div>
      )}
      <main className="px-4 py-6 sm:px-6 lg:px-8 lg:py-8">{children}</main>
    </div>
  );
}
