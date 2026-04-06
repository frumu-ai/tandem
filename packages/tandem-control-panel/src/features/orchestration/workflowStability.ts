export function workflowCheckpoint(run: any) {
  return run?.checkpoint || {};
}

function workflowTimestamp(raw: any) {
  const value = Number(raw || 0);
  if (!Number.isFinite(value) || value <= 0) return null;
  return value < 1_000_000_000_000 ? value * 1000 : value;
}

export function workflowEventRunId(event: any) {
  const props = event?.properties || {};
  return String(
    event?.run_id ||
      event?.runId ||
      event?.runID ||
      props?.run_id ||
      props?.runId ||
      props?.runID ||
      props?.run?.id ||
      ""
  ).trim();
}

export function workflowEventType(event: any) {
  return String(event?.type || event?.event_type || event?.event || "").trim();
}

export function workflowEventAt(event: any) {
  const props = event?.properties || {};
  const raw =
    event?.timestamp_ms ||
    event?.timestampMs ||
    props?.timestamp_ms ||
    props?.timestampMs ||
    props?.firedAtMs ||
    Date.now();
  const value = Number(raw);
  return Number.isFinite(value) ? value : Date.now();
}

export function workflowEventReason(event: any) {
  const props = event?.properties || event || {};
  return String(
    props?.reason ||
      props?.detail ||
      props?.error?.message ||
      props?.error ||
      props?.message ||
      props?.status ||
      ""
  ).trim();
}

export function workflowEventSessionId(event: any, fallbackSessionId = "") {
  return String(event?.properties?.sessionID || event?.sessionID || fallbackSessionId || "").trim();
}

function checkpointStringArray(checkpoint: any, snakeKey: string, camelKey: string) {
  const raw = Array.isArray(checkpoint?.[snakeKey])
    ? checkpoint[snakeKey]
    : Array.isArray(checkpoint?.[camelKey])
      ? checkpoint[camelKey]
      : [];
  return raw.map((value: any) => String(value || "").trim()).filter(Boolean);
}

function workflowStringArray(value: any) {
  return Array.isArray(value)
    ? value.map((entry: any) => String(entry || "").trim()).filter(Boolean)
    : [];
}

function workflowReceiptTimelineEntries(value: any) {
  const raw = Array.isArray(value?.records) ? value.records : Array.isArray(value) ? value : [];
  return raw
    .map((record: any, index: number) => {
      if (!record || typeof record !== "object") return null;
      const payload = record?.payload || {};
      const receiptKind = String(payload?.receipt_kind || payload?.receiptKind || "").trim();
      const status = String(payload?.status || "").trim();
      const blockedReason = String(payload?.blocked_reason || payload?.blockedReason || "").trim();
      const validatorReason = String(
        payload?.validator_summary?.reason || payload?.validatorSummary?.reason || ""
      ).trim();
      const detail =
        status ||
        blockedReason ||
        validatorReason ||
        String(payload?.blocker_category || payload?.blockerCategory || "").trim() ||
        String(payload?.failure_kind || payload?.failureKind || "").trim() ||
        "receipt record";
      return {
        seq: Number(record?.seq || index + 1),
        at: Number(record?.ts_ms || record?.tsMs || 0),
        attempt: Number(record?.attempt || 0),
        eventType: String(record?.event_type || record?.eventType || "").trim(),
        receiptKind,
        sessionId: String(record?.session_id || record?.sessionId || "").trim(),
        nodeId: String(record?.node_id || record?.nodeId || "").trim(),
        detail,
        raw: record,
      };
    })
    .filter(Boolean)
    .sort((a: any, b: any) => Number(a.seq || 0) - Number(b.seq || 0));
}

export function workflowCompletedNodeIds(run: any) {
  return checkpointStringArray(workflowCheckpoint(run), "completed_nodes", "completedNodes");
}

function workflowNodeRuntimeStatus(output: any) {
  return String(output?.status || output?.content?.status || "")
    .trim()
    .toLowerCase();
}

export function workflowBlockedNodeIds(run: any) {
  const ids = new Set(
    checkpointStringArray(workflowCheckpoint(run), "blocked_nodes", "blockedNodes")
  );
  for (const { nodeId, value } of workflowNodeOutputEntries(run)) {
    if (workflowNodeRuntimeStatus(value) === "blocked") ids.add(nodeId);
  }
  return Array.from(ids);
}

