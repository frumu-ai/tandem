import type {
  OrchestrationAggregateResponse,
  OrchestrationListResponse,
  OrchestrationSpec,
} from "@frumu/tandem-client";
import type { QueryClient } from "@tanstack/react-query";

export async function synchronizeSavedDraftQueries(
  queryClient: QueryClient,
  orchestration: OrchestrationSpec
) {
  const orchestrationId = orchestration.orchestration_id;
  queryClient.setQueryData<OrchestrationAggregateResponse>(
    ["orchestrations", orchestrationId],
    (current) => (current ? { ...current, draft: orchestration } : current)
  );
  queryClient.setQueryData<OrchestrationListResponse>(["orchestrations", "library"], (current) =>
    current
      ? {
          ...current,
          orchestrations: current.orchestrations.map((summary) =>
            summary.orchestration_id === orchestrationId
              ? {
                  ...summary,
                  name: orchestration.name,
                  draft: {
                    status: orchestration.status === "archived" ? "archived" : "draft",
                    updated_at_ms: orchestration.updated_at_ms,
                  },
                }
              : summary
          ),
        }
      : current
  );
  queryClient.setQueriesData(
    { queryKey: ["orchestrations", orchestrationId, "library-status"] },
    (current: any) => (current ? { ...current, spec: orchestration } : current)
  );
  await Promise.all([
    queryClient.invalidateQueries({
      queryKey: ["orchestrations", orchestrationId, "validation"],
      exact: true,
    }),
    queryClient.invalidateQueries({
      queryKey: ["orchestrations", orchestrationId, "stale"],
      exact: true,
    }),
    queryClient.invalidateQueries({
      queryKey: ["orchestrations", orchestrationId, "library-status"],
    }),
  ]);
}
