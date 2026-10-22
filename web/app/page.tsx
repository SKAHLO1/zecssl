import { ChainSection, Comparison, FinalCta, Hero, LiveStats, MarketsSection, Roadmap, Security } from "@/components/home";
import { SiteFooter, SiteHeader } from "@/components/site-chrome";

export default function HomePage() {
  return (
    <>
      <SiteHeader />
      <main className="space-y-20 pb-20 sm:space-y-24">
        <div>
          <Hero />
          <LiveStats />
        </div>
        <MarketsSection />
        <Comparison />
        <ChainSection />
        <Security />
        <Roadmap />
        <FinalCta />
      </main>
      <SiteFooter />
    </>
  );
}
