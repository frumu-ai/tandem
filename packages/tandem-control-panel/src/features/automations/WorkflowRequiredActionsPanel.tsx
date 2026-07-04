type MutationLike = {
  mutate: (payload: any) => void;
  isPending?: boolean;
};

type WorkflowRequiredActionsPanelProps = {
  runRepairGuidanceEntries: Array<{ nodeId: string; guidance: any }>;
  blockedNodeIds: string[];
  needsRepairNodeIds: string[];
  isWorkflowRun: boolean;
  selectedRunId: string;
  hasActiveSessions: boolean;
  onFocusNode: (nodeId: string) => void;
  workflowTaskRetryMutation: MutationLike;
  workflowTaskContinueMutation: MutationLike;
  workflowTaskRequeueMutation: MutationLike;
};

export function WorkflowRequiredActionsPanel({
  runRepairGuidanceEntries,
  blockedNodeIds,
  needsRepairNodeIds,
  isWorkflowRun,
  selectedRunId,
  hasActiveSessions,
  onFocusNode,
  workflowTaskRetryMutation,
  workflowTaskContinueMutation,
  workflowTaskRequeueMutation,
}: WorkflowRequiredActionsPanelProps) {
  const normalizedBlockedNodeIds = Array.isArray(blockedNodeIds) ? blockedNodeIds : [];
  const normalizedNeedsRepairNodeIds = Array.isArray(needsRepairNodeIds) ? needsRepairNodeIds : [];
  const actionableEntries = runRepairGuidanceEntries.filter(({ nodeId, guidance }) => {
    const normalizedStatus = String(guidance?.status || "")
      .trim()
      .toLowerCase();
    return (
      normalizedBlockedNodeIds.includes(nodeId) &&
      ["blocked", "needs_repair"].includes(normalizedStatus)
    );
  });
  if (!actionableEntries.length) return null;

  return (
    <div className="tcp-list-item overflow-visible">
      <div className="mb-2 flex items-center justify-between gap-2">
        <div className="font-medium">Next Required Actions</div>
        <span className="border border-emerald-400/60 bg-emerald-400/10 text-emerald-200 tcp-badge">
          {actionableEntries.length} node
          {actionableEntries.length === 1 ? "" : "s"}
        </span>
      </div>
      <div className="grid gap-2">
        {actionableEntries.map(({ nodeId, guidance }) => {
          const actions = Array.isArray(guidance?.requiredNextToolActions)
            ? guidance.requiredNextToolActions
            : [];
          const unmet = Array.isArray(guidance?.unmetRequirements)
            ? guidance.unmetRequirements
            : [];
          const reason = String(guidance?.reason || "").trim();
          const blockingClassification = String(guidance?.blockingClassification || "").trim();
          const failureKind = String(guidance?.failureKind || "").trim();
          const status = String(guidance?.status || "").trim();
          const normalizedStatus = status.toLowerCase();
          const canGuidanceRetry =
            isWorkflowRun &&
            !!selectedRunId &&
            !hasActiveSessions &&
            (normalizedBlockedNodeIds.includes(nodeId) ||
              normalizedNeedsRepairNodeIds.includes(nodeId) ||
              normalizedStatus === "failed");
          const canGuidanceContinue =
            isWorkflowRun &&
            !!selectedRunId &&
            !hasActiveSessions &&
            normalizedBlockedNodeIds.includes(nodeId) &&
            ["blocked", "needs_repair"].includes(normalizedStatus);

          return (
            <div
              key={nodeId}
              className="rounded-lg border border-emerald-500/30 bg-emerald-950/20 p-3"
            >
              <div className="mb-2 flex flex-wrap items-center gap-2">
                <strong>{nodeId}</strong>
                {status ? (
                  <span className="border border-emerald-400/60 bg-emerald-400/10 text-emerald-200 tcp-badge">
                    {status}
                  </span>
                ) : null}
                {blockingClassification ? (
                  <span className="tcp-subtle tcp-text-caption">
                    {blockingClassification.replace(/_/g, " ")}
                  </span>
                ) : null}
                {guidance?.repairAttemptsRemaining !== null &&
                guidance?.repairAttemptsRemaining !== undefined ? (
                  <span className="tcp-subtle tcp-text-caption">
                    {String(guidance.repairAttemptsRemaining)} repair attempt
                    {Number(guidance.repairAttemptsRemaining) === 1 ? "" : "s"} left
                  </span>
                ) : null}
              </div>
              <div className="mb-2 flex flex-wrap gap-2">
                <button
                  type="button"
                  className="tcp-btn h-7 px-2 text-xs"
                  onClick={() => onFocusNode(nodeId)}
                >
                  Focus
                </button>
                {canGuidanceRetry ? (
                  <button
                    type="button"
                    className="tcp-btn h-7 px-2 text-xs"
                    onClick={() =>
                      workflowTaskRetryMutation.mutate({
                        runId: selectedRunId,
                        nodeId,
                        reason: `retried task ${nodeId} from repair guidance`,
                      })
                    }
                    disabled={
                      workflowTaskRetryMutation.isPending ||
                      workflowTaskContinueMutation.isPending ||
                      workflowTaskRequeueMutation.isPending
                    }
                  >
                    {workflowTaskRetryMutation.isPending ? "Retrying..." : "Retry"}
                  </button>
                ) : null}
                {canGuidanceContinue ? (
                  <button
                    type="button"
                    className="tcp-btn h-7 px-2 text-xs"
                    onClick={() =>
                      workflowTaskContinueMutation.mutate({
                        runId: selectedRunId,
                        nodeId,
                        reason: `continued task ${nodeId} from repair guidance`,
                      })
                    }
                    disabled={
                      workflowTaskRetryMutation.isPending ||
                      workflowTaskContinueMutation.isPending ||
                      workflowTaskRequeueMutation.isPending
                    }
                  >
                    {workflowTaskContinueMutation.isPending ? "Continuing..." : "Continue"}
                  </button>
                ) : null}
              </div>
              {reason ? (
                <div className="mb-2 whitespace-pre-wrap break-words text-sm text-emerald-100/90">
                  {reason}
                </div>
              ) : null}
              {actions.length ? (
                <div className="grid gap-1">
                  {actions.map((action: any, index: number) => (
                    <div
                      key={`${nodeId}-action-${index}`}
                      className="rounded-md border border-emerald-400/20 bg-black/20 px-2 py-1 text-xs text-emerald-50"
                    >
                      {String(action || "")}
                    </div>
                  ))}
                </div>
              ) : null}
              {!actions.length && unmet.length ? (
                <div className="mt-2 flex flex-wrap gap-1">
                  {unmet.map((item: any) => (
                    <span
                      key={`${nodeId}-${String(item)}`}
                      className="rounded-full border border-emerald-400/25 bg-black/20 px-2 py-1 tcp-text-caption text-emerald-100/90"
                    >
                      {String(item || "").replace(/_/g, " ")}
                    </span>
                  ))}
                </div>
              ) : null}
              {failureKind ? (
                <div className="tcp-subtle mt-2 tcp-text-caption">failure kind: {failureKind}</div>
              ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}
