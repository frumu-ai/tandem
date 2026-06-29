const RUN_SOURCE_FILTERS = [
  { value: "all", label: "All sources" },
  { value: "workflow", label: "Workflows" },
  { value: "automation", label: "Automations" },
  { value: "context", label: "Context runs" },
];

const RUN_STATUS_FILTERS = [
  { value: "all", label: "All statuses" },
  { value: "active", label: "Active" },
  { value: "waiting", label: "Waiting" },
  { value: "queued", label: "Queued" },
  { value: "failed", label: "Failed" },
  { value: "completed", label: "Completed" },
];

const DEFAULT_STATEFUL_RUN_FILTERS = {
  query: "",
  status: "all",
  source: "all",
  tenant: "",
  workspace: "",
  wait: "",
};

function compact(values) {
  return values
    .map((value) => stringValue(value))
    .filter(Boolean);
}

function stringValue(value, fallback = "") {
  if (value === null || value === undefined) return fallback;
  const text = String(value).trim();
  return text || fallback;
}

function normalizeKey(value) {
  return stringValue(value)
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/[\s-]+/g, "_")
    .toLowerCase();
}

function titleCase(value, fallback = "Unknown") {
  return stringValue(value, fallback)
    .replace(/[_-]+/g, " ")
    .replace(/\b\w/g, (match) => match.toUpperCase());
}

function timestampMs(value) {
  if (value === null || value === undefined || value === "") return 0;
  const numeric = Number(value);
  if (Number.isFinite(numeric) && numeric > 0) return numeric;
  const parsed = Date.parse(String(value));
  return Number.isFinite(parsed) ? parsed : 0;
}

function firstTimestamp(values) {
  for (const value of values) {
    const parsed = timestampMs(value);
    if (parsed > 0) return parsed;
  }
  return 0;
}

function toArray(input, key) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function runIdOf(row) {
  return stringValue(
    row?.run_id ||
      row?.runId ||
      row?.id ||
      row?.run?.run_id ||
      row?.run?.id ||
      row?.context_run_id ||
      row?.contextRunId
  );
}

function canonicalRunId(id) {
  const text = stringValue(id);
  return text.startsWith("automation-v2-") ? text.replace(/^automation-v2-/, "") : text;
}

function tenantContextOf(row) {
  const tenant = row?.tenant_context || row?.tenantContext || row?.scheduler?.tenant_context || {};
  return {
    orgId: stringValue(tenant.org_id || tenant.orgId || row?.org_id || row?.orgId, "local"),
    workspaceId: stringValue(
      tenant.workspace_id || tenant.workspaceId || row?.workspace_id || row?.workspaceId,
      "local"
    ),
    deploymentId: stringValue(tenant.deployment_id || tenant.deploymentId || row?.deployment_id || row?.deploymentId),
    actorId: stringValue(tenant.actor_id || tenant.actorId || row?.actor_id || row?.actorId),
  };
}

function workspaceOf(row) {
  return stringValue(
    row?.workspace_root ||
      row?.workspaceRoot ||
      row?.workspace ||
      row?.workspace_path ||
      row?.workspacePath ||
      row?.runtime_context?.workspace_root ||
      row?.runtimeContext?.workspaceRoot ||
      row?.automation_snapshot?.workspace_root ||
      row?.automationSnapshot?.workspaceRoot
  );
}

function statusGroup(status, waitKind) {
  const normalized = normalizeKey(status);
  if (["queued", "pending", "scheduled"].includes(normalized)) return "queued";
  if (
    [
      "awaiting_approval",
      "approval_required",
      "blocked",
      "paused",
      "waiting",
      "waiting_for_human",
      "stalled",
    ].includes(normalized) ||
    waitKind
  ) {
    return "waiting";
  }
  if (["failed", "error", "cancelled", "canceled", "timeout", "timed_out"].includes(normalized)) {
    return "failed";
  }
  if (["completed", "succeeded", "success", "done"].includes(normalized)) return "completed";
  if (["running", "active", "executing", "planning", "in_progress", "pausing"].includes(normalized)) {
    return "active";
  }
  return "unknown";
}

