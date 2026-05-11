import { formatJson } from "../../pages/ui";
import {
  workflowActiveLifecycleTaskIds,
  workflowActiveSessionCount,
  workflowArtifactValidation,
  workflowDerivedRunStatus,
  workflowEventBlockers,
  workflowFirstPendingTaskId,
  workflowLatestLifecycleTaskId,
  workflowNodeOutputEntries,
  workflowNodeOutputText,
  workflowNodeToolTelemetry,
  workflowRunIsPossiblyStale,
  workflowRunLastActivityAt,
  workflowRunWasStalePaused,
  workflowSessionIds,
} from "../orchestration/workflowStability";

export type ExecutionProfile = "strict" | "guided" | "yolo";

export function executionProfileLabel(value?: string | null): string {
  if (value === "yolo") return "Lenient";
  if (value === "guided") return "Guided";
  return "Strict";
}

export function executionProfileDescription(value?: string | null): string {
  if (value === "yolo") {
    return "Lenient experimental run. Runtime logging, receipts, spend caps, and approvals remain active.";
  }
  if (value === "guided") {
    return "Assisted iteration. Non-critical validation failures become warnings; critical failures still block.";
  }
  return "Production discipline. All validators enforced.";
}

export function workflowEffectiveExecutionProfile(run: any): ExecutionProfile {
  const raw = String(
    run?.effective_execution_profile ||
      run?.effectiveExecutionProfile ||
      run?.requested_execution_profile ||
      run?.requestedExecutionProfile ||
      ""
  )
    .trim()
    .toLowerCase();
  if (raw === "guided" || raw === "yolo") return raw;
  return "strict";
}

export function workflowRequestedExecutionProfile(run: any): ExecutionProfile | null {
  const raw = String(run?.requested_execution_profile || run?.requestedExecutionProfile || "")
    .trim()
    .toLowerCase();
  if (raw === "strict" || raw === "guided" || raw === "yolo") return raw;
  return null;
}

export function artifactValidationIsExperimental(validation: any): boolean {
  if (!validation || typeof validation !== "object") return false;
  return Boolean(
    validation.experimental === true ||
    String(validation.effective_outcome || validation.effectiveOutcome || "")
      .trim()
      .toLowerCase() === "experimental"
  );
}

export function artifactValidationRelaxedClasses(validation: any): Array<{
  class: string;
  detail?: string;
  original_outcome?: string;
  effective_outcome?: string;
}> {
  if (!validation || typeof validation !== "object") return [];
  const raw =
    (validation.relaxed_validator_classes as unknown[]) ||
    (validation.relaxedValidatorClasses as unknown[]) ||
    [];
  if (!Array.isArray(raw)) return [];
  return raw
    .filter((row): row is Record<string, unknown> => Boolean(row && typeof row === "object"))
    .map((row) => ({
      class: String(row["class"] ?? ""),
      detail: typeof row["detail"] === "string" ? (row["detail"] as string) : undefined,
      original_outcome:
        typeof row["original_outcome"] === "string"
          ? (row["original_outcome"] as string)
          : undefined,
      effective_outcome:
        typeof row["effective_outcome"] === "string"
          ? (row["effective_outcome"] as string)
          : undefined,
    }))
    .filter((row) => row.class.length > 0);
}

export function isActiveRunStatus(status: string) {
  const normalized = String(status || "")
    .trim()
    .toLowerCase();
  return [
    "queued",
    "running",
    "in_progress",
    "executing",
    "pending_approval",
    "awaiting_approval",
  ].includes(normalized);
}

export function workflowQueueReason(run: any) {
  return String(
    run?.scheduler?.queue_reason || run?.scheduler?.queueReason || run?.scheduler?.reason || ""
  )
    .trim()
    .toLowerCase();
}

function workflowQueueResourceKey(run: any) {
  return String(run?.scheduler?.resource_key || run?.scheduler?.resourceKey || "").trim();
}

function relativeTimeFromNow(timestampMs: number | null) {
  if (!timestampMs) return "time unavailable";
  const deltaMs = Math.max(0, Date.now() - timestampMs);
  const seconds = Math.floor(deltaMs / 1000);
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  return `${Math.floor(seconds / 3600)}h ago`;
}

