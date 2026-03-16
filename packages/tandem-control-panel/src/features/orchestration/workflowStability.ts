export function workflowCheckpoint(run: any) {
  return run?.checkpoint || {};
}

function checkpointStringArray(checkpoint: any, snakeKey: string, camelKey: string) {
  const raw = Array.isArray(checkpoint?.[snakeKey])
    ? checkpoint[snakeKey]
    : Array.isArray(checkpoint?.[camelKey])
      ? checkpoint[camelKey]
      : [];
  return raw.map((value: any) => String(value || "").trim()).filter(Boolean);
}

export function workflowCompletedNodeIds(run: any) {
  return checkpointStringArray(workflowCheckpoint(run), "completed_nodes", "completedNodes");
}

export function workflowBlockedNodeIds(run: any) {
  return checkpointStringArray(workflowCheckpoint(run), "blocked_nodes", "blockedNodes");
}

export function workflowPendingNodeIds(run: any) {
  return checkpointStringArray(workflowCheckpoint(run), "pending_nodes", "pendingNodes");
}

export function workflowNodeOutputs(run: any): Record<string, any> {
  const checkpoint = workflowCheckpoint(run);
  return (checkpoint?.node_outputs || checkpoint?.nodeOutputs || {}) as Record<string, any>;
}

export function workflowNodeOutputEntries(run: any) {
  return Object.entries(workflowNodeOutputs(run)).map(([nodeId, value]) => ({
    nodeId,
    value,
  }));
}

export function workflowNodeOutput(run: any, nodeId: string) {
  const normalized = String(nodeId || "").trim();
  if (!normalized) return null;
  const outputs = workflowNodeOutputs(run);
  return outputs[normalized] || null;
}

export function workflowLifecycleHistory(run: any): any[] {
  const checkpoint = workflowCheckpoint(run);
  if (Array.isArray(checkpoint?.lifecycle_history)) return checkpoint.lifecycle_history;
  if (Array.isArray(checkpoint?.lifecycleHistory)) return checkpoint.lifecycleHistory;
  return [];
}

export function workflowLatestLifecycleEvent(run: any) {
  const lifecycleHistory = workflowLifecycleHistory(run);
  if (!lifecycleHistory.length) return null;
  return (
    [...lifecycleHistory]
      .sort(
        (a: any, b: any) =>
          Number(b?.recorded_at_ms || b?.recordedAtMs || 0) -
          Number(a?.recorded_at_ms || a?.recordedAtMs || 0)
      )
      .find((event: any) => String(event?.event || "").trim()) || null
  );
}

export function workflowRecentNodeEvents(run: any, nodeId: string, limit = 8) {
  const normalized = String(nodeId || "").trim();
  if (!normalized) return [];
  return workflowLifecycleHistory(run)
    .filter((event: any) => {
      const metadataNodeId = String(
        event?.metadata?.node_id || event?.metadata?.nodeId || ""
      ).trim();
      return metadataNodeId === normalized;
    })
    .slice(-limit)
    .reverse();
}

export function workflowLatestNodeOutput(run: any) {
  const outputs = Object.values(workflowNodeOutputs(run)).filter(Boolean);
  if (!outputs.length) return null;
  return outputs[outputs.length - 1] || null;
}

export function workflowNodeOutputText(output: any) {
  const summary = String(output?.summary || "").trim();
  const status = String(output?.status || output?.content?.status || "").trim();
  const blockedReason = String(output?.blocked_reason || output?.blockedReason || "").trim();
  const content = output?.content || {};
  const text = String(content?.text || content?.raw_text || "").trim();
  return [summary, status ? `status: ${status}` : "", blockedReason, text]
    .filter(Boolean)
    .join("\n")
    .trim();
}

export function workflowNodeToolTelemetry(output: any) {
  return output?.tool_telemetry || output?.toolTelemetry || null;
}

export function workflowArtifactValidation(output: any) {
  return output?.artifact_validation || output?.artifactValidation || null;
}

export function workflowArtifactCandidates(output: any): any[] {
  const validation = workflowArtifactValidation(output);
  return Array.isArray(validation?.artifact_candidates) ? validation.artifact_candidates : [];
}

export function workflowNodeStability(output: any) {
  const validation = workflowArtifactValidation(output);
  return {
    workflowClass: String(
      output?.workflow_class ||
        output?.workflowClass ||
        validation?.execution_policy?.workflow_class ||
        ""
    ).trim(),
    phase: String(output?.phase || output?.node_phase || "").trim(),
    failureKind: String(output?.failure_kind || output?.failureKind || "").trim(),
  };
}

export function workflowLatestStabilitySnapshot(run: any) {
  const latestOutput = workflowLatestNodeOutput(run);
  const latestEvent = workflowLatestLifecycleEvent(run);
  const stability = workflowNodeStability(latestOutput);
  return {
    latestOutput,
    latestEvent,
    phase: stability.phase || String(latestEvent?.metadata?.phase || "").trim(),
    failureKind: stability.failureKind || String(latestEvent?.metadata?.failure_kind || "").trim(),
    reason: String(latestEvent?.reason || latestOutput?.blocked_reason || "").trim(),
    status: String(run?.status || latestOutput?.status || "never_run").trim(),
  };
}

export function workflowEventSummary(event: any) {
  const metadata = (event?.metadata || {}) as Record<string, any>;
  return {
    event: String(event?.event || "event").trim() || "event",
    recordedAtMs: Number(event?.recorded_at_ms || event?.recordedAtMs || 0),
    phase: String(event?.phase || metadata?.phase || "").trim(),
    failureKind: String(
      event?.failure_kind || event?.failureKind || metadata?.failure_kind || ""
    ).trim(),
    workflowClass: String(
      event?.workflow_class || event?.workflowClass || metadata?.workflow_class || ""
    ).trim(),
    reason: String(event?.reason || metadata?.reason || "").trim() || "No reason recorded",
    status: String(event?.status || metadata?.status || "").trim(),
  };
}

export function workflowSessionIds(run: any) {
  const direct = Array.isArray(run?.active_session_ids)
    ? run.active_session_ids
    : Array.isArray(run?.activeSessionIds)
      ? run.activeSessionIds
      : [];
  const latest = [
    String(run?.latest_session_id || "").trim(),
    String(run?.latestSessionId || "").trim(),
  ].filter(Boolean);
  const nodeOutputSessionIds = Object.values(workflowNodeOutputs(run))
    .map((entry: any) => {
      const content = entry?.content || {};
      return String(content?.session_id || content?.sessionId || "").trim();
    })
    .filter(Boolean);
  return Array.from(
    new Set(
      [
        ...latest,
        ...direct.map((row: any) => String(row || "").trim()).filter(Boolean),
        ...nodeOutputSessionIds,
      ].filter(Boolean)
    )
  );
}