function statusTone(group) {
  if (group === "active") return "live";
  if (group === "waiting" || group === "queued") return "warn";
  if (group === "failed") return "err";
  if (group === "completed") return "ok";
  return "ghost";
}

function latestLifecycleEvent(row) {
  const history = Array.isArray(row?.checkpoint?.lifecycle_history)
    ? row.checkpoint.lifecycle_history
    : Array.isArray(row?.checkpoint?.lifecycleHistory)
      ? row.checkpoint.lifecycleHistory
      : [];
  return history.reduce((latest, record) => {
    const at = timestampMs(record?.recorded_at_ms || record?.recordedAtMs);
    if (!latest || at >= latest.atMs) {
      return {
        label: titleCase(record?.event || "event"),
        detail: stringValue(record?.reason),
        atMs: at,
      };
    }
    return latest;
  }, null);
}

function currentWaitOf(row) {
  const gate = row?.checkpoint?.awaiting_gate || row?.checkpoint?.awaitingGate || row?.awaiting_gate;
  if (gate) {
    return {
      kind: "approval",
      label: stringValue(gate.title, "Awaiting approval"),
      detail: stringValue(gate.node_id || gate.nodeId),
    };
  }

  const scheduler = row?.scheduler || {};
  const queueReason = stringValue(scheduler.queue_reason || scheduler.queueReason);
  if (queueReason) {
    return {
      kind: "queue",
      label: `Queued: ${titleCase(queueReason)}`,
      detail: stringValue(scheduler.resource_key || scheduler.resourceKey || scheduler.rate_limited_provider),
    };
  }

  const blockedNodes = row?.checkpoint?.blocked_nodes || row?.checkpoint?.blockedNodes || [];
  if (Array.isArray(blockedNodes) && blockedNodes.length) {
    return {
      kind: "blocked",
      label: `Blocked nodes: ${blockedNodes.slice(0, 3).join(", ")}`,
      detail: blockedNodes.length > 3 ? `${blockedNodes.length} total` : "",
    };
  }

  const pauseReason = stringValue(row?.pause_reason || row?.pauseReason);
  if (pauseReason) return { kind: "paused", label: pauseReason, detail: "" };

  return { kind: "", label: "", detail: "" };
}

function phaseOf(row, wait) {
  const explicit = stringValue(
    row?.workflow_phase ||
      row?.workflowPhase ||
      row?.phase ||
      row?.current_phase ||
      row?.currentPhase ||
      row?.checkpoint?.phase
  );
  if (explicit) return titleCase(explicit);
  if (wait.kind === "approval") return "Awaiting Approval";
  if (wait.kind === "queue") return "Queued";
  if (wait.kind === "blocked") return "Blocked";
  if (row?.active_session_ids?.length || row?.active_instance_ids?.length) return "Executing";
  return titleCase(row?.status || "Unknown");
}

function retryStateOf(row) {
  const failure = row?.checkpoint?.last_failure || row?.checkpoint?.lastFailure || row?.last_failure;
  if (failure) {
    return {
      label: "Last failure",
      detail: compact([failure.node_id || failure.nodeId, failure.reason]).join(": "),
    };
  }

  const attempts = row?.checkpoint?.node_attempts || row?.checkpoint?.nodeAttempts || {};
  const maxAttempts = Object.values(attempts).reduce((max, value) => {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? Math.max(max, parsed) : max;
  }, 0);
  if (maxAttempts > 1) return { label: `${maxAttempts} attempts`, detail: "" };

  const retry = row?.retry_state || row?.retryState || row?.scheduler?.retry_state || row?.scheduler?.retryState;
  if (retry) return { label: titleCase(retry), detail: "" };
  return { label: "None", detail: "" };
}

function triggerSourceOf(row) {
  const metadata = row?.metadata || row?.automation_snapshot?.metadata || row?.automationSnapshot?.metadata || {};
  return titleCase(
    row?.trigger_type ||
      row?.triggerType ||
      row?.trigger ||
      metadata.trigger_source ||
      metadata.triggerSource ||
      metadata.source ||
      "manual"
  );
}

