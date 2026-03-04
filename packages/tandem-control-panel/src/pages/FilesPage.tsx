import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

export function FilesPage({ api, toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const [dir, setDir] = useState("uploads");

  const filesQuery = useQuery({
    queryKey: ["files", dir],
    queryFn: () =>
      api(`/api/files/list?dir=${encodeURIComponent(dir)}`).catch(() => ({ files: [] })),
    refetchInterval: 12000,
  });

  const deleteMutation = useMutation({
    mutationFn: (path: string) =>
      api(`/api/files/delete`, { method: "POST", body: JSON.stringify({ path }) }),
    onSuccess: async () => {
      toast("ok", "File removed.");
      await queryClient.invalidateQueries({ queryKey: ["files", dir] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const rows = toArray(filesQuery.data, "files");

  return (
    <div className="grid gap-4">
      <PageCard title="Files" subtitle="Browse and delete uploaded files">
        <div className="mb-3 flex gap-2">
          <input
            className="tcp-input"
            value={dir}
            onInput={(e) => setDir((e.target as HTMLInputElement).value)}
            placeholder="uploads"
          />
          <button className="tcp-btn" onClick={() => filesQuery.refetch()}>
            Refresh
          </button>
        </div>
        <div className="grid gap-2">
          {rows.length ? (
            rows.map((file: any, index: number) => {
              const path = String(file?.path || file?.name || `file-${index}`);
              return (
                <div key={path} className="tcp-list-item">
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <strong className="truncate">{path}</strong>
                    <span className="tcp-subtle text-xs">{String(file?.size || 0)} bytes</span>
                  </div>
                  <div className="mt-2">
                    <button
                      className="tcp-btn-danger h-7 px-2 text-xs"
                      onClick={() => deleteMutation.mutate(path)}
                    >
                      Delete
                    </button>
                  </div>
                </div>
              );
            })
          ) : (
            <EmptyState text="No files found for this directory." />
          )}
        </div>
      </PageCard>
    </div>
  );
}
