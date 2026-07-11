import { Spinner } from "../../ui";

type MutationLike = {
  mutate: (payload: any) => void;
  isPending?: boolean;
};

type WorkflowTaskActionsPanelProps = {
  selectedBoardTask: any;
  selectedBoardTaskIsWorkflowNode: boolean;
  selectedBoardTaskIsProjectedBacklogItem: boolean;
  selectedBoardTaskNodeId: string;
  selectedBoardTaskStateNormalized: string;
  selectedBoardTaskImpactSummary: any;
  selectedBoardTaskResetOutputPaths: string[];
  selectedRunId: string;
  canTaskContinue: boolean;
  canTaskRetry: boolean;
  runDebuggerRetryNodeId: string;
  continueBlockedNodeId: string;
  selectedBoardTaskServerActionMessage: string;
  canTaskRequeue: boolean;
  canBacklogTaskClaim: boolean;
  canBacklogTaskRequeue: boolean;
  canRecoverWorkflowRun: boolean;
  backlogTaskClaimMutation: MutationLike;
  backlogTaskRequeueMutation: MutationLike;
  workflowTaskContinueMutation: MutationLike;
  workflowTaskRetryMutation: MutationLike;
  workflowTaskRequeueMutation: MutationLike;
  workflowRepairMutation: MutationLike;
  workflowRecoverMutation: MutationLike;
  taskResetPreviewQuery: { isLoading?: boolean };
};