function titleOf(row, source) {
  if (source === "workflow") {
    return stringValue(
      row?.automation_snapshot?.name ||
        row?.automationSnapshot?.name ||
        row?.automation_name ||
        row?.automationName ||
        row?.title ||
        row?.name,
      "Workflow run"
    );
  }
  if (source === "context") {
    return stringValue(row?.title || row?.objective || row?.goal || row?.run_type || row?.runType, "Context run");
  }
  return stringValue(row?.automation_name || row?.automationName || row?.name || row?.title, "Automation run");
}

function updatedAtOf(row) {
  return firstTimestamp([
    row?.updated_at_ms,
    row?.updatedAtMs,
    row?.updated_at,
    row?.updatedAt,
    row?.finished_at_ms,
    row?.finishedAtMs,
    row?.started_at_ms,
    row?.startedAtMs,
    row?.created_at_ms,
    row?.createdAtMs,
    row?.created_at,
    row?.createdAt,
  ]);
}

function projectRun(row, source) {
  const id = runIdOf(row);
  if (!id) return null;
  const wait = currentWaitOf(row);
  const status = stringValue(row?.status?.run?.status || row?.status, "unknown");
  const group = statusGroup(status, wait.kind);
  const tenant = tenantContextOf(row);
  const lastEvent = latestLifecycleEvent(row);
  const updatedAtMs = updatedAtOf(row);
  const createdAtMs = firstTimestamp([row?.created_at_ms, row?.createdAtMs, row?.created_at, row?.createdAt]);
  const retry = retryStateOf(row);

  return {
    id,
    canonicalId: canonicalRunId(id),
    source,
    sourceLabel: source === "workflow" ? "Workflow" : source === "context" ? "Context" : "Automation",
    title: titleOf(row, source),
    subtitle: stringValue(row?.trigger_reason || row?.triggerReason || row?.detail || row?.description),
    status,
    statusLabel: titleCase(status),
    statusGroup: group,
    statusTone: statusTone(group),
    phase: phaseOf(row, wait),
    triggerSource: triggerSourceOf(row),
    tenantOrg: tenant.orgId,
    tenantWorkspace: tenant.workspaceId,
    tenantDeployment: tenant.deploymentId,
    tenantActor: tenant.actorId,
    workspace: workspaceOf(row),
    waitKind: wait.kind,
    currentWait: wait.label,
    waitDetail: wait.detail,
    retryState: retry.label,
    retryDetail: retry.detail,
    lastEventLabel: lastEvent?.label || titleCase(row?.detail || status),
    lastEventDetail: lastEvent?.detail || "",
    lastEventAtMs: lastEvent?.atMs || updatedAtMs,
    updatedAtMs,
    createdAtMs,
    route: source === "context" ? "orchestrator" : "automations",
    sourcePriority: source === "workflow" ? 3 : source === "automation" ? 2 : 1,
  };
}

function mergeRows(existing, candidate) {
  if (!existing) return candidate;
  const useCandidate = candidate.sourcePriority > existing.sourcePriority;
  const primary = useCandidate ? { ...candidate } : { ...existing };
  const secondary = useCandidate ? existing : candidate;
  if (secondary.updatedAtMs > primary.updatedAtMs) {
    primary.updatedAtMs = secondary.updatedAtMs;
    primary.lastEventAtMs = Math.max(primary.lastEventAtMs, secondary.lastEventAtMs);
    if (!primary.lastEventLabel || primary.lastEventLabel === primary.statusLabel) {
      primary.lastEventLabel = secondary.lastEventLabel;
      primary.lastEventDetail = secondary.lastEventDetail;
    }
  }
  for (const key of ["workspace", "currentWait", "waitDetail", "retryDetail", "tenantActor", "tenantDeployment"]) {
    if (!primary[key] && secondary[key]) primary[key] = secondary[key];
  }
  if (primary.tenantOrg === "local" && secondary.tenantOrg !== "local") primary.tenantOrg = secondary.tenantOrg;
  if (primary.tenantWorkspace === "local" && secondary.tenantWorkspace !== "local") {
    primary.tenantWorkspace = secondary.tenantWorkspace;
  }
  return primary;
}

