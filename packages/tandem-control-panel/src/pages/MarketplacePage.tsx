import { useMemo, useState } from "react";
import { AnimatedPage, Badge, PageHeader, PanelCard, SplitView } from "../ui/index";
import { EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

type FeaturedMarketplacePack = {
  packId: string;
  title: string;
  summary: string;
  audience: string;
  categories: string[];
  tags: string[];
  highlight: string;
};

const FEATURED_PACKS: FeaturedMarketplacePack[] = [
  {
    packId: "planning-pack",
    title: "Planning Pack",
    summary:
      "Kickoff, weekly status, meeting notes, and action tracking for teams that need clarity fast.",
    audience: "Project leads, operators, and team managers",
    categories: ["planning", "project management", "operations"],
    tags: ["kickoff", "weekly status", "action tracking"],
    highlight: "Best starting point for general-purpose planning workflows.",
  },
  {
    packId: "writing-pack",
    title: "Writing Pack",
    summary:
      "Draft, polish, and repurpose articles with an SEO-aware workflow that keeps the story readable.",
    audience: "Writers, editors, and marketers",
    categories: ["writing", "seo", "content marketing"],
    tags: ["blog drafts", "copy editing", "content refresh"],
    highlight: "Useful when the output needs to read well and rank well.",
  },
  {
    packId: "research-pack",
    title: "Research Pack",
    summary:
      "Turn rough questions into structured briefs, source checks, and comparative analysis.",
    audience: "Researchers, analysts, and product teams",
    categories: ["research", "analysis", "fact finding"],
    tags: ["briefs", "source verification", "competitive scan"],
    highlight: "Good for turning uncertainty into a clear evidence trail.",
  },
  {
    packId: "build-pack",
    title: "Build Pack",
    summary:
      "Break features into implementation tasks, review changes, and keep engineering work moving.",
    audience: "Developers, reviewers, and release teams",
    categories: ["engineering", "delivery", "release"],
    tags: ["feature breakdown", "bug triage", "pr review"],
    highlight: "Best when you want a workflow that maps directly to coding work.",
  },
];

function safeString(value: unknown) {
  return String(value || "").trim();
}

export function MarketplacePage(_props: AppPageProps) {
  void _props;
  const [selectedPackId, setSelectedPackId] = useState(FEATURED_PACKS[0]?.packId || "");

  const selectedPack = useMemo(
    () => FEATURED_PACKS.find((entry) => entry.packId === selectedPackId) || FEATURED_PACKS[0],
    [selectedPackId]
  );

  return (
    <AnimatedPage className="grid gap-4">
      <PageHeader
        eyebrow="Marketplace"
        title="Marketplace coming soon"
        subtitle="This route is intentionally hidden by default. If someone enables it manually, it stays a placeholder until the tandem.ac marketplace is live."
        badges={
          <>
            <Badge tone="info">coming soon</Badge>
            <Badge tone="ghost">{FEATURED_PACKS.length} seed concepts</Badge>
            <Badge tone="ghost">disabled by default</Badge>
          </>
        }
      />

      <PanelCard
        title="Marketplace not live yet"
        subtitle="The public store will live on tandem.ac. This page is a placeholder until search, login, and purchase are ready."
      >
        <div className="grid gap-3">
          <div className="tcp-list-item">
            <div className="tcp-subtle text-xs uppercase tracking-[0.24em]">Status</div>
            <div className="mt-2 text-sm">
              The marketplace page is intentionally a placeholder until the tandem.ac catalog API
              and listing pages go live.
            </div>
          </div>
          <div className="flex flex-wrap gap-2">
            <button className="tcp-btn-primary" type="button" disabled>
              <i data-lucide="search"></i>
              Search marketplace
            </button>
            <button className="tcp-btn" type="button" disabled>
              <i data-lucide="external-link"></i>
              Open tandem.ac
            </button>
          </div>
        </div>
      </PanelCard>

      <SplitView
        main={
          <div className="grid gap-4">
            <PanelCard
              title="Seed concepts"
              subtitle="These are planning concepts, not published marketplace listings."
            >
              <div className="grid gap-3">
                {FEATURED_PACKS.map((pack) => {
                  const active = selectedPack?.packId === pack.packId;
                  return (
                    <button
                      key={pack.packId}
                      type="button"
                      className={`tcp-list-item text-left ${active ? "border-amber-400/70" : ""}`}
                      onClick={() => setSelectedPackId(pack.packId)}
                    >
                      <div className="flex items-start justify-between gap-3">
                        <div className="min-w-0 flex-1">
                          <div className="mb-1 flex items-center gap-2">
                            <strong>{pack.title}</strong>
                            <Badge tone="info">{pack.packId}</Badge>
                            <Badge tone="ghost">planned seed</Badge>
                          </div>
                          <div className="tcp-subtle text-sm">{pack.summary}</div>
                          <div className="mt-2 flex flex-wrap gap-2 text-xs">
                            <Badge tone="ghost">{pack.audience}</Badge>
                            {pack.categories.map((category) => (
                              <Badge key={`${pack.packId}-${category}`} tone="ghost">
                                {category}
                              </Badge>
                            ))}
                          </div>
                        </div>
                        <Badge tone="ghost">{pack.highlight}</Badge>
                      </div>
                    </button>
                  );
                })}
              </div>
            </PanelCard>

            <PanelCard
              title="What tandem.ac will own later"
              subtitle="The store belongs on the web, not inside the control panel."
            >
              <div className="grid gap-2 text-sm tcp-subtle">
                <div>• login and account state</div>
                <div>• pricing and purchase</div>
                <div>• pack detail pages and publisher pages</div>
                <div>• public search and browse results</div>
                <div>• future entitlement and redemption flows</div>
              </div>
            </PanelCard>
          </div>
        }
        aside={
          <PanelCard
            title="Selected concept"
            subtitle="This is a seed idea that will become a real listing later."
          >
            {selectedPack ? (
              <div className="grid gap-3">
                <div className="tcp-list-item">
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <strong>{selectedPack.title}</strong>
                    <Badge tone="info">{selectedPack.packId}</Badge>
                    <Badge tone="ghost">concept only</Badge>
                  </div>
                  <div className="tcp-subtle text-sm">{selectedPack.summary}</div>
                  <div className="mt-2 flex flex-wrap gap-2 text-xs">
                    {selectedPack.categories.map((category) => (
                      <Badge key={`${selectedPack.packId}-${category}-aside`} tone="ghost">
                        {category}
                      </Badge>
                    ))}
                    {selectedPack.tags.map((tag) => (
                      <Badge key={`${selectedPack.packId}-${tag}`} tone="ghost">
                        {tag}
                      </Badge>
                    ))}
                  </div>
                </div>

                <div className="tcp-list-item">
                  <div className="tcp-subtle text-xs uppercase tracking-[0.24em]">
                    Why it exists
                  </div>
                  <div className="mt-2 text-sm">{selectedPack.highlight}</div>
                </div>
              </div>
            ) : (
              <EmptyState text="Select a featured concept." />
            )}
          </PanelCard>
        }
      />
    </AnimatedPage>
  );
}
