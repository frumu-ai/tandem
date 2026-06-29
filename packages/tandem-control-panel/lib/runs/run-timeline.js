const DEFAULT_TIMELINE_LIMIT = 250;

const SUCCESS_WORDS = ["completed", "succeeded", "approved", "executed", "recorded", "saved"];
const FAILURE_WORDS = ["failed", "error", "denied", "rejected", "timeout", "dead_letter"];
const RUNNING_WORDS = ["started", "running", "queued", "pending", "awaiting", "waiting", "sleeping"];

function asObject(value) {
  return value && typeof value === "object" && !Array.isArray(value) ? value : {};
}

function compactString(value) {
  if (value == null) return "";
  if (typeof value === "string") return value.trim();
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return "";
}

function firstString(...values) {
  for (const value of values) {
    const text = compactString(value);
    if (text) return text;
  }
  return "";
}

function toNumber(value) {
  if (typeof value === "number") return Number.isFinite(value) ? value : 0;
  const text = compactString(value);
  if (!text) return 0;
  const numeric = Number(text);
  return Number.isFinite(numeric) ? numeric : 0;
}

function timestampMs(value) {
  const numeric = toNumber(value);
  if (numeric > 0) return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric;
  const parsed = Date.parse(compactString(value));
  return Number.isFinite(parsed) ? parsed : 0;
}

function titleCase(value) {
  return compactString(value)
    .replace(/[_./:-]+/g, " ")
    .replace(/\b\w/g, (match) => match.toUpperCase())
    .trim();
}

function actorLabel(actor) {
  if (!actor) return "";
  if (typeof actor === "string") return actor.trim();
  const row = asObject(actor);
  return firstString(
    row.display_name,
    row.displayName,
    row.name,
    row.email,
    row.user_id,
    row.userId,
    row.principal_id,
    row.principalId,
    row.id,
    row.kind && row.id ? `${row.kind}:${row.id}` : ""
  );
}

function tenantScope(raw, payload) {
  const row = asObject(raw);
  const body = asObject(payload);
  const candidates = [
    row.tenant_context,
    row.tenantContext,
    row.scope?.tenant_context,
    row.scope?.tenantContext,
    body.tenant_context,
    body.tenantContext,
  ];
  const tenant = candidates.map(asObject).find((candidate) => Object.keys(candidate).length) || {};
  return {
    orgId: firstString(tenant.org_id, tenant.orgId, tenant.organization_id, tenant.organizationId),
    workspaceId: firstString(tenant.workspace_id, tenant.workspaceId),
    deploymentId: firstString(tenant.deployment_id, tenant.deploymentId),
  };
}

function eventPayload(raw) {
  const row = asObject(raw);
  const payload = asObject(row.payload);
  return Object.keys(payload).length ? payload : asObject(row.properties);
}

function envelope(raw) {
  return asObject(asObject(raw).envelope);
}

function eventType(raw, payload) {
  const row = asObject(raw);
  return firstString(row.event_type, row.eventType, row.type, row.event, payload.event_type, payload.type);
}

function eventTone(type, payload) {
  const haystack = `${type} ${compactString(payload.status)} ${compactString(payload.decision)} ${compactString(
    payload.error
  )}`.toLowerCase();
  if (FAILURE_WORDS.some((word) => haystack.includes(word))) return "failed";
  if (SUCCESS_WORDS.some((word) => haystack.includes(word))) return "ok";
  if (RUNNING_WORDS.some((word) => haystack.includes(word))) return "running";
  return "info";
}

function summarizePayload(type, payload) {
  const row = asObject(payload);
  const direct = firstString(
    row.summary,
    row.message,
    row.detail,
    row.reason,
    row.status,
    row.decision,
    row.error?.message,
    row.error,
    row.tool,
    row.tool_name,
    row.action,
    row.action_type,
    row.wait_kind,
    row.phase,
    row.phase_id
  );
  if (direct) return direct.slice(0, 240);

  const keys = Object.keys(row).filter((key) => !["tenantContext", "tenant_context"].includes(key));
  if (!keys.length) return type || "Runtime event";
  const preview = {};
  for (const key of keys.slice(0, 4)) preview[key] = row[key];
  try {
    return JSON.stringify(preview).slice(0, 240);
  } catch {
    return keys.slice(0, 4).join(", ");
  }
}

