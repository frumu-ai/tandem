import type { ChatInterfaceMessage } from "../../components/ChatInterfacePanel";

export type PlannerProviderOption = {
  id: string;
  models: string[];
  configured?: boolean;
};

function mergeStrings(...groups: string[][]) {
  const seen = new Set<string>();
  const merged: string[] = [];
  for (const group of groups) {
    for (const raw of group) {
      const value = safeString(raw);
      if (!value || seen.has(value)) continue;
      seen.add(value);
      merged.push(value);
    }
  }
  return merged;
}

function collectModels(raw: any) {
  if (Array.isArray(raw)) return raw.map((value) => safeString(value)).filter(Boolean);
  if (raw && typeof raw === "object")
    return Object.keys(raw)
      .map((value) => safeString(value))
      .filter(Boolean);
  const single = safeString(raw);
  return single ? [single] : [];
}

function isInternalProviderId(providerId: string) {
  const normalized = safeString(providerId).toLowerCase();
  return normalized.startsWith("mcp_header::") || normalized.startsWith("channel::");
}

export type PlannerSessionStatus =
  | "draft"
  | "waiting_for_clarification"
  | "ready_to_publish"
  | "published"
  | "published_with_new_edits";

export type PlannerSessionSummary = {
  id: string;
  title: string;
  projectSlug: string;
  workspaceRoot: string;
  currentPlanId: string;
  goal: string;
  plannerProvider: string;
  plannerModel: string;
  updatedAtMs: number;
  updatedAtLabel: string;
  previewText: string;
  status: PlannerSessionStatus;
  statusLabel: string;
  statusTone: "ok" | "warn" | "info" | "ghost";
  revisionCount: number;
  publishedAtMs: number | null;
  hasUnpublishedChanges: boolean;
};

function safeString(value: unknown) {
  return String(value || "").trim();
}

function compactKnowledgeSubject(subject: string) {
  const clean = safeString(subject);
  if (!clean) return "";
  const firstMeaningfulLine =
    clean
      .split(/\r?\n/)
      .map((line) => line.trim())
      .find(Boolean) || clean;
  const normalized = firstMeaningfulLine.replace(/\s+/g, " ").trim();
  if (normalized.length <= 180) return normalized;
  return `${normalized.slice(0, 177).trimEnd()}...`;
}

export function formatRelativePlannerTime(updatedAtMs: number) {
  const value = Number(updatedAtMs || 0);
  if (!value) return "unknown";
  const diffMs = Date.now() - value;
  const minutes = Math.max(1, Math.round(diffMs / 60000));
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  return `${days}d ago`;
}

function truncateText(value: string, length = 96) {
  const text = safeString(value);
  if (!text) return "";
  return text.length > length ? `${text.slice(0, Math.max(0, length - 1)).trimEnd()}…` : text;
}

function plannerSessionMessages(conversation: any) {
  return Array.isArray(conversation?.messages) ? conversation.messages : [];
}

export function plannerSessionStatusLabel(status: PlannerSessionStatus) {
  switch (status) {
    case "waiting_for_clarification":
      return "Waiting for clarification";
    case "ready_to_publish":
      return "Ready to publish";
    case "published":
      return "Published";
    case "published_with_new_edits":
      return "Published with new edits";
    case "draft":
    default:
      return "Draft";
  }
}

export function plannerSessionStatusTone(
  status: PlannerSessionStatus
): "ok" | "warn" | "info" | "ghost" {
  switch (status) {
    case "published":
      return "ok";
    case "ready_to_publish":
      return "info";
    case "published_with_new_edits":
    case "waiting_for_clarification":
      return "warn";
    case "draft":
    default:
      return "ghost";
  }
}

export function plannerConversationHasUnpublishedChanges(input: {
  publishedAtMs?: number | null;
  updatedAtMs?: number | null;
}) {
  const publishedAtMs = Number(input.publishedAtMs || 0);
  const updatedAtMs = Number(input.updatedAtMs || 0);
  if (!publishedAtMs || !updatedAtMs) return false;
  return updatedAtMs > publishedAtMs;
}

