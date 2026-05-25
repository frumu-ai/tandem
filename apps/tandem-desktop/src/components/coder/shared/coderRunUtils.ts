import type {
  AutomationV2RunRecord,
  AutomationV2Spec,
  CoderAutomationMetadata,
  UserProject,
} from "@/lib/tauri";

export type DerivedCoderRun = {
  automation: AutomationV2Spec;
  run: AutomationV2RunRecord;
  coderMetadata: CoderAutomationMetadata;
};

export type SessionPreview = {
  sessionId: string;
  messageCount: number;
  latestText: string;
};

export type RunBlocker = {
  key: string;
  title: string;
  reason: string;
};

export type RunLifecycleEntry = {
  event: string;
  recorded_at_ms: number;
  reason: string | null;
  stop_kind: string | null;
  metadata: Record<string, unknown> | null;
};

export type WorkflowEventSummary = {
  event: string;
  recordedAtMs: number;
  phase: string;
  failureKind: string;
  workflowClass: string;
  reason: string;
  status: string;
};

type LifecycleEntryLike = {
  event: string;
  recorded_at_ms: number;
  reason?: string | null;
  stop_kind?: string | null;
  metadata?: Record<string, unknown> | null;
};

export function coderMetadataFromAutomation(
  automation: AutomationV2Spec | null | undefined
): CoderAutomationMetadata | null {
  const metadata = (automation?.metadata as Record<string, unknown> | undefined) || {};
  const coder = metadata.coder;
  if (!coder || typeof coder !== "object") return null;
  const surface = String((coder as Record<string, unknown>).surface || "").trim();
  if (surface !== "coder") return null;
  return coder as CoderAutomationMetadata;
}

export function runStatusLabel(run: AutomationV2RunRecord | null) {
  const status = String(run?.status || "")
    .trim()
    .toLowerCase();
  const stopKind = String((run as Record<string, unknown> | null)?.stop_kind || "")
    .trim()
    .toLowerCase();
  if (status === "cancelled" && stopKind === "operator_stopped") return "operator stopped";
  if (status === "cancelled" && stopKind === "guardrail_stopped") return "guardrail stopped";
  return status || "unknown";
}

export function shortText(raw: unknown, max = 160) {
  const text = String(raw || "")
    .replace(/\s+/g, " ")
    .trim();
  if (!text) return "";
  return text.length > max ? `${text.slice(0, max - 1).trimEnd()}...` : text;
}

export function runSummary(run: AutomationV2RunRecord | null) {
  return String(
    run?.stop_reason ||
      run?.checkpoint?.summary ||
      run?.checkpoint?.error ||
      run?.checkpoint?.status_detail ||
      run?.checkpoint?.statusDetail ||
      ""
  ).trim();
}

export function runStopReason(run: AutomationV2RunRecord | null) {
  return String((run as Record<string, unknown> | null)?.stop_reason || "").trim();
}

export function runDisplayTitle(run: AutomationV2RunRecord | null) {
  const explicitName = String((run as Record<string, unknown> | null)?.name || "").trim();
  if (explicitName) return explicitName;
  const checkpoint = runCheckpoint(run);
  const objective = String(
    checkpoint.objective ||
      checkpoint.title ||
      checkpoint.summary ||
      checkpoint.status_detail ||
      checkpoint.statusDetail ||
      ""
  ).trim();
  if (objective) return shortText(objective, 96);
  const automationId = String(run?.automation_id || "").trim();
  return automationId || "Run";
}

function finiteNumber(raw: unknown) {
  return typeof raw === "number" && Number.isFinite(raw) ? raw : null;
}

export function runCheckpoint(run: AutomationV2RunRecord | null) {
  return ((run?.checkpoint as Record<string, unknown> | undefined) || {}) as Record<
    string,
    unknown
  >;
}

