export type ToolActivityStatus = "started" | "completed" | "failed";

export type ToolActivityItem = {
  id: string;
  tool: string;
  status: ToolActivityStatus;
  at: number;
};

export type PackEventItem = {
  id: string;
  type: string;
  path: string;
  attachmentId: string;
  connector: string;
  channelId: string;
  senderId: string;
  error: string;
  summary: string;
  at: number;
};

export type PermissionRequest = {
  id: string;
  tool: string;
  permission: string;
  pattern: string;
  sessionId: string;
  status: string;
};

export const TOOL_START_EVENTS = new Set(["tool.called", "tool_call.started", "session.tool_call"]);
export const TOOL_END_EVENTS = new Set([
  "tool.result",
  "tool_call.completed",
  "tool_call.failed",
  "session.tool_result",
]);
export const TERMINAL_SUCCESS_EVENTS = new Set([
  "run.complete",
  "run.completed",
  "session.run.finished",
  "session.run.completed",
]);
export const TERMINAL_FAILURE_EVENTS = new Set([
  "run.failed",
  "session.run.failed",
  "run.cancelled",
  "run.canceled",
  "session.run.cancelled",
  "session.run.canceled",
]);

function normalizeToolName(name: string): string {
  return String(name || "")
    .trim()
    .replace(/\s+/g, " ")
    .replace(/[<>]/g, "");
}

export function extractToolName(payload: any): string {
  const source = payload || {};
  const nested = source.call || source.toolCall || source.part || {};
  return normalizeToolName(
    source.tool ||
      source.name ||
      source.toolName ||
      source.tool_id ||
      source.toolID ||
      nested.tool ||
      nested.name ||
      nested.toolName ||
      ""
  );
}

export function extractToolCallId(payload: any): string {
  const source = payload || {};
  const nested = source.call || source.toolCall || source.part || {};
  return String(
    source.callID ||
      source.toolCallID ||
      source.tool_call_id ||
      source.id ||
      nested.callID ||
      nested.toolCallID ||
      nested.tool_call_id ||
      nested.id ||
      ""
  ).trim();
}

export function extractRunId(event: any, fallback = ""): string {
  const props = event?.properties || {};
  return String(
    event?.runId ||
      event?.runID ||
      event?.run_id ||
      props.runID ||
      props.runId ||
      props.run_id ||
      props.run?.id ||
      fallback
  ).trim();
}

export function normalizePermissionRequest(raw: any): PermissionRequest | null {
  if (!raw) return null;
  const nested = raw.request || raw.approval || raw.permission || {};
  const id = String(
    raw.id ||
      raw.requestID ||
      raw.requestId ||
      raw.approvalID ||
      nested.id ||
      nested.requestID ||
      ""
  ).trim();
  if (!id) return null;
  return {
    id,
    tool: normalizeToolName(raw.tool || nested.tool || nested.name || "tool") || "tool",
    permission: String(raw.permission || nested.permission || "").trim(),
    pattern: String(raw.pattern || nested.pattern || "").trim(),
    sessionId: String(
      raw.sessionId || raw.sessionID || raw.session_id || nested.sessionId || nested.sessionID || ""
    ).trim(),
    status: String(raw.status || nested.status || "")
      .trim()
      .toLowerCase(),
  };
}

export function isPendingPermissionStatus(statusRaw: string): boolean {
  const status = String(statusRaw || "")
    .trim()
    .toLowerCase();
  if (!status) return true;
  if (
    status.includes("approved") ||
    status.includes("rejected") ||
    status.includes("denied") ||
    status.includes("resolved") ||
    status.includes("expired") ||
    status.includes("cancel") ||
    status.includes("complete") ||
    status.includes("done") ||
    status.includes("timeout")
  ) {
    return false;
  }
  return (
    status.includes("pending") ||
    status.includes("request") ||
    status.includes("ask") ||
    status.includes("await") ||
    status.includes("open") ||
    status.includes("queue") ||
    status.includes("new") ||
    status.includes("progress") ||
    status === "unknown"
  );
}

export function normalizePackEvent(rawType: string, rawProps: any): PackEventItem {
  const props = rawProps && typeof rawProps === "object" ? rawProps : {};
  const type = String(rawType || "").trim() || "pack.event";
  const path = String(props.path || "").trim();
  const attachmentId = String(props.attachment_id || props.attachmentId || "").trim();
  const connector = String(props.connector || "").trim();
  const channelId = String(props.channel_id || props.channelId || "").trim();
  const senderId = String(props.sender_id || props.senderId || "").trim();
  const name = String(props.name || "").trim();
  const version = String(props.version || "").trim();
  const error = String(props.error || "").trim();
  const detailBits = [];
  if (name) detailBits.push(name);
  if (version) detailBits.push(version);
  if (path) detailBits.push(path);
  if (connector) detailBits.push(connector);
  if (channelId) detailBits.push(`channel=${channelId}`);
  if (senderId) detailBits.push(`sender=${senderId}`);
  const summary = detailBits.join(" · ");

  return {
    id: `${type}:${attachmentId || path || name || "event"}`,
    type,
    path,
    attachmentId,
    connector,
    channelId,
    senderId,
    error,
    summary: summary || type,
    at: Date.now(),
  };
}