export function workflowStatusDisplay(run: any) {
  const status = workflowDerivedRunStatus(run);
  if (status !== "queued") return status || "unknown";
  const reason = workflowQueueReason(run);
  if (reason === "workspace_lock") return "queued (workspace lock)";
  if (reason === "capacity") return "queued (capacity)";
  if (reason === "rate_limit") {
    const provider = String(
      run?.scheduler?.rate_limited_provider || run?.scheduler?.rateLimitedProvider || ""
    ).trim();
    return provider ? `queued (rate limit: ${provider})` : "queued (rate limit)";
  }
  return "queued";
}

export function workflowStatusSubtleDetail(run: any) {
  if (workflowRunWasStalePaused(run)) {
    return "Run was paused automatically after provider activity stopped";
  }
  if (workflowRunIsPossiblyStale(run)) {
    const activeSessions = workflowActiveSessionCount(run);
    if (activeSessions > 0) {
      return `Waiting on ${activeSessions} active session${
        activeSessions === 1 ? "" : "s"
      }; last provider activity ${relativeTimeFromNow(workflowRunLastActivityAt(run))}`;
    }
    return `Run is possibly stale: last provider activity ${relativeTimeFromNow(workflowRunLastActivityAt(run))}`;
  }
  if (workflowDerivedRunStatus(run) === "stalled") {
    return `Run appears stalled: last provider activity ${relativeTimeFromNow(
      workflowRunLastActivityAt(run)
    )}`;
  }
  const reason = workflowQueueReason(run);
  if (!reason) return "";
  if (reason === "workspace_lock") {
    const resourceKey = workflowQueueResourceKey(run);
    return resourceKey
      ? `Waiting for workspace lock: ${resourceKey}`
      : "Waiting for workspace lock";
  }
  if (reason === "capacity") return "Waiting for scheduler capacity";
  if (reason === "rate_limit") {
    const provider = String(
      run?.scheduler?.rate_limited_provider || run?.scheduler?.rateLimitedProvider || ""
    ).trim();
    return provider
      ? `Waiting for provider rate limit: ${provider}`
      : "Waiting for provider rate limit";
  }
  return "";
}

export function runTimeLabel(run: any) {
  const started = Number(run?.started_at_ms || run?.fired_at_ms || run?.created_at_ms || 0);
  if (!Number.isFinite(started) || started <= 0) return "time unavailable";
  const deltaMs = Date.now() - started;
  const seconds = Math.max(0, Math.floor(deltaMs / 1000));
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  return `${Math.floor(seconds / 3600)}h`;
}

export function deriveRunDebugHints(run: any, artifacts: any[]) {
  const hints: string[] = [];
  const status = workflowDerivedRunStatus(run);
  if (workflowRunWasStalePaused(run)) {
    hints.push("Run was paused automatically after provider activity stopped.");
  } else if (workflowRunIsPossiblyStale(run)) {
    hints.push("Run may be stale. Check the latest lifecycle event and provider activity.");
  }
  if (status === "pending_approval" || status === "awaiting_approval") {
    hints.push("Run is waiting for approval before external actions.");
  }
  if (status === "blocked_policy") {
    hints.push("Run was blocked by policy. Check tool allowlist and integration permissions.");
  }
  if (status === "failed" || status === "error") {
    hints.push("Run failed. Inspect detail/error fields for root cause.");
  }
  if ((status === "completed" || status === "done") && !artifacts.length) {
    hints.push("Run completed but produced no artifacts. Verify output target and tool actions.");
  }
  if (run?.requires_approval === true) {
    hints.push("Automation policy requires human approval. Disable it for fully automated runs.");
  }
  return hints;
}

export function normalizeTimestamp(raw: any) {
  const value = Number(raw || 0);
  if (!Number.isFinite(value) || value <= 0) return Date.now();
  return value < 1_000_000_000_000 ? value * 1000 : value;
}

export function timestampOrNull(raw: any) {
  const value = Number(raw || 0);
  if (!Number.isFinite(value) || value <= 0) return null;
  return value < 1_000_000_000_000 ? value * 1000 : value;
}

export function formatTimestampLabel(raw: any) {
  const value = timestampOrNull(raw);
  return value ? new Date(value).toLocaleTimeString() : "time unavailable";
}

