import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useMemo, useState } from "react";
import { MemoryImportDialog } from "../components/MemoryImportDialog";
import { renderMarkdownSafe } from "../lib/markdown";
import { AnimatedPage, Badge, PanelCard, Toolbar } from "../ui/index.tsx";
import { EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

export function MemoryPage({ api, client, toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const [query, setQuery] = useState("");
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [manualOpen, setManualOpen] = useState(false);
  const [manualContent, setManualContent] = useState("");
  const [manualKind, setManualKind] = useState<"note" | "fact" | "solution_capsule">("note");
  const [manualProjectId, setManualProjectId] = useState("");
  const [manualVisibility, setManualVisibility] = useState<"private" | "shared">("private");

  const memoryQuery = useQuery({
    queryKey: ["memory", query],
    queryFn: () =>
      (query.trim()
        ? client.memory.search({ query, limit: 40 })
        : client.memory.list({ q: "", limit: 40 })
      ).catch(() => ({ items: [] })),
    refetchInterval: 15000,
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => client.memory.delete(id),
    onSuccess: async () => {
      toast("ok", "Memory record deleted.");
      await queryClient.invalidateQueries({ queryKey: ["memory"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const addMemoryMutation = useMutation({
    mutationFn: async () => {
      const projectId = manualProjectId.trim() || "default";
      const tier = manualVisibility === "shared" ? "project" : "session";
      const runId = `manual-memory-${Date.now()}`;
      return api("/api/engine/memory/put", {
        method: "POST",
        body: JSON.stringify({
          run_id: runId,
          partition: {
            org_id: "local",
            workspace_id: "control-panel",
            project_id: projectId,
            tier,
          },
          kind: manualKind,
          content: manualContent.trim(),
          classification: "internal",
          metadata: {
            origin: "control_panel_manual_add",
            visibility: manualVisibility,
          },
          capability: {
            run_id: runId,
            subject: "control-panel",
            org_id: "local",
            workspace_id: "control-panel",
            project_id: projectId,
            memory: {
              read_tiers: ["session", "project"],
              write_tiers: [tier],
              promote_targets: ["project"],
              require_review_for_promote: false,
              allow_auto_use_tiers: ["curated"],
            },
            expires_at: 9007199254740991,
          },
        }),
      });
    },
    onSuccess: async () => {
      toast("ok", "Memory saved.");
      setManualContent("");
      setManualOpen(false);
      await queryClient.invalidateQueries({ queryKey: ["memory"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const itemRows = toArray(memoryQuery.data, "items");
  const resultRows = toArray(memoryQuery.data, "results");
  const items = itemRows.length ? itemRows : resultRows;
  const rendered = useMemo(
    () =>
      items.map((item: any, index: number) => {
        const id = String(item?.id || `mem-${index}`);
        const text = String(item?.text || item?.content || item?.value || "");
        const compact = text.length > 340 ? `${text.slice(0, 340)}...` : text;
        const metadata = item?.metadata || {};
        const linkage = item?.linkage || {};
        const sourcePath = String(
          item?.source_path ||
            item?.sourcePath ||
            metadata?.path ||
            metadata?.source_path ||
            metadata?.import_root ||
            ""
        );
        const sourceType = String(
          item?.source_type || item?.sourceType || item?.kind || item?.source || ""
        );
        const project = String(
          item?.project_tag ||
            item?.projectTag ||
            item?.project_id ||
            item?.projectId ||
            linkage?.project_id ||
            metadata?.project_id ||
            ""
        );
        const visibility = String(item?.visibility || metadata?.visibility || "");
        const runId = String(item?.run_id || item?.runId || linkage?.run_id || "");
        return {
          id,
          text,
          compact,
          html: renderMarkdownSafe(text),
          sourcePath,
          sourceType,
          project,
          visibility,
          runId,
        };
      }),
    [items]
  );

  return (
    <AnimatedPage className="grid gap-4">
      <PanelCard
        title="Memory"
        subtitle="Search recent records and open full entry details inline."
        actions={
          <>
            <Badge tone="info">{rendered.length} results</Badge>
            {query.trim() ? (
              <Badge tone="ghost">Filter: {query}</Badge>
            ) : (
              <Badge tone="ghost">Browsing latest memory</Badge>
            )}
            <button type="button" className="tcp-btn-primary" onClick={() => setImportOpen(true)}>
              <i data-lucide="database-zap"></i>
              Import Knowledge
            </button>
            <button type="button" className="tcp-btn" onClick={() => setManualOpen((v) => !v)}>
              <i data-lucide="plus"></i>
              Add Memory
            </button>
          </>
        }
      >
        {manualOpen ? (
          <div className="mb-3 grid gap-3 rounded-xl border border-white/10 bg-black/20 p-3">
            <textarea
              className="tcp-input min-h-28 resize-y"
              value={manualContent}
              onChange={(event) => setManualContent(event.target.value)}
              placeholder="Store a durable note, fact, or solution capsule."
            />
            <div className="grid gap-3 md:grid-cols-[1fr_1fr_1fr_auto]">
              <select
                className="tcp-select"
                value={manualKind}
                onChange={(event) => setManualKind(event.target.value as typeof manualKind)}
              >
                <option value="note">note</option>
                <option value="fact">fact</option>
                <option value="solution_capsule">solution_capsule</option>
              </select>
              <input
                className="tcp-input"
                value={manualProjectId}
                onChange={(event) => setManualProjectId(event.target.value)}
                placeholder="project id"
              />
              <select
                className="tcp-select"
                value={manualVisibility}
                onChange={(event) =>
                  setManualVisibility(event.target.value as typeof manualVisibility)
                }
              >
                <option value="private">private</option>
                <option value="shared">shared</option>
              </select>
              <button
                type="button"
                className="tcp-btn-primary"
                disabled={!manualContent.trim() || addMemoryMutation.isPending}
                onClick={() => addMemoryMutation.mutate()}
              >
                <i data-lucide="save"></i>
                Save
              </button>
            </div>
          </div>
        ) : null}

        <Toolbar className="mb-3">
          <input
            className="tcp-input flex-1"
            value={query}
            onInput={(e) => setQuery((e.target as HTMLInputElement).value)}
            placeholder="Search memory"
          />
          <button className="tcp-btn" onClick={() => memoryQuery.refetch()}>
            <i data-lucide="search"></i>
            Search
          </button>
        </Toolbar>

        <div className="grid gap-2">
          {rendered.length ? (
            rendered.map((item) => {
              const expanded = expandedId === item.id;
              return (
                <motion.article
                  key={item.id}
                  layout
                  className={`tcp-list-item cursor-pointer ${expanded ? "border-amber-500/60" : ""}`}
                  onClick={() => setExpandedId(expanded ? null : item.id)}
                >
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <strong className="truncate">{item.id}</strong>
                    <div className="flex items-center gap-2">
                      <Badge tone={expanded ? "info" : "ghost"}>
                        {expanded ? "expanded" : "compact"}
                      </Badge>
                      <button
                        className="tcp-btn-danger h-7 px-2 text-xs"
                        onClick={(event) => {
                          event.stopPropagation();
                          deleteMutation.mutate(item.id);
                        }}
                      >
                        <i data-lucide="trash-2"></i>
                        Delete
                      </button>
                    </div>
                  </div>
                  <div className="mb-2 flex flex-wrap items-center gap-2">
                    {item.sourceType ? <Badge tone="ghost">{item.sourceType}</Badge> : null}
                    {item.project ? <Badge tone="info">{item.project}</Badge> : null}
                    {item.visibility ? <Badge tone="ghost">{item.visibility}</Badge> : null}
                    {item.runId ? <Badge tone="ghost">{item.runId}</Badge> : null}
                  </div>
                  {item.sourcePath ? (
                    <div className="tcp-subtle mb-2 truncate text-[11px]">{item.sourcePath}</div>
                  ) : null}

                  <AnimatePresence initial={false} mode="wait">
                    {expanded ? (
                      <motion.div
                        key="expanded"
                        initial={{ opacity: 0, height: 0 }}
                        animate={{ opacity: 1, height: "auto" }}
                        exit={{ opacity: 0, height: 0 }}
                        transition={{ duration: 0.18, ease: "easeOut" }}
                        className="overflow-hidden"
                      >
                        <div
                          className="tcp-markdown tcp-markdown-ai rounded-lg border border-slate-700/60 bg-black/20 p-3 text-sm"
                          dangerouslySetInnerHTML={{ __html: item.html }}
                        />
                      </motion.div>
                    ) : (
                      <motion.div
                        key="compact"
                        initial={{ opacity: 0, y: 4 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: -4 }}
                        transition={{ duration: 0.14, ease: "easeOut" }}
                        className="tcp-subtle whitespace-pre-wrap text-xs"
                      >
                        {item.compact || "(empty)"}
                      </motion.div>
                    )}
                  </AnimatePresence>
                </motion.article>
              );
            })
          ) : (
            <EmptyState text="No memory records found." />
          )}
        </div>
      </PanelCard>

      <MemoryImportDialog
        open={importOpen}
        client={client}
        initialTier="global"
        toast={toast}
        onCancel={() => setImportOpen(false)}
        onSuccess={async () => {
          await queryClient.invalidateQueries({ queryKey: ["memory"] });
        }}
      />
    </AnimatedPage>
  );
}
