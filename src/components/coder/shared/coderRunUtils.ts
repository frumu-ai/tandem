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
