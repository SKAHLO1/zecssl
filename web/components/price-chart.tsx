"use client";

import { useEffect, useId, useMemo, useRef, useState } from "react";
import { api } from "@/lib/api";
import { price } from "@/lib/format";
import type { ChartRange, PriceChart as Chart } from "@/lib/types";
import { Change, cx, Skeleton } from "./ui";

const ranges: ChartRange[] = ["1D", "7D", "30D", "1Y"];
const W = 720;
const H = 260;
const PAD_T = 12;
const PAD_B = 24;

function fmtTime(t: number, range: ChartRange): string {
  const d = new Date(t);
  if (range === "1D") return d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  if (range === "1Y") return d.toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" });
  return d.toLocaleDateString("en-US", { month: "short", day: "numeric", hour: "numeric" });
}

/**
 * Exchange-style price chart: headline price and change for the selected range, an area
 * line, and a crosshair with the hovered price and time. Single series, so no legend.
 */
export function PriceChart({ id, name, livePrice }: { id: string; name: string; livePrice?: number | null }) {
  const [range, setRange] = useState<ChartRange>("1D");
  const [chart, setChart] = useState<Chart | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hover, setHover] = useState<number | null>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const gradId = useId();

  useEffect(() => {
    let cancelled = false;
    setError(null);
    const load = () =>
      api
        .chart(id, range)
        .then((c) => !cancelled && setChart(c))
        .catch((e: Error) => !cancelled && setError(e.message));
    load();
    const t = setInterval(load, range === "1D" ? 120_000 : 600_000);
    return () => {
      cancelled = true;
      clearInterval(t);
    };
  }, [id, range]);

  const pts = useMemo(() => (chart && chart.id === id && chart.range === range ? chart.points : []), [chart, id, range]);
  const geo = useMemo(() => {
    if (pts.length < 2) return null;
    const prices = pts.map((p) => p.price);
    const min = Math.min(...prices);
    const max = Math.max(...prices);
    const span = max - min || 1;
    const t0 = pts[0].t;
    const t1 = pts[pts.length - 1].t;
    const x = (t: number) => ((t - t0) / (t1 - t0)) * W;
    const y = (v: number) => PAD_T + (1 - (v - min) / span) * (H - PAD_T - PAD_B);
    const line = pts.map((p, i) => `${i ? "L" : "M"}${x(p.t).toFixed(1)},${y(p.price).toFixed(1)}`).join("");
    return { min, max, x, y, line, area: `${line}L${W},${H - PAD_B}L0,${H - PAD_B}Z` };
  }, [pts]);

  const first = pts[0]?.price;
  const last = livePrice ?? pts[pts.length - 1]?.price;
  const hovered = hover != null ? pts[hover] : null;
  const shown = hovered?.price ?? last;
  const change = first && shown ? ((shown - first) / first) * 100 : null;
  const rising = (change ?? 0) >= 0;
  const stroke = rising ? "var(--color-up)" : "var(--color-down)";

  function onMove(e: React.PointerEvent<SVGSVGElement>) {
    if (!geo || !svgRef.current) return;
    const rect = svgRef.current.getBoundingClientRect();
    const tx = ((e.clientX - rect.left) / rect.width) * W;
    let best = 0;
    let bestD = Infinity;
    for (let i = 0; i < pts.length; i++) {
      const d = Math.abs(geo.x(pts[i].t) - tx);
      if (d < bestD) {
        bestD = d;
        best = i;
      }
    }
    setHover(best);
  }

  return (
    <div>
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <div className="text-[13px] text-ink-3">{name} price</div>
          <div className="tnum mt-1 text-[32px] leading-none font-semibold tracking-tight">{shown ? price(shown) : <Skeleton className="h-8 w-40" />}</div>
          <div className="mt-2 flex items-center gap-2 text-[13px]">
            <Change value={change} />
            <span className="text-ink-3">{hovered ? fmtTime(hovered.t, range) : `past ${range === "1D" ? "24 hours" : range === "7D" ? "week" : range === "30D" ? "month" : "year"}`}</span>
          </div>
        </div>
        <div role="tablist" aria-label="Chart range" className="flex rounded-full bg-sunken p-1">
          {ranges.map((r) => (
            <button
              key={r}
              role="tab"
              aria-selected={r === range}
              onClick={() => setRange(r)}
              className={cx(
                "rounded-full px-3 py-1 text-xs font-semibold transition",
                r === range ? "bg-surface text-ink shadow-sm" : "text-ink-3 hover:text-ink",
              )}
            >
              {r}
            </button>
          ))}
        </div>
      </div>

      <div className="relative mt-4">
        {geo ? (
          <svg
            ref={svgRef}
            viewBox={`0 0 ${W} ${H}`}
            preserveAspectRatio="none"
            className="h-[240px] w-full touch-none"
            onPointerMove={onMove}
            onPointerLeave={() => setHover(null)}
            role="img"
            aria-label={`${name} price, ${range}. Low ${price(geo.min)}, high ${price(geo.max)}.`}
          >
            <defs>
              <linearGradient id={gradId} x1="0" x2="0" y1="0" y2="1">
                <stop offset="0" stopColor={stroke} stopOpacity="0.16" />
                <stop offset="1" stopColor={stroke} stopOpacity="0" />
              </linearGradient>
            </defs>
            {[0.25, 0.5, 0.75].map((f) => (
              <line key={f} x1="0" x2={W} y1={PAD_T + f * (H - PAD_T - PAD_B)} y2={PAD_T + f * (H - PAD_T - PAD_B)} stroke="var(--color-line)" strokeDasharray="2 4" vectorEffect="non-scaling-stroke" />
            ))}
            <path d={geo.area} fill={`url(#${gradId})`} />
            <path d={geo.line} fill="none" stroke={stroke} strokeWidth="2" vectorEffect="non-scaling-stroke" strokeLinejoin="round" />
            {hovered && (
              <line x1={geo.x(hovered.t)} x2={geo.x(hovered.t)} y1={PAD_T} y2={H - PAD_B} stroke="var(--color-ink-3)" strokeDasharray="3 3" vectorEffect="non-scaling-stroke" />
            )}
          </svg>
        ) : (
          <div className="grid h-[240px] place-items-center rounded-xl bg-sunken/60 text-sm text-ink-3">
            {error ? `Chart unavailable: ${error}` : "Loading chart…"}
          </div>
        )}
        {geo && hovered && (
          <span
            className="pointer-events-none absolute size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-surface"
            style={{ left: `${(geo.x(hovered.t) / W) * 100}%`, top: `${(geo.y(hovered.price) / H) * 240}px`, background: stroke }}
          />
        )}
        {geo && (
          <div className="tnum pointer-events-none absolute top-1 right-0 flex h-[216px] flex-col justify-between text-right text-[11px] text-ink-3">
            <span className="rounded bg-surface/80 px-1">{price(geo.max)}</span>
            <span className="rounded bg-surface/80 px-1">{price(geo.min)}</span>
          </div>
        )}
        {geo && (
          <div className="tnum mt-1 flex justify-between text-[11px] text-ink-3">
            <span>{fmtTime(pts[0].t, range)}</span>
            <span>{fmtTime(pts[Math.floor(pts.length / 2)].t, range)}</span>
            <span>{fmtTime(pts[pts.length - 1].t, range)}</span>
          </div>
        )}
      </div>
    </div>
  );
}
