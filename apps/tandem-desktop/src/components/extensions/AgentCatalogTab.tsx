import { useMemo, useState } from "react";
import { openPath } from "@tauri-apps/plugin-opener";
import { Copy, FolderOpen, Search } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import agentCatalog from "@/generated/agent-catalog.json";

type AgentCatalogCategory = {
  id: string;
  title: string;
  summary: string;
  source_path: string;
  count: number;
};

type AgentCatalogEntry = {
  id: string;
  name: string;
  summary: string;
  category_id: string;
  category_title: string;
  category_summary: string;
  source_path: string;
  source_file: string;
  sandbox_mode: string;
  target_surfaces: string[];
  instructions: string;
  tags: string[];
  requires: string[];
  role: string;
};

type AgentCatalogIndex = {
  generated_at: string;
  source_root: string;
  categories: AgentCatalogCategory[];
  agents: AgentCatalogEntry[];
};

const CATALOG = agentCatalog as AgentCatalogIndex;

function normalize(value: unknown) {
  return String(value || "")
    .trim()
    .toLowerCase();
}

function instructionPreview(entry: AgentCatalogEntry) {
  const firstBlock = String(entry.instructions || "")
    .trim()
    .split(/\n\s*\n/)
    .find(Boolean)
    ?.trim();
  return firstBlock || entry.summary;
}

export function AgentCatalogTab() {
  const [query, setQuery] = useState("");
  const [activeCategory, setActiveCategory] = useState("all");
  const [status, setStatus] = useState<string>("");

  const filteredAgents = useMemo(() => {
    const q = normalize(query);
    return CATALOG.agents.filter((entry) => {
      const matchesCategory = activeCategory === "all" || entry.category_id === activeCategory;
      if (!matchesCategory) return false;
      if (!q) return true;
      const haystack = [
        entry.name,
        entry.summary,
        entry.category_title,
        entry.category_id,
        entry.source_path,
        entry.source_file,
        entry.sandbox_mode,
        entry.role,
        ...(entry.tags || []),
        ...(entry.requires || []),
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(q);
    });
  }, [activeCategory, query]);

  const filteredCategories = useMemo(() => {
    const map = new Map<string, AgentCatalogEntry[]>();
    for (const entry of filteredAgents) {
      const rows = map.get(entry.category_id) || [];
      rows.push(entry);
      map.set(entry.category_id, rows);
    }
    return CATALOG.categories
      .map((category) => ({
        ...category,
        agents: map.get(category.id) || [],
      }))
      .filter((category) => category.agents.length > 0);
  }, [filteredAgents]);

  const writeToClipboard = async (text: string) => {
    const clipboard = globalThis.navigator?.clipboard;
    if (!clipboard) {
      throw new Error("Clipboard is not available in this environment");
    }
    await clipboard.writeText(text);
  };

  const copyPath = async (path: string) => {
    try {
      await writeToClipboard(path);
      setStatus(`Copied ${path}`);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
    }
  };

  const openSource = async (sourcePath: string) => {
    try {
      await openPath(sourcePath);
      setStatus(`Opened ${sourcePath}`);
    } catch {
      try {
        await writeToClipboard(sourcePath);
        setStatus(`Copied ${sourcePath}`);
      } catch (error) {
        setStatus(error instanceof Error ? error.message : String(error));
      }
    }
  };

  return (
    <div className="grid gap-4">
      <div className="grid gap-3 rounded-lg border border-border bg-surface-elevated/50 p-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <div className="text-sm font-medium text-text">Agent catalog</div>
            <div className="text-xs text-text-muted">
              {filteredAgents.length} of {CATALOG.agents.length} entries across{" "}
              {CATALOG.categories.length} categories
            </div>
          </div>
          <div className="text-xs text-text-subtle">{CATALOG.generated_at}</div>
        </div>
        <div className="grid gap-2 md:grid-cols-[1fr_auto]">
          <div className="relative">
            <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-subtle" />
            <Input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search name, category, tag, or path"
              className="pl-9"
            />
          </div>
          <Button
            variant="secondary"
            onClick={() => setQuery("")}
            disabled={!query && activeCategory === "all"}
          >
            Clear
          </Button>
        </div>
        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            className={`tcp-btn h-8 px-3 text-xs ${activeCategory === "all" ? "border-amber-400/60 bg-amber-400/10" : ""}`}
            onClick={() => setActiveCategory("all")}
          >
            All
          </button>
          {CATALOG.categories.map((category) => {
            const active = activeCategory === category.id;
            return (
              <button
                key={category.id}
                type="button"
                className={`tcp-btn h-8 px-3 text-xs ${active ? "border-amber-400/60 bg-amber-400/10" : ""}`}
                onClick={() => setActiveCategory(category.id)}
              >
                {category.title} ({category.count})
              </button>
            );
          })}
        </div>
        {status ? <div className="text-xs text-text-subtle">{status}</div> : null}
      </div>

      {filteredCategories.length ? (
        filteredCategories.map((category) => (
          <div key={category.id} className="grid gap-2">
            <div className="flex items-center justify-between gap-2">
              <div>
                <div className="text-sm font-medium text-text">{category.title}</div>
                <div className="text-xs text-text-muted">{category.summary}</div>
              </div>
              <div className="tcp-badge-info">{category.agents.length} agents</div>
            </div>
            <div className="grid gap-2 md:grid-cols-2">
              {category.agents.map((entry) => {
                const preview = instructionPreview(entry);
                return (
                  <div key={entry.source_path} className="tcp-list-item grid gap-2">
                    <div className="flex flex-wrap items-start justify-between gap-2">
                      <div className="min-w-0">
                        <div className="truncate font-semibold">{entry.name}</div>
                        <div className="text-xs text-text-muted">{entry.summary}</div>
                      </div>
                      <div className="flex flex-wrap gap-2">
                        <span className="tcp-badge-info">{entry.role}</span>
                        <span
                          className={
                            entry.sandbox_mode === "read-only" ? "tcp-badge-warn" : "tcp-badge-ok"
                          }
                        >
                          {entry.sandbox_mode}
                        </span>
                      </div>
                    </div>
                    <div className="text-xs text-text-subtle font-mono">{entry.source_path}</div>
                    <div className="line-clamp-3 text-xs text-slate-200">{preview}</div>
                    <div className="flex flex-wrap gap-1">
                      {entry.tags.slice(0, 4).map((tag) => (
                        <span key={`${entry.id}-${tag}`} className="tcp-badge-muted">
                          {tag}
                        </span>
                      ))}
                      {entry.requires.slice(0, 3).map((req) => (
                        <span key={`${entry.id}-${req}`} className="tcp-badge-info">
                          {req}
                        </span>
                      ))}
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => void copyPath(entry.source_path)}
                        className="h-8 px-3 text-xs"
                      >
                        <Copy className="mr-2 h-3.5 w-3.5" />
                        Copy path
                      </Button>
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => void openSource(entry.source_path)}
                        className="h-8 px-3 text-xs"
                      >
                        <FolderOpen className="mr-2 h-3.5 w-3.5" />
                        Open source
                      </Button>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        ))
      ) : (
        <div className="rounded-lg border border-border bg-surface-elevated p-4 text-sm text-text-muted">
          No agents match your search.
        </div>
      )}
    </div>
  );
}
