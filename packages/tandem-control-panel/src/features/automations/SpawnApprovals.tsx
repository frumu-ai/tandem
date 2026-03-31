import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { EmptyState } from "../../pages/ui";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

export function SpawnApprovals({ client, toast }: { client: any; toast: any }) {
  const queryClient = useQueryClient();

  const approvalsQuery = useQuery({
    queryKey: ["automations", "approvals"],
    queryFn: () =>
      client?.agentTeams?.listApprovals?.().catch(() => ({ spawnApprovals: [] })) ??
      Promise.resolve({ spawnApprovals: [] }),
    refetchInterval: 6000,
  });

  const instancesQuery = useQuery({
    queryKey: ["automations", "instances"],
    queryFn: () =>
      client?.agentTeams?.listInstances?.().catch(() => ({ instances: [] })) ??
      Promise.resolve({ instances: [] }),
    refetchInterval: 8000,
  });

  const replyMutation = useMutation({
    mutationFn: ({ requestId, decision }: { requestId: string; decision: "approve" | "deny" }) =>
      client?.agentTeams?.replyApproval?.(requestId, decision),
    onSuccess: async () => {
      toast("ok", "Approval updated.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const approvals = toArray(approvalsQuery.data, "spawnApprovals");
  const instances = toArray(instancesQuery.data, "instances");

  return (
    <div className="grid gap-4">
      {approvals.length > 0 ? (
        <div className="grid gap-2">
          <p className="text-xs font-medium uppercase tracking-wide text-slate-500">
            Pending Approvals
          </p>
          {approvals.map((approval: any, index: number) => {
            const requestId = String(approval?.request_id || approval?.id || `request-${index}`);
            return (
              <div key={requestId} className="tcp-list-item border-amber-500/40">
                <div className="mb-1 font-medium text-amber-300">
                  ⚠️ {String(approval?.reason || approval?.title || "Spawn request")}
                </div>
                <div className="tcp-subtle text-xs">{requestId}</div>
                <div className="mt-2 flex gap-2">
                  <button
                    className="tcp-btn-primary h-7 px-2 text-xs"
                    onClick={() => replyMutation.mutate({ requestId, decision: "approve" })}
                  >
                    <i data-lucide="badge-check"></i>
                    Approve
                  </button>
                  <button
                    className="tcp-btn-danger h-7 px-2 text-xs"
                    onClick={() => replyMutation.mutate({ requestId, decision: "deny" })}
                  >
                    <i data-lucide="x"></i>
                    Deny
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      ) : null}

      {instances.length > 0 ? (
        <div className="grid gap-2">
          <p className="text-xs font-medium uppercase tracking-wide text-slate-500">Active Teams</p>
          {instances.map((instance: any, index: number) => (
            <div
              key={String(instance?.instance_id || instance?.id || index)}
              className="tcp-list-item"
            >
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  <span>👥</span>
                  <strong>
                    {String(
                      instance?.name || instance?.template_id || instance?.instance_id || "Instance"
                    )}
                  </strong>
                </div>
                <span className="tcp-badge-info">{String(instance?.status || "active")}</span>
              </div>
              <div className="tcp-subtle mt-1 text-xs">
                Mission: {String(instance?.mission_id || "—")}
              </div>
            </div>
          ))}
        </div>
      ) : null}

      {!approvals.length && !instances.length ? (
        <EmptyState text="No active teams or pending approvals right now." />
      ) : null}
    </div>
  );
}
