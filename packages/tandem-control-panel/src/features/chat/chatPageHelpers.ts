import {
  extractToolCallId,
  extractToolName,
  summarizeToolPayload,
  type ToolActivityItem,
  type ToolActivityStatus,
} from "./streaming";

const CHAT_AUTO_APPROVE_KEY = "tandem_control_panel_chat_auto_approve_tools";
const CHAT_TOOL_ALLOWLIST_KEY = "tandem_control_panel_chat_tool_allowlist";
const AUTOMATION_PLANNER_SEED_KEY = "tandem.automations.plannerSeed";
const WORKFLOW_PLANNER_SEED_KEY = "tandem.workflow.plannerSeed";
const EXT_MIME: Record<string, string> = {
  md: "text/markdown",
  txt: "text/plain",
  csv: "text/csv",
  json: "application/json",
  pdf: "application/pdf",
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
};

export type UploadFile = {
  name: string;
  path: string;
  size: number;
  mime?: string;
  url?: string;
};

export type UploadProgressRow = {
  id: string;
  name: string;
  progress: number;
  error: string;
};

export type ConfirmDeleteState = { id: string; title: string } | null;
export type RunTimelineTone = "running" | "ok" | "failed" | "info";

export type RunTimelineItem = {
  id: string;
  type: string;
  title: string;
  summary: string;
  tone: RunTimelineTone;
  at: number;
  runId: string;
};

type SetupDecision = "pass_through" | "intercept" | "clarify";
type SetupIntentKind =
  | "provider_setup"
  | "integration_setup"
  | "automation_create"
  | "workflow_planner_create"
  | "channel_setup_help"
  | "setup_help"
  | "general";

export type SetupUnderstandResponse = {
  decision: SetupDecision;
  intent_kind: SetupIntentKind;
  clarifier?: { question: string; options: { id: string; label: string }[] } | null;
  slots: {
    provider_ids: string[];
    model_ids: string[];
    integration_targets: string[];
    channel_targets: string[];
    goal?: string | null;
  };
  proposed_action: { type: string; payload: Record<string, unknown> };
};

export type SetupCard = {
  title: string;
  body: string;
  cta: string;
  actionType: string;
  payload: Record<string, unknown>;
  clarifier?: { question: string; options: { id: string; label: string }[] };
};

export function inferMime(name = "") {
  const ext = String(name).toLowerCase().split(".").pop() || "";
  return EXT_MIME[ext] || "application/octet-stream";
}

export function joinRootAndRel(root = "", rel = "") {
  if (!root || !rel) return rel || "";
  const lhs = String(root).replace(/[\\/]+$/, "");
  const rhs = String(rel).replace(/^[\\/]+/, "");
  return `${lhs}/${rhs}`;
}