function eventTitle(type) {
  const text = titleCase(type);
  return text || "Runtime Event";
}

function sourceLabel(source) {
  const normalized = compactString(source).toLowerCase();
  if (normalized === "runtime") return "runtime";
  if (normalized === "stateful") return "stateful";
  if (normalized === "context") return "context";
  if (normalized === "automation" || normalized === "automations") return "automation";
  if (normalized === "live" || normalized === "sse") return "live";
  return normalized || "runtime";
}

function dedupeKeyFor(row) {
  if (row.eventId) return `event:${row.eventId}`;
  if (row.sequence > 0) return `seq:${row.runId || "unknown"}:${row.sequence}`;
  if (row.rawId) return `${row.source}:raw:${row.rawId}`;
  return `${row.source}:${row.runId}:${row.eventType}:${row.occurredAtMs}:${row.rowIndex}`;
}

function entryScore(row) {
  let score = 0;
  if (row.eventId) score += 8;
  if (row.sequence > 0) score += 4;
  if (row.source === "runtime" || row.source === "stateful") score += 3;
  if (row.actor) score += 1;
  if (row.phase) score += 1;
  return score;
}

function normalizeRunTimelineEntry(raw, options = {}) {
  const row = asObject(raw);
  const payload = eventPayload(row);
  const env = envelope(row);
  const type = eventType(row, payload);
  const source = sourceLabel(options.source || row.source || payload.source);
  const sequence = toNumber(row.seq ?? row.sequence ?? row.sequence_number ?? env.seq ?? payload.seq);
  const occurredAtMs = timestampMs(
    row.occurred_at_ms ??
      row.occurredAtMs ??
      row.timestamp_ms ??
      row.timestampMs ??
      row.created_at_ms ??
      row.createdAtMs ??
      env.occurred_at_ms ??
      env.occurredAtMs ??
      payload.timestamp_ms ??
      payload.timestampMs
  );
  const tenant = tenantScope(row, payload);
  const eventId = firstString(row.event_id, row.eventId, env.event_id, env.eventId, payload.event_id);
  const runId = firstString(
    row.run_id,
    row.runId,
    row.runID,
    env.run_id,
    env.runId,
    payload.run_id,
    payload.runId,
    payload.runID,
    options.fallbackRunId
  );
  const actor = actorLabel(row.actor || payload.actor || payload.principal || payload.user);
  const phase = firstString(row.phase_id, row.phaseId, payload.phase_id, payload.phaseId, payload.phase);
  const sessionId = firstString(row.session_id, row.sessionId, env.session_id, env.sessionId, payload.sessionID);
  const nodeId = firstString(row.node_id, row.nodeId, env.node_id, env.nodeId, payload.nodeID, payload.node_id);

  const entry = {
    id: "",
    dedupeKey: "",
    eventId,
    rawId: firstString(row.id, payload.id),
    runId,
    source,
    eventType: type,
    title: eventTitle(type),
    summary: summarizePayload(type, payload),
    tone: eventTone(type, payload),
    actor,
    phase,
    sequence,
    occurredAtMs,
    sessionId,
    nodeId,
    tenantOrg: tenant.orgId,
    tenantWorkspace: tenant.workspaceId,
    tenantDeployment: tenant.deploymentId,
    payload,
    rowIndex: options.rowIndex || 0,
    pageIndex: options.pageIndex || 0,
  };
  entry.dedupeKey = dedupeKeyFor(entry);
  entry.id = entry.eventId || entry.dedupeKey;
  return entry;
}

