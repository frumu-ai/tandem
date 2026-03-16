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

export function workflowCompletedNodeCount(run: any) {
  return workflowCompletedNodeIds(run).length;
}

export function workflowPendingNodeCount(run: any) {
  return workflowPendingNodeIds(run).length;
}

export function workflowActiveSessionCount(run: any) {
  const direct = Array.isArray(run?.active_session_ids)
    ? run.active_session_ids
    : Array.isArray(run?.activeSessionIds)
      ? run.activeSessionIds
      : [];
  return direct.length;
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

export function workflowNodeAttemptCount(run: any, nodeId: string) {
  const checkpoint = workflowCheckpoint(run);
  const normalized = String(nodeId || "").trim();
  if (!normalized) return 0;
  return Number(
    checkpoint?.node_attempts?.[normalized] || checkpoint?.nodeAttempts?.[normalized] || 0
  );
}

export function workflowNodeOutputSessionId(run: any, nodeId: string) {
  const output = workflowNodeOutput(run, nodeId);
  return String(output?.content?.session_id || output?.content?.sessionId || "").trim();
}

export function workflowTaskState(
  run: any,
  nodeId: string,
  dependencyTaskIds: string[]
): "pending" | "runnable" | "done" | "failed" | "blocked" {
  const completed = new Set(workflowCompletedNodeIds(run));
  const blocked = new Set(workflowBlockedNodeIds(run));
  const pending = new Set(workflowPendingNodeIds(run));
  const taskId = String(nodeId || "").trim();
  if (!taskId) return dependencyTaskIds.length ? "pending" : "runnable";
  if (completed.has(taskId)) return "done";
  const output = workflowNodeOutput(run, taskId);
  const outputStatus = String(output?.status || output?.content?.status || "")
    .trim()
    .toLowerCase();
  if (outputStatus === "done") return "done";
  if (outputStatus === "verify_failed" || outputStatus === "failed") return "failed";
  if (blocked.has(taskId) || outputStatus === "blocked") return "blocked";
  const errorText = String(
    output?.error ||
      output?.content?.error ||
      output?.content?.message ||
      output?.content?.status_message ||
      ""
  )
    .trim()
    .toLowerCase();
  if (errorText && (errorText.includes("failed") || errorText.includes("error"))) return "failed";
  if (!pending.has(taskId)) {
    return dependencyTaskIds.length ? "pending" : "runnable";
  }
  return dependencyTaskIds.length ? "pending" : "runnable";
}

export function workflowCurrentTaskId(
  tasks: Array<{ id: string; state: string }> | null | undefined,
  activeTaskId = ""
) {
  const explicit = String(activeTaskId || "").trim();
  if (explicit) return explicit;
  const rows = Array.isArray(tasks) ? tasks : [];
  return (
    rows.find((task) => task.state === "in_progress" || task.state === "assigned")?.id ||
    rows.find((task) => task.state === "runnable")?.id ||
    ""
  );
}

export function workflowProjectionFromRunSnapshot(run: any, activeTaskId = "") {
  const snapshotNodes = Array.isArray(run?.automation_snapshot?.flow?.nodes)
    ? run.automation_snapshot.flow.nodes
    : [];
  if (!snapshotNodes.length) {
    return { currentTaskId: "", taskSource: "empty" as const, tasks: [] as any[] };
  }
  const completed = new Set(workflowCompletedNodeIds(run));
  const tasks = snapshotNodes.map((node: any) => {
    const nodeId = String(node?.node_id || "").trim();
    const taskId = `node-${nodeId}`;
    const dependencies = Array.isArray(node?.depends_on)
      ? node.depends_on.map((dep: unknown) => `node-${String(dep || "").trim()}`).filter(Boolean)
      : [];
    const ready = dependencies.every((depId) => completed.has(depId.replace(/^node-/, "")));
    const state = workflowTaskState(run, nodeId, ready ? [] : dependencies);
    const output = workflowNodeOutput(run, nodeId) || {};
    const inferredState =
      activeTaskId === taskId &&
      String(run?.status || "")
        .trim()
        .toLowerCase() === "running"
        ? "in_progress"
        : state === "pending" && ready
          ? "runnable"
          : state;
    const builder = node?.metadata?.builder || {};
    return {
      id: taskId,
      title: String(node?.objective || nodeId || "Workflow node"),
      description: String(node?.agent_id ? `agent: ${node.agent_id}` : ""),
      dependencies,
      state: inferredState,
      retry_count: workflowNodeAttemptCount(run, nodeId),
      error_message: String(output?.error || output?.content?.error || ""),
      runtime_status: String(output?.status || output?.content?.status || ""),
      runtime_detail: String(output?.summary || output?.content?.message || ""),
      assigned_role: String(node?.agent_id || ""),
      workflow_id: String(run?.automation_id || ""),
      session_id: workflowNodeOutputSessionId(run, nodeId),
      projects_backlog_tasks: Boolean(builder?.project_backlog_tasks),
      backlog_task_id: String(builder?.task_id || ""),
      task_kind: String(builder?.task_kind || ""),
      repo_root: String(builder?.repo_root || ""),
      write_scope: String(builder?.write_scope || ""),
      acceptance_criteria: String(builder?.acceptance_criteria || ""),
      task_dependencies: String(builder?.task_dependencies || ""),
      verification_state: String(builder?.verification_state || ""),
      task_owner: String(builder?.task_owner || ""),
      verification_command: String(builder?.verification_command || ""),
      output_path: String(builder?.output_path || ""),
    };
  });
  return {
    currentTaskId: workflowCurrentTaskId(tasks, activeTaskId),
    taskSource: "checkpoint" as const,
    tasks,
  };
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