export function WorkflowTaskActionsPanel({
  selectedBoardTask,
  selectedBoardTaskIsWorkflowNode,
  selectedBoardTaskIsProjectedBacklogItem,
  selectedBoardTaskNodeId,
  selectedBoardTaskStateNormalized,
  selectedBoardTaskImpactSummary,
  selectedBoardTaskResetOutputPaths,
  selectedRunId,
  canTaskContinue,
  canTaskRetry,
  runDebuggerRetryNodeId,
  continueBlockedNodeId,
  selectedBoardTaskServerActionMessage,
  canTaskRequeue,
  canBacklogTaskClaim,
  canBacklogTaskRequeue,
  canRecoverWorkflowRun,
  backlogTaskClaimMutation,
  backlogTaskRequeueMutation,
  workflowTaskContinueMutation,
  workflowTaskRetryMutation,
  workflowTaskRequeueMutation,
  workflowRepairMutation,
  workflowRecoverMutation,
  taskResetPreviewQuery,
}: WorkflowTaskActionsPanelProps) {
  const shouldShowContinueTaskButton =
    selectedBoardTaskIsWorkflowNode &&
    !!selectedBoardTaskNodeId &&
    selectedBoardTaskStateNormalized === "blocked";
  const shouldShowRetryTaskButton =
    selectedBoardTaskIsWorkflowNode &&
    !!selectedBoardTaskNodeId &&
    ["blocked", "failed"].includes(selectedBoardTaskStateNormalized);
  if (
    !selectedRunId ||
    !(
      shouldShowContinueTaskButton ||
      shouldShowRetryTaskButton ||
      canTaskContinue ||
      canTaskRetry ||
      canTaskRequeue ||
      canBacklogTaskClaim ||
      canBacklogTaskRequeue ||
      canRecoverWorkflowRun
    )
  ) {
    return null;
  }

  return (
    <div className="mt-3 space-y-3">
      {selectedBoardTaskIsWorkflowNode ? (
        <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2 tcp-text-caption text-slate-300">
          <div className="font-medium text-slate-100">Action impact</div>
          <div className="mt-1 tcp-subtle">
            {taskResetPreviewQuery.isLoading
              ? "Loading engine preview..."
              : selectedBoardTaskImpactSummary.previewBacked
                ? "Engine preview"
                : "UI-estimated preview"}
          </div>
          <div className="mt-1">Selected task: {selectedBoardTaskImpactSummary.rootTitle}</div>
          <div>
            Reset scope:{" "}
            {canTaskContinue
              ? "minimal reset of the blocked task"
              : `${selectedBoardTaskImpactSummary.subtreeCount} task${
                  selectedBoardTaskImpactSummary.subtreeCount === 1 ? "" : "s"
                }${
                  selectedBoardTaskImpactSummary.descendantCount > 0
                    ? ` (${selectedBoardTaskImpactSummary.descendantCount} descendant${
                        selectedBoardTaskImpactSummary.descendantCount === 1 ? "" : "s"
                      })`
                    : ""
                }`}
          </div>
          <div>
            Preserves:{" "}
            {selectedBoardTaskImpactSummary.preservesUpstreamOutputs
              ? "completed upstream outputs outside this subtree"
              : "n/a"}
          </div>
          <div>
            Clears: stale outputs for {selectedBoardTaskImpactSummary.outputCount} declared artifact
            {selectedBoardTaskImpactSummary.outputCount === 1 ? "" : "s"}
          </div>
          {selectedBoardTaskResetOutputPaths.length ? (
            <div className="mt-2 flex flex-wrap gap-1">
              {selectedBoardTaskResetOutputPaths.map((path) => (
                <span
                  key={path}
                  className="rounded-full border border-slate-700/70 bg-slate-950/30 px-2 py-1 font-mono tcp-text-caption text-slate-300"
                >
                  {path}
                </span>
              ))}
            </div>
          ) : null}
        </div>
      ) : selectedBoardTaskIsProjectedBacklogItem ? (
        <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2 tcp-text-caption text-slate-300">
          <div className="font-medium text-slate-100">Action impact</div>
          <div className="mt-1">
            Claiming assigns this backlog task to an agent without resetting any workflow nodes.
          </div>
          <div>Requeueing clears stale lease state and returns the task to the runnable queue.</div>
          <div>
            Upstream workflow artifacts are preserved because this acts on the projected backlog
            task only.
          </div>
        </div>
      ) : null}
      <div className="flex flex-wrap gap-2">
        {canBacklogTaskClaim ? (
          <div className="space-y-1">
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() =>
                backlogTaskClaimMutation.mutate({
                  runId: selectedRunId,
                  taskId: String(selectedBoardTask.id || ""),
                  agentId: String((selectedBoardTask as any).task_owner || "").trim() || undefined,
                  reason: `claimed backlog task ${String(selectedBoardTask.id || "")} from debugger`,
                })
              }
              disabled={backlogTaskClaimMutation.isPending || backlogTaskRequeueMutation.isPending}
            >
              {backlogTaskClaimMutation.isPending ? (
                <>
                  <Spinner /> Claiming...
                </>
              ) : (
                "Claim Task"
              )}
            </button>
            <div className="tcp-subtle tcp-text-caption">
              Assign this projected coding task and start its lease.
            </div>
          </div>
        ) : null}
        {canBacklogTaskRequeue ? (
          <div className="space-y-1">
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() =>
                backlogTaskRequeueMutation.mutate({
                  runId: selectedRunId,
                  taskId: String(selectedBoardTask.id || ""),
                  reason: `requeued backlog task ${String(selectedBoardTask.id || "")} from debugger`,
                })
              }
              disabled={backlogTaskClaimMutation.isPending || backlogTaskRequeueMutation.isPending}
            >
              {backlogTaskRequeueMutation.isPending ? (
                <>
                  <Spinner /> Requeueing...
                </>
              ) : (
                "Requeue Backlog Task"
              )}
            </button>
            <div className="tcp-subtle tcp-text-caption">
              Use when the task is blocked, failed, or its lease went stale.
            </div>
          </div>
        ) : null}
        {shouldShowContinueTaskButton ? (
          <div className="space-y-1">
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() =>
                workflowTaskContinueMutation.mutate({
                  runId: selectedRunId,
                  nodeId: continueBlockedNodeId,
                  reason: `continued blocked task ${continueBlockedNodeId} from debugger`,
                })
              }
              title={canTaskContinue ? "" : selectedBoardTaskServerActionMessage}
              disabled={
                !canTaskContinue ||
                workflowTaskContinueMutation.isPending ||
                workflowTaskRetryMutation.isPending ||
                workflowTaskRequeueMutation.isPending ||
                backlogTaskClaimMutation.isPending ||
                backlogTaskRequeueMutation.isPending
              }
            >
              {workflowTaskContinueMutation.isPending ? (
                <>
                  <Spinner /> Continuing...
                </>
              ) : (
                "Continue Task"
              )}
            </button>
            <div className="tcp-subtle tcp-text-caption">
              {canTaskContinue
                ? "Minimal reset: reruns the blocked task itself and preserves descendants unless they need to rerun later."
                : selectedBoardTaskServerActionMessage}
            </div>
          </div>
        ) : null}
        {shouldShowRetryTaskButton ? (
          <div className="space-y-1">
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() =>
                workflowTaskRetryMutation.mutate({
                  runId: selectedRunId,
                  nodeId: runDebuggerRetryNodeId,
                  reason: `retried task ${runDebuggerRetryNodeId} from debugger`,
                })
              }
              title={canTaskRetry ? "" : selectedBoardTaskServerActionMessage}
              disabled={
                !canTaskRetry ||
                workflowTaskContinueMutation.isPending ||
                workflowTaskRetryMutation.isPending ||
                workflowTaskRequeueMutation.isPending ||
                backlogTaskClaimMutation.isPending ||
                backlogTaskRequeueMutation.isPending
              }
            >
              {workflowTaskRetryMutation.isPending ? (
                <>
                  <Spinner /> Retrying task...
                </>
              ) : (
                "Retry Task"
              )}
            </button>
            <div className="tcp-subtle tcp-text-caption">
              {canTaskRetry
                ? "Best for blocked or failed work that should rerun from this task downward."
                : selectedBoardTaskServerActionMessage}
            </div>
          </div>
        ) : null}
        {canTaskRequeue ? (
          <div className="space-y-1">
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() =>
                workflowTaskRequeueMutation.mutate({
                  runId: selectedRunId,
                  nodeId: selectedBoardTaskNodeId,
                  reason: `requeued task ${selectedBoardTaskNodeId} from debugger`,
                })
              }
              disabled={
                workflowTaskContinueMutation.isPending ||
                workflowTaskRetryMutation.isPending ||
                workflowTaskRequeueMutation.isPending ||
                backlogTaskClaimMutation.isPending ||
                backlogTaskRequeueMutation.isPending
              }
            >
              {workflowTaskRequeueMutation.isPending ? (
                <>
                  <Spinner /> Requeueing...
                </>
              ) : (
                "Requeue Task"
              )}
            </button>
            <div className="tcp-subtle tcp-text-caption">
              Use when this task should go back onto the queue with its descendants reset.
            </div>
          </div>
        ) : null}
        {canTaskContinue &&
        selectedBoardTaskStateNormalized === "blocked" &&
        continueBlockedNodeId ? (
          <div className="space-y-1">
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() =>
                workflowRepairMutation.mutate({
                  runId: selectedRunId,
                  nodeId: continueBlockedNodeId,
                  reason: `continued from blocked node ${continueBlockedNodeId}`,
                })
              }
              disabled={
                workflowTaskContinueMutation.isPending ||
                workflowRepairMutation.isPending ||
                !continueBlockedNodeId
              }
            >
              {workflowRepairMutation.isPending ? (
                <>
                  <Spinner /> Repairing...
                </>
              ) : (
                "Repair Blocked Step"
              )}
            </button>
            <div className="tcp-subtle tcp-text-caption">
              Heavier reset/repair flow for blocked nodes when minimal continue is not enough.
            </div>
          </div>
        ) : null}
        {canRecoverWorkflowRun ? (
          <div className="space-y-1">
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() =>
                workflowRecoverMutation.mutate({
                  runId: selectedRunId,
                  reason: `retried from task ${String(selectedBoardTask.id || "").replace(/^node-/, "")}`,
                })
              }
              disabled={workflowRecoverMutation.isPending}
            >
              {workflowRecoverMutation.isPending ? (
                <>
                  <Spinner /> Retrying...
                </>
              ) : (
                "Retry Workflow"
              )}
            </button>
            <div className="tcp-subtle tcp-text-caption">
              Recover the whole run, not just this task subtree.
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