export function formatBytes(bytes: number) {
  const n = Number(bytes || 0);
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

export function sameSession(eventSessionId: string, activeSessionId: string) {
  if (!eventSessionId || !activeSessionId) return true;
  return String(eventSessionId).trim() === String(activeSessionId).trim();
}

export function isRunSignalEvent(eventType: string) {
  const t = String(eventType || "").trim();
  return t !== "server.connected" && t !== "engine.lifecycle.ready";
}

export function toolStatusClass(status: ToolActivityStatus) {
  if (status === "completed") return "chat-tool-chip-ok";
  if (status === "failed") return "chat-tool-chip-failed";
  return "chat-tool-chip-running";
}

export function loadAutoApprovePreference() {
  try {
    return localStorage.getItem(CHAT_AUTO_APPROVE_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveAutoApprovePreference(enabled: boolean) {
  try {
    localStorage.setItem(CHAT_AUTO_APPROVE_KEY, enabled ? "1" : "0");
  } catch {
    // ignore
  }
}

export function loadToolAllowlistPreference() {
  try {
    const parsed = JSON.parse(localStorage.getItem(CHAT_TOOL_ALLOWLIST_KEY) || "[]");
    return Array.isArray(parsed)
      ? parsed.map((tool) => String(tool || "").trim()).filter(Boolean)
      : [];
  } catch {
    return [];
  }
}

export function saveToolAllowlistPreference(tools: string[]) {
  try {
    localStorage.setItem(
      CHAT_TOOL_ALLOWLIST_KEY,
      JSON.stringify([...new Set(tools.map((tool) => tool.trim()).filter(Boolean))].sort())
    );
  } catch {
    // ignore
  }
}

export function seedAutomationPlanner(payload: Record<string, unknown>) {
  try {
    const prompt = String(payload?.prompt || payload?.goal || "").trim();
    if (!prompt) return;
    sessionStorage.setItem(
      AUTOMATION_PLANNER_SEED_KEY,
      JSON.stringify({
        prompt,
        plan_source: String(payload?.plan_source || "chat_setup").trim() || "chat_setup",
      })
    );
  } catch {
    // ignore
  }
}

export function seedWorkflowPlanner(payload: Record<string, unknown>) {
  try {
    const prompt = String(payload?.prompt || payload?.goal || "").trim();
    if (!prompt) return;
    sessionStorage.setItem(
      WORKFLOW_PLANNER_SEED_KEY,
      JSON.stringify({
        prompt,
        plan_source: String(payload?.plan_source || "chat_setup").trim() || "chat_setup",
        session_id: String(payload?.session_id || "").trim() || "",
        source_platform: String(payload?.source_platform || "").trim() || "",
        source_channel: String(payload?.source_channel || "").trim() || "",
        workspace_root: String(payload?.workspace_root || "").trim() || "",
      })
    );
  } catch {
    // ignore
  }
}

export function toTextError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export function setupCardFromResponse(response: SetupUnderstandResponse): SetupCard | null {
  if (response.decision === "pass_through") return null;
  if (response.intent_kind === "provider_setup") {
    return {
      title: "Provider setup",
      body: `Configure ${response.slots.provider_ids[0] || "a provider"} in Providers.`,
      cta: "Open Providers",
      actionType: "open_provider_setup",
      payload: response.proposed_action.payload || {},
    };
  }
  if (response.intent_kind === "integration_setup") {
    return {
      title: "Tool connection",
      body: `Connect ${response.slots.integration_targets[0] || "the matching tool"} through MCP.`,
      cta: "Open MCP",
      actionType: "open_mcp_setup",
      payload: response.proposed_action.payload || {},
    };
  }
  if (response.intent_kind === "automation_create") {
    return {
      title: "Automation setup",
      body: response.slots.goal || "Create an automation from this request.",
      cta: "Open Automations",
      actionType: "open_automations",
      payload: response.proposed_action.payload || {},
    };
  }
  if (response.intent_kind === "workflow_planner_create") {
    const payload = {
      ...(response.proposed_action.payload || {}),
      prompt:
        String(response.proposed_action.payload?.prompt || response.slots.goal || "").trim() ||
        undefined,
      plan_source:
        String(response.proposed_action.payload?.plan_source || "").trim() || "chat_setup",
    };
    return {
      title: response.decision === "clarify" ? "Workflow planning questions" : "Workflow planning",
      body:
        response.clarifier?.question ||
        response.slots.goal ||
        "Open the planner to draft a governed workflow plan.",
      cta: "Open Planner",
      actionType: "open_planner",
      payload,
      clarifier: response.clarifier || undefined,
    };
  }
  return {
    title: "Setup help",
    body: response.clarifier?.question || "Choose a setup path.",
    cta: "Open Providers",
    actionType: "open_provider_setup",
    payload: response.proposed_action.payload || {},
    clarifier: response.clarifier || undefined,
  };
}

function normalizePartType(part: any) {
  return String(part?.type || part?.kind || part?.role || "")
    .trim()
    .toLowerCase()
    .replace(/_/g, "-");
}

export function toolStatusFromPayload(part: any): ToolActivityStatus {
  const state = part?.state;
  const status = String(
    (state && typeof state === "object" ? state.status : state) || part?.status || part?.phase || ""
  )
    .trim()
    .toLowerCase();
  if (
    part?.error ||
    (state && typeof state === "object" && state.error) ||
    status.includes("fail") ||
    status.includes("error") ||
    status.includes("deny") ||
    status.includes("reject") ||
    status.includes("cancel")
  ) {
    return "failed";
  }
  if (
    part?.result ||
    part?.output ||
    (state && typeof state === "object" && (state.output || state.result)) ||
    status.includes("done") ||
    status.includes("complete") ||
    status.includes("success")
  ) {
    return "completed";
  }
  return "started";
}

export function collectToolActivityFromMessages(raw: any): ToolActivityItem[] {
  const rows = Array.isArray(raw) ? raw : Array.isArray(raw?.messages) ? raw.messages : [];
  const items: ToolActivityItem[] = [];
  rows.forEach((row: any, rowIndex: number) => {
    const parts = [
      ...(Array.isArray(row?.parts) ? row.parts : []),
      ...(Array.isArray(row?.content) ? row.content : []),
      ...(Array.isArray(row?.message?.parts) ? row.message.parts : []),
    ];
    parts.forEach((part: any, partIndex: number) => {
      const partType = normalizePartType(part);
      const tool = extractToolName(part);
      const looksLikeTool =
        !!tool &&
        (partType.includes("tool") ||
          part?.tool ||
          part?.toolName ||
          part?.tool_name ||
          part?.toolCall ||
          part?.call);
      if (!looksLikeTool) return;
      const callId = extractToolCallId(part);
      const runId = String(row?.runID || row?.runId || row?.run_id || "").trim();
      const at =
        Number(row?.created_at_ms || row?.createdAtMs || row?.timestamp_ms || 0) || Date.now();
      items.push({
        id: `history:${row?.id || rowIndex}:${callId || partIndex}:${tool}:${toolStatusFromPayload(part)}`,
        tool,
        status: toolStatusFromPayload(part),
        at,
        callId,
        runId,
        source: "history",
        summary: summarizeToolPayload(part),
        detail: summarizeToolPayload(part),
      });
    });
  });
  return items.slice(-80).reverse();
}

export function shouldShowSetupCard(prompt: string, setup: SetupUnderstandResponse) {
  if (setup.decision === "pass_through") return false;
  if (setup.intent_kind !== "workflow_planner_create") return true;
  const text = String(prompt || "")
    .trim()
    .toLowerCase();
  const explicitWorkflowAction =
    /\b(create|build|make|draft|design|plan|schedule|automate|orchestrate)\b/.test(text) &&
    /\b(workflow|workflows|pipeline|handoff|automation|automations)\b/.test(text);
  const explicitPlannerPhrase =
    /\b(workflow plan|workflow draft|plan this workflow|draft a plan|turn this into a plan|open planner)\b/.test(
      text
    );
  return explicitWorkflowAction || explicitPlannerPhrase;
}

export function shouldContinueSetupToModel(prompt: string, setup: SetupUnderstandResponse) {
  if (
    setup.intent_kind === "automation_create" ||
    setup.intent_kind === "workflow_planner_create"
  ) {
    return true;
  }
  const text = String(prompt || "")
    .trim()
    .toLowerCase();
  return (
    /\b(create|build|make|draft|design|plan|schedule|automate|orchestrate)\b/.test(text) &&
    /\b(workflow|workflows|pipeline|handoff|automation|automations)\b/.test(text)
  );
}

export function timelineToneForEvent(type: string, props: any): RunTimelineTone {
  const lower = String(type || "").toLowerCase();
  const status = String(props?.status || props?.state || "").toLowerCase();
  if (lower.includes("fail") || lower.includes("error") || status.includes("fail")) return "failed";
  if (lower.includes("finished") || lower.includes("complete") || status.includes("complete")) {
    return props?.error ? "failed" : "ok";
  }
  if (lower.includes("started") || lower.includes("running") || status.includes("running")) {
    return "running";
  }
  return "info";
}

export function timelineTitleForEvent(type: string) {
  const lower = String(type || "").toLowerCase();
  if (lower.includes("permission") || lower.includes("approval")) return "Approval";
  if (lower.includes("tool")) return "Tool activity";
  if (lower.includes("response")) return "Response";
  if (lower.includes("status")) return "Status";
  if (lower.includes("finished") || lower.includes("complete")) return "Run finished";
  if (lower.includes("started")) return "Run started";
  return type || "Session event";
}

export function timelineSummaryForEvent(type: string, props: any) {
  const bits = [
    extractToolName(props),
    String(props?.status || props?.state || "").trim(),
    String(props?.error || props?.message || "").trim(),
  ].filter(Boolean);
  if (bits.length) return bits.join(" · ");
  return String(type || "event");
}

export function collectRunTimelineFromMessages(raw: any): RunTimelineItem[] {
  const rows = Array.isArray(raw) ? raw : Array.isArray(raw?.messages) ? raw.messages : [];
  return rows
    .map((row: any, index: number) => {
      const role = String(row?.info?.role || row?.role || row?.message_role || "assistant")
        .trim()
        .toLowerCase();
      const text = String(row?.text || row?.content || row?.message || "").trim();
      const at =
        Number(row?.created_at_ms || row?.createdAtMs || row?.timestamp_ms || row?.at) ||
        Date.now() - (rows.length - index) * 1000;
      return {
        id: `message:${row?.id || index}:${role}`,
        type: "message",
        title: role === "user" ? "User message" : "Assistant message",
        summary: text ? text.slice(0, 140) : role,
        tone: role === "user" ? "info" : ("ok" as RunTimelineTone),
        at,
        runId: String(row?.runID || row?.runId || row?.run_id || "").trim(),
      };
    })
    .reverse()
    .slice(0, 40);
}