export function normalizePlannerSessionPreview(input: {
  session?: any;
  draft?: any;
  clarificationState?: { status: "none" | "waiting"; question?: string } | null;
}) {
  const session = input.session || {};
  const draft = input.draft || null;
  const conversation = draft?.conversation || session?.draft?.conversation || null;
  const messages = plannerSessionMessages(conversation);
  const reversed = [...messages].reverse();
  const lastAssistant = reversed.find(
    (message: any) => safeString(message?.role).toLowerCase() === "assistant"
  );
  const lastUser = reversed.find(
    (message: any) => safeString(message?.role).toLowerCase() === "user"
  );
  if (input.clarificationState?.status === "waiting") {
    return truncateText(input.clarificationState.question || "Planner needs clarification.");
  }
  const currentPlanTitle = safeString(
    draft?.current_plan?.title ||
      draft?.currentPlan?.title ||
      session?.current_plan?.title ||
      session?.plan?.title ||
      session?.title
  );
  if (lastAssistant?.text) return truncateText(lastAssistant.text);
  if (lastUser?.text) return truncateText(lastUser.text);
  if (currentPlanTitle) return truncateText(currentPlanTitle);
  const goal = safeString(session?.goal || draft?.goal);
  if (goal) return truncateText(goal);
  return "No plan activity yet.";
}

export function summarizePlannerSession(input: {
  session: any;
  draft?: any | null;
  clarificationState?: { status: "none" | "waiting"; question?: string } | null;
}): PlannerSessionSummary {
  const session = input.session || {};
  const draft = input.draft || session?.draft || null;
  const currentPlan = draft?.current_plan || draft?.currentPlan || session?.current_plan || null;
  const sessionId = safeString(session?.session_id || session?.sessionId);
  const updatedAtMs = Number(session?.updated_at_ms || session?.updatedAtMs || 0) || 0;
  const publishedAtMs =
    Number(session?.published_at_ms || session?.publishedAtMs || draft?.published_at_ms || 0) ||
    null;
  const currentPlanId = safeString(
    session?.current_plan_id ||
      session?.currentPlanId ||
      currentPlan?.plan_id ||
      currentPlan?.planId
  );
  const revisionCount = Number(
    draft?.plan_revision ||
      draft?.planRevision ||
      currentPlan?.plan_revision ||
      currentPlan?.planRevision ||
      session?.plan_revision ||
      session?.planRevision ||
      (currentPlanId ? 1 : 0)
  );
  const hasUnpublishedChanges = plannerConversationHasUnpublishedChanges({
    publishedAtMs,
    updatedAtMs,
  });
  let status: PlannerSessionStatus = "draft";
  if (input.clarificationState?.status === "waiting") {
    status = "waiting_for_clarification";
  } else if (publishedAtMs && hasUnpublishedChanges) {
    status = "published_with_new_edits";
  } else if (publishedAtMs) {
    status = "published";
  } else if (currentPlanId) {
    status = "ready_to_publish";
  }

  return {
    id: sessionId,
    title: safeString(session?.title || currentPlan?.title || "Untitled plan"),
    projectSlug: safeString(session?.project_slug || session?.projectSlug),
    workspaceRoot: safeString(session?.workspace_root || session?.workspaceRoot),
    currentPlanId,
    goal: safeString(session?.goal || draft?.goal),
    plannerProvider: safeString(session?.planner_provider || session?.plannerProvider),
    plannerModel: safeString(session?.planner_model || session?.plannerModel),
    updatedAtMs,
    updatedAtLabel: formatRelativePlannerTime(updatedAtMs),
    previewText: normalizePlannerSessionPreview({
      session,
      draft,
      clarificationState: input.clarificationState || null,
    }),
    status,
    statusLabel: plannerSessionStatusLabel(status),
    statusTone: plannerSessionStatusTone(status),
    revisionCount: Number.isFinite(revisionCount) ? revisionCount : currentPlanId ? 1 : 0,
    publishedAtMs,
    hasUnpublishedChanges,
  };
}