function checkpointStringArray(
  checkpoint: Record<string, unknown>,
  snakeKey: string,
  camelKey: string
) {
  const raw = Array.isArray(checkpoint[snakeKey])
    ? checkpoint[snakeKey]
    : Array.isArray(checkpoint[camelKey])
      ? checkpoint[camelKey]
      : [];
  return raw.map((value) => String(value || "").trim()).filter(Boolean);
}

export function extractSessionIdsFromRun(run: AutomationV2RunRecord | null) {
  const direct = Array.isArray(run?.active_session_ids) ? run.active_session_ids : [];
  const checkpoint = runCheckpoint(run);
  const latest = [
    String((run as Record<string, unknown> | null)?.latest_session_id || "").trim(),
    String((run as Record<string, unknown> | null)?.latestSessionId || "").trim(),
    String(checkpoint.latest_session_id || checkpoint.latestSessionId || "").trim(),
  ].filter(Boolean);
  const nodeOutputs =
    (checkpoint.node_outputs as Record<string, Record<string, unknown>>) ||
    (checkpoint.nodeOutputs as Record<string, Record<string, unknown>>) ||
    {};
  const nodeSessionIds = Object.values(nodeOutputs)
    .map((entry) => {
      const content = (entry?.content as Record<string, unknown> | undefined) || {};
      return String(content.session_id || content.sessionId || "").trim();
    })
    .filter(Boolean);
  return Array.from(
    new Set([...latest, ...direct.map((row) => String(row || "").trim()), ...nodeSessionIds])
  );
}

export function extractRunNodeOutputs(run: AutomationV2RunRecord | null) {
  const checkpoint = runCheckpoint(run);
  const outputs =
    (checkpoint.node_outputs as Record<string, Record<string, unknown>>) ||
    (checkpoint.nodeOutputs as Record<string, Record<string, unknown>>) ||
    {};
  return Object.entries(outputs).map(([nodeId, value]) => ({
    nodeId,
    value,
  }));
}

export function nodeOutputText(value: Record<string, unknown>) {
  const summary = String(value?.summary || "").trim();
  const status = String(value?.status || "").trim();
  const blockedReason = String(value?.blocked_reason || value?.blockedReason || "").trim();
  const content = (value?.content as Record<string, unknown> | undefined) || {};
  const text = String(content.text || content.raw_text || "").trim();
  return [summary, status ? `status: ${status}` : "", blockedReason, text]
    .filter(Boolean)
    .join("\n")
    .trim();
}

export function nodeOutputSummary(value: Record<string, unknown>) {
  return String(value?.summary || "").trim();
}

export function nodeOutputSessionId(value: Record<string, unknown>) {
  const content = (value?.content as Record<string, unknown> | undefined) || {};
  return String(content.session_id || content.sessionId || "").trim();
}

export function runNodeOutputMap(run: AutomationV2RunRecord | null) {
  const checkpoint = runCheckpoint(run);
  return (
    (checkpoint.node_outputs as Record<string, Record<string, unknown>>) ||
    (checkpoint.nodeOutputs as Record<string, Record<string, unknown>>) ||
    {}
  );
}

export function runNodeAttempts(run: AutomationV2RunRecord | null) {
  const checkpoint = runCheckpoint(run);
  return (
    (checkpoint.node_attempts as Record<string, number>) ||
    (checkpoint.nodeAttempts as Record<string, number>) ||
    {}
  );
}

export function extractRunLifecycleHistory(run: AutomationV2RunRecord | null) {
  const checkpoint = runCheckpoint(run);
  const history = Array.isArray(checkpoint.lifecycle_history)
    ? checkpoint.lifecycle_history
    : Array.isArray(checkpoint.lifecycleHistory)
      ? checkpoint.lifecycleHistory
      : [];
  return history.map(
    (entry) => ((entry as Record<string, unknown>) || {}) as Record<string, unknown>
  );
}

