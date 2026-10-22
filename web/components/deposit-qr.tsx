"use client";

import QRCode from "qrcode";
import { useEffect, useState } from "react";
import { cx } from "./ui";

/**
 * A QR code for a ZIP 321 payment URI (address, amount and memo), so a mobile Zcash
 * wallet can fill the whole deposit in one scan. Rendered as inline SVG, locally.
 */
export function DepositQr({ uri, size = 184, className }: { uri: string; size?: number; className?: string }) {
  const [svg, setSvg] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    QRCode.toString(uri, { type: "svg", margin: 1, errorCorrectionLevel: "M", color: { dark: "#13211b", light: "#ffffff" } })
      .then((s) => !cancelled && setSvg(s))
      .catch(() => !cancelled && setSvg(null));
    return () => {
      cancelled = true;
    };
  }, [uri]);
  return (
    <div
      role="img"
      aria-label="QR code for the deposit: address, amount and memo"
      className={cx("shrink-0 overflow-hidden rounded-xl border border-line bg-white p-2", className)}
      style={{ width: size, height: size }}
      // The SVG is generated locally from the order's payment URI.
      dangerouslySetInnerHTML={svg ? { __html: svg } : undefined}
    />
  );
}