function normalizeRunTimelinePage(page, options = {}) {
  const payload = asObject(page);
  const events = Array.isArray(page)
    ? page
    : Array.isArray(payload.events)
      ? payload.events
      : Array.isArray(payload.rows)
        ? payload.rows
        : [];
  return events.map((event, rowIndex) =>
    normalizeRunTimelineEntry(event, {
      ...options,
      fallbackRunId: options.fallbackRunId || payload.run_id || payload.runId,
      rowIndex,
    })
  );
}

function flattenTimelineInputs(collections) {
  return collections.flatMap((collection) => {
    if (!collection) return [];
    if (Array.isArray(collection)) return collection;
    if (Array.isArray(collection.entries)) return collection.entries;
    return [collection];
  });
}

function compareTimelineEntries(left, right) {
  if (left.occurredAtMs && right.occurredAtMs && left.occurredAtMs !== right.occurredAtMs) {
    return left.occurredAtMs - right.occurredAtMs;
  }
  if (left.sequence && right.sequence && left.sequence !== right.sequence) {
    return left.sequence - right.sequence;
  }
  if (left.pageIndex !== right.pageIndex) return left.pageIndex - right.pageIndex;
  if (left.rowIndex !== right.rowIndex) return left.rowIndex - right.rowIndex;
  return left.id.localeCompare(right.id);
}

function mergeRunTimelineEntries(...collections) {
  const byKey = new Map();
  for (const row of flattenTimelineInputs(collections)) {
    const entry = row?.dedupeKey ? row : normalizeRunTimelineEntry(row);
    const existing = byKey.get(entry.dedupeKey);
    if (!existing || entryScore(entry) >= entryScore(existing)) {
      byKey.set(entry.dedupeKey, entry);
    }
  }
  return Array.from(byKey.values()).sort(compareTimelineEntries);
}

function buildRunTimeline({ persistedPages = [], liveEvents = [], existingEntries = [], limit } = {}) {
  const persisted = persistedPages.flatMap((page, pageIndex) =>
    normalizeRunTimelinePage(page, { source: "runtime", pageIndex })
  );
  const live = liveEvents.map((event, rowIndex) =>
    normalizeRunTimelineEntry(event, { source: "live", rowIndex })
  );
  const merged = mergeRunTimelineEntries(existingEntries, persisted, live);
  return Number.isFinite(limit) && limit > 0 ? merged.slice(-limit) : merged;
}

function appendRunTimelineLiveEvent(existingEntries, event, options = {}) {
  return mergeRunTimelineEntries(
    existingEntries,
    normalizeRunTimelineEntry(event, { ...options, source: options.source || "live" })
  );
}

function nextRunTimelineAfterSeq(entries) {
  return mergeRunTimelineEntries(entries).reduce(
    (max, entry) => Math.max(max, toNumber(entry.sequence)),
    0
  );
}

function runTimelineRequestPath(runId, options = {}) {
  const encoded = encodeURIComponent(compactString(runId));
  const params = new URLSearchParams();
  const afterSeq = toNumber(options.afterSeq ?? options.after_seq);
  const beforeSeq = toNumber(options.beforeSeq ?? options.before_seq);
  const limit = toNumber(options.limit) || DEFAULT_TIMELINE_LIMIT;
  if (afterSeq > 0) params.set("after_seq", String(afterSeq));
  if (beforeSeq > 0) params.set("before_seq", String(beforeSeq));
  params.set("limit", String(limit));
  if (options.tail) params.set("tail", String(toNumber(options.tail) || limit));
  return `/api/engine/stateful-runtime/runs/${encoded}/events?${params.toString()}`;
}

export {
  DEFAULT_TIMELINE_LIMIT,
  appendRunTimelineLiveEvent,
  buildRunTimeline,
  mergeRunTimelineEntries,
  nextRunTimelineAfterSeq,
  normalizeRunTimelineEntry,
  normalizeRunTimelinePage,
  runTimelineRequestPath,
};
