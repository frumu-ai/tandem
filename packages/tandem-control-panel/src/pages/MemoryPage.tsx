import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
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
          {items.length ? (
            items.map((item: any, index: number) => {
              const id = String(item?.id || `mem-${index}`);
              return (
                <div key={id} className="tcp-list-item">
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <strong>{id}</strong>
                    <button
                      className="tcp-btn-danger h-7 px-2 text-xs"
                      onClick={() => deleteMutation.mutate(id)}
                    >
                      Delete
                    </button>
                  </div>
                  <div className="tcp-subtle text-xs whitespace-pre-wrap">
                    {String(item?.text || item?.content || item?.value || "")}
                  </div>
                </div>
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