export function compactIdentifier(raw: any, max = 28) {
  const value = String(raw || "").trim();
  if (!value) return "";
  if (value.length <= max) return value;
  const head = Math.max(8, Math.floor((max - 1) / 2));
  const tail = Math.max(6, max - head - 1);
  return `${value.slice(0, head)}…${value.slice(-tail)}`;
}

export function shortText(raw: any, max = 88) {
  const text = String(raw || "")
    .replace(/\s+/g, " ")
    .trim();
  if (!text) return "";
  return text.length > max ? `${text.slice(0, max - 1).trimEnd()}…` : text;
}

export function runObjectiveText(run: any) {
  return String(
    run?.mission_snapshot?.objective || run?.mission?.objective || run?.objective || run?.name || ""
  )
    .replace(/\s+/g, " ")
    .trim();
}

function looksLikeOpaqueRunLabel(value: string) {
  const text = String(value || "").trim();
  if (!text) return false;
  if (
    /^automation-v2(?:-run)?-[0-9a-f]{8,}(?:-[0-9a-f]{4,}){1,}$/i.test(text) ||
    /^run-[0-9a-f]{8,}(?:-[0-9a-f]{4,}){1,}$/i.test(text)
  ) {
    return true;
  }
  if (/^[0-9a-f]{8,}(?:-[0-9a-f]{4,}){2,}$/i.test(text)) return true;
  return false;
}

export function runDisplayTitle(run: any) {
  const labelCandidates = [
    run?.display_title,
    run?.displayTitle,
    run?.automation_name,
    run?.automationName,
    run?.automation_title,
    run?.automationTitle,
    run?.mission_snapshot?.title,
    run?.mission?.title,
    run?.title,
    run?.name,
  ]
    .map((value) => String(value || "").trim())
    .filter(Boolean);
  const readableLabel = labelCandidates.find((value) => !looksLikeOpaqueRunLabel(value));
  if (readableLabel) return readableLabel;
  const objective = runObjectiveText(run);
  if (objective) return shortText(objective, 96);
  const opaqueLabel = labelCandidates[0];
  if (opaqueLabel) return opaqueLabel;
  const automationId = String(run?.automation_id || run?.routine_id || "").trim();
  if (automationId) return automationId;
  return "Run";
}

export function formatRunDateTime(raw: any) {
  const value = Number(raw || 0);
  if (!Number.isFinite(value) || value <= 0) return "";
  return new Date(normalizeTimestamp(value)).toLocaleString();
}

export function uniqueStrings(values: Array<any>) {
  const seen = new Set<string>();
  const rows: string[] = [];
  for (const value of values) {
    const text = String(value || "").trim();
    if (!text || seen.has(text)) continue;
    seen.add(text);
    rows.push(text);
  }
  return rows;
}

function looksLikePath(text: string) {
  const value = String(text || "").trim();
  if (!value) return false;
  if (value.includes("/") || value.includes("\\")) return true;
  return /\.[a-z0-9]{1,8}$/i.test(value);
}

export function collectPathStrings(value: any, keyHint = "", depth = 0): string[] {
  if (depth > 4 || value == null) return [];
  if (typeof value === "string") {
    const text = value.trim();
    if (!text) return [];
    if (/(path|file|artifact)/i.test(keyHint) || looksLikePath(text)) return [text];
    return [];
  }
  if (Array.isArray(value)) {
    return value.flatMap((item) => collectPathStrings(item, keyHint, depth + 1));
  }
  if (typeof value === "object") {
    return Object.entries(value).flatMap(([key, entry]) =>
      collectPathStrings(entry, key, depth + 1)
    );
  }
  return [];
}

export function sessionMessageText(message: any) {
  const parts = Array.isArray(message?.parts) ? message.parts : [];
  const rows = parts
    .map((part: any) => {
      const type = String(part?.type || "").trim();
      if (type === "text" || type === "reasoning") return String(part?.text || "").trim();
      if (type === "tool") {
        const tool = String(part?.tool || "tool").trim();
        const error = String(part?.error || "").trim();
        const result = part?.result ? formatJson(part.result) : "";
        return [`tool: ${tool}`, error ? `error: ${error}` : "", result].filter(Boolean).join("\n");
      }
      return String(part?.text || "").trim();
    })
    .filter(Boolean);
  return rows.join("\n\n").trim();
}