export function runLifecycleEntries(run: AutomationV2RunRecord | null): RunLifecycleEntry[] {
  return extractRunLifecycleHistory(run)
    .map((row) => ({
      event: String(row.event || "").trim(),
      recorded_at_ms: Number(row.recorded_at_ms || 0),
      reason: row.reason ? String(row.reason) : null,
      stop_kind: row.stop_kind ? String(row.stop_kind) : null,
      metadata: ((row.metadata as Record<string, unknown> | undefined) || null) as Record<
        string,
        unknown
      > | null,
    }))
    .sort((a, b) => Number(b.recorded_at_ms || 0) - Number(a.recorded_at_ms || 0));
}

function normalizedLifecycleEvent(entry: Pick<LifecycleEntryLike, "event">) {
  return String(entry.event || "")
    .trim()
    .toLowerCase();
}

export function recoveryLifecycleEntries(
  entries: LifecycleEntryLike[] | AutomationV2RunRecord | null
): RunLifecycleEntry[] {
  const history = Array.isArray(entries) ? entries : runLifecycleEntries(entries);
  return history
    .filter((entry) => normalizedLifecycleEvent(entry).includes("recover"))
    .map((entry) => ({
      event: String(entry.event || "").trim(),
      recorded_at_ms: Number(entry.recorded_at_ms || 0),
      reason: entry.reason ? String(entry.reason) : null,
      stop_kind: entry.stop_kind ? String(entry.stop_kind) : null,
      metadata: (entry.metadata || null) as Record<string, unknown> | null,
    }));
}

export function promotionLifecycleEntries(
  entries: LifecycleEntryLike[] | AutomationV2RunRecord | null
): RunLifecycleEntry[] {
  const history = Array.isArray(entries) ? entries : runLifecycleEntries(entries);
  return history
    .filter((entry) => normalizedLifecycleEvent(entry) === "milestone_promoted")
    .map((entry) => ({
      event: String(entry.event || "").trim(),
      recorded_at_ms: Number(entry.recorded_at_ms || 0),
      reason: entry.reason ? String(entry.reason) : null,
      stop_kind: entry.stop_kind ? String(entry.stop_kind) : null,
      metadata: (entry.metadata || null) as Record<string, unknown> | null,
    }));
}

export function repairLifecycleEntries(
  entries: LifecycleEntryLike[] | AutomationV2RunRecord | null
): RunLifecycleEntry[] {
  const history = Array.isArray(entries) ? entries : runLifecycleEntries(entries);
  return history
    .filter((entry) => normalizedLifecycleEvent(entry) === "run_step_repaired")
    .map((entry) => ({
      event: String(entry.event || "").trim(),
      recorded_at_ms: Number(entry.recorded_at_ms || 0),
      reason: entry.reason ? String(entry.reason) : null,
      stop_kind: entry.stop_kind ? String(entry.stop_kind) : null,
      metadata: (entry.metadata || null) as Record<string, unknown> | null,
    }));
}

export function failureChainLifecycleEntries(
  entries: LifecycleEntryLike[] | AutomationV2RunRecord | null,
  lastFailure?: Record<string, unknown> | null,
  limit = 8
): RunLifecycleEntry[] {
  const history = Array.isArray(entries) ? entries : runLifecycleEntries(entries);
  const rows = history.filter((entry) => {
    const event = normalizedLifecycleEvent(entry);
    return (
      event.includes("fail") ||
      event.includes("recover") ||
      event.includes("stop") ||
      event.includes("cancel")
    );
  });
  if (lastFailure) {
    rows.unshift({
      event: "last_failure",
      recorded_at_ms: Number(lastFailure.recorded_at_ms || 0),
      reason: lastFailure.reason ? String(lastFailure.reason) : null,
      stop_kind: null,
      metadata: null,
    });
  }
  return rows.slice(0, limit).map((entry) => ({
    event: String(entry.event || "").trim(),
    recorded_at_ms: Number(entry.recorded_at_ms || 0),
    reason: entry.reason ? String(entry.reason) : null,
    stop_kind: entry.stop_kind ? String(entry.stop_kind) : null,
    metadata: (entry.metadata || null) as Record<string, unknown> | null,
  }));
}

