import { useMemo, useState } from "react";
import { AnimatedPage, Badge, PageHeader, PanelCard, SplitView } from "../ui/index";
import { EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

const MARKETPLACE_BASE_URL = "https://tandem.ac/marketplace";

type FeaturedMarketplacePack = {
  packId: string;
  title: string;
  summary: string;
  audience: string;
  searchQuery: string;
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
    searchQuery: "planning workflow pack",
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
    searchQuery: "writing workflow pack",
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
    searchQuery: "research workflow pack",
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
    searchQuery: "build workflow pack",
    categories: ["engineering", "delivery", "release"],
    tags: ["feature breakdown", "bug triage", "pr review"],
    highlight: "Best when you want a workflow that maps directly to coding work.",
  },
];

function safeString(value: unknown) {
  return String(value || "").trim();
}

function marketplaceUrl(path = "", params?: Record<string, string>) {
  const base = MARKETPLACE_BASE_URL.replace(/\/+$/, "");
  const cleanPath = path ? `/${String(path).replace(/^\/+/, "")}` : "";
  const url = new URL(`${base}${cleanPath}`);
  for (const [key, value] of Object.entries(params || {})) {
    const text = safeString(value);
    if (text) {
      url.searchParams.set(key, text);
    }
  }
  return url.toString();
}

export function MarketplacePage(_props: AppPageProps) {
  void _props;
  const [selectedPackId, setSelectedPackId] = useState(FEATURED_PACKS[0]?.packId || "");
  const [searchQuery, setSearchQuery] = useState("");

  const selectedPack = useMemo(
    () => FEATURED_PACKS.find((entry) => entry.packId === selectedPackId) || FEATURED_PACKS[0],
    [selectedPackId]
  );

  const marketplaceHomeUrl = marketplaceUrl();
  const externalSearchUrl = marketplaceUrl("/search", { q: searchQuery });
  const selectedSearchUrl = marketplaceUrl("/search", { q: selectedPack?.searchQuery || "" });

  return (
    <AnimatedPage className="grid gap-4">
      <PageHeader
        eyebrow="Marketplace"
        title="Workflow packs on tandem.ac"
        subtitle="These are starter concepts for the public marketplace. Search tandem.ac for live catalog results, login, and purchase."
        badges={
          <>
            <Badge tone="info">{FEATURED_PACKS.length} featured packs</Badge>
            <Badge tone="ghost">browse only</Badge>
            <Badge tone="ghost">tandem.ac source of truth</Badge>
          </>
        }
      />

      <PanelCard
        title="Find a workflow pack"
        subtitle="Search opens tandem.ac marketplace results. The control panel does not own checkout or install."
      >
        <form
          className="flex flex-col gap-3 md:flex-row md:items-center"
          onSubmit={(event) => {
            event.preventDefault();
            if (!safeString(searchQuery)) return;
            window.open(externalSearchUrl, "_blank", "noopener,noreferrer");
          }}
        >
          <input
            className="tcp-input flex-1"
            value={searchQuery}
            onChange={(event) => setSearchQuery(event.target.value)}
            placeholder="Search for planning, writing, research, or build packs"
          />
          <div className="flex flex-wrap gap-2">
            <a
              className="tcp-btn-primary"
              href={externalSearchUrl}
              target="_blank"
              rel="noreferrer"
            >
              <i data-lucide="search"></i>
              Search on tandem.ac
            </a>
            <a className="tcp-btn" href={marketplaceHomeUrl} target="_blank" rel="noreferrer">
              <i data-lucide="external-link"></i>
              Open marketplace home
            </a>
          </div>
        </form>
      </PanelCard>

      <SplitView
        main={
          <div className="grid gap-4">
            <PanelCard
              title="Featured shelf"
              subtitle="These are starter concepts we plan to seed on tandem.ac, not live inventory."
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
              title="What tandem.ac owns"
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
            subtitle="Search tandem.ac for live results matching this starter concept."
            actions={
              <div className="flex flex-wrap gap-2">
                <a
                  className="tcp-btn-primary"
                  href={selectedSearchUrl}
                  target="_blank"
                  rel="noreferrer"
                >
                  <i data-lucide="external-link"></i>
                  Search concept
                </a>
                <a className="tcp-btn" href={selectedSearchUrl} target="_blank" rel="noreferrer">
                  <i data-lucide="search"></i>
                  Search similar
                </a>
              </div>
            }
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

                <div className="tcp-list-item">
                  <div className="tcp-subtle text-xs uppercase tracking-[0.24em]">Browse flow</div>
                  <div className="mt-2 grid gap-2 text-sm tcp-subtle">
                    <div>1. Search or browse on tandem.ac</div>
                    <div>2. Sign in on the web marketplace</div>
                    <div>3. Purchase or redeem there later</div>
                    <div>4. Open the pack detail page from the listing</div>
                  </div>
                </div>
              </div>
            ) : (
              <EmptyState text="Select a featured pack to open its listing." />
            )}
          </PanelCard>
        }
      />
    </AnimatedPage>
  );
}
