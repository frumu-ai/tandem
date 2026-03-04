import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useMemo, useState } from "react";
import { renderMarkdownSafe } from "../lib/markdown";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

export function MemoryPage({ client, toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const [query, setQuery] = useState("");
  const [expandedId, setExpandedId] = useState<string | null>(null);

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

  const items = toArray(memoryQuery.data, "items");
  const rendered = useMemo(
    () =>
      items.map((item: any, index: number) => {
        const id = String(item?.id || `mem-${index}`);
        const text = String(item?.text || item?.content || item?.value || "");
        const compact = text.length > 340 ? `${text.slice(0, 340)}...` : text;
        return {
          id,
          text,
          compact,
          html: renderMarkdownSafe(text),
        };
      }),
    [items]
  );

  return (
    <div className="grid gap-4">
      <PageCard title="Memory" subtitle="Search and manage memory records">
        <div className="mb-3 flex gap-2">
          <input
            className="tcp-input"
            value={query}
            onInput={(e) => setQuery((e.target as HTMLInputElement).value)}
            placeholder="Search memory"
          />
          <button className="tcp-btn" onClick={() => memoryQuery.refetch()}>
            Search
          </button>
        </div>

        <div className="grid gap-2">
          {rendered.length ? (
            rendered.map((item) => {
              const expanded = expandedId === item.id;
              return (
                <motion.article
                  key={item.id}
                  layout
                  className={`tcp-list-item cursor-pointer transition-colors ${expanded ? "border-amber-500/60" : ""}`}
                  onClick={() => setExpandedId(expanded ? null : item.id)}
                >
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <strong className="truncate">{item.id}</strong>
                    <div className="flex items-center gap-2">
                      <span className={`text-[11px] ${expanded ? "tcp-badge-info" : "tcp-subtle"}`}>
                        {expanded ? "expanded" : "compact"}
                      </span>
                      <button
                        className="tcp-btn-danger h-7 px-2 text-xs"
                        onClick={(event) => {
                          event.stopPropagation();
                          deleteMutation.mutate(item.id);
                        }}
                      >
                        Delete
                      </button>
                    </div>
                  </div>

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
                        className="tcp-subtle text-xs whitespace-pre-wrap"
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
      </PageCard>
    </div>
  );
}