export function summarizeWorkflowEvent(
  event: Record<string, unknown> | null | undefined
): WorkflowEventSummary {
  return {
    event: String(event?.event || "event").trim() || "event",
    recordedAtMs: Number(event?.recorded_at_ms || event?.recordedAtMs || 0),
    phase: String(event?.phase || "").trim(),
    failureKind: String(event?.failure_kind || event?.failureKind || "").trim(),
    workflowClass: String(event?.workflow_class || event?.workflowClass || "").trim(),
    reason: String(event?.reason || "").trim() || "No reason recorded",
    status: String(event?.status || "").trim(),
  };
}

export function completedNodeIds(run: AutomationV2RunRecord | null) {
  return checkpointStringArray(runCheckpoint(run), "completed_nodes", "completedNodes");
}

export function pendingNodeIds(run: AutomationV2RunRecord | null) {
  return checkpointStringArray(runCheckpoint(run), "pending_nodes", "pendingNodes");
}

export function blockedNodeIds(run: AutomationV2RunRecord | null) {
  return checkpointStringArray(runCheckpoint(run), "blocked_nodes", "blockedNodes");
}

export function runAwaitingGate(run: AutomationV2RunRecord | null) {
  const checkpoint = runCheckpoint(run);
  return (checkpoint.awaiting_gate as Record<string, unknown> | undefined) || null;
}

export function runLastFailure(run: AutomationV2RunRecord | null) {
  const checkpoint = runCheckpoint(run);
  return (checkpoint.last_failure as Record<string, unknown> | undefined) || null;
}

export function runGateHistory(run: AutomationV2RunRecord | null) {
  const checkpoint = runCheckpoint(run);
  const history = Array.isArray(checkpoint.gate_history)
    ? checkpoint.gate_history
    : Array.isArray(checkpoint.gateHistory)
      ? checkpoint.gateHistory
      : [];
  return history.map(
    (entry) => ((entry as Record<string, unknown>) || {}) as Record<string, unknown>
  );
}

export function runUsageMetrics(run: AutomationV2RunRecord | null) {
  const checkpoint = runCheckpoint(run);
  const runRecord = (run as Record<string, unknown> | null) || null;
  return {
    totalTokens:
      finiteNumber(runRecord?.total_tokens) ??
      finiteNumber(checkpoint.total_tokens) ??
      finiteNumber(checkpoint.totalTokens),
    estimatedCostUsd:
      finiteNumber(runRecord?.estimated_cost_usd) ??
      finiteNumber(runRecord?.estimatedCostUsd) ??
      finiteNumber(checkpoint.estimated_cost_usd) ??
      finiteNumber(checkpoint.estimatedCostUsd),
    totalToolCalls:
      finiteNumber(runRecord?.total_tool_calls) ??
      finiteNumber(runRecord?.totalToolCalls) ??
      finiteNumber(checkpoint.total_tool_calls) ??
      finiteNumber(checkpoint.totalToolCalls),
  };
}

export function extractRunBlockers(run: AutomationV2RunRecord | null): RunBlocker[] {
  const blockers: RunBlocker[] = [];
  const push = (key: string, title: string, reason: string) => {
    const normalizedReason = String(reason || "").trim();
    if (!normalizedReason) return;
    if (blockers.some((item) => item.key === key)) return;
    blockers.push({ key, title, reason: normalizedReason });
  };
  const detail = runSummary(run);
  const status = String(run?.status || "").trim();
  if (status === "failed") {
    push("run-failed", "Run failed", detail || "Run finished with failed status.");
  }
  if (status === "paused") {
    push("run-paused", "Run paused", detail || "Run was paused before completion.");
  }
  if (!extractSessionIdsFromRun(run).length) {
    push(
      "missing-session",
      "No linked session transcript",
      "This run does not currently expose a linked session transcript."
    );
  }
  for (const output of extractRunNodeOutputs(run)) {
    const body = nodeOutputText((output.value || {}) as Record<string, unknown>);
    const lower = body.toLowerCase();
    if (
      lower.includes("failed") ||
      lower.includes("error") ||
      lower.includes("blocked") ||
      lower.includes("timed out")
    ) {
      push(`node-${output.nodeId}`, `Node issue: ${output.nodeId}`, shortText(body, 320));
    }
  }
  return blockers;
}