export function buildPlannerProviderOptions(options: {
  providerCatalog: any;
  providerConfig: any;
  defaultProvider: string;
  defaultModel: string;
  includeUnconfiguredProviders?: boolean;
  providerAuthStatus?: any;
}): PlannerProviderOption[] {
  const rows = Array.isArray(options.providerCatalog?.all)
    ? options.providerCatalog.all
    : Array.isArray(options.providerCatalog?.providers)
      ? options.providerCatalog.providers
      : [];
  const configuredProviders = ((
    options.providerConfig as { providers?: Record<string, any> } | undefined
  )?.providers || {}) as Record<string, any>;
  const configuredLookup = new Map<string, { id: string; value: any }>();
  for (const [providerId, value] of Object.entries(configuredProviders)) {
    const id = safeString(providerId);
    if (!id || isInternalProviderId(id)) continue;
    configuredLookup.set(id.toLowerCase(), { id, value });
  }
  const connectedIds = new Set<string>();
  if (Array.isArray(options.providerCatalog?.connected)) {
    for (const raw of options.providerCatalog.connected) {
      const value = safeString(raw).toLowerCase();
      if (value) connectedIds.add(value);
    }
  }
  const authStatus = options.providerAuthStatus;
  if (authStatus && typeof authStatus === "object") {
    const nested = (authStatus as { providers?: Record<string, any> }).providers;
    const authRows = nested && typeof nested === "object" ? nested : authStatus;
    if (authRows && typeof authRows === "object") {
      for (const [providerId, value] of Object.entries(authRows)) {
        const normalizedId = safeString(providerId).toLowerCase();
        if (!normalizedId || !value || typeof value !== "object") continue;
        const status = safeString((value as any).status).toLowerCase();
        if (
          (value as any).connected === true ||
          status === "connected" ||
          status === "configured" ||
          (value as any).has_key === true ||
          (value as any).hasKey === true
        ) {
          connectedIds.add(normalizedId);
        }
      }
    }
  }

  const includeAllProviders = options.includeUnconfiguredProviders === true;
  const mapped = new Map<string, PlannerProviderOption>();
  for (const provider of rows) {
    const id = safeString(provider?.id);
    if (!id || isInternalProviderId(id)) continue;
    const normalizedId = id.toLowerCase();
    const configuredEntry = configuredLookup.get(normalizedId);
    const configured = !!configuredEntry;
    const configEntry = configuredEntry?.value || {};
    const models = mergeStrings(
      collectModels(provider?.models),
      collectModels(configEntry?.models),
      collectModels(configEntry?.default_model || configEntry?.defaultModel)
    );
    const shouldInclude = includeAllProviders || !!configured || connectedIds.has(normalizedId);
    if (!shouldInclude) continue;
    mapped.set(normalizedId, {
      id,
      models,
      configured: !!configured || connectedIds.has(normalizedId),
    });
  }

  for (const { id, value } of configuredLookup.values()) {
    const normalizedId = id.toLowerCase();
    if (mapped.has(normalizedId)) continue;
    mapped.set(normalizedId, {
      id,
      models: mergeStrings(
        collectModels((value as any)?.models),
        collectModels((value as any)?.default_model || (value as any)?.defaultModel)
      ),
      configured: true,
    });
  }

  for (const normalizedId of connectedIds) {
    if (
      mapped.has(normalizedId) ||
      configuredLookup.has(normalizedId) ||
      isInternalProviderId(normalizedId)
    ) {
      continue;
    }
    mapped.set(normalizedId, {
      id: normalizedId,
      models: [],
      configured: true,
    });
  }

  return [...mapped.values()].sort((left, right) => left.id.localeCompare(right.id));
}

export function buildDefaultKnowledgeOperatorPreferences(subject: string) {
  const cleanSubject = compactKnowledgeSubject(subject);
  return {
    knowledge: {
      enabled: true,
      reuse_mode: "preflight",
      trust_floor: "promoted",
      read_spaces: [{ scope: "project" }],
      promote_spaces: [{ scope: "project" }],
      ...(cleanSubject ? { subject: cleanSubject } : {}),
    },
  };
}

export function buildKnowledgeRolloutGuidance(subject: string) {
  const cleanSubject = safeString(subject);
  return {
    rollout: {
      rollout_mode: "project_first_pilot",
      guardrails: [
        "Start in one project space before widening scope.",
        "Keep reuse_mode at preflight and trust_floor at promoted by default.",
        "Promote only validated outcomes; do not treat raw run output as shared truth.",
        "Use approved_default sparingly, only for reviewed default guidance.",
        "Watch reuse_reason, skip_reason, and freshness_reason during the pilot.",
        "Expand namespaces only after the pilot demonstrates stable reuse.",
      ],
      recommended_sequence: [
        "Run one workflow once to seed working knowledge.",
        "Promote validated outcomes into the project knowledge space.",
        "Run a second workflow with the same subject to verify reuse.",
        "Check that unrelated workflows still miss the same knowledge key.",
      ],
      subject: cleanSubject || null,
    },
  };
}

export function normalizePlannerConversationMessages(
  conversation: any,
  markdown = true
): ChatInterfaceMessage[] {
  const rows = Array.isArray(conversation?.messages) ? conversation.messages : [];
  return rows.map((message: any, index: number) => ({
    id: String(message?.created_at_ms || message?.createdAtMs || `${index}`),
    role: safeString(message?.role || "assistant").toLowerCase(),
    displayRole: safeString(message?.role || "assistant"),
    text: safeString(message?.text || "") || " ",
    markdown,
  }));
}