export function sessionMessageVariant(message: any) {
  const role = String(message?.info?.role || "")
    .trim()
    .toLowerCase();
  if (role === "user") return "user";
  if (role === "assistant") return "assistant";
  const body = sessionMessageText(message).toLowerCase();
  if (body.includes("engine_error") || body.includes("error")) return "error";
  return "system";
}

export function sessionMessageParts(message: any) {
  return Array.isArray(message?.parts) ? message.parts : [];
}

export function sessionMessageCreatedAt(message: any) {
  return normalizeTimestamp(
    message?.info?.time?.created || message?.info?.created_at_ms || message?.created_at_ms || 0
  );
}

export function sessionMessageId(message: any, index: number) {
  return (
    String(message?.info?.id || message?.id || `message-${index}`).trim() || `message-${index}`
  );
}

export function sessionLabel(sessionId: string) {
  const value = String(sessionId || "").trim();
  return value ? `session ${compactIdentifier(value, 18)}` : "session";
}

function normalizeWorkflowTaskId(raw: string) {
  const value = String(raw || "").trim();
  if (!value) return "";
  return value.startsWith("node-") ? value : `node-${value}`;
}

function workflowNodeIdFromText(raw: string) {
  const text = String(raw || "").trim();
  if (!text) return "";
  for (const pattern of [
    /node id:\s*([a-z0-9._-]+)/i,
    /step[_\s]id:\s*([a-z0-9._-]+)/i,
    /task[_\s]id:\s*(?:node-)?([a-z0-9._-]+)/i,
  ]) {
    const match = text.match(pattern);
    if (match?.[1]) return normalizeWorkflowTaskId(match[1]);
  }
  return "";
}

export function workflowDescendantTaskIds(tasks: any[], rootTaskId: string) {
  const root = String(rootTaskId || "").trim();
  if (!root) return [];
  const descendants = new Set<string>([root]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const task of Array.isArray(tasks) ? tasks : []) {
      const taskId = String(task?.id || "").trim();
      if (!taskId || descendants.has(taskId)) continue;
      const deps = Array.isArray(task?.dependencies)
        ? task.dependencies.map((dep: any) => String(dep || "").trim()).filter(Boolean)
        : [];
      if (deps.some((dep) => descendants.has(dep))) {
        descendants.add(taskId);
        changed = true;
      }
    }
  }
  return Array.from(descendants);
}

export function detectWorkflowActiveTaskId(
  run: any,
  sessionMessages: Array<{ sessionId: string; message: any }>,
  sessionEvents: Array<{ id: string; at: number; event: any }>
) {
  const activeTaskIds = detectWorkflowActiveTaskIds(run, sessionMessages, sessionEvents);
  if (activeTaskIds.length) return activeTaskIds[0];
  return "";
}

export function detectWorkflowActiveTaskIds(
  run: any,
  sessionMessages: Array<{ sessionId: string; message: any }>,
  sessionEvents: Array<{ id: string; at: number; event: any }>
) {
  const status = String(run?.status || "")
    .trim()
    .toLowerCase();
  if (!["running", "pausing", "paused"].includes(status)) return [];
  const lifecycleTaskIds = workflowActiveLifecycleTaskIds(run);
  if (lifecycleTaskIds.length) return lifecycleTaskIds;
  const lifecycleTaskId = workflowLatestLifecycleTaskId(run);
  if (lifecycleTaskId) return [lifecycleTaskId];
  for (let i = sessionEvents.length - 1; i >= 0; i -= 1) {
    const payload = sessionEvents[i]?.event?.properties || sessionEvents[i]?.event || {};
    const explicit = normalizeWorkflowTaskId(
      String(payload?.task_id || payload?.step_id || payload?.node_id || "").trim()
    );
    if (explicit) return [explicit];
    const fromText = workflowNodeIdFromText(
      String(payload?.message || payload?.detail || payload?.reason || "")
    );
    if (fromText) return [fromText];
  }
  for (let i = sessionMessages.length - 1; i >= 0; i -= 1) {
    const fromText = workflowNodeIdFromText(sessionMessageText(sessionMessages[i]?.message));
    if (fromText) return [fromText];
  }
  const firstPending = workflowFirstPendingTaskId(run);
  return firstPending ? [firstPending] : [];
}

