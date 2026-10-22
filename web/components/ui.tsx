import type { ReactNode } from "react";
import { pct } from "@/lib/format";

export function cx(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}

export function Logo({ tone = "dark", compact = false }: { tone?: "dark" | "light"; compact?: boolean }) {
  const ink = tone === "light" ? "text-[#f5f1e6]" : "text-ink";
  return (
    <span className={cx("inline-flex items-center gap-2.5", ink)}>
      <svg viewBox="0 0 32 32" className="size-8 shrink-0" aria-hidden>
        <path d="M16 2.5 4.5 7v8.6c0 7 4.8 12.4 11.5 14.4 6.7-2 11.5-7.4 11.5-14.4V7L16 2.5Z" fill="#c4862a" />
        <path d="M16 5.6 7.3 9v6.6c0 5.6 3.7 9.9 8.7 11.6 5-1.7 8.7-6 8.7-11.6V9L16 5.6Z" fill="#12392d" />
        <path d="M11.6 11.4h8.8v1.9l-5.9 6.9h5.9v2.2h-8.8v-1.9l5.9-6.9h-5.9z" fill="#f2c063" />
        <path d="M15.1 9.3h1.8v2.2h-1.8zM15.1 22.4h1.8v2.2h-1.8z" fill="#f2c063" />
      </svg>
      {!compact && (
        <span className="leading-none whitespace-nowrap">
          <span className="block text-[17px] font-semibold tracking-tight">ZEC SSL</span>
          <span className={cx("mt-0.5 hidden text-[10.5px] font-medium sm:block", tone === "light" ? "text-[#f5f1e6]/60" : "text-ink-3")}>
            Shielded Settlement Layer
          </span>
        </span>
      )}
    </span>
  );
}

/** Signed percentage change, colored by direction with a triangle so it never relies on color alone. */
export function Change({ value, className, digits = 2 }: { value: number | null | undefined; className?: string; digits?: number }) {
  if (value == null || !Number.isFinite(value)) return <span className={cx("text-ink-3", className)}>—</span>;
  const up = value >= 0;
  return (
    <span className={cx("tnum inline-flex items-center gap-0.5 font-medium", up ? "text-up" : "text-down", className)}>
      <svg viewBox="0 0 10 10" className={cx("size-2", !up && "rotate-180")} aria-hidden>
        <path d="M5 1.5 9 8.5H1z" fill="currentColor" />
      </svg>
      <span className="sr-only">{up ? "up" : "down"}</span>
      {pct(value, digits)}
    </span>
  );
}

export function Sparkline({ data, up, width = 120, height = 36 }: { data: number[]; up?: boolean; width?: number; height?: number }) {
  if (data.length < 2) return <div style={{ width, height }} />;
  const min = Math.min(...data);
  const max = Math.max(...data);
  const span = max - min || 1;
  const step = width / (data.length - 1);
  const pts = data.map((v, i) => `${(i * step).toFixed(1)},${(height - 2 - ((v - min) / span) * (height - 4)).toFixed(1)}`);
  const rising = up ?? data[data.length - 1] >= data[0];
  return (
    <svg width={width} height={height} viewBox={`0 0 ${width} ${height}`} aria-hidden className="overflow-visible">
      <polyline points={pts.join(" ")} fill="none" stroke={rising ? "var(--color-up)" : "var(--color-down)"} strokeWidth="1.5" strokeLinejoin="round" strokeLinecap="round" />
    </svg>
  );
}

type Tone = "neutral" | "up" | "down" | "warn" | "brand" | "gold";

const toneClass: Record<Tone, string> = {
  neutral: "bg-sunken text-ink-2",
  up: "bg-up-50 text-up",
  down: "bg-down-50 text-down",
  warn: "bg-warn-50 text-warn",
  brand: "bg-forest-50 text-forest",
  gold: "bg-gold-50 text-gold-600",
};

