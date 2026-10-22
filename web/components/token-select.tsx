"use client";

import { ChevronDown, Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { chainName, price } from "@/lib/format";
import type { IntentsToken } from "@/lib/types";
import { cx } from "./ui";

export function TokenBadge({ token, size = 28 }: { token: Pick<IntentsToken, "symbol" | "blockchain">; size?: number }) {
  const palette: Record<string, string> = {
    USDC: "bg-[#2775ca] text-white",
    USDT: "bg-[#26a17b] text-white",
    wNEAR: "bg-ink text-white",
    NEAR: "bg-ink text-white",
    ETH: "bg-[#627eea] text-white",
    BTC: "bg-[#f7931a] text-white",
    SOL: "bg-[#14151a] text-[#14f195]",
    ZEC: "bg-gold text-forest-900",
  };
  return (
    <span
      className={cx("grid shrink-0 place-items-center rounded-full text-[9.5px] font-bold", palette[token.symbol] ?? "bg-sunken text-ink-2")}
      style={{ width: size, height: size }}
      aria-hidden
    >
      {token.symbol.replace(/^w/, "").slice(0, 4)}
    </span>
  );
}

/** Searchable token picker over the live NEAR Intents token list. */
export function TokenSelect({ tokens, value, onChange }: { tokens: IntentsToken[]; value: IntentsToken | null; onChange: (t: IntentsToken) => void }) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);

  const list = useMemo(() => {
    const s = q.trim().toLowerCase();
    const hits = s ? tokens.filter((t) => t.symbol.toLowerCase().includes(s) || chainName(t.blockchain).toLowerCase().includes(s)) : tokens;
    return hits.slice(0, 60);
  }, [q, tokens]);

  return (
    <div className="relative" ref={ref}>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-haspopup="listbox"
        aria-expanded={open}
        className="flex items-center gap-2 rounded-full border border-line bg-surface py-1.5 pr-2.5 pl-1.5 text-sm font-semibold shadow-sm transition hover:bg-sunken"
      >
        {value ? <TokenBadge token={value} size={24} /> : <span className="size-6 rounded-full bg-sunken" />}
        <span>{value?.symbol ?? "Select"}</span>
        {value && <span className="text-xs font-medium text-ink-3">{chainName(value.blockchain)}</span>}
        <ChevronDown className="size-4 text-ink-3" />
      </button>
      {open && (
        <div className="absolute right-0 z-30 mt-2 w-72 overflow-hidden rounded-xl border border-line bg-surface shadow-[var(--shadow-float)]">
          <label className="flex items-center gap-2 border-b border-line px-3 py-2.5">
            <Search className="size-4 text-ink-3" />
            <input
              autoFocus
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder="Search token or chain"
              className="w-full bg-transparent text-sm outline-none placeholder:text-ink-3"
            />
          </label>
          <ul role="listbox" className="max-h-72 overflow-y-auto py-1">
            {list.map((t) => (
              <li key={t.assetId}>
                <button
                  type="button"
                  role="option"
                  aria-selected={t.assetId === value?.assetId}
                  onClick={() => {
                    onChange(t);
                    setOpen(false);
                    setQ("");
                  }}
                  className={cx("flex w-full items-center gap-3 px-3 py-2 text-left hover:bg-paper", t.assetId === value?.assetId && "bg-forest-50/60")}
                >
                  <TokenBadge token={t} />
                  <span className="min-w-0 flex-1">
                    <span className="block text-sm font-semibold">{t.symbol}</span>
                    <span className="block text-xs text-ink-3">on {chainName(t.blockchain)}</span>
                  </span>
                  <span className="tnum text-xs text-ink-2">{price(t.price)}</span>
                </button>
              </li>
            ))}
            {list.length === 0 && <li className="px-3 py-4 text-center text-sm text-ink-3">No matching tokens</li>}
          </ul>
        </div>
      )}
    </div>
  );
}