export function explainRunFailure(run: any) {
  const detail = String(run?.detail || "").trim();
  if (!detail) return "";
  if (detail.includes("BASH_COMMAND_MISSING")) {
    return "This workflow failed because the agent called the `bash` tool without providing a shell command. The tool was available, but the request payload was missing its required `command` field.";
  }
  if (detail.includes("WEBFETCH_URL_MISSING")) {
    return "This workflow failed because a web fetch tool call was made without a URL.";
  }
  if (detail.includes("No such file or directory")) {
    return "This workflow failed because the agent tried to read a path that does not exist from the configured workspace root.";
  }
  return detail;
}

export function buildRunBlockers(run: any, sessionEvents: any[], runEvents: any[]) {
  const blockers: Array<{
    key: string;
    title: string;
    reason: string;
    source: string;
    at?: number;
  }> = [];
  const push = (key: string, title: string, reason: string, source: string, at?: number) => {
    if (!reason.trim()) return;
    if (blockers.some((row) => row.key === key)) return;
    blockers.push({ key, title, reason, source, at });
  };

  if (run?.requires_approval === true || String(run?.status || "").trim() === "pending_approval") {
    push(
      "approval-required",
      "Approval required",
      String(
        run?.approval_reason || "Manual approval required before external side effects."
      ).trim(),
      "policy"
    );
  }
  if (String(run?.denial_reason || "").trim()) {
    push("denied", "Run denied", String(run.denial_reason).trim(), "run");
  }
  if (String(run?.paused_reason || "").trim()) {
    push("paused", "Run paused", String(run.paused_reason).trim(), "run");
  }
  if (String(run?.detail || "").trim()) {
    const detail = String(run.detail).trim();
    if (
      detail.toLowerCase().includes("tool") ||
      detail.toLowerCase().includes("bash_command_missing") ||
      detail.toLowerCase().includes("command_missing") ||
      detail.toLowerCase().includes("permission") ||
      detail.toLowerCase().includes("approval") ||
      detail.toLowerCase().includes("mcp") ||
      detail.toLowerCase().includes("auth") ||
      detail.toLowerCase().includes("failed after")
    ) {
      push("detail", "Failure reason", explainRunFailure(run), "run");
    }
  }
  if (!workflowSessionIds(run).length) {
    push(
      "missing-session",
      "No linked session transcript",
      "This run does not expose a linked session transcript, so only telemetry/history are available.",
      "run"
    );
  }
  for (const output of workflowNodeOutputEntries(run)) {
    const body = workflowNodeOutputText(output.value);
    const telemetry = workflowNodeToolTelemetry(output.value);
    const artifactValidation = workflowArtifactValidation(output.value);
    if (
      String(output?.value?.status || "")
        .trim()
        .toLowerCase() === "blocked"
    ) {
      const executed = Array.isArray(telemetry?.executed_tools)
        ? telemetry.executed_tools.join(", ")
        : "";
      const requested = Array.isArray(telemetry?.requested_tools)
        ? telemetry.requested_tools.join(", ")
        : "";
      push(
        `node-status-${output.nodeId}`,
        `Node blocked: ${output.nodeId}`,
        [
          String(output?.value?.blocked_reason || output?.value?.blockedReason || "").trim(),
          String(output?.value?.blocker_category || output?.value?.blockerCategory || "").trim()
            ? `blocker category: ${String(
                output?.value?.blocker_category || output?.value?.blockerCategory || ""
              ).trim()}`
            : "",
          requested ? `offered tools: ${requested}` : "",
          executed ? `executed tools: ${executed}` : "",
          String(
            output?.value?.preflight?.budget_status || output?.value?.preflight?.budgetStatus || ""
          )
            ? `preflight budget: ${String(
                output?.value?.preflight?.budget_status ||
                  output?.value?.preflight?.budgetStatus ||
                  ""
              ).trim()}`
            : "",
          Array.isArray(output?.value?.capability_resolution?.missing_capabilities) &&
          output.value.capability_resolution.missing_capabilities.length
            ? `missing capabilities: ${output.value.capability_resolution.missing_capabilities.join(", ")}`
            : "",
          output?.value?.capability_resolution?.email_tool_diagnostics
            ? `selected mcp servers: ${
                Array.isArray(
                  output.value.capability_resolution.email_tool_diagnostics.selected_servers
                ) &&
                output.value.capability_resolution.email_tool_diagnostics.selected_servers.length
                  ? output.value.capability_resolution.email_tool_diagnostics.selected_servers.join(
                      ", "
                    )
                  : "none"
              }`
            : "",
          output?.value?.capability_resolution?.email_tool_diagnostics
            ? `email-like tools discovered: ${
                Array.isArray(
                  output.value.capability_resolution.email_tool_diagnostics.available_tools
                ) &&
                output.value.capability_resolution.email_tool_diagnostics.available_tools.length
                  ? output.value.capability_resolution.email_tool_diagnostics.available_tools.join(
                      ", "
                    )
                  : "none"
              }`
            : "",
          output?.value?.capability_resolution?.email_tool_diagnostics
            ? `email-like tools offered: ${
                Array.isArray(
                  output.value.capability_resolution.email_tool_diagnostics.offered_tools
                ) && output.value.capability_resolution.email_tool_diagnostics.offered_tools.length
                  ? output.value.capability_resolution.email_tool_diagnostics.offered_tools.join(
                      ", "
                    )
                  : "none"
              }`
            : "",
          output?.value?.capability_resolution?.mcp_tool_diagnostics
            ? `mcp remote tools: ${
                Array.isArray(
                  output.value.capability_resolution.mcp_tool_diagnostics.remote_tools
                ) && output.value.capability_resolution.mcp_tool_diagnostics.remote_tools.length
                  ? output.value.capability_resolution.mcp_tool_diagnostics.remote_tools.join(", ")
                  : "none"
              }`
            : "",
          output?.value?.capability_resolution?.mcp_tool_diagnostics
            ? `registered mcp tools: ${
                Array.isArray(
                  output.value.capability_resolution.mcp_tool_diagnostics.registered_tools
                ) && output.value.capability_resolution.mcp_tool_diagnostics.registered_tools.length
                  ? output.value.capability_resolution.mcp_tool_diagnostics.registered_tools.join(
                      ", "
                    )
                  : "none"
              }`
            : "",
          String(artifactValidation?.semantic_block_reason || "").trim()
            ? `research validation: ${String(artifactValidation?.semantic_block_reason || "").trim()}`
            : "",
          String(artifactValidation?.rejected_artifact_reason || "").trim()
            ? `artifact validation: ${String(artifactValidation?.rejected_artifact_reason || "").trim()}`
            : "",
          Array.isArray(artifactValidation?.unmet_requirements) &&
          artifactValidation.unmet_requirements.length
            ? `unmet requirements: ${artifactValidation.unmet_requirements.join(", ")}`
            : "",
          Array.isArray(artifactValidation?.undeclared_files_created) &&
          artifactValidation.undeclared_files_created.length
            ? `undeclared files created: ${artifactValidation.undeclared_files_created.join(", ")}`
            : "",
          artifactValidation?.auto_cleaned ? "artifact cleanup was applied" : "",
          telemetry && !telemetry?.web_research_used ? "web research was not used" : "",
          telemetry && !telemetry?.workspace_inspection_used
            ? "workspace inspection was not used"
            : "",
        ]
          .filter(Boolean)
          .join("\n"),
        output.nodeId
      );
    }
    if (!body) continue;
    const lower = body.toLowerCase();
    if (
      lower.includes("could not complete") ||
      lower.includes("invalid attachment") ||
      lower.includes("timed out") ||
      lower.includes("blocked") ||
      lower.includes("no email delivery tool") ||
      lower.includes("auth was not approved")
    ) {
      push(
        `node-output-${output.nodeId}`,
        `Node issue: ${output.nodeId}`,
        shortText(body, 360),
        output.nodeId,
        Number(output.value?.created_at_ms || output.value?.createdAtMs || 0)
      );
    }
  }

  workflowEventBlockers([...sessionEvents, ...runEvents]).forEach((blocker) => {
    push(blocker.key, blocker.title, blocker.reason, blocker.source, blocker.at);
  });

  return blockers.sort((a, b) => (b.at || 0) - (a.at || 0));
}
