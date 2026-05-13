import { useMemo } from "react";
import {
  executionProfileLabel,
  workflowEffectiveExecutionProfile,
  workflowRequestedExecutionProfile,
} from "./AutomationsRunHelpers";
import {
  workflowBlockedNodeCount,
  workflowCompletedNodeCount,
  workflowPendingNodeCount,
} from "../orchestration/workflowStability";

export function useRunSummaryRows({
  isWorkflowRun,
  runArtifacts,
  runStatus,
  runStatusDerivedNote,
  selectedRun,
  workflowContextEvents,
  workflowContextPatches,
  workflowProjection,
}: any) {
  return useMemo(() => {
    const rows: Array<{ label: string; value: string }> = [];
    rows.push({ label: "status", value: runStatus || "unknown" });
    if (runStatusDerivedNote) {
      rows.push({ label: "status note", value: runStatusDerivedNote });
    }
    const effectiveProfile = workflowEffectiveExecutionProfile(selectedRun);
    const requestedProfile = workflowRequestedExecutionProfile(selectedRun);
    const profileValue =
      requestedProfile && requestedProfile !== effectiveProfile
        ? `${executionProfileLabel(effectiveProfile)} (requested ${executionProfileLabel(requestedProfile)})`
        : executionProfileLabel(effectiveProfile);
    rows.push({ label: "execution profile", value: profileValue });
    rows.push({ label: "artifacts", value: String(runArtifacts.length) });
    if (isWorkflowRun) {
      rows.push({ label: "tasks", value: String(workflowProjection.tasks.length) });
      rows.push({ label: "context events", value: String(workflowContextEvents.length) });
      rows.push({ label: "blackboard patches", value: String(workflowContextPatches.length) });
      rows.push({
        label: "completed nodes",
        value: String(workflowCompletedNodeCount(selectedRun)),
      });
      rows.push({ label: "pending nodes", value: String(workflowPendingNodeCount(selectedRun)) });
      rows.push({ label: "blocked nodes", value: String(workflowBlockedNodeCount(selectedRun)) });
    }
    if (String(selectedRun?.detail || "").trim()) {
      rows.push({ label: "detail", value: String(selectedRun.detail).trim() });
    }
    if (selectedRun?.requires_approval !== undefined) {
      rows.push({
        label: "requires approval",
        value: String(Boolean(selectedRun?.requires_approval)),
      });
    }
    if (String(selectedRun?.approval_reason || "").trim()) {
      rows.push({ label: "approval reason", value: String(selectedRun.approval_reason).trim() });
    }
    if (String(selectedRun?.denial_reason || "").trim()) {
      rows.push({ label: "denial reason", value: String(selectedRun.denial_reason).trim() });
    }
    if (String(selectedRun?.paused_reason || "").trim()) {
      rows.push({ label: "paused reason", value: String(selectedRun.paused_reason).trim() });
    }
    return rows;
  }, [
    isWorkflowRun,
    runArtifacts.length,
    runStatus,
    runStatusDerivedNote,
    selectedRun,
    workflowContextEvents.length,
    workflowContextPatches.length,
    workflowProjection.tasks.length,
  ]);
}
