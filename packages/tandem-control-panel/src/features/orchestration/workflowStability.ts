export function workflowCheckpoint(run: any) {
  return run?.checkpoint || {};
}

export function workflowNodeOutputs(run: any): Record<string, any> {
  const checkpoint = workflowCheckpoint(run);
  return (checkpoint?.node_outputs || checkpoint?.nodeOutputs || {}) as Record<string, any>;
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
