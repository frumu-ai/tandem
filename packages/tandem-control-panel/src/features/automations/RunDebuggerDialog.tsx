import { motion } from "motion/react";
import { TaskBoard } from "../orchestration/TaskBoard";
import { WorkflowTaskActionsPanel } from "./WorkflowTaskActionsPanel";
import { WorkflowArtifactsPanel } from "./WorkflowArtifactsPanel";
import { WorkflowBlockersPanel } from "./WorkflowBlockersPanel";
import { WorkflowDebugHintsPanel } from "./WorkflowDebugHintsPanel";
import { WorkflowLiveSessionLogPanel } from "./WorkflowLiveSessionLogPanel";
import { WorkflowMissionObjectivePanel } from "./WorkflowMissionObjectivePanel";
import { WorkflowRequiredActionsPanel } from "./WorkflowRequiredActionsPanel";
import { WorkflowRunTelemetryPanel } from "./WorkflowRunTelemetryPanel";
import { WorkflowRunSummaryPanel } from "./WorkflowRunSummaryPanel";
import { WorkflowTaskSignalsPanel } from "./WorkflowTaskSignalsPanel";
import { formatJson } from "../../pages/ui";

export function RunDebuggerDialog({ state, actions, helpers }: any) {
  const {
    selectedRunId,
    selectedRun,
    isWorkflowRun,
    runStatus,
    runStatusDerivedFromBlockedNodes,
    canContinueBlockedWorkflow,
    continueBlockedNodeId,
    canRecoverWorkflowRun,
    runDebuggerRetryNodeId,
    serverBlockedNodeIds,
    serverNeedsRepairNodeIds,
    selectedContextRunId,
    runSummaryRows,
    workflowProjection,
    runArtifacts,
    selectedBoardTaskId,
    selectedBoardTask,
    boardDetailRef,
    selectedBoardTaskOutput,
    selectedBoardTaskValidationOutcome,
    selectedBoardTaskWarningCount,
    selectedBoardTaskTelemetry,
    selectedBoardTaskArtifactValidation,
    selectedBoardTaskIsWorkflowNode,
    selectedBoardTaskIsProjectedBacklogItem,
    selectedBoardTaskWorkflowClass,
    selectedBoardTaskPhase,
    selectedBoardTaskFailureKind,
    selectedBoardTaskQualityMode,
    selectedBoardTaskEmergencyRollbackEnabled,
    selectedBoardTaskBlockerCategory,
    selectedBoardTaskValidationBasis,
    selectedBoardTaskReceiptLedger,
    selectedBoardTaskArtifactCandidates,
    selectedBoardTaskWarningRequirements,
    selectedBoardTaskReceiptTimeline,
    selectedBoardTaskLifecycleEvents,
    selectedBoardTaskResearchReadPaths,
    selectedBoardTaskDiscoveredRelevantPaths,
    selectedBoardTaskUnmetResearchRequirements,
    selectedBoardTaskReviewedPathsBackedByRead,
    selectedBoardTaskUnreviewedRelevantPaths,
    selectedBoardTaskVerificationOutcome,
    selectedBoardTaskVerificationPassed,
    selectedBoardTaskVerificationResults,
    selectedBoardTaskFailureDetail,
    selectedBoardTaskRelatedPaths,
    selectedBoardTaskRelatedArtifacts,
    selectedBoardTaskNodeId,
    selectedBoardTaskStateNormalized,
    selectedBoardTaskImpactSummary,
    selectedBoardTaskResetOutputPaths,
    canTaskContinue,
    canTaskRetry,
    selectedBoardTaskServerActionMessage,
    canTaskRequeue,
    canBacklogTaskClaim,
    canBacklogTaskRequeue,
    selectedBoardTaskTouchedFiles,
    selectedBoardTaskUndeclaredFiles,
    selectedBoardTaskRequestedQualityMode,
    selectedSessionId,
    selectedSessionFilterId,
    availableSessionIds,
    sessionLogEntries,
    sessionLogRef,
    selectedLogSource,
    telemetryEvents,
    filteredRunEventEntries,
    blockers,
    runHints,
    runRepairGuidanceEntries,
    artifactsSectionRef,
    runArtifactEntries,
    selectedRunArtifactKey,
    runHistoryEvents,
    workflowContextRun,
    workflowBlackboard,
  } = state;

  const {
    onSelectRunId,
    onRefresh,
    onClose,
    onSelectBoardTaskId,
    focusArtifactEntry,
    onSessionFilterChange,
    onCopySessionLog,
    onJumpToLatest,
    onPinnedStateChange,
    onSelectLogSource,
    onFocusNode,
    onToggleArtifact,
    onCopyFullDebugContext,
    onNavigateFeed,
    workflowTaskContinueMutation,
    workflowTaskRetryMutation,
    workflowTaskRequeueMutation,
    workflowRepairMutation,
    workflowRecoverMutation,
    backlogTaskClaimMutation,
    backlogTaskRequeueMutation,
    runActionMutation,
    taskResetPreviewQuery,
  } = actions;

  const {
    runTimeLabel,
    workflowCompletedNodeCount,
    workflowBlockedNodeCount,
    workflowActiveSessionCount,
    statusColor,
    isActiveRunStatus,
    compactIdentifier,
    sessionLabel,
    formatTimestampLabel,
  } = helpers;
  const pendingRunAction = runActionMutation.isPending
    ? String(runActionMutation.variables?.action || "").trim()
    : "";
  const pendingRunActionLabel =
    pendingRunAction === "cancel"
      ? "Cancelling..."
      : pendingRunAction === "resume"
        ? "Resuming..."
        : pendingRunAction === "pause"
          ? "Pausing..."
          : "Working...";
  const pendingRunActionMessage = pendingRunAction
    ? `Waiting for ${pendingRunAction} request to finish.`
    : "";

  if (!selectedRunId) return null;

  return (
    <motion.div
      className="tcp-confirm-overlay"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={() => onSelectRunId("")}
    >
      <motion.div
        className="tcp-confirm-dialog tcp-run-debugger-modal"
        initial={{ opacity: 0, y: 8, scale: 0.98 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: 6, scale: 0.98 }}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="mb-3 flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
          <div className="grid gap-1">
            <h3 className="tcp-confirm-title">Run Debugger</h3>
            <div className="tcp-subtle text-xs">
              automation:{" "}
              {String(selectedRun?.automation_id || selectedRun?.routine_id || "unknown")}
              {" · "}run: {selectedRunId}
              {" · "}running for {runTimeLabel(selectedRun)}
            </div>
            {isWorkflowRun ? (
              <div className="tcp-subtle text-xs">
                completed nodes: {workflowCompletedNodeCount(selectedRun)}
                {" · "}blocked nodes: {workflowBlockedNodeCount(selectedRun)}
                {" · "}active sessions: {workflowActiveSessionCount(selectedRun)}
              </div>
            ) : null}
          </div>
          <div className="flex w-full flex-col gap-2 sm:flex-row sm:flex-wrap sm:items-center lg:w-auto">
            <span className={statusColor(runStatus)}>{runStatus || "unknown"}</span>
            {runStatusDerivedFromBlockedNodes ? (
              <span className="tcp-subtle">derived from blocked nodes</span>
            ) : null}
            {canContinueBlockedWorkflow ? (
              <button
                type="button"
                className="tcp-btn h-8 w-full px-2 text-xs sm:w-auto"
                onClick={() =>
                  workflowTaskContinueMutation.mutate({
                    runId: selectedRunId,
                    nodeId: continueBlockedNodeId,
                    reason: `continued blocked task ${continueBlockedNodeId} from run debugger`,
                  })
                }
                disabled={
                  !continueBlockedNodeId ||
                  workflowTaskContinueMutation.isPending ||
                  runActionMutation.isPending
                }
                title={
                  pendingRunActionMessage ||
                  (continueBlockedNodeId
                    ? `Continue blocked task ${continueBlockedNodeId} with minimal reset`
                    : "Select a blocked node to continue")
                }
              >
                <i data-lucide="skip-forward"></i>
                {workflowTaskContinueMutation.isPending ? "Continuing..." : "Continue"}
              </button>
            ) : null}
            {canRecoverWorkflowRun ? (
              <button
                type="button"
                className="tcp-btn h-8 w-full px-2 text-xs sm:w-auto"
                onClick={() =>
                  runDebuggerRetryNodeId
                    ? workflowTaskRetryMutation.mutate({
                        runId: selectedRunId,
                        nodeId: runDebuggerRetryNodeId,
                        reason: `retried task ${runDebuggerRetryNodeId} from run debugger`,
                      })
                    : workflowRecoverMutation.mutate({
                        runId: selectedRunId,
                        reason: "retried from run debugger",
                      })
                }
                disabled={
                  !selectedRunId ||
                  workflowRecoverMutation.isPending ||
                  workflowTaskRetryMutation.isPending ||
                  runActionMutation.isPending
                }
                title={
                  pendingRunActionMessage ||
                  (runDebuggerRetryNodeId
                    ? `Retry selected task ${runDebuggerRetryNodeId}`
                    : "Retry the whole run")
                }
              >
                <i data-lucide="rotate-ccw"></i>
                {runDebuggerRetryNodeId
                  ? workflowTaskRetryMutation.isPending
                    ? "Retrying task..."
                    : "Retry Task"
                  : workflowRecoverMutation.isPending
                    ? "Retrying..."
                    : "Retry"}
              </button>
            ) : null}
            <button
              type="button"
              className="tcp-btn h-8 w-full px-2 text-xs sm:w-auto"
              onClick={() =>
                runActionMutation.mutate({
                  action: runStatus === "paused" ? "resume" : "pause",
                  runId: selectedRunId,
                  family: isWorkflowRun ? "v2" : "legacy",
                })
              }
              disabled={
                !selectedRunId ||
                runActionMutation.isPending ||
                !(runStatus === "paused" || isActiveRunStatus(runStatus))
              }
            >
              <i data-lucide={runStatus === "paused" ? "play" : "pause"}></i>
              {runStatus === "paused" ? "Resume" : "Pause"}
            </button>
            {isWorkflowRun ? (
              <button
                type="button"
                className="tcp-btn h-8 w-full px-2 text-xs sm:w-auto"
                onClick={() =>
                  runActionMutation.mutate({
                    action: "cancel",
                    runId: selectedRunId,
                    family: "v2",
                    reason: "cancelled from run debugger",
                  })
                }
                disabled={
                  !selectedRunId || runActionMutation.isPending || runStatus === "cancelled"
                }
                title="Force stop this workflow run and clear active sessions"
              >
                <i data-lucide="square"></i>
                {runActionMutation.isPending ? pendingRunActionLabel : "Cancel"}
              </button>
            ) : null}
            <button className="tcp-btn h-8 w-full px-2 text-xs sm:w-auto" onClick={onRefresh}>
              <i data-lucide="refresh-cw"></i>
              Refresh
            </button>
            <button className="tcp-btn h-8 w-full px-2 text-xs sm:w-auto" onClick={onClose}>
              <i data-lucide="x"></i>
              Close
            </button>
          </div>
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto pr-1">
          <div className="grid min-h-full content-start gap-3">
            <WorkflowRunSummaryPanel runSummaryRows={runSummaryRows} />
            {isWorkflowRun ? (
              <div className="tcp-list-item">
                <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <div className="font-medium">Workflow Board</div>
                    <div className="tcp-subtle text-xs">
                      context run: {compactIdentifier(selectedContextRunId || "unlinked", 44)}
                      {" · "}tasks: {workflowProjection.tasks.length}
                      {" · "}artifacts: {runArtifacts.length}
                    </div>
                  </div>
                  <span className="tcp-badge-info">
                    {workflowProjection.taskSource === "hybrid"
                      ? "blackboard + context"
                      : workflowProjection.taskSource === "checkpoint"
                        ? "run checkpoint"
                        : workflowProjection.taskSource}
                  </span>
                </div>
                <TaskBoard
                  tasks={workflowProjection.tasks}
                  currentTaskId={workflowProjection.currentTaskId}
                  selectedTaskId={selectedBoardTaskId}
                  onTaskSelect={(task) =>
                    onSelectBoardTaskId((current: string) => (current === task.id ? "" : task.id))
                  }
                />
              </div>
            ) : null}
            <div className="grid min-h-0 items-start gap-3 xl:grid-cols-[1.62fr_1fr]">
              <div className="grid min-h-0 gap-3">
                {selectedBoardTask ? (
                  <div
                    ref={boardDetailRef}
                    className="tcp-list-item relative max-h-[56vh] overflow-y-auto sm:max-h-[28rem]"
                  >
                    <div className="sticky -top-3 z-10 -mx-3 -mt-3 mb-2 flex items-center justify-between gap-2 rounded-t-xl border-b border-slate-800/80 bg-[color:color-mix(in_srgb,var(--color-surface-elevated)_96%,#000_4%)] px-3 py-3 backdrop-blur-sm">
                      <div className="font-medium">Task Details</div>
                      <button
                        type="button"
                        className="chat-icon-btn h-7 w-7"
                        aria-label="Close task details"
                        onClick={() => onSelectBoardTaskId("")}
                      >
                        <i data-lucide="x-circle"></i>
                      </button>
                    </div>
                    <div className="grid gap-2 pr-1 text-sm text-slate-200">
                      <div className="whitespace-pre-wrap break-words font-medium leading-snug">
                        {selectedBoardTask.title}
                      </div>
                      {selectedBoardTask.description ? (
                        <div className="tcp-subtle whitespace-pre-wrap break-words">
                          {selectedBoardTask.description}
                        </div>
                      ) : null}
                      <div className="flex flex-wrap gap-2 text-xs">
                        <span className="tcp-badge-info">{selectedBoardTask.state}</span>
                        {selectedBoardTask.assigned_role ? (
                          <span className="tcp-badge-info">
                            agent: {selectedBoardTask.assigned_role}
                          </span>
                        ) : null}
                        {String((selectedBoardTask as any).task_kind || "").trim() ? (
                          <span className="tcp-badge-info">
                            task: {String((selectedBoardTask as any).task_kind).trim()}
                          </span>
                        ) : null}
                        {String((selectedBoardTask as any).backlog_task_id || "").trim() ? (
                          <span className="tcp-badge-info">
                            backlog: {String((selectedBoardTask as any).backlog_task_id).trim()}
                          </span>
                        ) : null}
                        {String((selectedBoardTask as any).task_owner || "").trim() ? (
                          <span className="tcp-badge-info">
                            owner: {String((selectedBoardTask as any).task_owner).trim()}
                          </span>
                        ) : null}
                        {selectedBoardTask.session_id ? (
                          <span className="tcp-badge-info">
                            {sessionLabel(selectedBoardTask.session_id)}
                          </span>
                        ) : null}
                      </div>
                      {selectedBoardTaskOutput ? (
                        <div className="flex flex-wrap gap-2 text-xs">
                          {String(selectedBoardTaskOutput?.status || "").trim() ? (
                            <span
                              className={
                                String(selectedBoardTaskOutput?.status || "")
                                  .trim()
                                  .toLowerCase() === "blocked"
                                  ? "tcp-badge-blocked"
                                  : selectedBoardTaskValidationOutcome === "accepted_with_warnings"
                                    ? "tcp-badge-warn"
                                    : "tcp-badge-ok"
                              }
                            >
                              status:{" "}
                              {selectedBoardTaskValidationOutcome === "accepted_with_warnings"
                                ? "completed with warnings"
                                : String(selectedBoardTaskOutput?.status || "").trim()}
                            </span>
                          ) : null}
                          {selectedBoardTaskWarningCount > 0 ? (
                            <span className="tcp-badge-warn">
                              {selectedBoardTaskWarningCount} warning
                              {selectedBoardTaskWarningCount === 1 ? "" : "s"}
                            </span>
                          ) : null}
                          {typeof selectedBoardTaskOutput?.approved === "boolean" ? (
                            <span
                              className={
                                selectedBoardTaskOutput.approved ? "tcp-badge-ok" : "tcp-badge-warn"
                              }
                            >
                              approved: {String(selectedBoardTaskOutput.approved)}
                            </span>
                          ) : null}
                          {selectedBoardTaskTelemetry?.workspace_inspection_used ? (
                            <span className="tcp-badge-info">workspace inspected</span>
                          ) : null}
                          {selectedBoardTaskTelemetry?.web_research_used ? (
                            <span className="tcp-badge-info">web research used</span>
                          ) : null}
                          {String(
                            selectedBoardTaskArtifactValidation?.rejected_artifact_reason || ""
                          ).trim() ? (
                            <span className="tcp-badge-warn">artifact rejected</span>
                          ) : null}
                        </div>
                      ) : null}
                      {!selectedBoardTaskIsWorkflowNode ? (
                        <div className="rounded-lg border border-slate-700/60 bg-slate-950/20 p-3 text-xs text-slate-300">
                          {selectedBoardTaskIsProjectedBacklogItem
                            ? "This is a projected backlog task derived from workflow output. You can claim or requeue it here without resetting the source workflow node."
                            : "This is a projected backlog task derived from workflow output, not a direct automation node. Inspect it here, but use the source workflow stage for retry, continue, or requeue actions."}
                        </div>
                      ) : null}
                      {selectedBoardTask.runtime_detail ? (
                        <div className="whitespace-pre-wrap break-words rounded-lg border border-slate-700/60 bg-slate-950/30 p-3 text-xs text-slate-300">
                          {selectedBoardTask.runtime_detail}
                        </div>
                      ) : null}
                      {selectedBoardTaskWorkflowClass ||
                      selectedBoardTaskPhase ||
                      selectedBoardTaskFailureKind ||
                      selectedBoardTaskWarningCount ||
                      selectedBoardTaskArtifactCandidates.length ? (
                        <div className="rounded-lg border border-slate-700/60 bg-slate-950/20 p-3 text-xs text-slate-300">
                          <div className="font-medium text-slate-200">Stability Contract</div>
                          <div className="mt-3 grid gap-2 sm:grid-cols-2">
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">workflow class</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {selectedBoardTaskWorkflowClass || "n/a"}
                              </div>
                            </div>
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">phase</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {selectedBoardTaskPhase || "n/a"}
                              </div>
                            </div>
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">failure kind</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {selectedBoardTaskFailureKind || "n/a"}
                              </div>
                            </div>
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">validation outcome</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {selectedBoardTaskValidationOutcome || "n/a"}
                              </div>
                            </div>
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">quality mode</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {selectedBoardTaskQualityMode || "n/a"}
                              </div>
                            </div>
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">rollback</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {selectedBoardTaskEmergencyRollbackEnabled === null
                                  ? "n/a"
                                  : selectedBoardTaskEmergencyRollbackEnabled
                                    ? "enabled"
                                    : "disabled"}
                              </div>
                            </div>
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">blocker category</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {selectedBoardTaskBlockerCategory || "n/a"}
                              </div>
                            </div>
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2 sm:col-span-2">
                              <div className="tcp-subtle">validation basis</div>
                              <div className="mt-1 whitespace-pre-wrap break-words font-medium text-slate-100">
                                {selectedBoardTaskValidationBasis
                                  ? formatJson(selectedBoardTaskValidationBasis)
                                  : "n/a"}
                              </div>
                            </div>
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2 sm:col-span-2">
                              <div className="tcp-subtle">receipt ledger</div>
                              <div className="mt-1 whitespace-pre-wrap break-words font-medium text-slate-100">
                                {selectedBoardTaskReceiptLedger
                                  ? formatJson(selectedBoardTaskReceiptLedger)
                                  : "n/a"}
                              </div>
                            </div>
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">artifact candidates</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {selectedBoardTaskArtifactCandidates.length}
                              </div>
                            </div>
                          </div>
                          {selectedBoardTaskWarningRequirements.length ? (
                            <div className="mt-3 rounded-md border border-amber-500/30 bg-amber-500/10 p-3">
                              <div className="mb-2 font-medium text-amber-100">
                                Non-blocking warnings
                              </div>
                              <div className="flex flex-wrap gap-2">
                                {selectedBoardTaskWarningRequirements.map((item: string) => (
                                  <span key={item} className="tcp-badge-warn">
                                    {item.replace(/_/g, " ")}
                                  </span>
                                ))}
                              </div>
                            </div>
                          ) : null}
                          {selectedBoardTaskArtifactCandidates.length ? (
                            <div className="mt-3 grid gap-2">
                              {selectedBoardTaskArtifactCandidates.map(
                                (candidate: any, index: number) => (
                                  <div
                                    key={`${String(candidate?.source || "candidate")}-${index}`}
                                    className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2"
                                  >
                                    <div className="flex flex-wrap items-center gap-2">
                                      <span className="tcp-badge-info">
                                        {String(candidate?.source || "candidate")}
                                      </span>
                                      {candidate?.accepted ? (
                                        <span className="tcp-badge-ok">accepted</span>
                                      ) : null}
                                      {candidate?.substantive ? (
                                        <span className="tcp-badge-ok">substantive</span>
                                      ) : (
                                        <span className="tcp-badge-warn">non-substantive</span>
                                      )}
                                      {candidate?.placeholder_like ? (
                                        <span className="tcp-badge-warn">placeholder-like</span>
                                      ) : null}
                                    </div>
                                    <div className="mt-1 tcp-subtle">
                                      {Number(candidate?.length || 0)} chars
                                    </div>
                                  </div>
                                )
                              )}
                            </div>
                          ) : null}
                          {selectedBoardTaskReceiptTimeline.length ? (
                            <div className="mt-3 grid gap-2">
                              <div className="tcp-subtle">receipt timeline</div>
                              {selectedBoardTaskReceiptTimeline.map(
                                (receipt: any, index: number) => (
                                  <div
                                    key={`${String(receipt?.seq || index)}:${String(
                                      receipt?.eventType || receipt?.event_type || ""
                                    )}`}
                                    className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2"
                                  >
                                    <div className="flex flex-wrap items-center gap-2">
                                      <span className="tcp-badge-info">
                                        seq {String(receipt?.seq || index + 1)}
                                      </span>
                                      {String(receipt?.eventType || "").trim() ? (
                                        <span className="tcp-badge-info">
                                          {String(receipt.eventType).trim()}
                                        </span>
                                      ) : null}
                                      {String(receipt?.receiptKind || "").trim() ? (
                                        <span className="tcp-badge-info">
                                          {String(receipt.receiptKind).trim()}
                                        </span>
                                      ) : null}
                                      {String(receipt?.attempt || "").trim() ? (
                                        <span className="tcp-badge-info">
                                          attempt {String(receipt.attempt).trim()}
                                        </span>
                                      ) : null}
                                      {Number(receipt?.at || 0) > 0 ? (
                                        <span className="tcp-subtle text-[11px]">
                                          {formatTimestampLabel(receipt.at)}
                                        </span>
                                      ) : null}
                                    </div>
                                    <div className="mt-1 text-slate-300">
                                      {String(receipt?.detail || "").trim() || "receipt"}
                                    </div>
                                    <details className="mt-2">
                                      <summary className="cursor-pointer text-xs text-slate-400">
                                        raw record
                                      </summary>
                                      <pre className="tcp-code mt-2 max-h-40 overflow-auto text-[11px]">
                                        {formatJson(receipt?.raw || receipt)}
                                      </pre>
                                    </details>
                                  </div>
                                )
                              )}
                            </div>
                          ) : null}
                          {selectedBoardTaskLifecycleEvents.length ? (
                            <div className="mt-3 grid gap-2">
                              <div className="tcp-subtle">recent workflow events</div>
                              {selectedBoardTaskLifecycleEvents.map(
                                (summary: any, index: number) => (
                                  <div
                                    key={`${summary.event}-${String(summary.recordedAtMs || index)}`}
                                    className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2"
                                  >
                                    <div className="flex flex-wrap items-center gap-2">
                                      <span className="tcp-badge-info">{summary.event}</span>
                                      {summary.phase ? (
                                        <span className="tcp-badge-info">{summary.phase}</span>
                                      ) : null}
                                      {summary.failureKind ? (
                                        <span className="tcp-badge-warn">
                                          {summary.failureKind}
                                        </span>
                                      ) : null}
                                    </div>
                                    <div className="mt-1 text-slate-300">{summary.reason}</div>
                                  </div>
                                )
                              )}
                            </div>
                          ) : null}
                        </div>
                      ) : null}
                      {String((selectedBoardTask as any).write_scope || "").trim() ||
                      String((selectedBoardTask as any).repo_root || "").trim() ||
                      String((selectedBoardTask as any).acceptance_criteria || "").trim() ||
                      String((selectedBoardTask as any).task_dependencies || "").trim() ||
                      String((selectedBoardTask as any).verification_state || "").trim() ||
                      String((selectedBoardTask as any).verification_command || "").trim() ||
                      String((selectedBoardTask as any).output_path || "").trim() ? (
                        <div className="rounded-lg border border-slate-700/60 bg-slate-950/20 p-3 text-xs text-slate-300">
                          <div className="font-medium text-slate-200">Coding Task Context</div>
                          <div className="mt-2 space-y-1">
                            <div>
                              backlog task:{" "}
                              {String((selectedBoardTask as any).backlog_task_id || "").trim() ||
                                "n/a"}
                            </div>
                            <div>
                              repo root:{" "}
                              {String((selectedBoardTask as any).repo_root || "").trim() || "n/a"}
                            </div>
                            <div>
                              output path:{" "}
                              {String((selectedBoardTask as any).output_path || "").trim() || "n/a"}
                            </div>
                            <div>
                              write scope:{" "}
                              {String((selectedBoardTask as any).write_scope || "").trim() || "n/a"}
                            </div>
                            <div>
                              acceptance criteria:{" "}
                              {String(
                                (selectedBoardTask as any).acceptance_criteria || ""
                              ).trim() || "n/a"}
                            </div>
                            <div>
                              task dependencies:{" "}
                              {String((selectedBoardTask as any).task_dependencies || "").trim() ||
                                "n/a"}
                            </div>
                            <div>
                              verification state:{" "}
                              {String((selectedBoardTask as any).verification_state || "").trim() ||
                                "n/a"}
                            </div>
                            <div>
                              owner:{" "}
                              {String((selectedBoardTask as any).task_owner || "").trim() || "n/a"}
                            </div>
                            <div>
                              lease owner:{" "}
                              {String((selectedBoardTask as any).lease_owner || "").trim() || "n/a"}
                            </div>
                            <div>
                              lease expires:{" "}
                              {(selectedBoardTask as any).lease_expires_at_ms
                                ? formatTimestampLabel(
                                    (selectedBoardTask as any).lease_expires_at_ms
                                  )
                                : "n/a"}
                            </div>
                            <div>
                              stale lease: {(selectedBoardTask as any).stale_lease ? "yes" : "no"}
                            </div>
                            <div>
                              verification:{" "}
                              {String(
                                (selectedBoardTask as any).verification_command || ""
                              ).trim() || "n/a"}
                            </div>
                          </div>
                        </div>
                      ) : null}
                      {selectedBoardTaskResearchReadPaths.length ||
                      selectedBoardTaskDiscoveredRelevantPaths.length ||
                      selectedBoardTaskUnmetResearchRequirements.length ||
                      selectedBoardTaskArtifactValidation?.repair_attempted ? (
                        <div className="rounded-lg border border-slate-700/60 bg-slate-950/20 p-3 text-xs text-slate-300">
                          <div className="font-medium text-slate-200">
                            Research Requirement Status
                          </div>
                          <div className="mt-3 grid gap-2 sm:grid-cols-2">
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">discovered relevant files</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {selectedBoardTaskDiscoveredRelevantPaths.length}
                              </div>
                            </div>
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">actual read calls backed by path</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {selectedBoardTaskResearchReadPaths.length}
                              </div>
                            </div>
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">reviewed paths backed by read</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {selectedBoardTaskReviewedPathsBackedByRead.length}
                              </div>
                            </div>
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">web research</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {selectedBoardTaskArtifactValidation?.web_research_attempted
                                  ? selectedBoardTaskArtifactValidation?.web_research_succeeded
                                    ? "attempted and succeeded"
                                    : "attempted but not successful"
                                  : "not attempted"}
                              </div>
                            </div>
                          </div>
                          <div className="mt-3 grid gap-2 sm:grid-cols-2">
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">repair pass</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {selectedBoardTaskArtifactValidation?.repair_attempted
                                  ? selectedBoardTaskArtifactValidation?.repair_succeeded
                                    ? "attempted and satisfied"
                                    : selectedBoardTaskArtifactValidation?.repair_exhausted
                                      ? "attempted and exhausted"
                                      : "attempted and still active"
                                  : "not needed or not attempted"}
                              </div>
                              {selectedBoardTaskArtifactValidation?.repair_attempted ? (
                                <div className="mt-1 tcp-subtle">
                                  attempt{" "}
                                  {Number(selectedBoardTaskArtifactValidation?.repair_attempt || 0)}{" "}
                                  of{" "}
                                  {Number(
                                    selectedBoardTaskArtifactValidation?.repair_attempt || 0
                                  ) +
                                    Number(
                                      selectedBoardTaskArtifactValidation?.repair_attempts_remaining ||
                                        0
                                    )}
                                </div>
                              ) : null}
                            </div>
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">missing / unreviewed relevant files</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {selectedBoardTaskUnreviewedRelevantPaths.length}
                              </div>
                            </div>
                          </div>
                          {selectedBoardTaskUnmetResearchRequirements.length ? (
                            <div className="mt-3">
                              <div className="tcp-subtle mb-1">unmet requirements</div>
                              <div className="flex flex-wrap gap-1">
                                {selectedBoardTaskUnmetResearchRequirements.map((item: any) => (
                                  <span
                                    key={item}
                                    className="rounded-full border border-emerald-500/30 bg-emerald-950/20 px-2 py-1 text-[11px] text-emerald-100/90"
                                  >
                                    {item}
                                  </span>
                                ))}
                              </div>
                            </div>
                          ) : null}
                          {selectedBoardTaskUnreviewedRelevantPaths.length ? (
                            <div className="mt-3">
                              <div className="tcp-subtle mb-1">unreviewed relevant files</div>
                              <div className="flex flex-wrap gap-1">
                                {selectedBoardTaskUnreviewedRelevantPaths.map((path: any) => (
                                  <span
                                    key={path}
                                    className="rounded-full border border-slate-700/70 bg-slate-950/30 px-2 py-1 font-mono text-[11px] text-slate-300"
                                  >
                                    {path}
                                  </span>
                                ))}
                              </div>
                            </div>
                          ) : null}
                        </div>
                      ) : null}
                      {selectedBoardTaskOutput ||
                      selectedBoardTaskRelatedPaths.length ||
                      selectedBoardTaskFailureDetail ? (
                        <div className="rounded-lg border border-slate-700/60 bg-slate-950/20 p-3 text-xs text-slate-300">
                          <div className="font-medium text-slate-200">
                            Coding Verification & Failures
                          </div>
                          <div className="mt-3 grid gap-2 sm:grid-cols-2">
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">verification outcome</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {selectedBoardTaskVerificationOutcome}
                              </div>
                            </div>
                            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle">verification passed</div>
                              <div className="mt-1 font-medium text-slate-100">
                                {typeof selectedBoardTaskVerificationPassed === "boolean"
                                  ? selectedBoardTaskVerificationPassed
                                    ? "yes"
                                    : "no"
                                  : "n/a"}
                              </div>
                            </div>
                          </div>
                          {selectedBoardTaskTelemetry?.verification_expected ? (
                            <div className="mt-3 rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                              <div className="tcp-subtle mb-2">verification plan</div>
                              <div className="mb-2 text-slate-200/80">
                                {Number(selectedBoardTaskTelemetry?.verification_completed || 0)} /{" "}
                                {Number(selectedBoardTaskTelemetry?.verification_total || 0)} checks
                                ran
                              </div>
                              {selectedBoardTaskVerificationResults.length ? (
                                <div className="grid gap-2">
                                  {selectedBoardTaskVerificationResults.map(
                                    (result: any, index: number) => (
                                      <div
                                        key={`${String(result?.command || index)}-${index}`}
                                        className="rounded-md border border-slate-800/80 bg-slate-950/40 p-2"
                                      >
                                        <div className="flex flex-wrap items-center gap-2">
                                          <span className="tcp-badge-info">
                                            {String(result?.kind || "verify")}
                                          </span>
                                          <span
                                            className={
                                              result?.failed
                                                ? "tcp-badge-warn"
                                                : result?.ran
                                                  ? "tcp-badge-ok"
                                                  : "tcp-badge-info"
                                            }
                                          >
                                            {result?.failed
                                              ? "failed"
                                              : result?.ran
                                                ? "passed"
                                                : "not run"}
                                          </span>
                                        </div>
                                        <div className="mt-1 break-words font-mono text-[11px] text-slate-200">
                                          {String(result?.command || "").trim() || "n/a"}
                                        </div>
                                        {String(result?.failure || "").trim() ? (
                                          <div className="mt-1 whitespace-pre-wrap break-words text-[11px] text-emerald-100/90">
                                            {String(result?.failure || "").trim()}
                                          </div>
                                        ) : null}
                                      </div>
                                    )
                                  )}
                                </div>
                              ) : null}
                            </div>
                          ) : null}
                          {selectedBoardTaskFailureDetail ? (
                            <div className="mt-3 rounded-md border border-emerald-500/30 bg-emerald-950/20 p-2 text-emerald-100/90">
                              <div className="tcp-subtle mb-1 text-emerald-100/70">
                                failure detail
                              </div>
                              <div className="whitespace-pre-wrap break-words">
                                {selectedBoardTaskFailureDetail}
                              </div>
                            </div>
                          ) : null}
                          {selectedBoardTaskRelatedPaths.length ? (
                            <div className="mt-3">
                              <div className="tcp-subtle mb-1">related artifacts</div>
                              <div className="flex flex-wrap gap-2">
                                {selectedBoardTaskRelatedPaths.map((path: any) => (
                                  <button
                                    key={path}
                                    type="button"
                                    className="rounded-full border border-slate-700/70 bg-slate-950/30 px-2 py-1 font-mono text-[11px] text-slate-200 transition hover:border-sky-500/40 hover:text-sky-100"
                                    onClick={() => focusArtifactEntry(path)}
                                    title={path}
                                  >
                                    Open {compactIdentifier(path, 44)}
                                  </button>
                                ))}
                              </div>
                              {selectedBoardTaskRelatedArtifacts.length ? (
                                <div className="mt-2 tcp-subtle">
                                  matched run artifacts:{" "}
                                  {selectedBoardTaskRelatedArtifacts
                                    .map((entry: any) => entry.name)
                                    .join(", ")}
                                </div>
                              ) : (
                                <div className="mt-2 tcp-subtle">
                                  No matching run artifact found yet. The button will still jump to
                                  the artifacts section.
                                </div>
                              )}
                            </div>
                          ) : null}
                          <WorkflowTaskActionsPanel
                            selectedBoardTask={selectedBoardTask}
                            selectedBoardTaskIsWorkflowNode={selectedBoardTaskIsWorkflowNode}
                            selectedBoardTaskIsProjectedBacklogItem={
                              selectedBoardTaskIsProjectedBacklogItem
                            }
                            selectedBoardTaskNodeId={selectedBoardTaskNodeId}
                            selectedBoardTaskStateNormalized={selectedBoardTaskStateNormalized}
                            selectedBoardTaskImpactSummary={selectedBoardTaskImpactSummary}
                            selectedBoardTaskResetOutputPaths={selectedBoardTaskResetOutputPaths}
                            selectedRunId={selectedRunId}
                            canTaskContinue={canTaskContinue}
                            canTaskRetry={canTaskRetry}
                            runDebuggerRetryNodeId={runDebuggerRetryNodeId}
                            continueBlockedNodeId={continueBlockedNodeId}
                            selectedBoardTaskServerActionMessage={
                              selectedBoardTaskServerActionMessage
                            }
                            canTaskRequeue={canTaskRequeue}
                            canBacklogTaskClaim={canBacklogTaskClaim}
                            canBacklogTaskRequeue={canBacklogTaskRequeue}
                            canRecoverWorkflowRun={canRecoverWorkflowRun}
                            backlogTaskClaimMutation={backlogTaskClaimMutation}
                            backlogTaskRequeueMutation={backlogTaskRequeueMutation}
                            workflowTaskContinueMutation={workflowTaskContinueMutation}
                            workflowTaskRetryMutation={workflowTaskRetryMutation}
                            workflowTaskRequeueMutation={workflowTaskRequeueMutation}
                            workflowRepairMutation={workflowRepairMutation}
                            workflowRecoverMutation={workflowRecoverMutation}
                            taskResetPreviewQuery={taskResetPreviewQuery}
                          />
                        </div>
                      ) : null}
                      <WorkflowTaskSignalsPanel
                        selectedBoardTask={selectedBoardTask}
                        selectedBoardTaskTelemetry={selectedBoardTaskTelemetry}
                        selectedBoardTaskArtifactValidation={selectedBoardTaskArtifactValidation}
                        selectedBoardTaskTouchedFiles={selectedBoardTaskTouchedFiles}
                        selectedBoardTaskUndeclaredFiles={selectedBoardTaskUndeclaredFiles}
                        selectedBoardTaskRequestedQualityMode={
                          selectedBoardTaskRequestedQualityMode
                        }
                        selectedBoardTaskEmergencyRollbackEnabled={
                          selectedBoardTaskEmergencyRollbackEnabled
                        }
                        selectedBoardTaskBlockerCategory={selectedBoardTaskBlockerCategory}
                        selectedBoardTaskValidationBasis={selectedBoardTaskValidationBasis}
                        selectedBoardTaskReceiptTimeline={selectedBoardTaskReceiptTimeline}
                      />
                      {selectedBoardTask.error_message ? (
                        <div className="whitespace-pre-wrap break-words rounded-lg border border-rose-500/30 bg-rose-950/20 p-3 text-xs text-rose-200">
                          {selectedBoardTask.error_message}
                        </div>
                      ) : null}
                      {selectedBoardTask.dependencies.length ? (
                        <div className="flex flex-wrap gap-1 text-xs">
                          {selectedBoardTask.dependencies.map((dep: any) => (
                            <span key={dep} className="tcp-badge-info">
                              depends on {dep}
                            </span>
                          ))}
                        </div>
                      ) : null}
                    </div>
                  </div>
                ) : null}
                <WorkflowLiveSessionLogPanel
                  selectedSessionId={selectedSessionId}
                  selectedSessionFilterId={selectedSessionFilterId}
                  availableSessionIds={availableSessionIds}
                  sessionLogEntries={sessionLogEntries}
                  sessionLogRef={sessionLogRef}
                  compactIdentifier={compactIdentifier}
                  sessionLabel={sessionLabel}
                  onSessionFilterChange={onSessionFilterChange}
                  onCopySessionLog={onCopySessionLog}
                  onJumpToLatest={onJumpToLatest}
                  onPinnedStateChange={onPinnedStateChange}
                />
                <WorkflowRunTelemetryPanel
                  selectedLogSource={selectedLogSource}
                  telemetryEvents={telemetryEvents}
                  isWorkflowRun={isWorkflowRun}
                  filteredRunEventEntries={filteredRunEventEntries}
                  formatTimestampLabel={formatTimestampLabel}
                  onSelectLogSource={onSelectLogSource}
                />
              </div>
              <div className="grid min-h-0 content-start gap-3 overflow-visible">
                <WorkflowBlockersPanel blockers={blockers} />
                <WorkflowDebugHintsPanel runHints={runHints} />
                <WorkflowRequiredActionsPanel
                  runRepairGuidanceEntries={runRepairGuidanceEntries}
                  blockedNodeIds={serverBlockedNodeIds}
                  needsRepairNodeIds={serverNeedsRepairNodeIds}
                  isWorkflowRun={isWorkflowRun}
                  selectedRunId={selectedRunId}
                  hasActiveSessions={workflowActiveSessionCount(selectedRun) > 0}
                  onFocusNode={onFocusNode}
                  workflowTaskRetryMutation={workflowTaskRetryMutation}
                  workflowTaskContinueMutation={workflowTaskContinueMutation}
                  workflowTaskRequeueMutation={workflowTaskRequeueMutation}
                />
                <WorkflowMissionObjectivePanel
                  objective={String(selectedRun?.mission_snapshot?.objective || "n/a")}
                />
                <div ref={artifactsSectionRef}>
                  <WorkflowArtifactsPanel
                    artifactCount={runArtifacts.length}
                    artifactEntries={runArtifactEntries}
                    selectedArtifactKey={selectedRunArtifactKey}
                    isWorkflowRun={isWorkflowRun}
                    onToggleArtifact={onToggleArtifact}
                  />
                </div>
                <div className="tcp-list-item min-h-0">
                  <div className="font-medium">
                    {isWorkflowRun ? "Run History" : "Persisted History"}
                  </div>
                  {runHistoryEvents.length ? (
                    <div className="mt-2 grid gap-2 overflow-auto pr-1 sm:max-h-[14rem]">
                      {runHistoryEvents.map((event: any, index: number) => (
                        <details
                          key={`${String(event?.id || event?.event || event?.type || "history")}-${index}`}
                          className="rounded-lg border border-slate-700/40 bg-slate-900/25 p-2"
                        >
                          <summary className="cursor-pointer list-none">
                            <div className="flex items-center justify-between gap-2">
                              <span className="text-xs font-medium text-slate-200">
                                {String(event?.type || event?.event || event?.status || "history")}
                              </span>
                              <span className="tcp-subtle text-[11px]">
                                {formatTimestampLabel(
                                  event?.ts_ms || event?.tsMs || event?.at || event?.timestamp_ms
                                )}
                              </span>
                            </div>
                            <div className="tcp-subtle mt-1 text-xs">
                              {String(
                                event?.detail ||
                                  event?.reason ||
                                  event?.family ||
                                  event?.status ||
                                  "No summary available."
                              )}
                            </div>
                          </summary>
                          <pre className="tcp-code mt-2 max-h-32 overflow-auto text-[11px]">
                            {formatJson(event)}
                          </pre>
                        </details>
                      ))}
                    </div>
                  ) : (
                    <div className="tcp-subtle mt-2 text-xs">
                      {isWorkflowRun
                        ? "No context-run history has been persisted for this workflow run yet."
                        : "No persisted history rows returned for this automation."}
                    </div>
                  )}
                </div>
                <div className="tcp-list-item min-h-0">
                  <div className="mb-2 flex items-center justify-between gap-2">
                    <div className="font-medium">Raw Run Payload</div>
                    <button className="tcp-btn h-7 px-2 text-xs" onClick={onCopyFullDebugContext}>
                      <i data-lucide="copy-plus"></i>
                      Copy all debug context
                    </button>
                  </div>
                  <pre className="tcp-code overflow-auto sm:max-h-[18rem]">
                    {formatJson({
                      run: selectedRun,
                      contextRun: workflowContextRun,
                      blackboard: workflowBlackboard,
                    })}
                  </pre>
                </div>
              </div>
            </div>
          </div>
        </div>
        <div className="tcp-confirm-actions mt-3">
          <button className="tcp-btn" onClick={onNavigateFeed}>
            <i data-lucide="radio"></i>
            Open Live Feed
          </button>
          <button className="tcp-btn" onClick={() => onSelectRunId("")}>
            <i data-lucide="x"></i>
            Close
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}