function buildStatefulRunRows(input = {}) {
  const rows = [
    ...toArray(input.workflowRuns || input.automationV2Runs || input.automationRunsV2, "runs").map((row) =>
      projectRun(row, "workflow")
    ),
    ...toArray(input.legacyRuns || input.automationRuns, "runs").map((row) => projectRun(row, "automation")),
    ...toArray(input.contextRuns, "runs").map((row) => projectRun(row, "context")),
  ].filter(Boolean);

  const byId = new Map();
  for (const row of rows) {
    const key = row.canonicalId || row.id;
    byId.set(key, mergeRows(byId.get(key), row));
  }

  return Array.from(byId.values()).sort((left, right) => {
    const at = right.updatedAtMs - left.updatedAtMs;
    if (at) return at;
    return left.id.localeCompare(right.id);
  });
}

function normalizeStatefulRunFilters(filters = {}) {
  const input = filters && typeof filters === "object" ? filters : {};
  const status = RUN_STATUS_FILTERS.some((option) => option.value === input.status) ? input.status : "all";
  const source = RUN_SOURCE_FILTERS.some((option) => option.value === input.source) ? input.source : "all";
  return {
    query: stringValue(input.query),
    status,
    source,
    tenant: stringValue(input.tenant),
    workspace: stringValue(input.workspace),
    wait: stringValue(input.wait),
  };
}

function rowSearchText(row) {
  return compact([
    row.id,
    row.title,
    row.subtitle,
    row.statusLabel,
    row.phase,
    row.triggerSource,
    row.tenantOrg,
    row.tenantWorkspace,
    row.tenantDeployment,
    row.tenantActor,
    row.workspace,
    row.currentWait,
    row.waitDetail,
    row.retryState,
    row.retryDetail,
    row.lastEventLabel,
    row.sourceLabel,
  ])
    .join(" ")
    .toLowerCase();
}

function filterStatefulRunRows(rows, filters = DEFAULT_STATEFUL_RUN_FILTERS) {
  const normalized = normalizeStatefulRunFilters(filters);
  const query = normalized.query.toLowerCase();
  const tenant = normalized.tenant.toLowerCase();
  const workspace = normalized.workspace.toLowerCase();
  const wait = normalized.wait.toLowerCase();
  return (Array.isArray(rows) ? rows : []).filter((row) => {
    const text = rowSearchText(row);
    if (query && !text.includes(query)) return false;
    if (normalized.status !== "all" && row.statusGroup !== normalized.status) return false;
    if (normalized.source !== "all" && row.source !== normalized.source) return false;
    if (
      tenant &&
      !compact([row.tenantOrg, row.tenantWorkspace, row.tenantDeployment, row.tenantActor])
        .join(" ")
        .toLowerCase()
        .includes(tenant)
    ) {
      return false;
    }
    if (workspace && !stringValue(row.workspace).toLowerCase().includes(workspace)) return false;
    if (
      wait &&
      !compact([row.phase, row.currentWait, row.waitDetail, row.retryState, row.retryDetail])
        .join(" ")
        .toLowerCase()
        .includes(wait)
    ) {
      return false;
    }
    return true;
  });
}

function summarizeStatefulRuns(rows) {
  const summary = {
    total: 0,
    active: 0,
    waiting: 0,
    queued: 0,
    failed: 0,
    completed: 0,
    tenants: 0,
    workspaces: 0,
  };
  const tenants = new Set();
  const workspaces = new Set();
  for (const row of Array.isArray(rows) ? rows : []) {
    summary.total += 1;
    if (Object.prototype.hasOwnProperty.call(summary, row.statusGroup)) {
      summary[row.statusGroup] += 1;
    }
    tenants.add(`${row.tenantOrg}/${row.tenantWorkspace}`);
    if (row.workspace) workspaces.add(row.workspace);
  }
  summary.tenants = tenants.size;
  summary.workspaces = workspaces.size;
  return summary;
}

function formatRunTimestamp(ms, fallback = "n/a") {
  if (!ms) return fallback;
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(ms));
}

export {
  DEFAULT_STATEFUL_RUN_FILTERS,
  RUN_SOURCE_FILTERS,
  RUN_STATUS_FILTERS,
  buildStatefulRunRows,
  filterStatefulRunRows,
  formatRunTimestamp,
  normalizeStatefulRunFilters,
  summarizeStatefulRuns,
};
