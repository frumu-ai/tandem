import { normalizeStatefulRunFilters } from "./stateful-runs.js";

function toArray(input, key) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function compact(items) {
  return items.map((item) => stringValue(item)).filter(Boolean);
}

function stringValue(value, fallback = "") {
  if (value === null || value === undefined) return fallback;
  const text = String(value).trim();
  return text || fallback;
}

function numberValue(value, fallback = 0) {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function boolValue(value, fallback = false) {
  if (typeof value === "boolean") return value;
  if (value === "true") return true;
  if (value === "false") return false;
  return fallback;
}

function read(row, keys, fallback = "") {
  for (const key of keys) {
    const value = row?.[key];
    if (value !== undefined && value !== null && value !== "") return value;
  }
  return fallback;
}

function normalizeKey(value) {
  return stringValue(value)
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/[\s-]+/g, "_")
    .toLowerCase();
}

function titleCase(value, fallback = "Unknown") {
  const key = normalizeKey(value);
  if (!key) return fallback;
  return key
    .split("_")
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function statusTone(status) {
  const key = normalizeKey(status);
  if (["accepted", "succeeded", "sent", "completed", "resolved", "approved", "woken"].includes(key)) {
    return "ok";
  }
  if (
    [
      "failed",
      "rejected",
      "dead_letter",
      "dead_lettered",
      "open",
      "unknown",
      "cancelled",
      "expired",
    ].includes(key)
  ) {
    return "err";
  }
  if (
    [
      "duplicate",
      "suppressed",
      "disabled",
      "retry_requested",
      "awaiting_approval",
      "escalated",
      "pending",
      "claimed",
      "proposed",
      "running",
    ].includes(key)
  ) {
    return "warn";
  }
  return key ? "info" : "ghost";
}

function formatBytes(bytes) {
  const value = numberValue(bytes, 0);
  if (!value) return "0 B";
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${Math.round(value / 102.4) / 10} KB`;
  return `${Math.round(value / 1024 / 102.4) / 10} MB`;
}

function hasObjectValue(value) {
  return value && typeof value === "object" && Object.keys(value).length > 0;
}

function runRoute(runId) {
  return runId ? `runs?run=${encodeURIComponent(runId)}` : "";
}

function safeJson(value) {
  try {
    return JSON.stringify(value || {});
  } catch {
    return "";
  }
}

function queueStatusGroup(row) {
  const status = normalizeKey(row?.status);
  const statusGroup = normalizeKey(row?.statusGroup);
  const category = normalizeKey(row?.category);
  if (["failed", "rejected", "expired", "cancelled", "canceled", "dead_lettered"].includes(status)) {
    return "failed";
  }
  if (["failed", "dead_lettered", "retryable"].includes(category)) return "failed";
  if (["pending", "escalated", "proposed", "awaiting_approval", "waiting_backoff", "manually_blocked"].includes(status)) {
    return "waiting";
  }
  if (["waiting_backoff", "manually_blocked"].includes(category)) return "waiting";
  if (["accepted", "approved", "completed", "resolved", "sent", "succeeded"].includes(status)) {
    return "completed";
  }
  if (["accepted", "duplicate", "suppressed"].includes(statusGroup)) return "completed";
  if (["received", "queued", "claimed", "running"].includes(status)) return "active";
  return statusGroup || category || "unknown";
}

function queueSourceGroup(row) {
  const text = compact([row?.source, row?.kind, row?.provider, row?.operation, row?.sourceLabel, row?.raw?.source])
    .join(" ")
    .toLowerCase();
  if (text.includes("context")) return "context";
  if (text.includes("workflow")) return "workflow";
  if (text.includes("automation") || text.includes("webhook") || text.includes("approval")) return "automation";
  return "unknown";
}

function nestedScope(row) {
  const raw = row?.raw || {};
  const scope = raw.scope || raw.resource_scope || raw.resourceScope || {};
  const tenant = raw.tenant_context || raw.tenantContext || scope.tenant_context || scope.tenantContext || {};
  const enterprise = raw.enterprise_scope || raw.enterpriseScope || {};
  return { raw, scope, tenant, enterprise };
}

function queueFilterText(row, key) {
  const { raw, scope, tenant, enterprise } = nestedScope(row);
  const haystacks = {
    query: [
      row?.id,
      row?.runId,
      row?.title,
      row?.provider,
      row?.operation,
      row?.tool,
      row?.target,
      row?.reason,
      row?.scopeLabel,
      row?.correlationLabel,
      row?.verificationLabel,
      row?.dedupeLabel,
      row?.payloadLabel,
      row?.sourceLabel,
      safeJson(raw),
    ],
    tenant: [
      tenant.org_id,
      tenant.orgId,
      tenant.workspace_id,
      tenant.workspaceId,
      raw.org_id,
      raw.orgId,
      raw.workspace_id,
      raw.workspaceId,
      row?.scopeLabel,
    ],
    workspace: [
      raw.workspace,
      raw.workspace_root,
      raw.workspaceRoot,
      raw.workspace_path,
      raw.workspacePath,
      tenant.workspace_id,
      tenant.workspaceId,
      row?.scopeLabel,
    ],
    orgUnit: [
      scope.owning_org_unit_id,
      scope.owningOrgUnitId,
      enterprise.owning_org_unit_id,
      enterprise.owningOrgUnitId,
      enterprise.owning_org_unit?.name,
      enterprise.owningOrgUnit?.name,
      row?.scopeLabel,
    ],
    resource: [
      scope.resource_kind,
      scope.resourceKind,
      scope.resource_id,
      scope.resourceId,
      enterprise.resource_kind,
      enterprise.resourceKind,
      enterprise.resource_id,
      enterprise.resourceId,
      row?.target,
      row?.sourceLabel,
      row?.scopeLabel,
    ],
    policy: [
      scope.policy_version_id,
      scope.policyVersionId,
      enterprise.policy_version_id,
      enterprise.policyVersionId,
      raw.policy_decision_id,
      raw.policyDecisionId,
      row?.scopeLabel,
    ],
    dataClass: [scope.data_class, scope.dataClass, enterprise.data_class, enterprise.dataClass, safeJson(enterprise.data_classes)],
    knowledge: [
      safeJson(enterprise.visible_knowledge_sources),
      safeJson(enterprise.visibleKnowledgeSources),
      safeJson(raw.knowledge_sources),
      safeJson(raw.knowledgeSources),
    ],
    wait: [
      row?.phaseId,
      row?.nodeId,
      row?.transitionId,
      row?.status,
      row?.statusLabel,
      row?.categoryLabel,
      row?.timeoutLabel,
      row?.escalationLabel,
      row?.reason,
      row?.operation,
    ],
  };
  return normalizeKey(compact(haystacks[key] || []).join(" "));
}

function filterStatefulQueueRows(rows = [], filters = {}) {
  const normalized = normalizeStatefulRunFilters(filters);
  const query = normalizeKey(normalized.query);
  const status = normalizeKey(normalized.status);
  const source = normalizeKey(normalized.source);
  const tenant = normalizeKey(normalized.tenant);
  const workspace = normalizeKey(normalized.workspace);
  const orgUnit = normalizeKey(normalized.orgUnit);
  const resource = normalizeKey(normalized.resource);
  const policy = normalizeKey(normalized.policy);
  const dataClass = normalizeKey(normalized.dataClass);
  const knowledge = normalizeKey(normalized.knowledge);
  const wait = normalizeKey(normalized.wait);

  return (Array.isArray(rows) ? rows : []).filter((row) => {
    if (query && !queueFilterText(row, "query").includes(query)) return false;
    if (status && status !== "all" && queueStatusGroup(row) !== status) return false;
    if (source && source !== "all" && queueSourceGroup(row) !== source) return false;
    if (tenant && !queueFilterText(row, "tenant").includes(tenant)) return false;
    if (workspace && !queueFilterText(row, "workspace").includes(workspace)) return false;
    if (orgUnit && !queueFilterText(row, "orgUnit").includes(orgUnit)) return false;
    if (resource && !queueFilterText(row, "resource").includes(resource)) return false;
    if (policy && !queueFilterText(row, "policy").includes(policy)) return false;
    if (dataClass && !queueFilterText(row, "dataClass").includes(dataClass)) return false;
    if (knowledge && !queueFilterText(row, "knowledge").includes(knowledge)) return false;
    if (wait && !queueFilterText(row, "wait").includes(wait)) return false;
    return true;
  });
}

function webhookStatusGroup(status) {
  const key = normalizeKey(status);
  if (key === "accepted") return "accepted";
  if (key === "duplicate") return "duplicate";
  if (["rejected", "failed", "dead_letter", "dead_lettered"].includes(key)) return "failed";
  if (["suppressed", "disabled"].includes(key)) return "suppressed";
  return "received";
}

function webhookVerificationLabel(row, status, reason) {
  const key = normalizeKey(status);
  if (key === "rejected") return reason ? `Rejected: ${titleCase(reason)}` : "Rejected";
  if (key === "failed") return reason ? `Failed: ${titleCase(reason)}` : "Failed";
  if (key === "disabled") return "Trigger disabled";
  if (key === "suppressed") return "Suppressed by feedback guard";
  if (key === "duplicate") return "Accepted as duplicate";
  if (key === "accepted") return "Verified and accepted";
  if (hasObjectValue(read(row, ["headers_redacted", "headersRedacted"], null))) return "Headers verified";
  return "Received";
}

function webhookDedupeLabel(row) {
  const result = read(row, ["dedupe_result", "dedupeResult"]);
  const reason = read(row, ["dedupe_reason_code", "dedupeReasonCode"]);
  const idempotencyKey = read(row, ["idempotency_key", "idempotencyKey"]);
  if (result) {
    return compact([titleCase(result), reason ? titleCase(reason) : "", idempotencyKey ? `key ${idempotencyKey}` : ""]).join(
      " - "
    );
  }
  return idempotencyKey ? `Tracked key ${idempotencyKey}` : "No idempotency key";
}

function webhookCorrelationLabel(row, correlation, runId) {
  const outcome = read(correlation, ["outcome"]);
  if (outcome) return compact([titleCase(outcome), runId]).join(" - ");
  if (read(row, ["woken_run_id", "wokenRunID", "wokenRunId"])) return compact(["Wake Run", runId]).join(" - ");
  if (read(row, ["queued_run_id", "queuedRunID", "queuedRunId"])) return compact(["New Run", runId]).join(" - ");
  if (read(row, ["duplicate_of_run_id", "duplicateOfRunID", "duplicateOfRunId"])) {
    return compact(["Duplicate Of", runId]).join(" - ");
  }
  return "No run correlation";
}

function webhookPayloadLabel(row) {
  const payloadAvailable = boolValue(read(row, ["payload_available", "payloadAvailable"], true), true);
  const payload = read(row, ["payload"], null);
  const payloadRef = read(row, ["payload_ref", "payloadRef"]);
  const payloadBytes = numberValue(read(row, ["payload_bytes", "payloadBytes"], 0), 0);
  const headersRedacted = hasObjectValue(read(row, ["headers_redacted", "headersRedacted"], null));
  const storage = payloadAvailable
    ? payload !== null
      ? "Payload visible"
      : payloadRef
        ? "Payload retained"
        : "No payload"
    : "Payload expired";
  return compact([storage, payloadBytes ? formatBytes(payloadBytes) : "", headersRedacted ? "headers redacted" : ""]).join(
    " - "
  );
}

function buildWebhookInboxRows(payload = {}) {
  return toArray(payload?.events || payload?.webhook_events || payload?.webhookEvents, "events").map((row, index) => {
    const correlation = read(row, ["correlation"], {}) || {};
    const status = normalizeKey(read(row, ["status"], "received"));
    const reason = read(row, ["rejection_reason_code", "rejectionReasonCode"]);
    const runId = stringValue(
      read(row, [
        "woken_run_id",
        "wokenRunID",
        "wokenRunId",
        "queued_run_id",
        "queuedRunID",
        "queuedRunId",
        "duplicate_of_run_id",
        "duplicateOfRunID",
        "duplicateOfRunId",
      ]) ||
        read(correlation, ["woken_run_id", "wokenRunId", "queued_run_id", "queuedRunId", "run_id", "runId"])
    );
    const eventId = stringValue(read(row, ["event_id", "eventID", "eventId"]), `event-${index + 1}`);
    const deadLettered =
      normalizeKey(read(correlation, ["outcome"])) === "dead_letter" ||
      ["failed", "dead_letter", "dead_lettered"].includes(status);

    return {
      id: eventId,
      status,
      statusLabel: titleCase(status),
      statusTone: statusTone(status),
      statusGroup: webhookStatusGroup(status),
      provider: stringValue(read(row, ["provider"]), "unknown"),
      providerEventKind: stringValue(read(row, ["provider_event_kind", "providerEventKind"]), "event"),
      providerEventId: stringValue(read(row, ["provider_event_id", "providerEventID", "providerEventId"])),
      triggerId: stringValue(read(row, ["trigger_id", "triggerID", "triggerId"])),
      automationId: stringValue(read(row, ["automation_id", "automationID", "automationId"])),
      deliveryId: stringValue(read(row, ["delivery_id", "deliveryID", "deliveryId"])),
      receivedAtMs: numberValue(read(row, ["received_at_ms", "receivedAtMs"], 0), 0),
      updatedAtMs: numberValue(read(row, ["updated_at_ms", "updatedAtMs"], 0), 0),
      verificationLabel: webhookVerificationLabel(row, status, reason),
      dedupeLabel: webhookDedupeLabel(row),
      correlationLabel: webhookCorrelationLabel(row, correlation, runId),
      payloadLabel: webhookPayloadLabel(row),
      rejectionReason: stringValue(reason),
      idempotencyKey: stringValue(read(row, ["idempotency_key", "idempotencyKey"])),
      runId,
      runRoute: runRoute(runId),
      deadLettered,
      raw: row,
    };
  });
}

function summarizeWebhookInboxRows(rows = []) {
  const summary = { total: 0, accepted: 0, duplicate: 0, rejected: 0, failed: 0, redacted: 0, deadLetters: 0 };
  for (const row of Array.isArray(rows) ? rows : []) {
    summary.total += 1;
    if (row.statusGroup === "accepted") summary.accepted += 1;
    if (row.statusGroup === "duplicate") summary.duplicate += 1;
    if (row.status === "rejected") summary.rejected += 1;
    if (["failed", "dead_letter", "dead_lettered"].includes(row.status)) summary.failed += 1;
    if (String(row.payloadLabel || "").includes("redacted")) summary.redacted += 1;
    if (row.deadLettered) summary.deadLetters += 1;
  }
  return summary;
}

function deadlineLabel(expiresAtMs, now = Date.now()) {
  const expires = numberValue(expiresAtMs, 0);
  if (!expires) return "";
  const seconds = Math.round((expires - now) / 1000);
  if (seconds <= 0) return "expired";
  if (seconds < 60) return `expires in ${seconds}s`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `expires in ${minutes}m`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `expires in ${hours}h`;
  return `expires in ${Math.round(hours / 24)}d`;
}

function approvalStatus(row, wait, now = Date.now()) {
  const explicit = read(row, ["status"]) || read(wait, ["status"]);
  if (explicit) return normalizeKey(explicit);
  const deadline = deadlineLabel(read(row, ["expires_at_ms", "expiresAtMs"]) || read(wait, ["expires_at_ms", "expiresAtMs"]), now);
  if (deadline === "expired") return "expired";
  const escalation = read(row, ["escalation_state", "escalationState"]) || read(wait, ["escalation_state", "escalationState"]);
  if (normalizeKey(escalation) === "escalated") return "escalated";
  return "pending";
}

function decisionRows(row, wait) {
  const history = toArray(
    row?.decision_history || row?.decisionHistory || wait?.decision_history || wait?.decisionHistory,
    "decision_history"
  );
  if (history.length) {
    return history.map((decision, index) => ({
      id: stringValue(read(decision, ["decision_id", "decisionId", "id"]), `decision-${index + 1}`),
      actor: stringValue(read(decision, ["actor_id", "actorId", "actor", "principal_id", "principalId"]), "unknown"),
      decision: titleCase(read(decision, ["decision", "kind", "status"]), "Decision"),
      atMs: numberValue(read(decision, ["decided_at_ms", "decidedAtMs", "created_at_ms", "createdAtMs"], 0), 0),
      transition: stringValue(read(decision, ["transition_id", "transitionId", "target_transition_id", "targetTransitionId"])),
    }));
  }
  return toArray(row?.decisions || wait?.decisions, "decisions").map((decision, index) => ({
    id: `available-${index + 1}`,
    actor: "",
    decision: titleCase(decision),
    atMs: 0,
    transition: "",
    available: true,
  }));
}

function buildApprovalWaitRows(payload = {}, options = {}) {
  const now = numberValue(options.now, Date.now());
  return toArray(payload?.approvals || payload?.approval_requests || payload?.approvalRequests, "approvals").map(
    (row, index) => {
      const wait = read(row, ["approval_wait", "approvalWait"], {}) || {};
      const surfacePayload = read(row, ["surface_payload", "surfacePayload"], {}) || {};
      const runId = stringValue(read(row, ["run_id", "runId"]) || read(wait, ["run_id", "runId"]));
      const status = approvalStatus(row, wait, now);
      const expiresAtMs = read(row, ["expires_at_ms", "expiresAtMs"]) || read(wait, ["expires_at_ms", "expiresAtMs"]);
      const escalation =
        read(row, ["escalation_state", "escalationState"]) ||
        read(wait, ["escalation_state", "escalationState"]) ||
        read(surfacePayload, ["escalation_state", "escalationState"]);

      return {
        id: stringValue(
          read(row, ["request_id", "requestId", "approval_request_id", "approvalRequestId"]) ||
            read(wait, ["approval_request_id", "approvalRequestId"]),
          `approval-${index + 1}`
        ),
        status,
        statusLabel: titleCase(status),
        statusTone: statusTone(status),
        source: stringValue(read(row, ["source"]), "stateful"),
        runId,
        runRoute: runRoute(runId),
        nodeId: stringValue(read(row, ["node_id", "nodeId"]) || read(wait, ["node_id", "nodeId"])),
        phaseId: stringValue(read(row, ["phase_id", "phaseId"]) || read(wait, ["phase_id", "phaseId"])),
        transitionId: stringValue(
          read(wait, ["transition_id", "transitionId"]) ||
            read(surfacePayload, ["transition_id", "transitionId", "target_transition_id", "targetTransitionId"])
        ),
        title: stringValue(read(row, ["workflow_name", "workflowName", "title"]), "Approval wait"),
        actionKind: stringValue(read(row, ["action_kind", "actionKind"]), "approval"),
        requestedAtMs: numberValue(read(row, ["requested_at_ms", "requestedAtMs"], 0), 0),
        expiresAtMs: numberValue(expiresAtMs, 0),
        timeoutLabel: deadlineLabel(expiresAtMs, now),
        escalationLabel: escalation ? titleCase(escalation) : "",
        instructions: stringValue(read(row, ["instructions"])),
        decisionHistory: decisionRows(row, wait),
        raw: row,
      };
    }
  );
}

function summarizeApprovalWaitRows(rows = []) {
  const summary = { total: 0, pending: 0, expired: 0, escalated: 0, decided: 0 };
  for (const row of Array.isArray(rows) ? rows : []) {
    summary.total += 1;
    if (row.status === "pending") summary.pending += 1;
    if (row.status === "expired") summary.expired += 1;
    if (row.status === "escalated") summary.escalated += 1;
    if (["approved", "rejected", "cancelled", "canceled"].includes(row.status)) summary.decided += 1;
  }
  return summary;
}

function scopeLabel(row) {
  const scope = read(row, ["scope"], {}) || {};
  const tenant = read(scope, ["tenant_context", "tenantContext"], {}) || {};
  return compact([
    read(tenant, ["org_id", "orgId"]),
    read(tenant, ["workspace_id", "workspaceId"]),
    read(scope, ["owning_org_unit_id", "owningOrgUnitId"]),
  ]).join(" / ");
}

function recoveryCategory(kind, status, row) {
  const key = normalizeKey(status);
  if (kind === "dead_letter") {
    if (key === "retry_requested") return "retryable";
    if (key === "open") return "dead_lettered";
    if (["ignored", "resolved", "linked_to_compensation"].includes(key)) return "manually_blocked";
  }
  if (kind === "outbox") {
    if (["pending", "claimed"].includes(key)) return "waiting_backoff";
    if (key === "failed") return "retryable";
    if (key === "dead_lettered") return "dead_lettered";
  }
  if (kind === "tool_effect") {
    if (key === "pending") return "waiting_backoff";
    if (["failed", "unknown"].includes(key)) return "manually_blocked";
  }
  if (kind === "compensation") {
    if (["failed", "cancelled"].includes(key)) return "retryable";
    if (["proposed", "awaiting_approval"].includes(key)) return "manually_blocked";
    if (key === "running") return "waiting_backoff";
  }
  if (read(row, ["claim_expires_at_ms", "claimExpiresAtMs"])) return "waiting_backoff";
  return "other";
}

function reliabilityRow(kind, row, index) {
  const idKeys = {
    outbox: ["outbox_id", "outboxId"],
    tool_effect: ["effect_id", "effectId"],
    dead_letter: ["dead_letter_id", "deadLetterId"],
    compensation: ["compensation_id", "compensationId"],
  }[kind];
  const status = normalizeKey(read(row, ["status"], "unknown"));
  const runId = stringValue(read(row, ["run_id", "runId"]));
  const recoveryOptions = toArray(read(row, ["recovery_options", "recoveryOptions"], []), "recovery_options").map((option) =>
    normalizeKey(option)
  );
  const category = recoveryCategory(kind, status, row);
  const sourceType = stringValue(read(row, ["source_type", "sourceType", "source_kind", "sourceKind"]));
  const sourceId = stringValue(read(row, ["source_id", "sourceId"]));
  const operation = stringValue(read(row, ["operation", "compensation_type", "compensationType"]), titleCase(kind));

  return {
    id: stringValue(read(row, idKeys), `${kind}-${index + 1}`),
    kind,
    kindLabel: titleCase(kind),
    status,
    statusLabel: titleCase(status),
    statusTone: statusTone(status),
    category,
    categoryLabel: titleCase(category),
    runId,
    runRoute: runRoute(runId),
    sourceLabel: compact([sourceType, sourceId]).join(" - "),
    operation,
    provider: stringValue(read(row, ["provider"])),
    tool: stringValue(read(row, ["tool"])),
    target: stringValue(read(row, ["target"])),
    nodeId: stringValue(read(row, ["node_id", "nodeId"])),
    reason: stringValue(read(row, ["reason", "error", "disposition_reason", "dispositionReason"])),
    attempts: numberValue(read(row, ["attempts"], 0), 0),
    scopeLabel: scopeLabel(row),
    updatedAtMs: numberValue(read(row, ["updated_at_ms", "updatedAtMs", "created_at_ms", "createdAtMs"], 0), 0),
    recoveryOptions,
    raw: row,
  };
}

function buildRecoveryQueueRows(payload = {}) {
  const rows = [
    ...toArray(payload?.outbox, "outbox").map((row, index) => reliabilityRow("outbox", row, index)),
    ...toArray(payload?.tool_effects || payload?.toolEffects, "tool_effects").map((row, index) =>
      reliabilityRow("tool_effect", row, index)
    ),
    ...toArray(payload?.dead_letters || payload?.deadLetters, "dead_letters").map((row, index) =>
      reliabilityRow("dead_letter", row, index)
    ),
    ...toArray(payload?.compensations, "compensations").map((row, index) => reliabilityRow("compensation", row, index)),
  ];
  return rows.sort((a, b) => b.updatedAtMs - a.updatedAtMs);
}

function summarizeRecoveryQueueRows(rows = []) {
  const summary = { total: 0, retryable: 0, waitingBackoff: 0, deadLettered: 0, manuallyBlocked: 0 };
  for (const row of Array.isArray(rows) ? rows : []) {
    summary.total += 1;
    if (row.category === "retryable") summary.retryable += 1;
    if (row.category === "waiting_backoff") summary.waitingBackoff += 1;
    if (row.category === "dead_lettered") summary.deadLettered += 1;
    if (row.category === "manually_blocked") summary.manuallyBlocked += 1;
  }
  return summary;
}

export {
  buildApprovalWaitRows,
  buildRecoveryQueueRows,
  buildWebhookInboxRows,
  deadlineLabel,
  filterStatefulQueueRows,
  statusTone,
  summarizeApprovalWaitRows,
  summarizeRecoveryQueueRows,
  summarizeWebhookInboxRows,
  titleCase,
};
