import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

export function PacksPage({ client, toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const [path, setPath] = useState("");

  const listQuery = useQuery({
    queryKey: ["packs", "list"],
    queryFn: () =>
      client?.packs?.list?.().catch(() => ({ packs: [] })) ?? Promise.resolve({ packs: [] }),
    refetchInterval: 15000,
  });

  const installMutation = useMutation({
    mutationFn: () => client.packs.install({ path }),
    onSuccess: async () => {
      toast("ok", "Pack installed.");
      setPath("");
      await queryClient.invalidateQueries({ queryKey: ["packs"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const packs = toArray(listQuery.data, "packs");

  return (
    <div className="grid gap-4 xl:grid-cols-2">
      <PageCard title="Pack Library" subtitle="Installed starter packs and workflows">
        <div className="grid gap-2">
          {packs.length ? (
            packs.map((pack: any, index: number) => (
              <div key={String(pack?.id || pack?.name || index)} className="tcp-list-item">
                <div className="mb-1 flex items-center justify-between gap-2">
                  <strong>{String(pack?.name || pack?.id || "Pack")}</strong>
                  <span className="tcp-badge-info">{String(pack?.version || "-")}</span>
                </div>
                <div className="tcp-subtle text-xs">
                  {String(pack?.description || pack?.path || "")}
                </div>
              </div>
            ))
          ) : (
            <EmptyState text="No packs installed." />
          )}
        </div>
      </PageCard>

      <PageCard title="Install Pack" subtitle="Install local pack path or detected attachment path">
        <div className="grid gap-2">
          <input
            className="tcp-input"
            value={path}
            onInput={(e) => setPath((e.target as HTMLInputElement).value)}
            placeholder="/path/to/pack"
          />
          <button
            className="tcp-btn-primary"
            disabled={!path.trim()}
            onClick={() => installMutation.mutate()}
          >
            <i data-lucide="download"></i>
            Install from path
          </button>
        </div>
      </PageCard>
    </div>
  );
}
