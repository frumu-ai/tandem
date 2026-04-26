import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useQueryClient } from "@tanstack/react-query";
import { renderIcons } from "../app/icons.js";
import {
  clearPlannerComposerDraft,
  clearSelectedPlannerSession,
  loadPlannerComposerDraft,
  loadSelectedPlannerSession,
  savePlannerComposerDraft,
  saveSelectedPlannerSession,
} from "../features/planner/plannerSessionStorage";
import {
  buildPlannerProviderOptions,
  buildDefaultKnowledgeOperatorPreferences,
  buildKnowledgeRolloutGuidance,
  normalizePlannerConversationMessages,
  summarizePlannerSession,
  type PlannerProviderOption,
  type PlannerSessionSummary,
} from "../features/planner/plannerShared";
import { TaskPlanningPanelView } from "./TaskPlanningPanelView";

type TaskPlanningPanelProps = {
  client: any;
  api: (path: string, init?: RequestInit) => Promise<any>;
  toast: (kind: "ok" | "info" | "warn" | "err", text: string) => void;
  selectedProjectSlug: string;
  selectedProject: any | null;
  githubProjectBoardSnapshot: any | null;
  taskSourceType: string;
  workspaceRootSeed: string;
  connectedMcpServers: string[];
  engineHealthy: boolean;
  providerStatus: {
    defaultProvider: string;
    defaultModel: string;
  };
};

type PlanningDraft = {
  goal: string;
  workspaceRoot: string;
  notes: string;
  plannerProvider: string;
  plannerModel: string;
  plan: any | null;
  conversation: any | null;
  changeSummary: string[];
  plannerError: string;
  plannerDiagnostics: any | null;
  publishedAtMs: number | null;
  updatedAtMs?: number | null;
  publishedTasks: Array<{
    title: string;
    issueNumber?: number | null;
    issueUrl?: string;
    publishedAtMs: number;
  }>;
};

type PlannedTask = {
  id: string;
  title: string;
  objective: string;
  kind: string;
  dependsOn: string[];
  inputRefs: Array<{ fromStepId: string; alias: string }>;
  outputContract: string;
};

type ClarifierOption = {
  id: string;
  label: string;
};

type ClarificationState =
  | { status: "none" }
  | {
      status: "waiting";
      question: string;
      options: ClarifierOption[];
    };

type PlannerSessionRow = {
  id: string;
  title: string;
  updatedAtMs: number;
  projectSlug: string;
  workspaceRoot: string;
  currentPlanId: string;
  goal: string;
  plannerProvider: string;
  plannerModel: string;
  publishedAtMs: number | null;
  draft?: any | null;
};

type PlannerSessionCacheEntry = {
  session: any | null;
  draft: PlanningDraft | null;
  clarification: ClarificationState;
  planPreview: any | null;
  planningConversation: any | null;
  planningChangeSummary: string[];
  plannerDiagnostics: any | null;
  publishedTasks: PlanningDraft["publishedTasks"];
};

const PLANNER_HANDOFF_KEY = "tandem.intent-planner.codingTaskHandoff.v1";

function safeString(value: unknown) {
  return String(value || "").trim();
}

function isMissingWorkflowPlanError(message: string) {
  const text = safeString(message).toLowerCase();
  return text.includes("workflow_plan_not_found") || text.includes("workflow plan not found");
}

function inferGitHubWorkspaceRoot(selectedProject: any | null) {
  const repoNameFromTaskSource = safeString(selectedProject?.taskSource?.repo);
  if (repoNameFromTaskSource) {
    const repoName = repoNameFromTaskSource.split("/").pop() || repoNameFromTaskSource;
    return `/workspace/repos/${repoName}`;
  }
  const repoUrl = safeString(selectedProject?.repoUrl);
  if (!repoUrl) return "";
  try {
    const parsed = new URL(repoUrl);
    const parts = parsed.pathname.split("/").filter(Boolean);
    const repoName = (parts[parts.length - 1] || "").replace(/\.git$/, "");
    return repoName ? `/workspace/repos/${repoName}` : "";
  } catch {
    const match = repoUrl.match(/\/([^/]+?)(?:\.git)?(?:\?.*)?$/);
    return match?.[1] ? `/workspace/repos/${match[1]}` : "";
  }
}

function plannerSessionTitle(input: {
  goal?: string;
  plan?: any;
  fallbackTime?: number;
  existingTitle?: string;
}) {
  const planTitle = safeString(input.plan?.title);
  if (planTitle) return planTitle;
  const goal = safeString(input.goal);
  if (goal) return goal.length > 48 ? `${goal.slice(0, 47).trimEnd()}…` : goal;
  if (input.existingTitle) return input.existingTitle;
  return `Plan ${new Date(input.fallbackTime || Date.now()).toLocaleTimeString()}`;
}

function plannerSessionRowFromListItem(item: any): PlannerSessionRow {
  return {
    id: safeString(item?.session_id || item?.sessionId),
    title: safeString(item?.title || "Untitled plan"),
    updatedAtMs: Number(item?.updated_at_ms || item?.updatedAtMs || 0) || 0,
    projectSlug: safeString(item?.project_slug || item?.projectSlug),
    workspaceRoot: safeString(item?.workspace_root || item?.workspaceRoot),
    currentPlanId: safeString(item?.current_plan_id || item?.currentPlanId),
    goal: safeString(item?.goal || ""),
    plannerProvider: safeString(item?.planner_provider || item?.plannerProvider),
    plannerModel: safeString(item?.planner_model || item?.plannerModel),
    publishedAtMs: Number(item?.published_at_ms || item?.publishedAtMs || 0) || null,
    draft: item?.draft || null,
  };
}

function plannerSessionRowFromRecord(session: any): PlannerSessionRow {
  return {
    id: safeString(session?.session_id || session?.sessionId),
    title: safeString(session?.title || "Untitled plan"),
    updatedAtMs: Number(session?.updated_at_ms || session?.updatedAtMs || 0) || 0,
    projectSlug: safeString(session?.project_slug || session?.projectSlug),
    workspaceRoot: safeString(session?.workspace_root || session?.workspaceRoot),
    currentPlanId: safeString(session?.current_plan_id || session?.currentPlanId),
    goal: safeString(session?.goal || ""),
    plannerProvider: safeString(session?.planner_provider || session?.plannerProvider),
    plannerModel: safeString(session?.planner_model || session?.plannerModel),
    publishedAtMs: Number(session?.published_at_ms || session?.publishedAtMs || 0) || null,
    draft: session?.draft || null,
  };
}

function plannerSummaryFromRow(
  row: PlannerSessionRow,
  cache: PlannerSessionCacheEntry | undefined
): PlannerSessionSummary {
  return summarizePlannerSession({
    session: { ...row, ...(cache?.session || {}) },
    draft: cache?.draft || row.draft || null,
    clarificationState: cache?.clarification || { status: "none" },
  });
}

function emptyPlannerSessionCacheEntry(): PlannerSessionCacheEntry {
  return {
    session: null,
    draft: null,
    clarification: { status: "none" },
    planPreview: null,
    planningConversation: null,
    planningChangeSummary: [],
    plannerDiagnostics: null,
    publishedTasks: [],
  };
}

function normalizePlanSteps(plan: any): PlannedTask[] {
  const steps = Array.isArray(plan?.steps) ? plan.steps : [];
  return steps
    .map((step: any, index: number) => {
      const id = safeString(step?.step_id || step?.stepId || `step-${index + 1}`);
      const objective = safeString(step?.objective);
      const kind = safeString(step?.kind || "task");
      const dependsOn = Array.isArray(step?.depends_on || step?.dependsOn)
        ? (step?.depends_on || step?.dependsOn).map((row: any) => safeString(row)).filter(Boolean)
        : [];
      const inputRefs = Array.isArray(step?.input_refs || step?.inputRefs)
        ? (step?.input_refs || step?.inputRefs).map((row: any) => ({
            fromStepId: safeString(row?.from_step_id || row?.fromStepId),
            alias: safeString(row?.alias),
          }))
        : [];
      const outputContract = safeString(
        step?.output_contract?.kind || step?.outputContract?.kind || ""
      );
      return {
        id,
        title: objective || `${kind} ${index + 1}`,
        objective: objective || `${kind} ${index + 1}`,
        kind,
        dependsOn,
        inputRefs,
        outputContract,
      };
    })
    .filter((step: PlannedTask) => step.title || step.objective);
}

