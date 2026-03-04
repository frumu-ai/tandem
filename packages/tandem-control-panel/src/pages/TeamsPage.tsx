import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

export function TeamsPage({ client, toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const instancesQuery = useQuery({
    queryKey: ["teams", "instances"],
    queryFn: () =>
      client?.agentTeams?.listInstances?.().catch(() => ({ instances: [] })) ??
      Promise.resolve({ instances: [] }),
    refetchInterval: 8000,
  });
  const approvalsQuery = useQuery({
    queryKey: ["teams", "approvals"],
    queryFn: () =>
      client?.agentTeams?.listApprovals?.().catch(() => ({ spawnApprovals: [] })) ??
      Promise.resolve({ spawnApprovals: [] }),
    refetchInterval: 6000,
  });

  const replyMutation = useMutation({
    mutationFn: ({ requestId, decision }: { requestId: string; decision: "approve" | "deny" }) =>
      client?.agentTeams?.replyApproval?.(requestId, decision),
    onSuccess: async () => {
      toast("ok", "Approval updated.");
      await queryClient.invalidateQueries({ queryKey: ["teams"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const instances = toArray(instancesQuery.data, "instances");
  const approvals = toArray(approvalsQuery.data, "spawnApprovals");

  return (
    <div className="grid gap-4 xl:grid-cols-2">
      <PageCard title="Team Instances" subtitle="Running collaborative agent instances">
        <div className="grid gap-2">
          {instances.length ? (
            instances.map((instance: any, index: number) => (
              <div
                key={String(instance?.instance_id || instance?.id || index)}
                className="tcp-list-item"
              >
                <div className="mb-1 flex items-center justify-between gap-2">
                  <strong>{String(instance?.name || instance?.instance_id || "Instance")}</strong>
                  <span className="tcp-badge-info">{String(instance?.status || "active")}</span>
                </div>
                <div className="tcp-subtle text-xs">
                  {String(instance?.workspace || instance?.workspaceRoot || "")}
                </div>
              </div>
            ))
          ) : (
            <EmptyState text="No team instances found." />
          )}
        </div>
      </PageCard>

      <PageCard title="Spawn Approvals" subtitle="Pending team approval requests">
        <div className="grid gap-2">
          {approvals.length ? (
            approvals.map((approval: any, index: number) => {
              const requestId = String(approval?.request_id || approval?.id || `request-${index}`);
              return (
                <div key={requestId} className="tcp-list-item">
                  <div className="mb-1 font-medium">
                    {String(approval?.reason || approval?.title || requestId)}
                  </div>
                  <div className="tcp-subtle text-xs">{requestId}</div>
                  <div className="mt-2 flex gap-2">
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() => replyMutation.mutate({ requestId, decision: "approve" })}
                    >
                      Approve
                    </button>
                    <button
                      className="tcp-btn-danger h-7 px-2 text-xs"
                      onClick={() => replyMutation.mutate({ requestId, decision: "deny" })}
                    >
                      Deny
                    </button>
                  </div>
                </div>
              );
            })
          ) : (
            <EmptyState text="No pending approvals." />
          )}
        </div>
      </PageCard>
    </div>
  );
}
