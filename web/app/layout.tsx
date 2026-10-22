import type { Metadata } from "next";
import { Inter, JetBrains_Mono } from "next/font/google";
import { getOverview } from "@/lib/api";
import { LiveProvider } from "@/lib/live";
import "./globals.css";

const inter = Inter({ subsets: ["latin"], variable: "--font-inter", display: "swap" });
const mono = JetBrains_Mono({ subsets: ["latin"], variable: "--font-jbmono", display: "swap" });

export const metadata: Metadata = {
  title: { default: "ZEC SSL — Shielded Settlement Layer", template: "%s · ZEC SSL" },
  description:
    "Swap between shielded ZEC and assets on NEAR Intents with no transparent-address hop. Settled by a bonded, threshold-signed operator network.",
};

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  const overview = await getOverview();
  return (
    <html lang="en" className={`${inter.variable} ${mono.variable}`}>
      <body className="min-h-dvh font-sans">
        <LiveProvider initial={overview}>{children}</LiveProvider>
      </body>
    </html>
  );
}