export function formatTimestamp(value: unknown) {
  const asNumber = Number(value || 0);
  if (!Number.isFinite(asNumber) || asNumber <= 0) return "Unknown";
  return new Date(asNumber).toLocaleString();
}

export function runSortTimestamp(run: AutomationV2RunRecord | null | undefined) {
  return Number((run as Record<string, unknown> | null)?.updated_at_ms || run?.created_at_ms || 0);
}

export function canPauseRun(run: AutomationV2RunRecord) {
  const status = String(run.status || "")
    .trim()
    .toLowerCase();
  return status === "queued" || status === "running" || status === "awaiting_approval";
}

export function canResumeRun(run: AutomationV2RunRecord) {
  return (
    String(run.status || "")
      .trim()
      .toLowerCase() === "paused"
  );
}

export function canCancelRun(run: AutomationV2RunRecord) {
  const status = String(run.status || "")
    .trim()
    .toLowerCase();
  return ["queued", "running", "pausing", "paused", "awaiting_approval"].includes(status);
}

export function canRecoverRun(run: AutomationV2RunRecord) {
  const status = String(run.status || "")
    .trim()
    .toLowerCase();
  if (status === "paused") return true;
  if (status === "failed") return Boolean(runLastFailure(run));
  return status === "cancelled";
}

export function matchesActiveProject(
  automation: AutomationV2Spec,
  activeProject: UserProject | null
) {
  if (!activeProject?.path) return true;
  const workspaceRoot = String(automation.workspace_root || "").trim();
  if (!workspaceRoot) return true;
  return workspaceRoot === activeProject.path || workspaceRoot.startsWith(`${activeProject.path}/`);
}

export type RunStatusTone =
  | "running"
  | "queued"
  | "paused"
  | "awaiting"
  | "failed"
  | "completed"
  | "cancelled"
  | "unknown";

export function runStatusTone(run: AutomationV2RunRecord | null): RunStatusTone {
  if (!run) return "unknown";
  if (runAwaitingGate(run)) return "awaiting";
  const status = String(run.status || "")
    .trim()
    .toLowerCase();
  switch (status) {
    case "running":
    case "pausing":
      return "running";
    case "queued":
      return "queued";
    case "paused":
      return "paused";
    case "awaiting_approval":
      return "awaiting";
    case "failed":
      return "failed";
    case "completed":
    case "succeeded":
    case "success":
      return "completed";
    case "cancelled":
      return "cancelled";
    default:
      return "unknown";
  }
}

export function runIsActive(run: AutomationV2RunRecord | null) {
  const tone = runStatusTone(run);
  return tone === "running" || tone === "queued" || tone === "paused" || tone === "awaiting";
}

export type RunProgress = {
  completed: number;
  pending: number;
  blocked: number;
  total: number;
  percent: number;
};

export function runProgress(run: AutomationV2RunRecord | null): RunProgress {
  const completed = completedNodeIds(run).length;
  const pending = pendingNodeIds(run).length;
  const blocked = blockedNodeIds(run).length;
  const total = completed + pending + blocked;
  const percent = total > 0 ? Math.round((completed / total) * 100) : 0;
  return { completed, pending, blocked, total, percent };
}

export function relativeTimeFromMs(value: unknown): string {
  const asNumber = Number(value || 0);
  if (!Number.isFinite(asNumber) || asNumber <= 0) return "—";
  const deltaSec = Math.max(0, Math.floor((Date.now() - asNumber) / 1000));
  if (deltaSec < 5) return "just now";
  if (deltaSec < 60) return `${deltaSec}s ago`;
  const min = Math.floor(deltaSec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${day}d ago`;
  return new Date(asNumber).toLocaleDateString();
}