export function workflowFailedNodeIds(run: any) {
  const ids = new Set<string>();
  for (const { nodeId, value } of workflowNodeOutputEntries(run)) {
    const status = workflowNodeRuntimeStatus(value);
    if (status === "verify_failed" || status === "failed") ids.add(nodeId);
  }
  return Array.from(ids);
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

export function workflowBlockedNodeCount(run: any) {
  return workflowBlockedNodeIds(run).length;
}

export function workflowDerivedRunStatus(run: any) {
  const raw = String(run?.status || "")
    .trim()
    .toLowerCase();
  if ((raw === "completed" || raw === "done") && workflowBlockedNodeCount(run) > 0) {
    return "blocked";
  }
  if ((raw === "completed" || raw === "done") && workflowFailedNodeIds(run).length > 0) {
    return "failed";
  }
  return raw || "unknown";
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
  const output = workflowNodeOutput(run, taskId);
  const outputStatus = String(output?.status || output?.content?.status || "")
    .trim()
    .toLowerCase();
  if (blocked.has(taskId) || outputStatus === "blocked") return "blocked";
  if (outputStatus === "verify_failed" || outputStatus === "failed") return "failed";
  if (completed.has(taskId) || outputStatus === "done") return "done";
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

export function workflowFirstPendingTaskId(run: any) {
  const pending = workflowPendingNodeIds(run);
  const nodeId = String(pending[0] || "").trim();
  if (!nodeId) return "";
  return nodeId.startsWith("node-") ? nodeId : `node-${nodeId}`;
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

export function workflowEventNodeId(event: any) {
  return String(event?.metadata?.node_id || event?.metadata?.nodeId || "").trim();
}

export function workflowEventTaskId(event: any) {
  const nodeId = workflowEventNodeId(event);
  if (!nodeId) return "";
  return nodeId.startsWith("node-") ? nodeId : `node-${nodeId}`;
}

export function workflowRecentNodeEvents(run: any, nodeId: string, limit = 8) {
  const normalized = String(nodeId || "").trim();
  if (!normalized) return [];
  return workflowLifecycleHistory(run)
    .filter((event: any) => {
      const metadataNodeId = workflowEventNodeId(event);
      return metadataNodeId === normalized;
    })
    .slice(-limit)
    .reverse();
}

export function workflowRecentNodeEventSummaries(run: any, nodeId: string, limit = 8) {
  return workflowRecentNodeEvents(run, nodeId, limit).map((event: any) => ({
    ...workflowEventSummary(event),
    raw: event,
  }));
}

export function workflowLatestLifecycleTaskId(run: any) {
  const latestEvent = workflowLatestLifecycleEvent(run);
  return workflowEventTaskId(latestEvent);
}

export function workflowActiveLifecycleTaskIds(run: any) {
  const active = new Set<string>();
  const terminalEvents = new Set([
    "node_completed",
    "node_completed_with_warnings",
    "node_failed",
    "node_blocked",
    "node_verify_failed",
    "node_repair_requested",
    "node_skipped_no_work",
    "node_approval_rollback",
  ]);
  const history = [...workflowLifecycleHistory(run)].sort(
    (a: any, b: any) =>
      Number(a?.recorded_at_ms || a?.recordedAtMs || 0) -
      Number(b?.recorded_at_ms || b?.recordedAtMs || 0)
  );
  for (const event of history) {
    const taskId = workflowEventTaskId(event);
    if (!taskId) continue;
    const eventType = String(event?.event || "").trim();
    if (eventType === "node_started") {
      active.add(taskId);
      continue;
    }
    if (terminalEvents.has(eventType)) {
      active.delete(taskId);
    }
  }
  return Array.from(active);
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
    nodeId: workflowEventNodeId(event),
    taskId: workflowEventTaskId(event),
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

export function workflowContextHistoryEntries(events: any[], patches: any[]) {
  const eventRows = (Array.isArray(events) ? events : []).map((event: any) => ({
    id: `event:${String(event?.seq || "")}:${String(event?.event_type || event?.eventType || "")}`,
    family: "event",
    type: String(event?.event_type || event?.eventType || "context_event"),
    detail: String(
      event?.payload?.reason ||
        event?.payload?.detail ||
        event?.payload?.error ||
        event?.payload?.status ||
        event?.status ||
        ""
    ).trim(),
    at:
      workflowTimestamp(event?.created_at_ms || event?.timestamp_ms || event?.timestampMs) ||
      Number(event?.seq || 0),
    raw: event,
  }));
  const patchRows = (Array.isArray(patches) ? patches : []).map((patch: any) => ({
    id: `patch:${String(patch?.seq || "")}:${String(patch?.op || "")}`,
    family: "patch",
    type: String(patch?.op || "blackboard_patch"),
    detail: String(
      patch?.payload?.status ||
        patch?.payload?.task_id ||
        patch?.payload?.artifact_id ||
        patch?.payload?.title ||
        ""
    ).trim(),
    at: workflowTimestamp(patch?.created_at_ms || patch?.timestamp_ms) || Number(patch?.seq || 0),
    raw: patch,
  }));
  return [...eventRows, ...patchRows].sort((a, b) => Number(b.at || 0) - Number(a.at || 0));
}

export function workflowPersistedHistoryEntries(events: any[], runId = "") {
  return (Array.isArray(events) ? events : [])
    .map((event: any, index: number) => ({
      id: `persisted:${String(runId || "")}:${index}`,
      family: "run_event",
      type: String(workflowEventType(event) || "run.event"),
      detail: String(workflowEventReason(event) || "").trim(),
      at: workflowEventAt(event),
      raw: event,
    }))
    .sort((a, b) => Number(b.at || 0) - Number(a.at || 0));
}

export function workflowTelemetrySeedEvents(
  persistedEvents: any[],
  contextEvents: any[],
  isWorkflowRun: boolean,
  runId = ""
) {
  const persisted = (Array.isArray(persistedEvents) ? persistedEvents : []).map(
    (event: any, index: number) => ({
      id: `persisted:${String(runId || "")}:${String(event?.seq || index)}:${String(workflowEventType(event) || "")}`,
      source: "automations" as const,
      at: workflowEventAt(event),
      event,
    })
  );
  if (!isWorkflowRun) return persisted;
  return [
    ...persisted,
    ...(Array.isArray(contextEvents) ? contextEvents : []).map((event: any) => ({
      id: `context:${String(event?.seq || "")}:${String(event?.event_type || "")}`,
      source: "context" as const,
      at:
        workflowTimestamp(event?.created_at_ms || event?.timestamp_ms || event?.timestampMs) ||
        Date.now(),
      event,
    })),
  ];
}

export function workflowSessionLogEventEntries(
  sessionEvents: Array<{ id: string; at: number; event: any }>,
  fallbackSessionId = ""
) {
  return (Array.isArray(sessionEvents) ? sessionEvents : []).map((item) => {
    const sessionId = workflowEventSessionId(item?.event, fallbackSessionId);
    const type = workflowEventType(item?.event);
    return {
      id: `event:${item?.id || ""}`,
      kind: "event" as const,
      sessionId,
      at: Number(item?.at || 0),
      variant: type === "session.error" ? "error" : type.startsWith("session.") ? "system" : "tool",
      label: type || "event",
      body: workflowEventReason(item?.event),
      raw: item?.event,
      parts: [] as any[],
    };
  });
}

export function workflowEventBlockers(
  rows: Array<{ at?: number; event?: any } | null | undefined>
): Array<{
  key: string;
  title: string;
  reason: string;
  source: string;
  at?: number;
}> {
  const blockers: Array<{
    key: string;
    title: string;
    reason: string;
    source: string;
    at?: number;
  }> = [];
  const push = (key: string, title: string, reason: string, source: string, at?: number) => {
    if (!String(reason || "").trim()) return;
    if (blockers.some((row) => row.key === key)) return;
    blockers.push({ key, title, reason: String(reason).trim(), source, at });
  };

  for (const row of Array.isArray(rows) ? rows : []) {
    const payload = row?.event || row || {};
    const type = String(workflowEventType(payload) || "").trim();
    const reason = workflowEventReason(payload);
    const at = Number(row?.at || workflowEventAt(payload) || 0);
    if (
      type === "permission.asked" ||
      type === "approval.required" ||
      type === "routine.approval_required"
    ) {
      push(`event-${type}`, "Permission or approval required", reason || type, "permission", at);
    }
    if (type === "mcp.auth.required") {
      push(
        `event-${type}`,
        "MCP auth required",
        reason || "An MCP connector requires authorization.",
        "mcp",
        at
      );
    }
    if (type === "session.error" || type === "run.failed" || type === "routine.run.failed") {
      push(`event-${type}`, "Execution failure", reason || type, "session", at);
    }
    if (reason.toLowerCase().includes("no further tool calls")) {
      push("tool-mode", "Tool policy blocked progress", reason, "policy", at);
    }
    if (reason.toLowerCase().includes("timed out")) {
      push(`timeout-${type || at}`, "Timeout", reason, "session", at);
    }
  }

  return blockers.sort((a, b) => (b.at || 0) - (a.at || 0));
}

export function workflowTelemetryDisplayEntries(
  rows: Array<{ id: string; source: string; at: number; event: any } | null | undefined>
) {
  return (Array.isArray(rows) ? rows : []).map((row) => ({
    id: String(row?.id || "").trim(),
    source: String(row?.source || "").trim() || "event",
    at: Number(row?.at || 0),
    label: workflowEventType(row?.event) || "event",
    detail: workflowEventReason(row?.event) || "No summary available.",
    raw: row?.event?.properties || row?.event || null,
    event: row?.event || null,
  }));
}

export function workflowTaskInspectionDetails(task: any, output: any) {
  const telemetry = workflowNodeToolTelemetry(output);
  const artifactValidation = workflowArtifactValidation(output);
  const stability = workflowNodeStability(output);
  const validatorSummary = output?.validator_summary || output?.validatorSummary || {};
  const attemptEvidence = output?.attempt_evidence || output?.attemptEvidence || {};
  const receiptTimeline = workflowReceiptTimelineEntries(
    attemptEvidence?.receipt_timeline || attemptEvidence?.receiptTimeline
  );
  const verificationOutcome = (() => {
    const approved = output?.approved;
    if (typeof approved === "boolean") return approved ? "approved" : "not approved";
    const telemetryOutcome = String(telemetry?.verification_outcome || "")
      .trim()
      .toLowerCase();
    if (telemetryOutcome) return telemetryOutcome;
    const status = String(output?.status || "")
      .trim()
      .toLowerCase();
    if (status) return status;
    const state = String(task?.state || "")
      .trim()
      .toLowerCase();
    if (state === "done") return "completed";
    if (state === "blocked") return "blocked";
    if (state === "failed") return "failed";
    if (state) return state;
    return "unknown";
  })();

  return {
    telemetry,
    artifactValidation,
    validatorSummary,
    touchedFiles: workflowStringArray(artifactValidation?.touched_files),
    undeclaredFiles: workflowStringArray(artifactValidation?.undeclared_files_created),
    researchReadPaths: workflowStringArray(artifactValidation?.read_paths),
    discoveredRelevantPaths: workflowStringArray(artifactValidation?.discovered_relevant_paths),
    reviewedPathsBackedByRead: workflowStringArray(
      artifactValidation?.reviewed_paths_backed_by_read
    ),
    unreviewedRelevantPaths: workflowStringArray(artifactValidation?.unreviewed_relevant_paths),
    unmetResearchRequirements: workflowStringArray(artifactValidation?.unmet_requirements),
    warningRequirements: workflowStringArray(
      validatorSummary?.warning_requirements ||
        validatorSummary?.warningRequirements ||
        artifactValidation?.warning_requirements ||
        artifactValidation?.warningRequirements
    ),
    warningCount:
      Number(
        validatorSummary?.warning_count ||
          validatorSummary?.warningCount ||
          artifactValidation?.warning_count ||
          artifactValidation?.warningCount ||
          0
      ) || 0,
    validationOutcome: String(
      validatorSummary?.outcome || artifactValidation?.validation_outcome || ""
    )
      .trim()
      .toLowerCase(),
    verificationOutcome,
    verificationPassed:
      typeof output?.approved === "boolean"
        ? output.approved
        : ["approved", "completed", "done"].includes(verificationOutcome)
          ? true
          : ["blocked", "failed", "not approved"].includes(verificationOutcome)
            ? false
            : null,
    verificationResults: Array.isArray(telemetry?.verification_results)
      ? telemetry.verification_results
      : [],
    failureDetail: String(
      output?.blocked_reason ||
        output?.blockedReason ||
        artifactValidation?.semantic_block_reason ||
        artifactValidation?.rejected_artifact_reason ||
        task?.error_message ||
        ""
    ).trim(),
    validationBasis:
      artifactValidation?.validation_basis ||
      artifactValidation?.validationBasis ||
      output?.validation_basis ||
      output?.validationBasis ||
      null,
    qualityMode: String(output?.quality_mode || output?.qualityMode || "").trim(),
    requestedQualityMode: String(
      output?.requested_quality_mode || output?.requestedQualityMode || ""
    ).trim(),
    emergencyRollbackEnabled:
      typeof output?.emergency_rollback_enabled === "boolean"
        ? output.emergency_rollback_enabled
        : typeof output?.emergencyRollbackEnabled === "boolean"
          ? output.emergencyRollbackEnabled
          : null,
    blockerCategory: String(
      output?.blocker_category ||
        output?.blockerCategory ||
        artifactValidation?.blocking_classification ||
        artifactValidation?.blockingClassification ||
        ""
    ).trim(),
    receiptLedger: attemptEvidence?.receipt_ledger || attemptEvidence?.receiptLedger || null,
    receiptTimeline,
    workflowClass: stability.workflowClass,
    phase: stability.phase,
    failureKind: stability.failureKind,
    artifactCandidates: workflowArtifactCandidates(output),
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