export function Badge({ tone = "neutral", children, dot, className }: { tone?: Tone; children: ReactNode; dot?: boolean; className?: string }) {
  return (
    <span className={cx("inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-xs font-medium whitespace-nowrap", toneClass[tone], className)}>
      {dot && <span className="size-1.5 rounded-full bg-current" />}
      {children}
    </span>
  );
}

export function StatusDot({ state }: { state: "ok" | "warn" | "down" | "off" }) {
  const color = { ok: "bg-up", warn: "bg-warn", down: "bg-down", off: "bg-line-strong" }[state];
  return <span className={cx("inline-block size-2 shrink-0 rounded-full", color, state === "ok" && "live-dot")} aria-hidden />;
}

export function CoinIcon({ src, alt, size = 28 }: { src?: string | null; alt: string; size?: number }) {
  if (!src) {
    return (
      <span className="grid shrink-0 place-items-center rounded-full bg-sunken text-[10px] font-semibold text-ink-2" style={{ width: size, height: size }}>
        {alt.slice(0, 3).toUpperCase()}
      </span>
    );
  }
  // eslint-disable-next-line @next/next/no-img-element
  return <img src={src} alt={alt} width={size} height={size} className="shrink-0 rounded-full" />;
}

export function Card({ title, action, children, className, bodyClass }: { title?: ReactNode; action?: ReactNode; children: ReactNode; className?: string; bodyClass?: string }) {
  return (
    <section className={cx("card", className)}>
      {(title || action) && (
        <header className="flex items-center justify-between gap-3 px-5 pt-4 pb-3">
          {title && <h2 className="text-[15px] font-semibold tracking-tight">{title}</h2>}
          {action}
        </header>
      )}
      <div className={cx(title || action ? "px-5 pb-5" : "p-5", bodyClass)}>{children}</div>
    </section>
  );
}

export function Stat({ label, value, sub, icon }: { label: string; value: ReactNode; sub?: ReactNode; icon?: ReactNode }) {
  return (
    <div className="card flex items-start gap-3.5 p-4">
      {icon && <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-forest-50 text-forest">{icon}</span>}
      <div className="min-w-0">
        <div className="text-[13px] text-ink-3">{label}</div>
        <div className="tnum mt-0.5 truncate text-xl font-semibold tracking-tight">{value}</div>
        {sub && <div className="mt-0.5 text-xs text-ink-3">{sub}</div>}
      </div>
    </div>
  );
}

export function EmptyState({ icon, title, children, action }: { icon?: ReactNode; title: string; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className="flex flex-col items-center rounded-xl border border-dashed border-line-strong bg-paper/60 px-6 py-8 text-center">
      {icon && <span className="mb-3 grid size-10 place-items-center rounded-full bg-sunken text-ink-2">{icon}</span>}
      <div className="text-sm font-semibold">{title}</div>
      {children && <div className="mt-1 max-w-md text-[13px] leading-relaxed text-ink-3">{children}</div>}
      {action && <div className="mt-4">{action}</div>}
    </div>
  );
}

export function Skeleton({ className }: { className?: string }) {
  return <span className={cx("inline-block animate-pulse rounded-md bg-sunken", className)} />;
}

export function Mono({ children, className }: { children: ReactNode; className?: string }) {
  return <span className={cx("font-mono text-[12.5px]", className)}>{children}</span>;
}

export const buttonClass = {
  primary:
    "inline-flex items-center justify-center gap-2 rounded-full bg-forest px-5 py-2.5 text-sm font-semibold text-[#f5f1e6] transition hover:bg-forest-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-forest disabled:cursor-not-allowed disabled:opacity-50",
  secondary:
    "inline-flex items-center justify-center gap-2 rounded-full border border-line-strong bg-surface px-5 py-2.5 text-sm font-semibold text-ink transition hover:bg-sunken focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-forest",
  ghost: "inline-flex items-center gap-1 text-sm font-medium text-forest hover:underline underline-offset-4",
};