function findIssueNumber(result: any): number | null {
  const candidates: unknown[] = [result?.output];
  const metadataResult = result?.metadata?.result;
  if (metadataResult) {
    candidates.push(metadataResult);
    const content = metadataResult?.content;
    if (Array.isArray(content)) {
      for (const entry of content) {
        if (entry && typeof entry === "object") {
          candidates.push(entry?.text);
          candidates.push(entry);
        }
      }
    }
  }
  for (const candidate of candidates) {
    if (candidate == null) continue;
    if (typeof candidate === "number" && Number.isFinite(candidate)) return Number(candidate);
    if (typeof candidate === "object") {
      const obj = candidate as Record<string, any>;
      const direct =
        obj.issue_number ?? obj.issueNumber ?? obj.number ?? obj.issue?.number ?? obj.data?.number;
      if (typeof direct === "number" && Number.isFinite(direct)) return Number(direct);
      if (typeof direct === "string" && direct.trim()) {
        const parsed = Number.parseInt(direct, 10);
        if (Number.isFinite(parsed)) return parsed;
      }
      continue;
    }
    const text = String(candidate || "").trim();
    if (!text) continue;
    try {
      const parsed = JSON.parse(text);
      if (parsed && typeof parsed === "object") {
        const direct =
          parsed.issue_number ?? parsed.issueNumber ?? parsed.number ?? parsed.data?.number;
        if (typeof direct === "number" && Number.isFinite(direct)) return Number(direct);
        if (typeof direct === "string" && direct.trim()) {
          const parsedNumber = Number.parseInt(direct, 10);
          if (Number.isFinite(parsedNumber)) return parsedNumber;
        }
      }
    } catch {
      const match =
        text.match(/"issue_number"\s*:\s*(\d+)/i) ||
        text.match(/"issueNumber"\s*:\s*(\d+)/i) ||
        text.match(/"number"\s*:\s*(\d+)/i) ||
        text.match(/#(\d+)/);
      if (match) {
        const parsed = Number.parseInt(match[1], 10);
        if (Number.isFinite(parsed)) return parsed;
      }
    }
  }
  return null;
}

function findProjectItemId(result: any): number | null {
  const candidates: unknown[] = [result?.output];
  const metadataResult = result?.metadata?.result;
  if (metadataResult) {
    candidates.push(metadataResult);
    const content = metadataResult?.content;
    if (Array.isArray(content)) {
      for (const entry of content) {
        if (entry && typeof entry === "object") {
          candidates.push(entry?.text);
          candidates.push(entry);
        }
      }
    }
  }
  for (const candidate of candidates) {
    if (candidate == null) continue;
    if (typeof candidate === "number" && Number.isFinite(candidate)) return Number(candidate);
    if (typeof candidate === "object") {
      const obj = candidate as Record<string, any>;
      const direct =
        obj.project_item_id ?? obj.projectItemId ?? obj.item_id ?? obj.itemId ?? obj.id;
      if (typeof direct === "number" && Number.isFinite(direct)) return Number(direct);
      if (typeof direct === "string" && direct.trim()) {
        const parsed = Number.parseInt(direct, 10);
        if (Number.isFinite(parsed)) return parsed;
      }
      continue;
    }
    const text = String(candidate || "").trim();
    if (!text) continue;
    try {
      const parsed = JSON.parse(text);
      if (parsed && typeof parsed === "object") {
        const direct =
          parsed.project_item_id ??
          parsed.projectItemId ??
          parsed.item_id ??
          parsed.itemId ??
          parsed.id;
        if (typeof direct === "number" && Number.isFinite(direct)) return Number(direct);
        if (typeof direct === "string" && direct.trim()) {
          const parsedNumber = Number.parseInt(direct, 10);
          if (Number.isFinite(parsedNumber)) return parsedNumber;
        }
      }
    } catch {
      const match =
        text.match(/"project_item_id"\s*:\s*(\d+)/i) ||
        text.match(/"projectItemId"\s*:\s*(\d+)/i) ||
        text.match(/"item_id"\s*:\s*(\d+)/i) ||
        text.match(/"itemId"\s*:\s*(\d+)/i) ||
        text.match(/"id"\s*:\s*(\d+)/i);
      if (match) {
        const parsed = Number.parseInt(match[1], 10);
        if (Number.isFinite(parsed)) return parsed;
      }
    }
  }
  return null;
}

function buildTaskMarkdown(
  plan: any,
  task: PlannedTask,
  index: number,
  context: {
    goal: string;
    workspaceRoot: string;
    projectSlug: string;
    taskSourceType: string;
    selectedProject: any | null;
    notes: string;
    plannerProvider: string;
    plannerModel: string;
  }
) {
  const planDescription = safeString(plan?.description);
  const refs = task.inputRefs.length
    ? task.inputRefs.map((row) => `- ${row.alias || "ref"} <- ${row.fromStepId}`).join("\n")
    : "- None";
  const dependsOn = task.dependsOn.length
    ? task.dependsOn.map((row) => `- ${row}`).join("\n")
    : "- None";
  const repoUrl = safeString(context.selectedProject?.repoUrl);
  const taskSource = context.selectedProject?.taskSource || {};
  const sourceSummary =
    context.taskSourceType === "github_project"
      ? `GitHub Project ${safeString(taskSource.owner)}/${safeString(taskSource.repo)} #${safeString(taskSource.project)}`
      : context.taskSourceType === "kanban_board" || context.taskSourceType === "local_backlog"
        ? safeString(taskSource.path) || "local task source"
        : "manual";

  return [
    `# ${task.title}`,
    "",
    "## Summary",
    task.objective || "Planned repository task.",
    "",
    "## Context",
    `- Project: ${context.projectSlug || "unbound"}`,
    `- Task source: ${sourceSummary}`,
    `- Workspace root: ${context.workspaceRoot || "engine workspace root"}`,
    repoUrl ? `- Repo URL: ${repoUrl}` : "",
    `- Planner persona: scrum-master`,
    `- Planner provider: ${context.plannerProvider || "workspace default"}`,
    `- Planner model: ${context.plannerModel || "workspace default"}`,
    `- Step kind: ${task.kind || "task"}`,
    "",
    "## Expected Results",
    planDescription
      ? planDescription
      : "Follow the goal and preserve existing behavior unless the plan says otherwise.",
    "",
    "## Implementation Notes",
    `- Depends on:\n${dependsOn}`,
    `- Input refs:\n${refs}`,
    task.outputContract
      ? `- Output contract: ${task.outputContract}`
      : "- Output contract: not specified",
    "",
    "## Verification",
    "- Validate the touched files in the workspace.",
    "- Run the smallest relevant build or test command for the repo area.",
    "- Confirm the user-visible behavior described by the plan.",
    "",
    "## Rework Notes",
    context.notes ? context.notes : "None",
    "",
    "## Planner Snapshot",
    "```json",
    JSON.stringify(
      {
        task_number: index + 1,
        task_title: task.title,
        task_kind: task.kind,
        step_id: task.id,
        workspace_root: context.workspaceRoot,
        project_slug: context.projectSlug,
        planner_provider: context.plannerProvider,
        planner_model: context.plannerModel,
      },
      null,
      2
    ),
    "```",
  ]
    .filter(Boolean)
    .join("\n");
}

function buildExportMarkdown(
  plan: any,
  tasks: PlannedTask[],
  context: {
    goal: string;
    workspaceRoot: string;
    projectSlug: string;
    notes: string;
    plannerProvider: string;
    plannerModel: string;
  }
) {
  const body = [
    `# Task Plan`,
    "",
    `- Project: ${context.projectSlug || "unbound"}`,
    `- Workspace root: ${context.workspaceRoot || "engine workspace root"}`,
    context.goal ? `- Goal: ${context.goal}` : "- Goal: (none)",
    `- Planner provider: ${context.plannerProvider || "workspace default"}`,
    `- Planner model: ${context.plannerModel || "workspace default"}`,
    context.notes ? `- Rework notes: ${context.notes}` : "",
    plan?.title ? `- Plan title: ${plan.title}` : "",
    "",
    "## Tasks",
    ...tasks.flatMap((task, index) => [
      `### ${index + 1}. ${task.title}`,
      "",
      task.objective || "",
      "",
      task.outputContract ? `- Expected result: ${task.outputContract}` : "",
      task.dependsOn.length ? `- Depends on: ${task.dependsOn.join(", ")}` : "",
      "",
    ]),
    "",
    "## Raw Plan",
    "```json",
    JSON.stringify(plan || {}, null, 2),
    "```",
  ]
    .filter(Boolean)
    .join("\n");
  return body;
}

function plannerOperatorPreferences(options: {
  plannerProvider: string;
  plannerModel: string;
  defaultProvider: string;
  defaultModel: string;
  selectedProjectSlug: string;
  taskSourceType: string;
  selectedProject: any | null;
  goal: string;
  notes: string;
}) {
  const baseProvider = safeString(options.defaultProvider);
  const baseModel = safeString(options.defaultModel);
  const selectedPlannerProvider = safeString(options.plannerProvider);
  const selectedPlannerModel = safeString(options.plannerModel);
  const payload: Record<string, any> = {};

  if (baseProvider && baseModel) {
    payload.model_provider = baseProvider;
    payload.model_id = baseModel;
  }

  if (selectedPlannerProvider && selectedPlannerModel) {
    payload.role_models = {
      planner: {
        provider_id: selectedPlannerProvider,
        model_id: selectedPlannerModel,
      },
    };
  }

  Object.assign(payload, buildDefaultKnowledgeOperatorPreferences(options.goal));
  Object.assign(payload, buildKnowledgeRolloutGuidance(options.goal));

  return payload;
}

export function TaskPlanningPanel({
  client,
  api,
  toast,
  selectedProjectSlug,
  selectedProject,
  githubProjectBoardSnapshot,
  taskSourceType,
  workspaceRootSeed,
  connectedMcpServers,
  engineHealthy,
  providerStatus,
}: TaskPlanningPanelProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const queryClient = useQueryClient();
  const [plannerSessions, setPlannerSessions] = useState<PlannerSessionRow[]>([]);
  const [plannerSessionCache, setPlannerSessionCache] = useState<
    Record<string, PlannerSessionCacheEntry>
  >({});
  const [selectedSessionId, setSelectedSessionId] = useState("");
  const [sessionsOpen, setSessionsOpen] = useState(false);
  const [goal, setGoal] = useState("");
  const [workspaceRoot, setWorkspaceRoot] = useState("");
  const [notes, setNotes] = useState("");
  const [plannerInput, setPlannerInput] = useState("");
  const [plannerProvider, setPlannerProvider] = useState("");
  const [plannerModel, setPlannerModel] = useState("");
  const [planPreview, setPlanPreview] = useState<any>(null);
  const [planningConversation, setPlanningConversation] = useState<any>(null);
  const [planningChangeSummary, setPlanningChangeSummary] = useState<string[]>([]);
  const [plannerError, setPlannerError] = useState("");
  const [plannerDiagnostics, setPlannerDiagnostics] = useState<any>(null);
  const [publishing, setPublishing] = useState(false);
  const [resetting, setResetting] = useState(false);
  const [loadingDraft, setLoadingDraft] = useState(false);
  const [planningState, setPlanningState] = useState<
    "idle" | "generating" | "clarifying" | "revising"
  >("idle");
  const [clarification, setClarification] = useState<ClarificationState>({ status: "none" });
  const [renameSessionDialog, setRenameSessionDialog] = useState<{
    sessionId: string;
    value: string;
  } | null>(null);
  const [deleteSessionDialog, setDeleteSessionDialog] = useState<{
    sessionId: string;
    title: string;
  } | null>(null);
  const [saveStatus, setSaveStatus] = useState("");
  const [publishStatus, setPublishStatus] = useState("");
  const [lastSavedAtMs, setLastSavedAtMs] = useState<number | null>(null);
  const [publishedAtMs, setPublishedAtMs] = useState<number | null>(null);
  const [publishedTasks, setPublishedTasks] = useState<PlanningDraft["publishedTasks"]>([]);
  const selectedSessionIdRef = useRef("");
  const controlPanelConfigQuery = useQuery({
    queryKey: ["coding-workflows", "control-panel-config", selectedProjectSlug],
    queryFn: () => api("/api/control-panel/config"),
  });
  const providersCatalogQuery = useQuery({
    queryKey: ["coding-workflows", "providers", "catalog"],
    queryFn: () => client.providers.catalog().catch(() => ({ all: [] })),
    refetchInterval: 30000,
  });
  const providersConfigQuery = useQuery({
    queryKey: ["coding-workflows", "providers", "config"],
    queryFn: () => client.providers.config().catch(() => ({})),
    refetchInterval: 30000,
  });

  const plannedTasks = useMemo(() => normalizePlanSteps(planPreview), [planPreview]);
  const isGitHubProject = String(taskSourceType || "").trim() === "github_project";
  const providerOptions = useMemo<PlannerProviderOption[]>(() => {
    return buildPlannerProviderOptions({
      providerCatalog: providersCatalogQuery.data,
      providerConfig: providersConfigQuery.data,
      defaultProvider: providerStatus.defaultProvider,
      defaultModel: providerStatus.defaultModel,
    });
  }, [
    providerStatus.defaultModel,
    providerStatus.defaultProvider,
    providersCatalogQuery.data,
    providersConfigQuery.data,
  ]);
  const controlPanelRepoPath = safeString(
    controlPanelConfigQuery.data?.config?.repository?.path ||
      controlPanelConfigQuery.data?.config?.repository?.worktree_root ||
      ""
  );
  const resolvedWorkspaceRoot = useMemo(() => {
    if (isGitHubProject) {
      return (
        controlPanelRepoPath || inferGitHubWorkspaceRoot(selectedProject) || workspaceRootSeed || ""
      );
    }
    if (
      String(taskSourceType || "").trim() === "kanban_board" ||
      String(taskSourceType || "").trim() === "local_backlog"
    ) {
      return safeString(selectedProject?.taskSource?.path || workspaceRootSeed || "");
    }
    return workspaceRootSeed || "";
  }, [controlPanelRepoPath, isGitHubProject, selectedProject, taskSourceType, workspaceRootSeed]);
  const canPublishToGitHub =
    isGitHubProject &&
    !!safeString(selectedProject?.taskSource?.owner) &&
    !!safeString(selectedProject?.taskSource?.repo) &&
    Number.isFinite(Number(selectedProject?.taskSource?.project || 0));
  const hasBasePlannerModel = !!(
    safeString(providerStatus.defaultProvider) && safeString(providerStatus.defaultModel)
  );
  const hasExplicitPlannerOverride = !!(safeString(plannerProvider) && safeString(plannerModel));
  const plannerSelectionMatchesWorkspaceDefault =
    safeString(plannerProvider) === safeString(providerStatus.defaultProvider) &&
    safeString(plannerModel) === safeString(providerStatus.defaultModel);
  const plannerCanUseLlm = hasBasePlannerModel || hasExplicitPlannerOverride;
  const isGeneratingPlan = planningState === "generating";
  const isClarifying = planningState === "clarifying";
  const isPlanning = planningState !== "idle";
  const plannerFallbackReason = safeString(
    plannerDiagnostics?.fallback_reason || plannerDiagnostics?.fallbackReason
  );
  const clarificationNeeded = plannerFallbackReason === "clarification_needed";
  const latestAssistantMessage = safeString(
    Array.isArray(planningConversation?.messages)
      ? [...planningConversation.messages]
          .reverse()
          .find((message: any) => safeString(message?.role).toLowerCase() === "assistant")?.text
      : ""
  );
  const plannerTimedOut =
    /timed out before completion/i.test(safeString(plannerError)) ||
    /timed out before completion/i.test(latestAssistantMessage);
  const plannerChatMessages = useMemo(
    () => normalizePlannerConversationMessages(planningConversation, false),
    [planningConversation]
  );
  const plannerStatusTitle = isPlanning
    ? isGeneratingPlan
      ? "Generating task plan…"
      : isClarifying
        ? "Waiting for planner…"
        : "Revising task plan…"
    : "";
  const plannerStatusDetail = isClarifying
    ? "The planner is thinking. You'll be able to answer its question shortly."
    : "The request is running against the engine now. Repo-aware planning can take a bit longer while the model inspects files and drafts the task breakdown.";
  const plannerInputPlaceholder = planPreview
    ? "Send a follow-up message or revision request to the planner. (Enter to send, Shift+Enter newline)"
    : "Describe the outcome you want and send to planner. (Enter to send, Shift+Enter newline)";
  const planIsFallbackOnly =
    plannedTasks.length === 1 &&
    safeString(plannedTasks[0]?.id) === "execute_goal" &&
    (!!plannerFallbackReason || /fallback draft/i.test(safeString(planPreview?.description)));
  const displayTasks = planIsFallbackOnly ? [] : plannedTasks;

  useEffect(() => {
    selectedSessionIdRef.current = selectedSessionId;
  }, [selectedSessionId]);

  const activePlannerSessionSummary = useMemo(() => {
    const row = plannerSessions.find((entry) => entry.id === selectedSessionId);
    if (!row) return null;
    const cache = plannerSessionCache[row.id];
    return plannerSummaryFromRow(row, {
      ...(cache || {
        session: null,
        draft: null,
        clarification: { status: "none" },
        planPreview: null,
        planningConversation: null,
        planningChangeSummary: [],
        plannerDiagnostics: null,
        publishedTasks: [],
      }),
      clarification:
        selectedSessionId === row.id ? clarification : cache?.clarification || { status: "none" },
    });
  }, [clarification, plannerSessionCache, plannerSessions, selectedSessionId]);

  const plannerSessionSummaries = useMemo(
    () => plannerSessions.map((row) => plannerSummaryFromRow(row, plannerSessionCache[row.id])),
    [plannerSessionCache, plannerSessions]
  );

  const activatePlannerSession = useCallback(
    (sessionId: string) => {
      const currentSessionId = selectedSessionIdRef.current;
      if (currentSessionId && currentSessionId !== sessionId) {
        savePlannerComposerDraft(selectedProjectSlug, currentSessionId, plannerInput);
      }
      setSelectedSessionId(sessionId);
      setSessionsOpen(false);
    },
    [plannerInput, selectedProjectSlug]
  );

  const updateActivePlannerSessionCache = useCallback(
    (patch: Partial<PlannerSessionCacheEntry>) => {
      const sessionId = selectedSessionIdRef.current;
      if (!sessionId) return;
      const { session: patchSession, ...restPatch } = patch as {
        session?: any;
        draft?: PlanningDraft | null;
        clarification?: ClarificationState;
        planPreview?: any | null;
        planningConversation?: any | null;
        planningChangeSummary?: string[];
        plannerDiagnostics?: any | null;
        publishedTasks?: PlanningDraft["publishedTasks"];
      };
      setPlannerSessionCache((current) => ({
        ...current,
        [sessionId]: {
          ...emptyPlannerSessionCacheEntry(),
          ...(current[sessionId] || {}),
          session: patchSession
            ? { ...(current[sessionId]?.session || {}), ...(patchSession || {}) }
            : current[sessionId]?.session || null,
          ...restPatch,
        },
      }));
    },
    []
  );

  const hydrateDraft = (draft: PlanningDraft | null) => {
    if (draft) {
      setGoal(draft.goal);
      setWorkspaceRoot(draft.workspaceRoot || resolvedWorkspaceRoot || workspaceRootSeed || "");
      setNotes(draft.notes);
      setPlannerProvider(draft.plannerProvider || providerStatus.defaultProvider || "");
      setPlannerModel(draft.plannerModel || providerStatus.defaultModel || "");
      setPlanPreview(draft.plan);
      setPlanningConversation(draft.conversation);
      setPlanningChangeSummary(draft.changeSummary || []);
      setPlannerError(draft.plannerError || "");
      setPlannerDiagnostics(draft.plannerDiagnostics || null);
      setPublishStatus(
        draft.publishedAtMs
          ? `Last published ${new Date(draft.publishedAtMs).toLocaleString()}`
          : ""
      );
      setPublishedAtMs(draft.publishedAtMs || null);
      setLastSavedAtMs(draft.updatedAtMs || draft.publishedAtMs || null);
      setPublishedTasks(draft.publishedTasks || []);
      setClarification({ status: "none" });
      return;
    }
    setGoal("");
    setPlannerInput("");
    setWorkspaceRoot(resolvedWorkspaceRoot || workspaceRootSeed || "");
    setNotes("");
    setPlannerProvider(providerStatus.defaultProvider || "");
    setPlannerModel(providerStatus.defaultModel || "");
    setPlanPreview(null);
    setPlanningConversation(null);
    setPlanningChangeSummary([]);
    setPlannerError("");
    setPlannerDiagnostics(null);
    setClarification({ status: "none" });
    setPublishStatus("");
    setLastSavedAtMs(null);
    setPublishedAtMs(null);
    setPublishedTasks([]);
    setPlannerInput("");
  };

  const planningDraftFromSession = useCallback(
    (session: any | null): PlanningDraft | null => {
      if (!session) return null;
      const draft = session?.draft || null;
      const plan = draft?.current_plan || draft?.currentPlan || null;
      return {
        goal: safeString(session?.goal || plan?.title || ""),
        workspaceRoot: safeString(session?.workspace_root || session?.workspaceRoot || ""),
        notes: safeString(session?.notes || ""),
        plannerProvider: safeString(session?.planner_provider || session?.plannerProvider || ""),
        plannerModel: safeString(session?.planner_model || session?.plannerModel || ""),
        plan,
        conversation: draft?.conversation || null,
        changeSummary: [],
        plannerError: "",
        plannerDiagnostics: draft?.planner_diagnostics || draft?.plannerDiagnostics || null,
        publishedAtMs: Number(session?.published_at_ms || session?.publishedAtMs || 0) || null,
        updatedAtMs: Number(session?.updated_at_ms || session?.updatedAtMs || 0) || Date.now(),
        publishedTasks: Array.isArray(session?.published_tasks || session?.publishedTasks)
          ? session?.published_tasks || session?.publishedTasks
          : [],
      };
    },
    [
      resolvedWorkspaceRoot,
      providerStatus.defaultModel,
      providerStatus.defaultProvider,
      workspaceRootSeed,
    ]
  );

  const refreshPlannerSessions = useCallback(async () => {
    if (!client?.workflowPlannerSessions?.list) return [];
    const response = await client.workflowPlannerSessions.list({
      project_slug: selectedProjectSlug,
    });
    const rows = Array.isArray(response?.sessions) ? response.sessions : [];
    setPlannerSessions(
      rows
        .map(plannerSessionRowFromListItem)
        .filter((row: PlannerSessionRow) => row.id)
        .sort(
          (left: PlannerSessionRow, right: PlannerSessionRow) =>
            right.updatedAtMs - left.updatedAtMs
        )
    );
    return rows;
  }, [client?.workflowPlannerSessions?.list, selectedProjectSlug]);

  const loadPlannerSession = useCallback(
    async (sessionId: string) => {
      if (!client?.workflowPlannerSessions?.get || !sessionId) return;
      const response = await client.workflowPlannerSessions.get(sessionId);
      const session = response?.session || null;
      if (!session) return;
      const draft = planningDraftFromSession(session);
      hydrateDraft(draft);
      setPlannerInput(loadPlannerComposerDraft(selectedProjectSlug, sessionId));
      setPublishStatus(
        session.published_at_ms
          ? `Last published ${new Date(session.published_at_ms).toLocaleString()}`
          : ""
      );
      setPublishedAtMs(Number(session.published_at_ms || 0) || null);
      setLastSavedAtMs(Number(session.updated_at_ms || 0) || null);
      setPlannerSessionCache((current) => ({
        ...current,
        [sessionId]: {
          session,
          draft,
          clarification: current[sessionId]?.clarification || { status: "none" },
          planPreview: current[sessionId]?.planPreview || null,
          planningConversation: current[sessionId]?.planningConversation || null,
          planningChangeSummary: current[sessionId]?.planningChangeSummary || [],
          plannerDiagnostics: current[sessionId]?.plannerDiagnostics || null,
          publishedTasks: current[sessionId]?.publishedTasks || draft?.publishedTasks || [],
        },
      }));
      setPlannerSessions((current) => {
        const next = current.filter((row) => row.id !== session.session_id);
        next.unshift(plannerSessionRowFromRecord(session));
        return next.sort((left, right) => right.updatedAtMs - left.updatedAtMs);
      });
    },
    [client?.workflowPlannerSessions?.get, planningDraftFromSession, selectedProjectSlug]
  );

  const patchActivePlannerSession = useCallback(
    async (patch: Record<string, unknown>) => {
      if (!selectedSessionId || !client?.workflowPlannerSessions?.patch) return;
      try {
        const patchRecord = patch as any;
        const response = await client.workflowPlannerSessions.patch(selectedSessionId, patchRecord);
        const session = response?.session;
        if (session?.session_id) {
          setPlannerSessions((current) => {
            const next = current.filter((row) => row.id !== session.session_id);
            next.unshift(plannerSessionRowFromRecord(session));
            return next.sort((left, right) => right.updatedAtMs - left.updatedAtMs);
          });
          setPlannerSessionCache((current) => ({
            ...current,
            [session.session_id]: {
              ...(current[session.session_id] || {
                session: null,
                draft: null,
                clarification: { status: "none" },
                planPreview: null,
                planningConversation: null,
                planningChangeSummary: [],
                plannerDiagnostics: null,
                publishedTasks: [],
              }),
              session,
              draft: patchRecord.draft
                ? (patchRecord.draft as PlanningDraft)
                : current[session.session_id]?.draft || null,
              publishedTasks: Array.isArray(
                patchRecord.published_tasks || patchRecord.publishedTasks
              )
                ? ((patchRecord.published_tasks ||
                    patchRecord.publishedTasks) as PlanningDraft["publishedTasks"])
                : current[session.session_id]?.publishedTasks || [],
            },
          }));
        }
      } catch {
        // ignore transient patch failures while the user is typing
      }
    },
    [client?.workflowPlannerSessions?.patch, selectedSessionId]
  );

  useEffect(() => {
    let cancelled = false;
    setLoadingDraft(true);
    (async () => {
      try {
        // Session loading is read-only; blank draft sessions are created only by New plan.
        const rows = await refreshPlannerSessions();
        const storedSessionId = loadSelectedPlannerSession(selectedProjectSlug);
        const currentSessionId = selectedSessionIdRef.current;
        const rowsHaveCurrent =
          !!currentSessionId &&
          rows.some(
            (row: any) => safeString(row?.session_id || row?.sessionId) === currentSessionId
          );
        const rowsHaveStored =
          !!storedSessionId &&
          rows.some(
            (row: any) => safeString(row?.session_id || row?.sessionId) === storedSessionId
          );
        const nextSessionId = rowsHaveCurrent
          ? currentSessionId
          : rowsHaveStored
            ? storedSessionId
            : safeString(rows[0]?.session_id || rows[0]?.sessionId || "");
        if (nextSessionId && nextSessionId !== currentSessionId) {
          setSelectedSessionId(nextSessionId);
        } else if (!nextSessionId) {
          setSelectedSessionId("");
          clearSelectedPlannerSession(selectedProjectSlug);
        }
      } catch {
        // ignore load failures; the page will remain usable for a new session
      } finally {
        if (!cancelled) setLoadingDraft(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [client?.workflowPlannerSessions, refreshPlannerSessions, selectedProjectSlug]);

  useEffect(() => {
    if (!selectedSessionId) {
      hydrateDraft(null);
      setPlannerInput("");
      return;
    }
    saveSelectedPlannerSession(selectedProjectSlug, selectedSessionId);
    let cancelled = false;
    setLoadingDraft(true);
    (async () => {
      try {
        await loadPlannerSession(selectedSessionId);
        if (cancelled) return;
      } catch {
        if (!cancelled) hydrateDraft(null);
      } finally {
        if (!cancelled) setLoadingDraft(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [loadPlannerSession, selectedProjectSlug, selectedSessionId]);

  useEffect(() => {
    if (!selectedSessionId) return;
    let cancelled = false;
    setLoadingDraft(true);
    (async () => {
      try {
        await loadPlannerSession(selectedSessionId);
      } catch {
        hydrateDraft(null);
      } finally {
        if (!cancelled) setLoadingDraft(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [
    selectedSessionId,
    loadPlannerSession,
    resolvedWorkspaceRoot,
    workspaceRootSeed,
    providerStatus.defaultProvider,
    providerStatus.defaultModel,
  ]);

  useEffect(() => {
    if (typeof sessionStorage === "undefined") return;
    try {
      const raw = sessionStorage.getItem(PLANNER_HANDOFF_KEY);
      if (!raw) return;
      sessionStorage.removeItem(PLANNER_HANDOFF_KEY);
      const handoff = JSON.parse(raw);
      if (!client?.workflowPlannerSessions?.create) return;
      const loadHandoff = async () => {
        const response = await client.workflowPlannerSessions.create({
          project_slug: selectedProjectSlug,
          title: plannerSessionTitle({
            goal: handoff?.goal,
            plan: handoff?.plan,
            fallbackTime: Date.now(),
          }),
          workspace_root: safeString(
            handoff?.workspaceRoot || handoff?.workspace_root || workspaceRootSeed
          ),
          goal: safeString(handoff?.goal),
          notes: safeString(handoff?.notes),
          planner_provider: safeString(handoff?.plannerProvider || handoff?.planner_provider),
          planner_model: safeString(handoff?.plannerModel || handoff?.planner_model),
          plan_source: "coding_task_planning",
          plan: handoff?.plan ?? null,
          conversation: handoff?.conversation ?? null,
          planner_diagnostics: handoff?.plannerDiagnostics ?? null,
          plan_revision: handoff?.plan?.plan_revision ?? 1,
          last_success_materialization: handoff?.lastSuccessMaterialization ?? null,
          allowed_mcp_servers: Array.isArray(handoff?.allowedMcpServers)
            ? handoff.allowedMcpServers
            : connectedMcpServers,
          operator_preferences: handoff?.operatorPreferences ?? null,
        });
        const session = response?.session;
        if (session?.session_id) {
          setPlannerSessions((current) =>
            [
              plannerSessionRowFromRecord(session),
              ...current.filter((row) => row.id !== session.session_id),
            ].sort((left, right) => right.updatedAtMs - left.updatedAtMs)
          );
          setSelectedSessionId(session.session_id);
          hydrateDraft(planningDraftFromSession(session));
          setPublishStatus("Planner handoff loaded from Planner page.");
        }
      };
      void loadHandoff();
    } catch {
      // ignore
    }
  }, [
    client?.workflowPlannerSessions?.create,
    connectedMcpServers,
    planningDraftFromSession,
    selectedProjectSlug,
    workspaceRootSeed,
  ]);

  useEffect(() => {
    if (!workspaceRoot && resolvedWorkspaceRoot) {
      setWorkspaceRoot(resolvedWorkspaceRoot);
    }
  }, [resolvedWorkspaceRoot, workspaceRoot]);

  useEffect(() => {
    setPlannerError("");
    setPlannerDiagnostics(null);
    setSaveStatus("");
  }, [plannerProvider, plannerModel]);

  useEffect(() => {
    if (loadingDraft || !selectedSessionId) return;
    const plan = planPreview || null;
    const currentPlanId = safeString(plan?.plan_id || "");
    const draft =
      plan && plan.plan_id
        ? {
            initial_plan: plan,
            current_plan: plan,
            plan_revision: Number(plan.plan_revision || 1) || 1,
            conversation: planningConversation || {
              conversation_id: `wfchat-${selectedSessionId}`,
              plan_id: plan.plan_id,
              created_at_ms: Date.now(),
              updated_at_ms: Date.now(),
              messages: [],
            },
            planner_diagnostics: plannerDiagnostics,
            last_success_materialization: null,
          }
        : undefined;
    void patchActivePlannerSession({
      title: plannerSessionTitle({
        goal,
        plan,
        fallbackTime: lastSavedAtMs || Date.now(),
      }),
      workspace_root: workspaceRoot,
      goal,
      notes,
      planner_provider: plannerProvider,
      planner_model: plannerModel,
      plan_source: "coding_task_planning",
      allowed_mcp_servers: connectedMcpServers,
      operator_preferences: plannerOperatorPreferences({
        plannerProvider,
        plannerModel,
        defaultProvider: providerStatus.defaultProvider,
        defaultModel: providerStatus.defaultModel,
        selectedProjectSlug,
        taskSourceType,
        selectedProject,
        goal,
        notes,
      }),
      current_plan_id: currentPlanId || undefined,
      draft,
      published_at_ms: publishedAtMs || undefined,
      published_tasks: publishedTasks.length ? publishedTasks : undefined,
    });
  }, [
    connectedMcpServers,
    goal,
    lastSavedAtMs,
    loadingDraft,
    notes,
    planPreview,
    plannerDiagnostics,
    plannerError,
    planningChangeSummary,
    planningConversation,
    publishedTasks,
    publishedAtMs,
    selectedProjectSlug,
    selectedSessionId,
    taskSourceType,
    plannerModel,
    plannerProvider,
    patchActivePlannerSession,
    workspaceRoot,
    providerStatus.defaultProvider,
    providerStatus.defaultModel,
    selectedProject,
  ]);

  const createNewPlannerSession = async () => {
    if (!client?.workflowPlannerSessions?.create) return;
    try {
      if (selectedSessionIdRef.current) {
        savePlannerComposerDraft(selectedProjectSlug, selectedSessionIdRef.current, plannerInput);
      }
      const initialComposerDraft = selectedSessionIdRef.current ? "" : plannerInput;
      const response = await client.workflowPlannerSessions.create({
        project_slug: selectedProjectSlug,
        title: plannerSessionTitle({
          goal: "",
          fallbackTime: Date.now(),
        }),
        workspace_root: resolvedWorkspaceRoot || workspaceRootSeed || workspaceRoot || "",
        goal: "",
        notes: "",
        planner_provider: plannerProvider,
        planner_model: plannerModel,
        plan_source: "coding_task_planning",
        allowed_mcp_servers: connectedMcpServers,
        operator_preferences: plannerOperatorPreferences({
          plannerProvider,
          plannerModel,
          defaultProvider: providerStatus.defaultProvider,
          defaultModel: providerStatus.defaultModel,
          selectedProjectSlug,
          taskSourceType,
          selectedProject,
          goal: "",
          notes: "",
        }),
      });
      const session = response?.session;
      if (session?.session_id) {
        setPlannerSessions((current) =>
          [
            plannerSessionRowFromRecord(session),
            ...current.filter((row) => row.id !== session.session_id),
          ].sort((left, right) => right.updatedAtMs - left.updatedAtMs)
        );
        clearPlannerComposerDraft(selectedProjectSlug, session.session_id);
        activatePlannerSession(session.session_id);
        hydrateDraft(planningDraftFromSession(session));
        setPlannerInput(
          initialComposerDraft || loadPlannerComposerDraft(selectedProjectSlug, session.session_id)
        );
        toast("ok", "Started a new planner session.");
      }
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  };

  const renamePlannerSession = async (sessionId: string) => {
    const session = plannerSessions.find((row) => row.id === sessionId);
    if (!session || !client?.workflowPlannerSessions?.patch) return;
    setRenameSessionDialog({
      sessionId,
      value: session.title || "Untitled plan",
    });
  };

  const confirmRenamePlannerSession = async () => {
    const dialog = renameSessionDialog;
    if (!dialog || !client?.workflowPlannerSessions?.patch) return;
    const title = safeString(dialog.value);
    if (!title) {
      toast("warn", "Enter a session name.");
      return;
    }
    try {
      const response = await client.workflowPlannerSessions.patch(dialog.sessionId, { title });
      const updated = response?.session;
      if (updated?.session_id) {
        setPlannerSessions((current) =>
          current
            .filter((row) => row.id !== updated.session_id)
            .concat(plannerSessionRowFromRecord(updated))
            .sort((left, right) => right.updatedAtMs - left.updatedAtMs)
        );
        setPlannerSessionCache((current) => ({
          ...current,
          [updated.session_id]: {
            ...(current[updated.session_id] || {
              session: null,
              draft: null,
              clarification: { status: "none" },
              planPreview: null,
              planningConversation: null,
              planningChangeSummary: [],
              plannerDiagnostics: null,
              publishedTasks: [],
            }),
            session: updated,
          },
        }));
        if (selectedSessionIdRef.current === dialog.sessionId) {
          activatePlannerSession(updated.session_id);
        }
      }
      setRenameSessionDialog(null);
      toast("ok", "Planner session renamed.");
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  };

  const duplicatePlannerSession = async (sessionId: string) => {
    const session = plannerSessions.find((row) => row.id === sessionId);
    if (!session || !client?.workflowPlannerSessions?.duplicate) return;
    try {
      const response = await client.workflowPlannerSessions.duplicate(sessionId, {
        title: `Copy of ${session.title || "Untitled plan"}`,
      });
      const copied = response?.session;
      if (copied?.session_id) {
        setPlannerSessions((current) =>
          current
            .filter((row) => row.id !== copied.session_id)
            .concat(plannerSessionRowFromRecord(copied))
            .sort((left, right) => right.updatedAtMs - left.updatedAtMs)
        );
        clearPlannerComposerDraft(selectedProjectSlug, copied.session_id);
        activatePlannerSession(copied.session_id);
      }
      toast("ok", "Planner session duplicated.");
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  };

  const deletePlannerSession = async (sessionId: string) => {
    const session = plannerSessions.find((row) => row.id === sessionId);
    if (!session) return;
    setDeleteSessionDialog({
      sessionId,
      title: session.title || "Untitled plan",
    });
  };

  const confirmDeletePlannerSession = async () => {
    const dialog = deleteSessionDialog;
    if (!dialog) return;
    try {
      await client.workflowPlannerSessions.delete(dialog.sessionId);
      const remaining = plannerSessions.filter((row) => row.id !== dialog.sessionId);
      setPlannerSessions(remaining);
      setPlannerSessionCache((current) => {
        const next = { ...current };
        delete next[dialog.sessionId];
        return next;
      });
      clearPlannerComposerDraft(selectedProjectSlug, dialog.sessionId);
      if (selectedSessionIdRef.current === dialog.sessionId) {
        const nextSessionId = remaining[0]?.id || "";
        if (nextSessionId) {
          activatePlannerSession(nextSessionId);
        } else {
          void createNewPlannerSession();
        }
      }
      setDeleteSessionDialog(null);
      toast("ok", "Planner session deleted.");
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  };

  const previewMutation = async (messageOverride?: string) => {
    const trimmedGoal = safeString(messageOverride ?? plannerInput ?? goal);
    const trimmedWorkspaceRoot = safeString(workspaceRoot);
    if (!trimmedGoal) {
      toast("warn", "Describe the goal you want to turn into tasks.");
      return;
    }
    if (!trimmedWorkspaceRoot) {
      toast("warn", "Workspace root is required so the planner can inspect the repo.");
      return;
    }
    if (!trimmedWorkspaceRoot.startsWith("/")) {
      toast("warn", "Workspace root must be an absolute path.");
      return;
    }
    if (!selectedSessionId) {
      toast("warn", "Start a planner session first.");
      return;
    }
    if (!client?.workflowPlannerSessions?.start) {
      toast("err", "This control panel build is missing workflow planner support.");
      return;
    }
    setSaveStatus("");
    setPlannerError("");
    setPlannerDiagnostics(null);
    setGoal(trimmedGoal);
    let nextState: "idle" | "clarifying" = "idle";
    setPlanningState("generating");
    try {
      const response = await client.workflowPlannerSessions.start(selectedSessionId, {
        prompt: [
          `Create a Tandem workflow plan for the repo at ${trimmedWorkspaceRoot} to implement this goal: ${trimmedGoal}`,
          "Use small ordered implementation steps.",
          "Prefer step objectives that mention likely files, modules, or repo areas to inspect or change.",
          "If the target or desired outcome is ambiguous, return a clarification question instead of guessing.",
          safeString(selectedProject?.repoUrl)
            ? `Repo URL: ${safeString(selectedProject?.repoUrl)}`
            : "",
          selectedProjectSlug ? `Selected project: ${selectedProjectSlug}` : "",
          notes ? `User feedback / revision notes:\n${notes}` : "",
        ]
          .filter(Boolean)
          .join("\n"),
        plan_source: "coding_task_planning",
        workspace_root: trimmedWorkspaceRoot,
        allowed_mcp_servers: connectedMcpServers,
        operator_preferences: plannerOperatorPreferences({
          plannerProvider,
          plannerModel,
          defaultProvider: providerStatus.defaultProvider,
          defaultModel: providerStatus.defaultModel,
          selectedProjectSlug,
          taskSourceType,
          selectedProject,
          goal: trimmedGoal,
          notes,
        }),
      });
      setPlanPreview(response?.plan || null);
      setPlanningConversation(response?.conversation || null);
      setPlanningChangeSummary([]);
      setPlannerDiagnostics(response?.planner_diagnostics || response?.plannerDiagnostics || null);
      const clarQuestion =
        typeof response?.clarifier?.question === "string"
          ? String(response.clarifier.question)
          : "";
      const clarOptions: ClarifierOption[] = Array.isArray(response?.clarifier?.options)
        ? response.clarifier.options
            .map((o: any) => ({ id: String(o?.id || ""), label: String(o?.label || "") }))
            .filter((o: ClarifierOption) => o.id && o.label)
        : [];
      setPlannerError(clarQuestion);
      if (clarQuestion && clarOptions.length > 0) {
        setClarification({
          status: "waiting",
          question: clarQuestion,
          options: clarOptions,
        });
        nextState = "clarifying";
      } else {
        setClarification({ status: "none" });
      }
      setLastSavedAtMs(Date.now());
      setPublishedTasks([]);
      setSaveStatus("Plan synced to session.");
      setPlannerInput("");
      clearPlannerComposerDraft(selectedProjectSlug, selectedSessionId);
      updateActivePlannerSessionCache({
        session: response?.session || null,
        draft: response?.session ? planningDraftFromSession(response.session) : null,
        clarification:
          clarQuestion && clarOptions.length > 0
            ? {
                status: "waiting",
                question: clarQuestion,
                options: clarOptions,
              }
            : { status: "none" },
        planPreview: response?.plan || null,
        planningConversation: response?.conversation || null,
        planningChangeSummary: [],
        plannerDiagnostics: response?.planner_diagnostics || response?.plannerDiagnostics || null,
        publishedTasks: [],
      });
      toast(
        "ok",
        typeof response?.clarifier?.question === "string"
          ? "Planner needs clarification before it can draft richer tasks."
          : "Planner generated a task draft."
      );
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (isMissingWorkflowPlanError(message)) {
        setPlanPreview(null);
        setPlanningConversation(null);
        setPlanningChangeSummary([]);
        setPlannerError("");
        setPlannerDiagnostics(null);
        setClarification({ status: "none" });
        toast("info", "The previous planner session expired. Starting a fresh planning chat.");
        await previewMutation(trimmedGoal);
        return;
      }
      setPlannerError(message);
      setClarification({ status: "none" });
      setSaveStatus(
        message.includes("Request timed out after")
          ? "The panel timed out before the planner finished. The engine may still be working, so wait a moment before retrying."
          : ""
      );
      toast("err", message);
    } finally {
      setPlanningState(nextState);
    }
  };

  const reviseMutation = async (messageOverride?: string) => {
    if (!client?.workflowPlannerSessions?.message || !selectedSessionId) {
      toast("warn", "Generate a plan first before revising it.");
      return;
    }
    const trimmedNotes = safeString(messageOverride ?? plannerInput ?? notes);
    if (!trimmedNotes) {
      toast("warn", "Add a message for the planner first.");
      return;
    }
    setSaveStatus("");
    setPlannerError("");
    setPlannerDiagnostics(null);
    setNotes(trimmedNotes);
    let nextState: "idle" | "clarifying" = "idle";
    setPlanningState("revising");
    try {
      const response = await client.workflowPlannerSessions.message(selectedSessionId, {
        message: clarificationNeeded
          ? trimmedNotes
          : [
              `Revise the workflow plan for repo ${safeString(workspaceRoot)}.`,
              `Goal: ${safeString(goal)}`,
              selectedProjectSlug ? `Selected project: ${selectedProjectSlug}` : "",
              "Keep the workflow valid and machine-parseable.",
              "Prefer concrete repo-aware implementation steps.",
              "If the request is ambiguous, return a clarification question.",
              "",
              "Revision notes:",
              trimmedNotes,
            ]
              .filter(Boolean)
              .join("\n"),
      });
      setPlanPreview(response?.plan || null);
      setPlanningConversation(response?.conversation || null);
      setPlanningChangeSummary(
        Array.isArray(response?.change_summary)
          ? response.change_summary.map((row: any) => safeString(row)).filter(Boolean)
          : []
      );
      setPlannerDiagnostics(response?.planner_diagnostics || response?.plannerDiagnostics || null);
      const clarQuestion =
        typeof response?.clarifier?.question === "string"
          ? String(response.clarifier.question)
          : "";
      const clarOptions: ClarifierOption[] = Array.isArray(response?.clarifier?.options)
        ? response.clarifier.options
            .map((o: any) => ({ id: String(o?.id || ""), label: String(o?.label || "") }))
            .filter((o: ClarifierOption) => o.id && o.label)
        : [];
      setPlannerError(clarQuestion);
      if (clarQuestion && clarOptions.length > 0) {
        setClarification({
          status: "waiting",
          question: clarQuestion,
          options: clarOptions,
        });
        nextState = "clarifying";
      } else {
        setClarification({ status: "none" });
      }
      setLastSavedAtMs(Date.now());
      setPublishedTasks([]);
      setSaveStatus("Revision synced to session.");
      setPlannerInput("");
      clearPlannerComposerDraft(selectedProjectSlug, selectedSessionId);
      updateActivePlannerSessionCache({
        session: response?.session || null,
        draft: response?.session ? planningDraftFromSession(response.session) : null,
        clarification:
          clarQuestion && clarOptions.length > 0
            ? {
                status: "waiting",
                question: clarQuestion,
                options: clarOptions,
              }
            : { status: "none" },
        planPreview: response?.plan || null,
        planningConversation: response?.conversation || null,
        planningChangeSummary: Array.isArray(response?.change_summary)
          ? response.change_summary.map((row: any) => safeString(row)).filter(Boolean)
          : [],
        plannerDiagnostics: response?.planner_diagnostics || response?.plannerDiagnostics || null,
        publishedTasks: [],
      });
      toast("ok", "Plan revised.");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (isMissingWorkflowPlanError(message)) {
        setPlanPreview(null);
        setPlanningConversation(null);
        setPlanningChangeSummary([]);
        setPlannerError("");
        setPlannerDiagnostics(null);
        setClarification({ status: "none" });
        toast("info", "The previous planner session expired. Starting a fresh planning chat.");
        await previewMutation(trimmedNotes);
        return;
      }
      setPlannerError(message);
      setClarification({ status: "none" });
      setSaveStatus(
        message.includes("Request timed out after")
          ? "The panel timed out before the planner revision finished. The engine may still be working, so wait a moment before retrying."
          : ""
      );
      toast("err", message);
    } finally {
      setPlanningState(nextState);
    }
  };

  const resetMutation = async () => {
    if (!client?.workflowPlannerSessions?.reset || !selectedSessionId) {
      setGoal("");
      setPlannerInput("");
      setNotes("");
      setPlanPreview(null);
      setPlanningConversation(null);
      setPlanningChangeSummary([]);
      setPlannerError("");
      setPlannerDiagnostics(null);
      setClarification({ status: "none" });
      setPublishStatus("");
      setPublishedAtMs(null);
      setLastSavedAtMs(null);
      setSaveStatus("Draft cleared.");
      return;
    }
    setResetting(true);
    try {
      const response = await client.workflowPlannerSessions.reset(selectedSessionId);
      setPlanPreview(response?.plan || null);
      setPlanningConversation(response?.conversation || null);
      setPlannerInput("");
      setPlanningChangeSummary([]);
      setPlannerError("");
      setPlannerDiagnostics(response?.planner_diagnostics || response?.plannerDiagnostics || null);
      setPublishedTasks([]);
      setPublishStatus("");
      setPublishedAtMs(null);
      setSaveStatus("Plan reset.");
      setLastSavedAtMs(Date.now());
      clearPlannerComposerDraft(selectedProjectSlug, selectedSessionId);
      updateActivePlannerSessionCache({
        session: response?.session ? { ...response.session, published_at_ms: null } : null,
        draft: response?.session ? planningDraftFromSession(response.session) : null,
        clarification: { status: "none" },
        planPreview: response?.plan || null,
        planningConversation: response?.conversation || null,
        planningChangeSummary: [],
        plannerDiagnostics: response?.planner_diagnostics || response?.plannerDiagnostics || null,
        publishedTasks: [],
      });
      toast("ok", "Plan reset.");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (isMissingWorkflowPlanError(message)) {
        setGoal("");
        setPlannerInput("");
        setNotes("");
        setPlanPreview(null);
        setPlanningConversation(null);
        setPlanningChangeSummary([]);
        setPlannerError("");
        setPlannerDiagnostics(null);
        setClarification({ status: "none" });
        setPublishStatus("");
        setPublishedAtMs(null);
        setLastSavedAtMs(null);
        setSaveStatus("The expired planner session was cleared locally.");
        toast("info", "The previous planner session had expired, so the local draft was reset.");
        return;
      }
      setPlannerError(message);
      toast("err", message);
    } finally {
      setResetting(false);
    }
  };

  const publishTasks = async () => {
    const tasks = plannedTasks.length
      ? plannedTasks
      : goal.trim()
        ? [
            {
              id: "task-1",
              title: goal.trim(),
              objective: goal.trim(),
              kind: "task",
              dependsOn: [],
              inputRefs: [],
              outputContract: "",
            },
          ]
        : [];
    if (!tasks.length) {
      toast("warn", "Generate a plan before publishing tasks.");
      return;
    }
    setPublishing(true);
    setPublishStatus("");
    try {
      if (isGitHubProject && canPublishToGitHub) {
        const source = selectedProject?.taskSource || {};
        const owner = safeString(source.owner);
        const repo = safeString(source.repo);
        const projectNumber = Number(source.project);
        let projectBoardSnapshot = githubProjectBoardSnapshot;
        const readyStatusFieldId = Number(
          projectBoardSnapshot?.status_field_id || projectBoardSnapshot?.statusFieldId || 0
        );
        const readyOptionId = safeString(
          projectBoardSnapshot?.status_option_map?.ready ||
            projectBoardSnapshot?.statusOptionMap?.ready ||
            projectBoardSnapshot?.status_option_map?.Ready ||
            projectBoardSnapshot?.statusOptionMap?.Ready ||
            ""
        );
        if ((!readyStatusFieldId || !readyOptionId) && safeString(selectedProjectSlug)) {
          try {
            projectBoardSnapshot = await api(
              `/api/aca/projects/${encodeURIComponent(selectedProjectSlug)}/board?refresh=true`
            );
          } catch {
            // Fall back to the cached snapshot if a live refresh fails.
          }
        }
        const publishedTasks: PlanningDraft["publishedTasks"] = [];
        for (let index = 0; index < tasks.length; index += 1) {
          const task = tasks[index];
          const body = buildTaskMarkdown(planPreview, task, index, {
            goal,
            workspaceRoot,
            projectSlug: selectedProjectSlug,
            taskSourceType,
            selectedProject,
            notes,
            plannerProvider,
            plannerModel,
          });
          const issueResult = await client.executeTool("mcp.github.issue_write", {
            method: "create",
            owner,
            repo,
            title: task.title,
            body,
            labels: ["tandem", "planned-task", "ready"],
          });
          const issueNumber = findIssueNumber(issueResult);
          if (!issueNumber) {
            throw new Error(
              `Created GitHub issue for "${task.title}" but could not determine the issue number from the engine response.`
            );
          }
          const addProjectItemResult = await client.executeTool("mcp.github.projects_write", {
            method: "add_project_item",
            owner,
            project_number: projectNumber,
            item_type: "issue",
            item_owner: owner,
            item_repo: repo,
            issue_number: issueNumber,
          });
          const projectItemId = findProjectItemId(addProjectItemResult);
          if (projectItemId && readyStatusFieldId && readyOptionId) {
            await client.executeTool("mcp.github.projects_write", {
              method: "update_project_item",
              owner,
              project_number: projectNumber,
              item_id: projectItemId,
              updated_field: {
                id: readyStatusFieldId,
                value: readyOptionId,
              },
            });
          }
          publishedTasks.push({
            title: task.title,
            issueNumber,
            issueUrl: `https://github.com/${owner}/${repo}/issues/${issueNumber}`,
            publishedAtMs: Date.now(),
          });
        }
        const publishedAt = Date.now();
        setPublishedTasks(publishedTasks);
        setPublishStatus(`Published ${publishedTasks.length} GitHub tasks into Ready.`);
        setPublishedAtMs(publishedAt);
        setLastSavedAtMs(publishedAt);
        updateActivePlannerSessionCache({
          session: { published_at_ms: publishedAt, published_tasks: publishedTasks },
          publishedTasks,
        });
        await Promise.all([
          queryClient.invalidateQueries({
            queryKey: ["coding-workflows", "aca-project-board", selectedProjectSlug],
          }),
          queryClient.invalidateQueries({
            queryKey: ["coding-workflows", "aca-project-tasks", selectedProjectSlug],
          }),
          queryClient.invalidateQueries({
            queryKey: ["coding-workflows", "aca-projects"],
          }),
        ]);
        toast("ok", `Published ${publishedTasks.length} task(s) to the GitHub Project.`);
      } else {
        const exportMarkdown = buildExportMarkdown(planPreview, tasks, {
          goal,
          workspaceRoot,
          projectSlug: selectedProjectSlug,
          notes,
          plannerProvider,
          plannerModel,
        });
        const publishedAt = Date.now();
        setPublishedTasks(
          tasks.map((task) => ({
            title: task.title,
            publishedAtMs: publishedAt,
          }))
        );
        setPublishStatus(
          "Saved a local task bundle. Use the exported markdown to update your kanban file."
        );
        setPublishedAtMs(publishedAt);
        setLastSavedAtMs(publishedAt);
        updateActivePlannerSessionCache({
          session: { published_at_ms: publishedAt, published_tasks: tasks },
          publishedTasks: tasks.map((task) => ({
            title: task.title,
            publishedAtMs: publishedAt,
          })),
        });
        if (navigator.clipboard?.writeText) {
          await navigator.clipboard.writeText(exportMarkdown);
        }
        toast(
          "ok",
          "Saved a local task bundle. The plan markdown has been copied to your clipboard."
        );
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setPublishStatus(message);
      toast("err", message);
    } finally {
      setPublishing(false);
    }
  };

  const exportMarkdown = useMemo(
    () =>
      buildExportMarkdown(planPreview, plannedTasks, {
        goal,
        workspaceRoot,
        projectSlug: selectedProjectSlug,
        notes,
        plannerProvider,
        plannerModel,
      }),
    [
      goal,
      notes,
      planPreview,
      plannedTasks,
      plannerModel,
      plannerProvider,
      selectedProjectSlug,
      workspaceRoot,
    ]
  );

  const plannerTitle = isGitHubProject
    ? "GitHub Project task planning"
    : "Local kanban task planning";
  const hasPlannerResponse =
    !!planPreview ||
    !!plannerDiagnostics ||
    planningChangeSummary.length > 0 ||
    (Array.isArray(planningConversation?.messages) && planningConversation.messages.length > 0);

  useEffect(() => {
    if (rootRef.current) renderIcons(rootRef.current);
  }, [
    clarification.status,
    displayTasks.length,
    hasPlannerResponse,
    isPlanning,
    plannerError,
    planPreview,
    planningChangeSummary.length,
    planningConversation,
    plannerSessionSummaries.length,
    selectedSessionId,
    sessionsOpen,
    publishStatus,
    saveStatus,
  ]);

  return (
    <TaskPlanningPanelView
      rootRef={rootRef}
      session={{
        plannerSessions,
        selectedSessionId,
        sessionsOpen,
        plannerSessionSummaries,
        activePlannerSessionSummary,
        plannerTitle,
      }}
      planner={{
        goal,
        workspaceRoot,
        notes,
        plannerInput,
        plannerProvider,
        plannerModel,
        planPreview,
        planningConversation,
        planningChangeSummary,
        plannerError,
        plannerDiagnostics,
        publishing,
        resetting,
        clarification,
        saveStatus,
        publishStatus,
        lastSavedAtMs,
        publishedAtMs,
        publishedTasks,
        plannerChatMessages,
        plannerInputPlaceholder,
        plannerStatusTitle,
        plannerStatusDetail,
        exportMarkdown,
        plannerFallbackReason,
        hasPlannerResponse,
        displayTasks,
      }}
      status={{
        isGitHubProject,
        taskSourceType,
        engineHealthy,
        plannerCanUseLlm,
        hasExplicitPlannerOverride,
        hasBasePlannerModel,
        plannerSelectionMatchesWorkspaceDefault,
        resolvedWorkspaceRoot,
        canPublishToGitHub,
        plannerTimedOut,
        clarificationNeeded,
        planIsFallbackOnly,
        isPlanning,
      }}
      provider={{
        providerOptions,
        providerStatus,
        selectedProjectSlug,
        selectedProject,
        githubProjectBoardSnapshot,
        connectedMcpServers,
      }}
      actions={{
        activatePlannerSession,
        createNewPlannerSession,
        renamePlannerSession,
        duplicatePlannerSession,
        deletePlannerSession,
        setSessionsOpen,
        previewMutation,
        reviseMutation,
        resetMutation,
        patchActivePlannerSession,
        setPlannerInput,
        setGoal,
        setWorkspaceRoot,
        setNotes,
        setPlannerProvider,
        setPlannerModel,
        setPlannerError,
        setPlannerDiagnostics,
        setClarification,
        setSaveStatus,
        publishTasks,
        plannerOperatorPreferences,
        plannerSessionTitle,
      }}
      dialogs={{
        renameSessionDialog,
        deleteSessionDialog,
        confirmRenamePlannerSession,
        confirmDeletePlannerSession,
        setRenameSessionDialog,
        setDeleteSessionDialog,
      }}
    />
  );
}
