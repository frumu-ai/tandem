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
  orgUnit: "",
  resource: "",
  policy: "",
  dataClass: "",
  knowledge: "",
  wait: "",
};

const FALLBACK_ALLOWED_NEXT_PHASES = {
  created: ["queued", "cancelled"],
  queued: ["running_phase", "paused_attention_required", "cancelled"],
  running_phase: ["sleeping", "waiting_webhook", "awaiting_approval", "retrying", "paused_attention_required", "completed", "failed", "cancelled"],
  sleeping: ["running_phase", "paused_attention_required", "cancelled"],
  waiting_webhook: ["running_phase", "retrying", "paused_attention_required", "failed", "cancelled"],
  awaiting_approval: ["running_phase", "paused_attention_required", "failed", "cancelled"],
  retrying: ["running_phase", "paused_attention_required", "failed", "cancelled"],
  paused_attention_required: ["queued", "running_phase", "failed", "cancelled"],
  failed: [],
  completed: [],
  cancelled: [],
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
    .replace(/[_.-]+/g, " ")
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
  let text = stringValue(id);
  while (text.startsWith("automation-v2-")) {
    text = text.slice("automation-v2-".length);
  }
  return text;
}

function tenantContextOf(row) {
  const tenant =
    row?.tenant_context ||
    row?.tenantContext ||
    row?.scope?.tenant_context ||
    row?.scope?.tenantContext ||
    row?.scheduler?.tenant_context ||
    {};
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

function workspacePathValue(value) {
  if (!value || typeof value !== "object") return stringValue(value);
  return stringValue(
    value.canonical_path ||
      value.canonicalPath ||
      value.path ||
      value.current_path ||
      value.currentPath ||
      value.workspace_root ||
      value.workspaceRoot ||
      value.root
  );
}

function workspaceOf(row) {
  const candidates = [
    row?.workspace_root,
    row?.workspaceRoot,
    row?.workspace,
    row?.workspace_path,
    row?.workspacePath,
    row?.runtime_context?.workspace_root,
    row?.runtimeContext?.workspaceRoot,
    row?.automation_snapshot?.workspace_root,
    row?.automationSnapshot?.workspaceRoot,
  ];
  for (const candidate of candidates) {
    const workspace = workspacePathValue(candidate);
    if (workspace) return workspace;
  }
  return "";
}

function uniqueCompact(values) {
  return Array.from(new Set(compact(values)));
}

function resourceLabel(kind, id) {
  const resourceKind = stringValue(kind);
  const resourceId = stringValue(id);
  if (!resourceKind && !resourceId) return "";
  if (!resourceKind) return resourceId;
  if (!resourceId) return titleCase(resourceKind);
  return `${titleCase(resourceKind)} ${resourceId}`;
}

function principalLabel(principal) {
  if (!principal || typeof principal !== "object") return stringValue(principal);
  return compact([principal.kind, principal.id]).join(":");
}

function enterpriseScopeOf(row) {
  const enterprise = row?.enterprise_scope || row?.enterpriseScope || {};
  const scope = row?.scope || {};
  const orgUnit = enterprise.owning_org_unit || enterprise.owningOrgUnit || {};
  const resourceScope =
    enterprise.resource_scope || enterprise.resourceScope || scope.resource_scope || scope.resourceScope || {};
  const root = resourceScope.root || enterprise.root_resource || enterprise.rootResource || {};
  const sources = toArray(
    enterprise.visible_knowledge_sources || enterprise.visibleKnowledgeSources || enterprise.knowledge_sources,
    "sources"
  );
  const orgUnitId = stringValue(
    enterprise.owning_org_unit_id || enterprise.owningOrgUnitId || scope.owning_org_unit_id || scope.owningOrgUnitId
  );
  const orgUnitName = stringValue(orgUnit.display_name || orgUnit.displayName || orgUnit.name);
  const resourceKind = stringValue(enterprise.resource_kind || enterprise.resourceKind || root.resource_kind || root.resourceKind);
  const resourceId = stringValue(enterprise.resource_id || enterprise.resourceId || root.resource_id || root.resourceId);
  const policyVersion = stringValue(
    enterprise.policy_version_id || enterprise.policyVersionId || scope.policy_version_id || scope.policyVersionId
  );
  const dataClasses = uniqueCompact([
    ...toArray(enterprise.data_classes || enterprise.dataClasses || scope.data_classes || scope.dataClasses, "data_classes"),
    ...sources.map((source) => source?.data_class || source?.dataClass),
  ]);
  const knowledgeSourceIds = uniqueCompact(
    sources.map((source) => source?.binding_id || source?.bindingId || source?.source_binding_id || source?.sourceBindingId)
  );
  const knowledgeSourceLabels = uniqueCompact(
    sources.map((source) =>
      source?.source_root_label ||
      source?.sourceRootLabel ||
      source?.source_type ||
      source?.sourceType ||
      source?.binding_id ||
      source?.bindingId
    )
  );
  const delegationGrantIds = uniqueCompact(
    toArray(enterprise.delegation_grant_ids || enterprise.delegationGrantIds || scope.delegation_grant_ids, "grants")
  );
  const ownerPrincipal = principalLabel(enterprise.owner_principal || enterprise.ownerPrincipal || scope.owner_principal);
  const scopeSummary =
    compact([
      orgUnitName || orgUnitId,
      resourceLabel(resourceKind, resourceId),
      policyVersion ? `Policy ${policyVersion}` : "",
    ]).join(" · ") || "Tenant scoped";

  return {
    orgUnitId,
    orgUnitName,
    resourceKind,
    resourceId,
    resourceLabel: resourceLabel(resourceKind, resourceId),
    policyVersion,
    dataClasses,
    delegationGrantIds,
    ownerPrincipal,
    knowledgeSourceIds,
    knowledgeSourceLabels,
    knowledgeSourceCount: Number(enterprise.summary?.knowledge_source_count ?? enterprise.summary?.knowledgeSourceCount) || sources.length,
    scopeSummary,
  };
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
  const latestEvent = row?.latest_event || row?.latestEvent;
  if (latestEvent && typeof latestEvent === "object") {
    return {
      label: titleCase(latestEvent.event_type || latestEvent.eventType || "event"),
      detail: compact([latestEvent.phase_id || latestEvent.phaseId, latestEvent.wait_kind || latestEvent.waitKind]).join(" · "),
      atMs: timestampMs(latestEvent.occurred_at_ms || latestEvent.occurredAtMs),
    };
  }

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
  const wait = row?.current_wait || row?.currentWait;
  if (wait && typeof wait === "object") {
    const kind = stringValue(wait.wait_kind || wait.waitKind);
    return {
      kind,
      label: stringValue(wait.reason, kind ? titleCase(kind) : "Waiting"),
      detail: compact([wait.wait_id || wait.waitId, wait.status]).join(" · "),
    };
  }

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
  const wait = row?.current_wait || row?.currentWait;
  const timeout = wait?.timeout_policy || wait?.timeoutPolicy;
  if (timeout?.on_timeout || timeout?.onTimeout) {
    return {
      label: `Timeout: ${titleCase(timeout.on_timeout || timeout.onTimeout)}`,
      detail: timeout.timeout_at_ms || timeout.timeoutAtMs ? `at ${timeout.timeout_at_ms || timeout.timeoutAtMs}` : "",
    };
  }

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

function numberValue(value, fallback = 0) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function recordStatus(row) {
  return stringValue(row?.status || row?.decision || row?.state || row?.effect || row?.kind, "unknown");
}

function recordId(row, keys, fallback = "") {
  for (const key of keys) {
    const value = stringValue(row?.[key]);
    if (value) return value;
  }
  return fallback;
}

function objectValue(value) {
  return value && typeof value === "object" && !Array.isArray(value) ? value : {};
}

function firstValue(values) {
  for (const value of values) {
    if (value !== null && value !== undefined && value !== "") return value;
  }
  return undefined;
}

function stableStringify(value) {
  if (value === null || value === undefined || value === "") return "";
  if (typeof value !== "object") return stringValue(value);
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  const entries = Object.keys(value)
    .sort()
    .map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`);
  return `{${entries.join(",")}}`;
}

function valueSummary(value) {
  if (value === null || value === undefined || value === "") return "n/a";
  if (Array.isArray(value)) return value.length ? compact(value).join(", ") : "none";
  if (typeof value === "object") {
    const text = stableStringify(value);
    return text.length > 96 ? `${text.slice(0, 93)}...` : text || "none";
  }
  const text = stringValue(value, "n/a");
  return text.length > 96 ? `${text.slice(0, 93)}...` : text;
}

function comparableValue(value) {
  if (value === null || value === undefined || value === "") return "";
  return typeof value === "object" ? stableStringify(value) : stringValue(value);
}

function checkpointOf(snapshot) {
  return objectValue(snapshot?.checkpoint || snapshot?.payload?.checkpoint);
}

function checkpointField(snapshot, snakeKey, camelKey) {
  const checkpoint = checkpointOf(snapshot);
  return firstValue([checkpoint[snakeKey], checkpoint[camelKey]]);
}

function phaseHistoryRows(rows) {
  return toArray(rows, "phase_history").map((row, index) => ({
    id: recordId(row, ["event_id", "eventId"], `phase-history-${index}`),
    label: compact([row?.from_phase || row?.fromPhase, row?.to_phase || row?.toPhase || row?.phase]).join(" -> "),
    status: stringValue(row?.event_type || row?.eventType, "phase"),
    detail: compact([row?.phase_id || row?.phaseId, row?.reason]).join(" · "),
    occurredAtMs: firstTimestamp([row?.occurred_at_ms, row?.occurredAtMs]),
  }));
}

function projectObservabilityWait(wait) {
  const kind = stringValue(wait?.wait_kind || wait?.waitKind, "wait");
  return {
    id: recordId(wait, ["wait_id", "waitId"], kind),
    label: stringValue(wait?.reason, titleCase(kind)),
    status: recordStatus(wait),
    detail: compact([wait?.phase_id || wait?.phaseId, wait?.wait_id || wait?.waitId]).join(" · "),
    updatedAtMs: firstTimestamp([wait?.updated_at_ms, wait?.updatedAtMs, wait?.created_at_ms, wait?.createdAtMs]),
    raw: wait,
  };
}

function projectObservabilityEvent(event) {
  return {
    id: recordId(event, ["event_id", "eventId"], `seq-${event?.seq || "unknown"}`),
    label: titleCase(event?.event_type || event?.eventType || "event"),
    status: stringValue(event?.event_type || event?.eventType, "event"),
    seq: numberValue(event?.seq),
    detail: compact([event?.phase_id || event?.phaseId, event?.causation_id || event?.causationId]).join(" · "),
    correlationId: stringValue(event?.correlation_id || event?.correlationId),
    occurredAtMs: firstTimestamp([event?.occurred_at_ms, event?.occurredAtMs, event?.created_at_ms, event?.createdAtMs]),
  };
}

function projectObservabilitySnapshot(snapshot) {
  const seq = numberValue(snapshot?.seq);
  const checkpoint = checkpointOf(snapshot);
  const phaseId = stringValue(snapshot?.phase_id || snapshot?.phaseId);
  const phase = stringValue(snapshot?.phase || snapshot?.runtime_phase || snapshot?.runtimePhase);
  const payloadDigest = stringValue(snapshot?.payload_digest || snapshot?.payloadDigest);
  const checkpointDigest = stringValue(
    snapshot?.checkpoint_digest ||
      snapshot?.checkpointDigest ||
      checkpoint.checkpoint_digest ||
      checkpoint.checkpointDigest ||
      checkpoint.digest ||
      checkpoint.hash
  );
  const allowedNextPhases = uniqueCompact(
    toArray(snapshot?.allowed_next_phases || snapshot?.allowedNextPhases, "allowed_next_phases")
  );
  return {
    id: recordId(snapshot, ["snapshot_id", "snapshotId"], seq ? `snapshot-${seq}` : "snapshot"),
    label: seq ? `Snapshot ${seq}` : "Snapshot",
    status: recordStatus(snapshot),
    seq,
    phase,
    phaseId,
    payloadDigest,
    checkpointDigest,
    checkpoint,
    allowedNextPhases,
    phaseHistory: phaseHistoryRows(snapshot?.phase_history || snapshot?.phaseHistory),
    sourceRecordKind: stringValue(snapshot?.source_record_kind || snapshot?.sourceRecordKind),
    detail: compact([phase || phaseId, payloadDigest, checkpointDigest]).join(" · "),
    createdAtMs: firstTimestamp([snapshot?.created_at_ms, snapshot?.createdAtMs]),
    workflowDefinitionVersion: stringValue(snapshot?.workflow_definition_version || snapshot?.workflowDefinitionVersion),
    workflowDefinitionSnapshotHash: stringValue(
      snapshot?.workflow_definition_snapshot_hash || snapshot?.workflowDefinitionSnapshotHash
    ),
  };
}

function snapshotDiffChanges(from, to) {
  const fields = [
    { key: "status", label: "Status", get: (snapshot) => snapshot?.status },
    { key: "phase", label: "Runtime phase", get: (snapshot) => snapshot?.phase },
    { key: "phaseId", label: "Workflow phase", get: (snapshot) => snapshot?.phaseId },
    { key: "payloadDigest", label: "Payload digest", get: (snapshot) => snapshot?.payloadDigest },
    { key: "checkpointDigest", label: "Checkpoint digest", get: (snapshot) => snapshot?.checkpointDigest },
    {
      key: "workflowDefinitionSnapshotHash",
      label: "Workflow hash",
      get: (snapshot) => snapshot?.workflowDefinitionSnapshotHash,
    },
    { key: "completedNodes", label: "Completed nodes", get: (snapshot) => checkpointField(snapshot, "completed_nodes", "completedNodes") },
    { key: "pendingNodes", label: "Pending nodes", get: (snapshot) => checkpointField(snapshot, "pending_nodes", "pendingNodes") },
    { key: "blockedNodes", label: "Blocked nodes", get: (snapshot) => checkpointField(snapshot, "blocked_nodes", "blockedNodes") },
    { key: "awaitingGate", label: "Awaiting gate", get: (snapshot) => checkpointField(snapshot, "awaiting_gate", "awaitingGate") },
    { key: "nodeAttempts", label: "Node attempts", get: (snapshot) => checkpointField(snapshot, "node_attempts", "nodeAttempts") },
    { key: "lastFailure", label: "Last failure", get: (snapshot) => checkpointField(snapshot, "last_failure", "lastFailure") },
    { key: "resumeReason", label: "Resume reason", get: (snapshot) => checkpointField(snapshot, "resume_reason", "resumeReason") },
    { key: "executionClaim", label: "Execution claim", get: (snapshot) => checkpointField(snapshot, "execution_claim", "executionClaim") },
  ];
  return fields
    .map((field) => {
      const before = field.get(from);
      const after = field.get(to);
      if (comparableValue(before) === comparableValue(after)) return null;
      return {
        key: field.key,
        label: field.label,
        from: valueSummary(before),
        to: valueSummary(after),
      };
    })
    .filter(Boolean);
}

function snapshotSortValue(snapshot) {
  return snapshot?.seq || snapshot?.createdAtMs || 0;
}

function buildSnapshotDiffRows(snapshots) {
  const ordered = [...(Array.isArray(snapshots) ? snapshots : [])].sort((left, right) => {
    const bySeq = snapshotSortValue(left) - snapshotSortValue(right);
    if (bySeq) return bySeq;
    return String(left?.id || "").localeCompare(String(right?.id || ""));
  });
  const rows = [];
  for (let index = 1; index < ordered.length; index += 1) {
    const from = ordered[index - 1];
    const to = ordered[index];
    const changes = snapshotDiffChanges(from, to);
    rows.push({
      id: `${from.id}->${to.id}`,
      label: `${from.label} -> ${to.label}`,
      status: changes.length ? "changed" : "unchanged",
      statusGroup: changes.length ? "waiting" : "completed",
      seq: to.seq,
      createdAtMs: to.createdAtMs,
      detail: changes.length
        ? changes
            .slice(0, 3)
            .map((change) => `${change.label}: ${change.from} -> ${change.to}`)
            .join(" · ")
        : "No projected snapshot changes.",
      fromSnapshotId: from.id,
      toSnapshotId: to.id,
      changes,
    });
  }
  return rows.reverse();
}

function resumeReasonFrom({ run, latestSnapshot, snapshots }) {
  const runMetadata = objectValue(run?.metadata);
  const latestCheckpoint = checkpointOf(latestSnapshot);
  const fromSnapshots = (Array.isArray(snapshots) ? snapshots : [])
    .map((snapshot) => checkpointField(snapshot, "resume_reason", "resumeReason"))
    .reverse()
    .find(Boolean);
  return stringValue(
    run?.resume_reason ||
      run?.resumeReason ||
      runMetadata.resume_reason ||
      runMetadata.resumeReason ||
      latestCheckpoint.resume_reason ||
      latestCheckpoint.resumeReason ||
      fromSnapshots
  );
}

function crashRecoverySnapshotDiff(snapshotDiffs, resumeReason) {
  if (normalizeKey(resumeReason) !== "server_restart_rehydration") return null;
  const changed = snapshotDiffs.find((row) => row.changes?.length);
  const latest = changed || snapshotDiffs[0] || null;
  if (!latest) return null;
  return {
    ...latest,
    id: `crash-recovery:${latest.id}`,
    label: "Crash recovery checkpoint",
    status: latest.changes?.length ? "rehydrated" : "unchanged",
    statusGroup: latest.changes?.length ? "waiting" : "completed",
    detail: latest.changes?.length
      ? `server_restart_rehydration: ${latest.detail}`
      : "server_restart_rehydration with no projected checkpoint delta.",
  };
}

function projectPolicyDecision(decision) {
  return {
    id: recordId(decision, ["decision_id", "decisionId"], "policy-decision"),
    label: titleCase(decision?.decision || "decision"),
    status: recordStatus(decision),
    detail: compact([
      decision?.tool,
      decision?.reason_code || decision?.reasonCode,
      decision?.approval_id || decision?.approvalId,
    ]).join(" · "),
    createdAtMs: firstTimestamp([decision?.created_at_ms, decision?.createdAtMs]),
  };
}

function principalSummary(value) {
  if (!value || typeof value !== "object") return stringValue(value);
  return compact([
    value.kind || value.type || value.principal_kind || value.principalKind,
    value.id || value.principal_id || value.principalId || value.actor_id || value.actorId,
  ]).join(":");
}

function runAsContext(row) {
  const metadata = objectValue(row?.metadata);
  const runAs = objectValue(row?.run_as || row?.runAs || metadata.run_as || metadata.runAs || metadata.__mcp_run_as);
  const effectiveTenant = objectValue(runAs.effective_tenant_context || runAs.effectiveTenantContext);
  const principal =
    principalSummary(
      row?.acting_principal ||
        row?.actingPrincipal ||
        row?.execution_principal ||
        row?.executionPrincipal ||
        row?.principal ||
        row?.actor ||
        metadata.acting_principal ||
        metadata.actingPrincipal ||
        metadata.execution_principal ||
        metadata.executionPrincipal ||
        metadata.principal ||
        metadata.actor ||
        runAs.principal
    ) || "";
  const actor = stringValue(row?.actor_id || row?.actorId || metadata.actor_id || metadata.actorId || effectiveTenant.actor_id || effectiveTenant.actorId);
  const connection = stringValue(
    row?.connection_id || row?.connectionId || metadata.connection_id || metadata.connectionId || runAs.connection_id || runAs.connectionId
  );
  const runAsKind = stringValue(runAs.kind || runAs.type || runAs.mode);
  const parts = compact([
    principal ? `principal ${principal}` : "",
    actor ? `actor ${actor}` : "",
    connection ? `connection ${connection}` : "",
    runAsKind ? `run-as ${runAsKind}` : "",
  ]);
  return parts.join(" · ");
}

function projectReliabilityRecord(row, kind) {
  const id = recordId(
    row,
    [
      "effect_id",
      "effectId",
      "outbox_id",
      "outboxId",
      "dead_letter_id",
      "deadLetterId",
      "compensation_id",
      "compensationId",
    ],
    kind
  );
  return {
    id,
    kind,
    label: titleCase(row?.operation || row?.source_type || row?.sourceType || row?.compensation_type || row?.compensationType || kind),
    status: recordStatus(row),
    detail: compact([
      row?.tool,
      row?.provider,
      row?.target,
      row?.idempotency_key || row?.idempotencyKey,
      row?.policy_decision_id || row?.policyDecisionId,
      runAsContext(row),
      row?.reason,
      row?.error,
    ]).join(" · "),
    updatedAtMs: firstTimestamp([row?.updated_at_ms, row?.updatedAtMs, row?.created_at_ms, row?.createdAtMs]),
  };
}

function projectAuditEvent(event) {
  return {
    id: recordId(event, ["event_id", "eventId"], "audit-event"),
    label: titleCase(event?.event_type || event?.eventType || "audit"),
    status: stringValue(event?.event_type || event?.eventType, "audit"),
    seq: numberValue(event?.seq),
    detail: compact([event?.actor, event?.payload_digest || event?.payloadDigest || event?.record_hash || event?.recordHash]).join(" · "),
    createdAtMs: firstTimestamp([event?.created_at_ms, event?.createdAtMs]),
  };
}

function projectOperatorItem(row, fallbackKind) {
  const action = stringValue(row?.action || row?.kind, fallbackKind);
  return {
    id: compact([
      action,
      row?.wait_id || row?.waitId,
      row?.decision_id || row?.decisionId,
      row?.effect_id || row?.effectId,
      row?.dead_letter_id || row?.deadLetterId,
      row?.compensation_id || row?.compensationId,
    ]).join(":"),
    label: titleCase(action),
    status: recordStatus(row),
    detail: compact([
      row?.reason,
      row?.reason_code || row?.reasonCode,
      row?.status,
      row?.phase_id || row?.phaseId,
      row?.wait_kind || row?.waitKind,
      row?.tool,
    ]).join(" · "),
  };
}

function runtimePhaseValue(run, latestSnapshot) {
  return stringValue(run?.phase || run?.runtime_phase || run?.runtimePhase || latestSnapshot?.phase || run?.status);
}

function allowedNextPhaseValues({ run, latestSnapshot }) {
  const explicit = uniqueCompact([
    ...toArray(run?.allowed_next_phases || run?.allowedNextPhases, "allowed_next_phases"),
    ...(latestSnapshot?.allowedNextPhases || []),
  ]);
  if (explicit.length) return { values: explicit, source: "runtime" };
  const phase = normalizeKey(runtimePhaseValue(run, latestSnapshot));
  return { values: FALLBACK_ALLOWED_NEXT_PHASES[phase] || [], source: "fallback" };
}

function projectAllowedNextPhases({ run, latestSnapshot }) {
  const phase = runtimePhaseValue(run, latestSnapshot);
  const { values, source } = allowedNextPhaseValues({ run, latestSnapshot });
  if (!values.length && phase) {
    return [
      {
        id: `terminal:${normalizeKey(phase)}`,
        label: "No legal transitions",
        status: "terminal",
        statusGroup: "completed",
        detail: `${titleCase(phase)} is terminal or no transition data was reported.`,
      },
    ];
  }
  return values.map((value) => ({
    id: `next-phase:${normalizeKey(value)}`,
    label: titleCase(value),
    status: source === "runtime" ? "available" : "fallback",
    statusGroup: source === "runtime" ? "active" : "queued",
    detail: source === "runtime" ? "reported by allowed_next_phases" : `fallback from ${titleCase(phase || "unknown")}`,
  }));
}

function projectPhaseHistory({ run, latestSnapshot }) {
  const runHistory = phaseHistoryRows(run?.phase_history || run?.phaseHistory);
  if (runHistory.length) return runHistory.slice().reverse();
  return (latestSnapshot?.phaseHistory || []).slice().reverse();
}

function constraintRecord(row, fallbackKind, index) {
  const kind = stringValue(row?.kind || row?.type || row?.constraint_type || row?.constraintType, fallbackKind);
  const id = recordId(
    row,
    [
      "lock_id",
      "lockId",
      "claim_id",
      "claimId",
      "constraint_id",
      "constraintId",
      "lease_id",
      "leaseId",
      "resource_key",
      "resourceKey",
      "workspace_id",
      "workspaceId",
    ],
    `${kind}-${index}`
  );
  return {
    id: `${kind}:${id}`,
    label: titleCase(row?.label || row?.name || kind),
    status: stringValue(row?.status || row?.state || row?.effect, kind),
    statusGroup: "waiting",
    detail: compact([
      row?.resource_key || row?.resourceKey,
      row?.workspace_id || row?.workspaceId,
      row?.claimant_id || row?.claimantId || row?.claimed_by || row?.claimedBy,
      row?.claim_expires_at_ms || row?.claimExpiresAtMs || row?.expires_at_ms || row?.expiresAtMs,
      row?.reason,
    ]).join(" · "),
  };
}

function pushConstraint(rows, row) {
  if (!row?.id || rows.some((existing) => existing.id === row.id)) return;
  rows.push(row);
}

function scopeConstraintRows(run) {
  const rows = [];
  const scope = objectValue(run?.scope || run?.enterprise_scope || run?.enterpriseScope);
  const tenant = objectValue(scope.tenant_context || scope.tenantContext);
  const resourceScope = objectValue(scope.resource_scope || scope.resourceScope);
  const root = objectValue(resourceScope.root || scope.root_resource || scope.rootResource);
  const tenantDetail = compact([tenant.org_id || tenant.orgId, tenant.workspace_id || tenant.workspaceId, tenant.deployment_id || tenant.deploymentId]).join(
    " / "
  );
  if (tenantDetail) {
    rows.push({
      id: `tenant:${tenantDetail}`,
      label: "Tenant workspace",
      status: "scoped",
      statusGroup: "active",
      detail: tenantDetail,
    });
  }
  const resource = resourceLabel(root.resource_kind || root.resourceKind, root.resource_id || root.resourceId);
  if (resource) {
    rows.push({
      id: `resource:${resource}`,
      label: "Resource scope",
      status: "constrained",
      statusGroup: "waiting",
      detail: resource,
    });
  }
  const policy = stringValue(scope.policy_version_id || scope.policyVersionId);
  const dataClasses = toArray(scope.data_classes || scope.dataClasses, "data_classes");
  if (policy || dataClasses.length) {
    rows.push({
      id: `governance:${policy || dataClasses.join(":")}`,
      label: "Governance scope",
      status: "constrained",
      statusGroup: "waiting",
      detail: compact([policy ? `Policy ${policy}` : "", dataClasses.join(", ")]).join(" · "),
    });
  }
  return rows;
}

function projectLockConstraints({ input, run, latestSnapshot, currentWaitRow }) {
  const rows = [];
  for (const row of [
    ...toArray(input?.locks, "locks"),
    ...toArray(input?.held_locks || input?.heldLocks, "held_locks"),
    ...toArray(input?.lock_constraints || input?.lockConstraints, "lock_constraints"),
    ...toArray(input?.workspace_constraints || input?.workspaceConstraints, "workspace_constraints"),
    ...toArray(run?.locks, "locks"),
    ...toArray(run?.held_locks || run?.heldLocks, "held_locks"),
    ...toArray(run?.workspace_constraints || run?.workspaceConstraints, "workspace_constraints"),
  ]) {
    pushConstraint(rows, constraintRecord(row, "constraint", rows.length));
  }

  const checkpoint = checkpointOf(latestSnapshot);
  const executionClaim = checkpoint.execution_claim || checkpoint.executionClaim;
  if (executionClaim && typeof executionClaim === "object") {
    pushConstraint(rows, {
      id: `execution-claim:${executionClaim.claim_id || executionClaim.claimId || "active"}`,
      label: "Execution claim",
      status: "claimed",
      statusGroup: "waiting",
      detail: compact([
        executionClaim.claimant_id || executionClaim.claimantId,
        executionClaim.claim_id || executionClaim.claimId,
        executionClaim.claim_expires_at_ms || executionClaim.claimExpiresAtMs,
      ]).join(" · "),
    });
  } else if (checkpoint.execution_claim_epoch || checkpoint.executionClaimEpoch) {
    pushConstraint(rows, {
      id: `execution-claim-epoch:${checkpoint.execution_claim_epoch || checkpoint.executionClaimEpoch}`,
      label: "Execution claim epoch",
      status: "tracked",
      statusGroup: "active",
      detail: String(checkpoint.execution_claim_epoch || checkpoint.executionClaimEpoch),
    });
  }

  const activeSessions = toArray(checkpoint.active_session_ids || checkpoint.activeSessionIds, "active_session_ids");
  const activeInstances = toArray(checkpoint.active_instance_ids || checkpoint.activeInstanceIds, "active_instance_ids");
  if (activeSessions.length || activeInstances.length) {
    pushConstraint(rows, {
      id: `active-execution:${activeSessions.join(":") || activeInstances.join(":")}`,
      label: "Active execution",
      status: "held",
      statusGroup: "active",
      detail: compact([
        activeSessions.length ? `sessions ${activeSessions.join(", ")}` : "",
        activeInstances.length ? `instances ${activeInstances.join(", ")}` : "",
      ]).join(" · "),
    });
  }

  if (currentWaitRow?.raw?.claimed_by || currentWaitRow?.raw?.claimedBy || currentWaitRow?.raw?.claim_expires_at_ms) {
    pushConstraint(rows, {
      id: `wait-claim:${currentWaitRow.id}`,
      label: "Wait claim",
      status: stringValue(currentWaitRow.raw.status, "claimed"),
      statusGroup: "waiting",
      detail: compact([
        currentWaitRow.raw.claimed_by || currentWaitRow.raw.claimedBy,
        currentWaitRow.raw.claim_expires_at_ms || currentWaitRow.raw.claimExpiresAtMs,
      ]).join(" · "),
    });
  }

  for (const row of scopeConstraintRows(run)) pushConstraint(rows, row);
  return rows;
}

function replayUnsafeReasons({ operator, reliability }) {
  const reasons = [];
  for (const reason of toArray(operator.blocking_reasons || operator.blockingReasons, "blocking_reasons")) {
    const projected = projectOperatorItem(reason, "blocked");
    reasons.push(compact([projected.label, projected.detail]).join(": "));
  }
  for (const row of toArray(reliability.tool_effects || reliability.toolEffects, "tool_effects")) {
    const status = normalizeKey(row?.status);
    if (["failed", "unknown"].includes(status)) reasons.push(`Tool effect ${recordId(row, ["effect_id", "effectId"]) || "unknown"} is ${status}`);
  }
  for (const row of toArray(reliability.outbox, "outbox")) {
    const status = normalizeKey(row?.status);
    if (["pending", "failed", "dead_lettered"].includes(status)) {
      reasons.push(`Outbox ${recordId(row, ["outbox_id", "outboxId"]) || "unknown"} is ${status}`);
    }
  }
  for (const row of toArray(reliability.dead_letters || reliability.deadLetters, "dead_letters")) {
    const status = normalizeKey(row?.status);
    if (status === "open") reasons.push(`Dead letter ${recordId(row, ["dead_letter_id", "deadLetterId"]) || "unknown"} is open`);
  }
  return uniqueCompact(reasons);
}

function buildRunObservabilityDetail(payload = {}) {
  const input = payload && typeof payload === "object" ? payload : {};
  const run = input.run && typeof input.run === "object" ? input.run : {};
  const reliability = input.reliability && typeof input.reliability === "object" ? input.reliability : {};
  const operator = input.operator_summary || input.operatorSummary || {};
  const eventsBlock = input.events && typeof input.events === "object" && !Array.isArray(input.events) ? input.events : {};
  const snapshotsBlock =
    input.snapshots && typeof input.snapshots === "object" && !Array.isArray(input.snapshots) ? input.snapshots : {};
  const audit = input.audit && typeof input.audit === "object" ? input.audit : {};
  const waits = toArray(input.waits, "waits").map(projectObservabilityWait);
  const currentWait = input.current_wait || input.currentWait;
  const currentWaitRow = currentWait ? projectObservabilityWait(currentWait) : waits.find((wait) => normalizeKey(wait.status) === "waiting") || null;
  const events = toArray(eventsBlock.tail || eventsBlock.items || input.event_tail || input.eventTail, "tail").map(projectObservabilityEvent);
  const snapshots = toArray(snapshotsBlock.items || input.snapshots, "items").map(projectObservabilitySnapshot);
  const policyDecisions = toArray(input.policy_decisions || input.policyDecisions, "policy_decisions").map(projectPolicyDecision);
  const outbox = toArray(reliability.outbox, "outbox").map((row) => projectReliabilityRecord(row, "outbox"));
  const toolEffects = toArray(reliability.tool_effects || reliability.toolEffects, "tool_effects").map((row) =>
    projectReliabilityRecord(row, "tool_effect")
  );
  const deadLetters = toArray(reliability.dead_letters || reliability.deadLetters, "dead_letters").map((row) =>
    projectReliabilityRecord(row, "dead_letter")
  );
  const compensations = toArray(reliability.compensations, "compensations").map((row) =>
    projectReliabilityRecord(row, "compensation")
  );
  const protectedAuditEvents = toArray(audit.protected_events || audit.protectedEvents, "protected_events").map(projectAuditEvent);
  const blockingReasons = toArray(operator.blocking_reasons || operator.blockingReasons, "blocking_reasons").map((row) =>
    projectOperatorItem(row, "blocked")
  );
  const allowedActions = toArray(operator.allowed_actions || operator.allowedActions, "allowed_actions").map((row) =>
    projectOperatorItem(row, "action")
  );
  const latestEvent = eventsBlock.latest ? projectObservabilityEvent(eventsBlock.latest) : events[events.length - 1] || null;
  const latestSnapshotSummary = snapshotsBlock.latest ? projectObservabilitySnapshot(snapshotsBlock.latest) : null;
  const latestSnapshot =
    (latestSnapshotSummary && snapshots.find((snapshot) => snapshot.id === latestSnapshotSummary.id)) ||
    snapshots[snapshots.length - 1] ||
    latestSnapshotSummary ||
    null;
  const unsafeReasons = replayUnsafeReasons({ operator, reliability });
  const eventSeqs = events.map((event) => event.seq).filter((seq) => seq > 0);
  const counts = input.counts && typeof input.counts === "object" ? input.counts : {};
  const runStatus = stringValue(run.status, "unknown");
  const runtimePhase = runtimePhaseValue(run, latestSnapshot);
  const snapshotDiffs = buildSnapshotDiffRows(snapshots);
  const resumeReason = resumeReasonFrom({ run, latestSnapshot, snapshots });
  const recoverySnapshotDiff = crashRecoverySnapshotDiff(snapshotDiffs, resumeReason);

  return {
    runId: stringValue(input.run_id || input.runId || run.run_id || run.runId),
    source: stringValue(input.source, "stateful_runtime_observability"),
    status: runStatus,
    statusLabel: titleCase(runStatus),
    phase: titleCase(
      run.current_phase_id ||
        run.currentPhaseId ||
        run.phase?.phase_id ||
        run.phase?.phaseId ||
        run.phase?.name ||
        run.phase?.status ||
        runtimePhase ||
        runStatus
    ),
    runtimePhase: titleCase(runtimePhase || runStatus),
    kind: titleCase(run.kind || "workflow"),
    workflowDefinitionVersion: stringValue(
      run.workflow_definition_version || run.workflowDefinitionVersion || latestSnapshot?.workflowDefinitionVersion
    ),
    workflowDefinitionSnapshotHash: stringValue(
      run.workflow_definition_snapshot_hash || run.workflowDefinitionSnapshotHash || latestSnapshot?.workflowDefinitionSnapshotHash
    ),
    currentWait: currentWaitRow,
    waits,
    events,
    latestEvent,
    snapshots,
    latestSnapshot,
    snapshotDiffs,
    crashRecoverySnapshotDiff: recoverySnapshotDiff,
    policyDecisions,
    outbox,
    toolEffects,
    deadLetters,
    compensations,
    protectedAuditEvents,
    blockingReasons,
    allowedActions,
    allowedNextPhases: projectAllowedNextPhases({ run, latestSnapshot }),
    phaseHistory: projectPhaseHistory({ run, latestSnapshot }),
    lockConstraints: projectLockConstraints({ input, run, latestSnapshot, currentWaitRow }),
    resumeReason,
    isBlocked: Boolean(operator.is_blocked || operator.isBlocked || blockingReasons.length),
    counts: {
      waits: numberValue(counts.waits, waits.length),
      activeWaits: numberValue(counts.active_waits ?? counts.activeWaits, currentWaitRow ? 1 : 0),
      policyDecisions: numberValue(counts.policy_decisions ?? counts.policyDecisions, policyDecisions.length),
      outbox: numberValue(counts.outbox, outbox.length),
      toolEffects: numberValue(counts.tool_effects ?? counts.toolEffects, toolEffects.length),
      deadLetters: numberValue(counts.dead_letters ?? counts.deadLetters, deadLetters.length),
      compensations: numberValue(counts.compensations, compensations.length),
      protectedAuditEvents: numberValue(
        counts.protected_audit_events ?? counts.protectedAuditEvents,
        protectedAuditEvents.length
      ),
      events: numberValue(counts.events, events.length),
      snapshots: numberValue(counts.snapshots, snapshots.length),
    },
    replay: {
      eventCount: events.length,
      firstSeq: eventSeqs.length ? Math.min(...eventSeqs) : 0,
      lastSeq: eventSeqs.length ? Math.max(...eventSeqs) : 0,
      latestEventLabel: latestEvent?.label || "",
      latestSnapshotLabel: latestSnapshot?.label || "",
      unsafeReasons,
      isUnsafe: unsafeReasons.length > 0,
    },
    payloadPolicy: stringValue(audit.payload_policy || audit.payloadPolicy),
  };
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
  const enterprise = enterpriseScopeOf(row);

  return {
    id,
    canonicalId: canonicalRunId(id),
    observabilityRunId: id,
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
    orgUnitId: enterprise.orgUnitId,
    orgUnitName: enterprise.orgUnitName,
    resourceKind: enterprise.resourceKind,
    resourceId: enterprise.resourceId,
    resourceLabel: enterprise.resourceLabel,
    policyVersion: enterprise.policyVersion,
    dataClasses: enterprise.dataClasses,
    delegationGrantIds: enterprise.delegationGrantIds,
    ownerPrincipal: enterprise.ownerPrincipal,
    knowledgeSourceIds: enterprise.knowledgeSourceIds,
    knowledgeSourceLabels: enterprise.knowledgeSourceLabels,
    knowledgeSourceCount: enterprise.knowledgeSourceCount,
    scopeSummary: enterprise.scopeSummary,
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

function sourceFromStatefulKind(kind) {
  const normalized = normalizeKey(kind);
  if (normalized === "context_run") return "context";
  if (normalized === "workflow") return "workflow";
  return "workflow";
}

function projectCanonicalRun(row) {
  const run = row?.run && typeof row.run === "object" ? row.run : row;
  if (!run || typeof run !== "object") return null;
  const source = sourceFromStatefulKind(run.kind);
  return projectRun(
    {
      ...run,
      current_wait: row?.current_wait || row?.currentWait,
      latest_event: row?.latest_event || row?.latestEvent,
      latest_snapshot: row?.latest_snapshot || row?.latestSnapshot,
      replay_boundaries: row?.replay_boundaries || row?.replayBoundaries,
      enterprise_scope: row?.enterprise_scope || row?.enterpriseScope,
    },
    source
  );
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
  for (const key of [
    "workspace",
    "currentWait",
    "waitDetail",
    "retryDetail",
    "tenantActor",
    "tenantDeployment",
    "orgUnitId",
    "orgUnitName",
    "resourceKind",
    "resourceId",
    "resourceLabel",
    "policyVersion",
    "ownerPrincipal",
    "scopeSummary",
    "observabilityRunId",
  ]) {
    if (!primary[key] && secondary[key]) primary[key] = secondary[key];
  }
  for (const key of ["dataClasses", "delegationGrantIds", "knowledgeSourceIds", "knowledgeSourceLabels"]) {
    if (!primary[key]?.length && secondary[key]?.length) primary[key] = secondary[key];
  }
  if (!primary.knowledgeSourceCount && secondary.knowledgeSourceCount) {
    primary.knowledgeSourceCount = secondary.knowledgeSourceCount;
  }
  if (primary.tenantOrg === "local" && secondary.tenantOrg !== "local") primary.tenantOrg = secondary.tenantOrg;
  if (primary.tenantWorkspace === "local" && secondary.tenantWorkspace !== "local") {
    primary.tenantWorkspace = secondary.tenantWorkspace;
  }
  return primary;
}

function buildStatefulRunRows(input = {}) {
  const rows = [
    ...toArray(input.statefulRuns || input.canonicalRuns || input.runs, "runs").map((row) =>
      projectCanonicalRun(row)
    ),
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
    orgUnit: stringValue(input.orgUnit || input.org_unit),
    resource: stringValue(input.resource),
    policy: stringValue(input.policy || input.policyVersion || input.policy_version),
    dataClass: stringValue(input.dataClass || input.data_class),
    knowledge: stringValue(input.knowledge || input.sourceBinding || input.source_binding),
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
    row.orgUnitId,
    row.orgUnitName,
    row.resourceKind,
    row.resourceId,
    row.resourceLabel,
    row.policyVersion,
    row.dataClasses,
    row.delegationGrantIds,
    row.ownerPrincipal,
    row.knowledgeSourceIds,
    row.knowledgeSourceLabels,
    row.scopeSummary,
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
  const orgUnit = normalized.orgUnit.toLowerCase();
  const resource = normalized.resource.toLowerCase();
  const policy = normalized.policy.toLowerCase();
  const dataClass = normalized.dataClass.toLowerCase();
  const knowledge = normalized.knowledge.toLowerCase();
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
      orgUnit &&
      !compact([row.orgUnitId, row.orgUnitName, row.ownerPrincipal])
        .join(" ")
        .toLowerCase()
        .includes(orgUnit)
    ) {
      return false;
    }
    if (
      resource &&
      !compact([row.resourceKind, row.resourceId, row.resourceLabel, row.scopeSummary])
        .join(" ")
        .toLowerCase()
        .includes(resource)
    ) {
      return false;
    }
    if (policy && !stringValue(row.policyVersion).toLowerCase().includes(policy)) return false;
    if (dataClass && !compact(row.dataClasses || []).join(" ").toLowerCase().includes(dataClass)) return false;
    if (
      knowledge &&
      !compact([row.knowledgeSourceIds, row.knowledgeSourceLabels])
        .join(" ")
        .toLowerCase()
        .includes(knowledge)
    ) {
      return false;
    }
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
    orgUnits: 0,
    policyVersions: 0,
    knowledgeSources: 0,
  };
  const tenants = new Set();
  const workspaces = new Set();
  const orgUnits = new Set();
  const policyVersions = new Set();
  const knowledgeSources = new Set();
  for (const row of Array.isArray(rows) ? rows : []) {
    summary.total += 1;
    if (Object.prototype.hasOwnProperty.call(summary, row.statusGroup)) {
      summary[row.statusGroup] += 1;
    }
    tenants.add(`${row.tenantOrg}/${row.tenantWorkspace}`);
    if (row.workspace) workspaces.add(row.workspace);
    if (row.orgUnitId || row.orgUnitName) orgUnits.add(row.orgUnitId || row.orgUnitName);
    if (row.policyVersion) policyVersions.add(row.policyVersion);
    for (const source of [...(row.knowledgeSourceIds || []), ...(row.knowledgeSourceLabels || [])]) {
      if (source) knowledgeSources.add(source);
    }
  }
  summary.tenants = tenants.size;
  summary.workspaces = workspaces.size;
  summary.orgUnits = orgUnits.size;
  summary.policyVersions = policyVersions.size;
  summary.knowledgeSources = knowledgeSources.size;
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
  buildRunObservabilityDetail,
  buildStatefulRunRows,
  filterStatefulRunRows,
  formatRunTimestamp,
  normalizeStatefulRunFilters,
  summarizeStatefulRuns,
};
