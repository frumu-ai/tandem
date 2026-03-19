import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

export function AgentsPage({ client, toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const routinesQuery = useQuery({
    queryKey: ["agents", "routines"],
    queryFn: () => client.routines.list().catch(() => ({ routines: [] })),
    refetchInterval: 20000,
  });
  const runsQuery = useQuery({
    queryKey: ["agents", "runs"],
    queryFn: () => client.routines.listRuns({ limit: 40 }).catch(() => ({ runs: [] })),
    refetchInterval: 9000,
  });

  const runNowMutation = useMutation({
    mutationFn: (id: string) => client.routines.runNow(id),
    onSuccess: async () => {
      toast("ok", "Routine triggered.");
      await queryClient.invalidateQueries({ queryKey: ["agents"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const routines = toArray(routinesQuery.data, "routines");
  const runs = toArray(runsQuery.data, "runs");

  return (
    <div className="grid gap-4 xl:grid-cols-2">
      <PageCard title="Routines" subtitle="Automation schedules and run-now actions">
        <div className="grid gap-2">
          {routines.length ? (
            routines.map((routine: any) => {
              const id = String(routine?.id || routine?.routine_id || "");
              return (
                <div key={id} className="tcp-list-item">
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <strong>{String(routine?.name || id || "Routine")}</strong>
                    <span className="tcp-badge-info">{String(routine?.status || "active")}</span>
                  </div>
                  <div className="tcp-subtle text-xs">{String(routine?.schedule || "manual")}</div>
                  <div className="mt-2">
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() => runNowMutation.mutate(id)}
                    >
                      <i data-lucide="play"></i>
                      Run now
                    </button>
                  </div>
                </div>
              );
            })
          ) : (
            <EmptyState text="No routines found." />
          )}
        </div>
      </PageCard>

      <PageCard title="Recent Runs" subtitle="Latest automation executions">
        <div className="grid gap-2">
          {runs.length ? (
            runs.slice(0, 24).map((run: any, index: number) => (
              <div key={String(run?.run_id || run?.id || index)} className="tcp-list-item">
                <div className="mb-1 flex items-center justify-between gap-2">
                  <span className="font-medium">
                    {String(run?.name || run?.automation_id || run?.routine_id || "Run")}
                  </span>
                  <span className="tcp-badge-warn">{String(run?.status || "unknown")}</span>
                </div>
                <div className="tcp-subtle text-xs">{String(run?.run_id || run?.id || "")}</div>
              </div>
            ))
          ) : (
            <EmptyState text="No runs yet." />
          )}
        </div>
      </PageCard>
    </div>
  );
}
