import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ChatInterfacePanel } from "../components/ChatInterfacePanel";
import { AnimatedPage, Badge, PanelCard } from "../ui/index.tsx";
import type { AppPageProps } from "./pageTypes";
import {
  IntentBriefPanel,
  type IntentBriefDraft,
  type PlannerHorizon,
  type PlannerTargetSurface,
} from "../features/planner/IntentBriefPanel";
import { PlanSummaryPanel } from "../features/planner/PlanSummaryPanel";
import { TimelinePlanPanel } from "../features/planner/TimelinePlanPanel";
import { PlanValidationPanel } from "../features/planner/PlanValidationPanel";
import { PlanOverlapPanel } from "../features/planner/PlanOverlapPanel";
import { PlanReplayPanel } from "../features/planner/PlanReplayPanel";
import { PlannerDiagnosticsPanel } from "../features/planner/PlannerDiagnosticsPanel";
import { PlannerHandoffPanel } from "../features/planner/PlannerHandoffPanel";
import { PlanFormationPanel } from "../features/planner/PlanFormationPanel";
import { PlanGenerationAnimation } from "../features/planner/PlanGenerationAnimation";
import { PlannerDraftList } from "../features/planner/PlannerDraftList";
import {
  appendPlannerDraftHistory,
  clearPlannerDraft,
  deleteNamedPlannerDraft,
  deletePlannerDraftHistoryEntry,
  loadPlannerDraft,
  loadPlannerDraftHistoryEntry,
  namedPlannerDraftStoragePrefix,
  plannerDraftHistoryStorageKey,
  plannerDraftStorageKey,
  saveNamedPlannerDraft,
  savePlannerDraft,
} from "../features/planner/plannerDraftStorage";
import {
  buildPlannerProviderOptions,
  buildDefaultKnowledgeOperatorPreferences,
  buildKnowledgeRolloutGuidance,
  normalizePlannerConversationMessages,
  type PlannerProviderOption,
} from "../features/planner/plannerShared";
import { renderIcons } from "../app/icons.js";

type ClarifierOption = {
  id: string;
  label: string;
};

type PlannerDraftSnapshot = {
  updatedAtMs?: number;
  name?: string;
  title?: string;
  brief?: Partial<IntentBriefDraft> | null;
  goal?: string;
  workspaceRoot?: string;
  targetSurface?: PlannerTargetSurface;
  planningHorizon?: PlannerHorizon;
  outputExpectations?: string;
  constraints?: string;
  plannerProvider?: string;
  plannerModel?: string;
  selectedMcpServers?: string[];
  plannerInput?: string;
  planPreview?: any;
  planPreviewJson?: any;
  plan?: any;
  planningConversation?: any;
  conversation?: any;
  planningChangeSummary?: string[];
  changeSummary?: string[];
  plannerError?: string;
  plannerDiagnostics?: any;
  validationReport?: any;
  planPackage?: any;
  planPackageBundle?: any;
  planPackageReplay?: any;
  overlapAnalysis?: any;
  teachingLibrary?: any;
};

type PlanningState = "idle" | "generating" | "revising" | "clarifying";

const DEFAULT_BRIEF: IntentBriefDraft = {
  goal: "",
  workspaceRoot: "",
  targetSurface: "mission",
  planningHorizon: "mixed",
  outputExpectations: "",
  constraints: "",
  plannerProvider: "",
  plannerModel: "",
  selectedMcpServers: [],
};

function safeString(value: unknown) {
  return String(value || "").trim();
}

function normalizeServers(raw: any) {
  const rows = Array.isArray(raw)
    ? raw
    : Array.isArray(raw?.servers)
      ? raw.servers
      : raw && typeof raw === "object"
        ? Object.entries(raw).map(([name, row]) => ({ name, ...(row as any) }))
        : [];
  return rows
    .map((row: any) => ({
      name: String(row?.name || "").trim(),
      connected: !!row?.connected,
      enabled: row?.enabled !== false,
      transport: String(row?.transport || "").trim(),
      lastError: String(row?.last_error || row?.lastError || "").trim(),
    }))
    .filter((row: any) => row.name);
}

function validateWorkspaceRootInput(raw: string) {
  const value = safeString(raw);
  if (!value) return "Workspace root is required.";
  if (!value.startsWith("/")) return "Workspace root must be an absolute path.";
  return "";
}

function horizonPromptLabel(horizon: PlannerHorizon) {
  switch (horizon) {
    case "same_day":
      return "same-day split work";
    case "multi_day":
      return "multi-day plan";
    case "weekly":
      return "weekly recurring work";
    case "monthly":
      return "monthly recurring work";
    case "mixed":
    default:
      return "mixed one-time and recurring work";
  }
}

function targetPromptLabel(targetSurface: PlannerTargetSurface) {
  switch (targetSurface) {
    case "automation":
      return "workflow automation";
    case "mission":
      return "mission execution";
    case "coding":
      return "coding workflow";
    case "orchestrator":
    default:
      return "orchestrator handoff";
  }
}

function plannerPromptFromBrief(brief: IntentBriefDraft) {
  const goal = safeString(brief.goal);
  return [
    goal,
    `Planning horizon: ${horizonPromptLabel(brief.planningHorizon)}.`,
    `Target surface: ${targetPromptLabel(brief.targetSurface)}.`,
    safeString(brief.workspaceRoot) ? `Workspace root: ${safeString(brief.workspaceRoot)}` : "",
    safeString(brief.outputExpectations)
      ? `Expected outputs:\n${safeString(brief.outputExpectations)}`
      : "",
    safeString(brief.constraints) ? `Constraints:\n${safeString(brief.constraints)}` : "",
    "Default knowledge behavior: project-scoped preflight reuse with promoted trust floor.",
    "Use clarifying questions instead of guessing when important details are missing.",
    "Prefer multi-agent decomposition, milestones, and timeline-aware work waves when appropriate.",
    "Return a workflow plan that can later be handed off to Automations, Coding, or Orchestrator.",
  ]
    .filter(Boolean)
    .join("\n");
}

const WORKFLOW_IMPORT_HANDOFF_KEY = "tandem.workflow.importHandoff.v1";

function loadWorkflowImportHandoff() {
  try {
    const raw = localStorage.getItem(WORKFLOW_IMPORT_HANDOFF_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return null;
    return parsed as {
      session_id?: string;
      title?: string;
      project_slug?: string;
      source_kind?: string;
      source_bundle_digest?: string | null;
      current_plan_id?: string | null;
    };
  } catch {
    return null;
  }
}

export function IntentPlannerPage({
  api,
  client,
  toast,
  providerStatus,
  identity,
  navigate,
}: AppPageProps) {
  const [brief, setBrief] = useState<IntentBriefDraft>(DEFAULT_BRIEF);
  const [plannerInput, setPlannerInput] = useState("");
  const [planningState, setPlanningState] = useState<PlanningState>("idle");
  const [planPreview, setPlanPreview] = useState<any>(null);
  const [planningConversation, setPlanningConversation] = useState<any>(null);
  const [planningChangeSummary, setPlanningChangeSummary] = useState<string[]>([]);
  const [plannerError, setPlannerError] = useState("");
  const [plannerDiagnostics, setPlannerDiagnostics] = useState<any>(null);
  const [clarification, setClarification] = useState<{
    status: "none" | "waiting";
    question?: string;
    options?: ClarifierOption[];
  }>({ status: "none" });
  const [validationReport, setValidationReport] = useState<any>(null);
  const [planPackage, setPlanPackage] = useState<any>(null);
  const [planPackageBundle, setPlanPackageBundle] = useState<any>(null);
  const [planPackageReplay, setPlanPackageReplay] = useState<any>(null);
  const [overlapAnalysis, setOverlapAnalysis] = useState<any>(null);
  const [teachingLibrary, setTeachingLibrary] = useState<any>(null);
  const [namedDraftName, setNamedDraftName] = useState("");
  const [plannerDraftHydrated, setPlannerDraftHydrated] = useState(false);
  const [plannerDraftUpdatedAtMs, setPlannerDraftUpdatedAtMs] = useState<number | null>(null);
  const [plannerDraftRestored, setPlannerDraftRestored] = useState(false);
  const [importedWorkflowSessionApplied, setImportedWorkflowSessionApplied] = useState("");
  const [workflowImportHandoff, setWorkflowImportHandoff] =
    useState<ReturnType<typeof loadWorkflowImportHandoff>>(null);
  const [workspaceBrowserOpen, setWorkspaceBrowserOpen] = useState(false);
  const [workspaceBrowserDir, setWorkspaceBrowserDir] = useState("");
  const [workspaceBrowserSearch, setWorkspaceBrowserSearch] = useState("");

  const draftKey = useMemo(() => plannerDraftStorageKey(identity.botName), [identity.botName]);
  const namedDraftPrefix = useMemo(
    () => namedPlannerDraftStoragePrefix(identity.botName),
    [identity.botName]
  );
  const historyKey = useMemo(
    () => plannerDraftHistoryStorageKey(identity.botName),
    [identity.botName]
  );

  const providersCatalogQuery = useQuery({
    queryKey: ["intent-planner", "providers", "catalog"],
    queryFn: () => client.providers.catalog().catch(() => ({ all: [] })),
    refetchInterval: 30000,
  });
  const providersConfigQuery = useQuery({
    queryKey: ["intent-planner", "providers", "config"],
    queryFn: () => client.providers.config().catch(() => ({})),
    refetchInterval: 30000,
  });
  const mcpServersQuery = useQuery({
    queryKey: ["intent-planner", "mcp-servers"],
    queryFn: () => client.mcp.list().catch(() => ({})),
    refetchInterval: 10000,
  });
  const healthQuery = useQuery({
    queryKey: ["global", "health"],
    queryFn: () => client.health().catch(() => ({})),
    refetchInterval: 30000,
  });
  const workspaceBrowserQuery = useQuery({
    queryKey: ["intent-planner", "workspace-browser", workspaceBrowserDir],
    enabled: workspaceBrowserOpen && !!workspaceBrowserDir,
    queryFn: () =>
      api(`/api/orchestrator/workspaces/list?dir=${encodeURIComponent(workspaceBrowserDir)}`, {
        method: "GET",
      }).catch(() => ({})),
  });
  const importedWorkflowSessionQuery = useQuery({
    queryKey: ["intent-planner", "workflow-import-handoff", workflowImportHandoff?.session_id],
    enabled: !!workflowImportHandoff?.session_id,
    queryFn: () =>
      client.workflowPlannerSessions
        .get(String(workflowImportHandoff?.session_id || ""))
        .catch(() => ({ session: null })),
  });

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

  const mcpServers = useMemo(() => normalizeServers(mcpServersQuery.data), [mcpServersQuery.data]);
  const connectedMcpServers = useMemo(
    () => mcpServers.filter((server: any) => server.connected).map((server: any) => server.name),
    [mcpServers]
  );
  const plannerCanUseLlm = !!(
    (safeString(brief.plannerProvider) && safeString(brief.plannerModel)) ||
    (safeString(providerStatus.defaultProvider) && safeString(providerStatus.defaultModel))
  );
  const plannerChatMessages = useMemo(
    () => normalizePlannerConversationMessages(planningConversation),
    [planningConversation]
  );
  const isPlanning = planningState === "generating" || planningState === "revising";
  const hasPlan = !!safeString(planPreview?.plan_id || planPreview?.planId);
  const hasGeneratedDraft = !!(
    hasPlan ||
    planPackage ||
    validationReport ||
    overlapAnalysis ||
    planPackageReplay ||
    planningChangeSummary.length
  );
  const showThirdColumn = hasGeneratedDraft;
  const plannerStatusTitle =
    planningState === "generating"
      ? "Drafting plan"
      : planningState === "revising"
        ? "Revising plan"
        : planningState === "clarifying"
          ? "Waiting for clarification"
          : "";
  const plannerStatusDetail =
    planningState === "clarifying"
      ? "Reply to the planner's question so it can continue shaping the mission."
      : "The planner is composing a governed workflow plan from your intent.";
  const workspaceDirectories = Array.isArray((workspaceBrowserQuery.data as any)?.directories)
    ? (workspaceBrowserQuery.data as any).directories
    : [];
  const importedWorkflowSession = importedWorkflowSessionQuery.data?.session || null;
  const importedWorkflowSourceKind = safeString(
    importedWorkflowSession?.source_kind || workflowImportHandoff?.source_kind || ""
  );
  const workspaceParentDir = String((workspaceBrowserQuery.data as any)?.parent || "").trim();
  const workspaceCurrentBrowseDir = String(
    (workspaceBrowserQuery.data as any)?.dir || workspaceBrowserDir || ""
  ).trim();
  const filteredWorkspaceDirectories = useMemo(() => {
    const search = String(workspaceBrowserSearch || "")
      .trim()
      .toLowerCase();
    if (!search) return workspaceDirectories;
    return workspaceDirectories.filter((entry: any) =>
      String(entry?.name || entry?.path || "")
        .trim()
        .toLowerCase()
        .includes(search)
    );
  }, [workspaceBrowserSearch, workspaceDirectories]);
  const workspaceRootError = validateWorkspaceRootInput(brief.workspaceRoot);

  useEffect(() => {
    const suggestedWorkspaceRoot = String(
      (healthQuery.data as any)?.workspaceRoot || (healthQuery.data as any)?.workspace_root || ""
    ).trim();
    if (!suggestedWorkspaceRoot) return;
    setBrief((current) =>
      safeString(current.workspaceRoot)
        ? current
        : {
            ...current,
            workspaceRoot: suggestedWorkspaceRoot,
          }
    );
  }, [healthQuery.data]);

  useEffect(() => {
    setWorkflowImportHandoff(loadWorkflowImportHandoff());
  }, []);

  useEffect(() => {
    if (!workflowImportHandoff?.session_id || !importedWorkflowSession) return;
    if (importedWorkflowSession.session_id !== workflowImportHandoff.session_id) return;
    if (importedWorkflowSessionApplied === importedWorkflowSession.session_id) return;

    const importedPlan =
      importedWorkflowSession.draft?.current_plan || importedWorkflowSession.draft?.initial_plan;
    if (!importedPlan) return;

    setBrief({
      ...DEFAULT_BRIEF,
      goal: safeString(
        importedWorkflowSession.goal ||
          importedWorkflowSession.title ||
          importedPlan.original_prompt ||
          importedPlan.title
      ),
      workspaceRoot: safeString(
        importedWorkflowSession.workspace_root || importedPlan.workspace_root
      ),
      targetSurface: DEFAULT_BRIEF.targetSurface,
      planningHorizon: DEFAULT_BRIEF.planningHorizon,
      outputExpectations: safeString(importedPlan.description),
      constraints: "",
      plannerProvider: safeString(importedWorkflowSession.planner_provider),
      plannerModel: safeString(importedWorkflowSession.planner_model),
      selectedMcpServers: Array.isArray(importedPlan.allowed_mcp_servers)
        ? importedPlan.allowed_mcp_servers.map((row: any) => safeString(row)).filter(Boolean)
        : [],
    });
    setPlannerInput(
      safeString(
        importedWorkflowSession.goal ||
          importedWorkflowSession.title ||
          importedPlan.normalized_prompt ||
          importedPlan.original_prompt
      )
    );
    setPlanPreview(importedPlan);
    setPlanningConversation(importedWorkflowSession.draft?.conversation || null);
    setPlanningChangeSummary([]);
    setPlannerError("");
    setPlannerDiagnostics(
      importedWorkflowSession.import_scope_snapshot ||
        importedWorkflowSession.draft?.planner_diagnostics ||
        null
    );
    setValidationReport(null);
    setPlanPackage(null);
    setPlanPackageBundle(null);
    setPlanPackageReplay(null);
    setOverlapAnalysis(null);
    setTeachingLibrary(null);
    setPlanningState("idle");
    setPlannerDraftRestored(true);
    setPlannerDraftHydrated(true);
    setNamedDraftName(safeString(importedWorkflowSession.title || importedPlan.title));
    setImportedWorkflowSessionApplied(importedWorkflowSession.session_id);
    clearWorkflowImportHandoff(false);
  }, [importedWorkflowSession, importedWorkflowSessionApplied, workflowImportHandoff?.session_id]);

  const resetLocalState = () => {
    clearPlannerDraft(draftKey);
    setBrief(DEFAULT_BRIEF);
    setPlannerInput("");
    setPlanningState("idle");
    setPlanPreview(null);
    setPlanningConversation(null);
    setPlanningChangeSummary([]);
    setPlannerError("");
    setPlannerDiagnostics(null);
    setClarification({ status: "none" });
    setValidationReport(null);
    setPlanPackage(null);
    setPlanPackageBundle(null);
    setPlanPackageReplay(null);
    setOverlapAnalysis(null);
    setTeachingLibrary(null);
    setPlannerDraftUpdatedAtMs(null);
    setPlannerDraftRestored(false);
    setNamedDraftName("");
  };

  const applyPlannerDraftSnapshot = (draft: PlannerDraftSnapshot | null) => {
    if (!draft) return false;
    setBrief({
      ...DEFAULT_BRIEF,
      goal: safeString(draft.brief?.goal || draft.goal),
      workspaceRoot: safeString(draft.brief?.workspaceRoot || draft.workspaceRoot),
      targetSurface: (draft.brief?.targetSurface ||
        draft.targetSurface ||
        DEFAULT_BRIEF.targetSurface) as any,
      planningHorizon: (draft.brief?.planningHorizon ||
        draft.planningHorizon ||
        DEFAULT_BRIEF.planningHorizon) as any,
      outputExpectations: safeString(draft.brief?.outputExpectations || draft.outputExpectations),
      constraints: safeString(draft.brief?.constraints || draft.constraints),
      plannerProvider: safeString(draft.brief?.plannerProvider || draft.plannerProvider),
      plannerModel: safeString(draft.brief?.plannerModel || draft.plannerModel),
      selectedMcpServers: Array.isArray(draft.brief?.selectedMcpServers)
        ? draft.brief.selectedMcpServers.map((row: any) => safeString(row)).filter(Boolean)
        : Array.isArray(draft.selectedMcpServers)
          ? draft.selectedMcpServers.map((row: any) => safeString(row)).filter(Boolean)
          : [],
    });
    setPlannerInput(safeString(draft.plannerInput));
    setPlanningState("idle");
    setPlanPreview(draft.planPreview || draft.planPreviewJson || draft.plan || null);
    setPlanningConversation(draft.planningConversation || draft.conversation || null);
    setPlanningChangeSummary(
      Array.isArray(draft.planningChangeSummary || draft.changeSummary)
        ? (draft.planningChangeSummary || draft.changeSummary)
            .map((row: any) => safeString(row))
            .filter(Boolean)
        : []
    );
    setPlannerError(safeString(draft.plannerError));
    setPlannerDiagnostics(draft.plannerDiagnostics || null);
    setValidationReport(draft.validationReport || null);
    setPlanPackage(draft.planPackage || null);
    setPlanPackageBundle(draft.planPackageBundle || null);
    setPlanPackageReplay(draft.planPackageReplay || null);
    setOverlapAnalysis(draft.overlapAnalysis || null);
    setTeachingLibrary(draft.teachingLibrary || null);
    setPlannerDraftUpdatedAtMs(
      Number.isFinite(Number((draft as any).updatedAtMs))
        ? Number((draft as any).updatedAtMs)
        : null
    );
    setPlannerDraftRestored(true);
    setPlannerDraftHydrated(true);
    setNamedDraftName(safeString(draft.name || draft.title || draft.brief?.goal || draft.goal));
    return true;
  };

  useEffect(() => {
    setPlannerDraftHydrated(false);
    const draft = loadPlannerDraft<PlannerDraftSnapshot>(draftKey);
    if (!draft) {
      setPlannerDraftHydrated(true);
      setPlannerDraftRestored(false);
      return;
    }
    applyPlannerDraftSnapshot(draft);
  }, [draftKey]);

  useEffect(() => {
    try {
      renderIcons();
    } catch {}
  });

  useEffect(() => {
    if (!plannerDraftHydrated) return;
    const snapshot = {
      name: namedDraftName,
      brief,
      plannerInput,
      planPreview,
      planningConversation,
      planningChangeSummary,
      plannerError,
      plannerDiagnostics,
      validationReport,
      planPackage,
      planPackageBundle,
      planPackageReplay,
      overlapAnalysis,
      teachingLibrary,
    };
    const updatedAtMs = savePlannerDraft(draftKey, snapshot);
    if (typeof updatedAtMs === "number") {
      setPlannerDraftUpdatedAtMs(updatedAtMs);
    }
    appendPlannerDraftHistory(historyKey, snapshot);
  }, [
    brief,
    overlapAnalysis,
    draftKey,
    planPackage,
    planPackageBundle,
    planPackageReplay,
    planPreview,
    plannerDiagnostics,
    plannerError,
    plannerInput,
    planningChangeSummary,
    planningConversation,
    plannerDraftHydrated,
    historyKey,
    namedDraftName,
    teachingLibrary,
    validationReport,
  ]);

  const startPlanning = async (messageOverride?: string) => {
    const intent = safeString(messageOverride || plannerInput || brief.goal);
    const workspaceRoot = safeString(brief.workspaceRoot);
    if (!intent) {
      toast("warn", "Describe the intent you want the planner to turn into a long-horizon plan.");
      return;
    }
    if (!workspaceRoot) {
      toast("warn", "Workspace root is required so the planner can inspect the target workspace.");
      return;
    }
    if (!workspaceRoot.startsWith("/")) {
      toast("warn", "Workspace root must be an absolute path.");
      return;
    }
    setPlanningState("generating");
    setPlannerError("");
    setPlannerDiagnostics(null);
    setBrief((current) => ({ ...current, goal: intent }));
    try {
      const response = await client.workflowPlans.chatStart({
        prompt: plannerPromptFromBrief({ ...brief, goal: intent }),
        plan_source: "intent_planner_page",
        workspace_root: workspaceRoot,
        allowed_mcp_servers: brief.selectedMcpServers.length
          ? brief.selectedMcpServers
          : connectedMcpServers,
        operator_preferences: {
          planner_target_surface: brief.targetSurface,
          planning_horizon: brief.planningHorizon,
          expected_outputs: safeString(brief.outputExpectations),
          constraints: safeString(brief.constraints),
          planner_provider: safeString(brief.plannerProvider),
          planner_model: safeString(brief.plannerModel),
          ...buildDefaultKnowledgeOperatorPreferences(intent),
          ...buildKnowledgeRolloutGuidance(intent),
        },
      });
      const responseAny = response as any;
      setPlanPreview(responseAny?.plan || null);
      setPlanningConversation(responseAny?.conversation || null);
      setPlanningChangeSummary([]);
      setPlannerDiagnostics(responseAny?.planner_diagnostics || null);
      setValidationReport(responseAny?.plan_package_validation || null);
      setPlanPackage(responseAny?.plan_package || null);
      setPlanPackageBundle(responseAny?.plan_package_bundle || null);
      setPlanPackageReplay(responseAny?.plan_package_replay || null);
      setOverlapAnalysis(responseAny?.overlap_analysis || null);
      setTeachingLibrary(responseAny?.teaching_library || null);
      setPlannerDraftRestored(false);
      const clarQuestion =
        typeof responseAny?.clarifier?.question === "string"
          ? String(responseAny.clarifier.question)
          : "";
      const clarOptions: ClarifierOption[] = Array.isArray(responseAny?.clarifier?.options)
        ? responseAny.clarifier.options
            .map((option: any) => ({
              id: safeString(option?.id),
              label: safeString(option?.label),
            }))
            .filter((option: ClarifierOption) => option.id && option.label)
        : [];
      if (clarQuestion) {
        setPlannerError(clarQuestion);
      }
      if (clarQuestion && clarOptions.length > 0) {
        setClarification({ status: "waiting", question: clarQuestion, options: clarOptions });
        setPlanningState("clarifying");
      } else {
        setClarification({ status: "none" });
        setPlanningState("idle");
      }
      setPlannerInput("");
      toast(
        "ok",
        clarQuestion
          ? "Planner needs clarification before it can refine the mission."
          : "Planner generated a draft mission plan."
      );
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setPlannerError(message);
      setClarification({ status: "none" });
      setPlanningState("idle");
      toast("err", message);
    }
  };

  const revisePlan = async (messageOverride?: string) => {
    const planId = safeString(planPreview?.plan_id || planPreview?.planId);
    if (!planId) {
      toast("warn", "Start a planning session before revising the plan.");
      return;
    }
    const trimmedMessage = safeString(messageOverride ?? plannerInput);
    if (!trimmedMessage) {
      toast("warn", "Add a message for the planner first.");
      return;
    }
    setPlanningState("revising");
    setPlannerError("");
    setPlannerDiagnostics(null);
    try {
      const response = await client.workflowPlans.chatMessage({
        plan_id: planId,
        message: trimmedMessage,
      });
      const responseAny = response as any;
      setPlanPreview(responseAny?.plan || null);
      setPlanningConversation(responseAny?.conversation || null);
      setPlanningChangeSummary(
        Array.isArray(responseAny?.change_summary)
          ? responseAny.change_summary.map((row: any) => safeString(row)).filter(Boolean)
          : []
      );
      setPlannerDiagnostics(responseAny?.planner_diagnostics || null);
      setValidationReport(responseAny?.plan_package_validation || null);
      setPlanPackage(responseAny?.plan_package || null);
      setPlanPackageBundle(responseAny?.plan_package_bundle || null);
      setPlanPackageReplay(responseAny?.plan_package_replay || null);
      setOverlapAnalysis(responseAny?.overlap_analysis || null);
      setTeachingLibrary(responseAny?.teaching_library || null);
      setPlannerDraftRestored(false);
      const clarQuestion =
        typeof responseAny?.clarifier?.question === "string"
          ? String(responseAny.clarifier.question)
          : "";
      const clarOptions: ClarifierOption[] = Array.isArray(responseAny?.clarifier?.options)
        ? responseAny.clarifier.options
            .map((option: any) => ({
              id: safeString(option?.id),
              label: safeString(option?.label),
            }))
            .filter((option: ClarifierOption) => option.id && option.label)
        : [];
      if (clarQuestion) {
        setPlannerError(clarQuestion);
      }
      if (clarQuestion && clarOptions.length > 0) {
        setClarification({ status: "waiting", question: clarQuestion, options: clarOptions });
        setPlanningState("clarifying");
      } else {
        setClarification({ status: "none" });
        setPlanningState("idle");
      }
      setPlannerInput("");
      toast("ok", "Planner revised the draft plan.");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setPlannerError(message);
      setClarification({ status: "none" });
      setPlanningState("idle");
      toast("err", message);
    }
  };

  const resetPlan = async () => {
    const planId = safeString(planPreview?.plan_id || planPreview?.planId);
    if (!planId || !client.workflowPlans.chatReset) {
      resetLocalState();
      toast("ok", "Planner draft reset.");
      return;
    }
    try {
      const response = await client.workflowPlans.chatReset({ plan_id: planId });
      const responseAny = response as any;
      setPlanPreview(responseAny?.plan || null);
      setPlanningConversation(responseAny?.conversation || null);
      setPlanningChangeSummary([]);
      setPlannerError("");
      setPlannerDiagnostics(responseAny?.planner_diagnostics || null);
      setValidationReport(responseAny?.plan_package_validation || null);
      setPlanPackage(responseAny?.plan_package || null);
      setPlanPackageBundle(responseAny?.plan_package_bundle || null);
      setPlanPackageReplay(responseAny?.plan_package_replay || null);
      setOverlapAnalysis(responseAny?.overlap_analysis || null);
      setTeachingLibrary(responseAny?.teaching_library || null);
      setClarification({ status: "none" });
      setPlanningState("idle");
      setPlannerInput("");
      setPlannerDraftRestored(false);
      toast("ok", "Planner session reset.");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setPlannerError(message);
      toast("err", message);
    }
  };

  const saveCurrentNamedDraft = () => {
    const draftName = safeString(
      namedDraftName || brief.goal || planPreview?.title || planPreview?.plan_id
    );
    if (!draftName) {
      toast("warn", "Name the planner draft before saving it as a saved intent.");
      return;
    }
    const saved = saveNamedPlannerDraft(namedDraftPrefix, draftName, {
      name: draftName,
      brief,
      plannerInput,
      planPreview,
      planningConversation,
      planningChangeSummary,
      plannerError,
      plannerDiagnostics,
      validationReport,
      planPackage,
      planPackageBundle,
      planPackageReplay,
      overlapAnalysis,
      teachingLibrary,
    });
    if (!saved) {
      toast("err", "Unable to save the named planner draft.");
      return;
    }
    setNamedDraftName(draftName);
    setPlannerDraftRestored(false);
    toast("ok", `Saved planner draft “${draftName}”.`);
  };

  const openNamedDraft = (storageKey: string) => {
    const draft = loadPlannerDraft<PlannerDraftSnapshot>(storageKey);
    if (!draft) {
      toast("warn", "That saved planner draft could not be loaded.");
      return;
    }
    if (!applyPlannerDraftSnapshot(draft)) {
      toast("warn", "That saved planner draft was empty.");
      return;
    }
    toast("ok", `Reopened planner draft “${safeString(draft.name || draft.title)}”.`);
  };

  const deleteNamedDraft = (storageKey: string) => {
    deleteNamedPlannerDraft(storageKey);
    setPlannerDraftRestored(false);
    toast("ok", "Deleted the saved planner draft.");
  };

  const openHistoryDraft = (entryId: string) => {
    const draft = loadPlannerDraftHistoryEntry<PlannerDraftSnapshot>(historyKey, entryId);
    if (!draft) {
      toast("warn", "That autosaved planner state could not be loaded.");
      return;
    }
    if (!applyPlannerDraftSnapshot(draft)) {
      toast("warn", "That autosaved planner state was empty.");
      return;
    }
    toast("ok", "Reopened a recent autosaved planner state.");
  };

  const deleteHistoryDraft = (entryId: string) => {
    deletePlannerDraftHistoryEntry(historyKey, entryId);
    setPlannerDraftRestored(false);
    toast("ok", "Deleted the autosaved planner state.");
  };

  const clearWorkflowImportHandoff = (notify = true) => {
    try {
      localStorage.removeItem(WORKFLOW_IMPORT_HANDOFF_KEY);
    } catch {
      // Ignore storage failures.
    }
    setWorkflowImportHandoff(null);
    if (notify) {
      toast("ok", "Cleared the imported workflow handoff.");
    }
  };

  return (
    <AnimatedPage className="flex flex-col min-h-0 h-full overflow-hidden">
      {workflowImportHandoff ? (
        <PanelCard
          title="Imported workflow handoff"
          subtitle="This durable session was created from an imported bundle. Open the workflow center to inspect, revise, or continue it."
          className="mb-4"
          actions={
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                className="tcp-btn-secondary"
                onClick={() => navigate("workflows")}
              >
                Open workflow center
              </button>
              <button
                type="button"
                className="tcp-btn-secondary"
                onClick={() => clearWorkflowImportHandoff()}
              >
                Dismiss
              </button>
            </div>
          }
        >
          <div className="grid gap-3 text-sm">
            <div className="flex flex-wrap items-center gap-2">
              <Badge tone={importedWorkflowSourceKind === "imported_bundle" ? "warn" : "info"}>
                {importedWorkflowSourceKind || "imported_bundle"}
              </Badge>
              <Badge tone="ghost">{safeString(workflowImportHandoff.session_id)}</Badge>
            </div>
            <div className="grid gap-1 md:grid-cols-2">
              <div>
                <div className="tcp-subtle text-xs">Title</div>
                <div>
                  {safeString(
                    importedWorkflowSession?.title ||
                      workflowImportHandoff.title ||
                      "Imported workflow"
                  )}
                </div>
              </div>
              <div>
                <div className="tcp-subtle text-xs">Project</div>
                <div>
                  {safeString(
                    importedWorkflowSession?.project_slug ||
                      workflowImportHandoff.project_slug ||
                      "workflow-imports"
                  )}
                </div>
              </div>
              <div>
                <div className="tcp-subtle text-xs">Bundle digest</div>
                <div>
                  {safeString(
                    importedWorkflowSession?.source_bundle_digest ||
                      workflowImportHandoff.source_bundle_digest ||
                      "—"
                  )}
                </div>
              </div>
              <div>
                <div className="tcp-subtle text-xs">Current plan id</div>
                <div>
                  {safeString(
                    importedWorkflowSession?.current_plan_id ||
                      workflowImportHandoff.current_plan_id ||
                      "—"
                  )}
                </div>
              </div>
            </div>
          </div>
        </PanelCard>
      ) : null}

      <div className="grid flex-1 gap-4 min-h-0 xl:grid-cols-[minmax(400px,0.8fr)_minmax(0,1.2fr)]">
        <div className="flex flex-col gap-4 min-h-0 h-full overflow-y-auto pr-1">
          <PanelCard
            title="Intent brief"
            subtitle="Describe the mission goal."
            className="flex flex-col shrink-0"
            actions={
              <div className="flex flex-wrap gap-2 text-[10px]">
                <Badge tone="ok">intent → plan</Badge>
                {plannerDraftRestored ? <Badge tone="info">restored</Badge> : null}
              </div>
            }
          >
            <IntentBriefPanel
              draft={brief}
              onChange={setBrief}
              providerOptions={providerOptions}
              plannerCanUseLlm={plannerCanUseLlm}
              basePlannerLabel={
                safeString(providerStatus.defaultProvider) &&
                safeString(providerStatus.defaultModel)
                  ? `${safeString(providerStatus.defaultProvider)} / ${safeString(providerStatus.defaultModel)}`
                  : "not configured"
              }
              availableMcpServers={mcpServers}
              workspaceRootError={workspaceRootError}
              workspaceBrowserOpen={workspaceBrowserOpen}
              workspaceBrowserDir={workspaceBrowserDir}
              workspaceBrowserSearch={workspaceBrowserSearch}
              workspaceBrowserParentDir={workspaceParentDir}
              workspaceBrowserCurrentDir={workspaceCurrentBrowseDir}
              workspaceBrowserDirectories={filteredWorkspaceDirectories}
              onOpenWorkspaceBrowser={() => {
                const seed = String(
                  brief.workspaceRoot ||
                    (healthQuery.data as any)?.workspaceRoot ||
                    (healthQuery.data as any)?.workspace_root ||
                    "/"
                ).trim();
                setWorkspaceBrowserDir(seed || "/");
                setWorkspaceBrowserSearch("");
                setWorkspaceBrowserOpen(true);
              }}
              onCloseWorkspaceBrowser={() => {
                setWorkspaceBrowserOpen(false);
                setWorkspaceBrowserSearch("");
              }}
              onClearWorkspaceRoot={() =>
                setBrief((current) => ({
                  ...current,
                  workspaceRoot: "",
                }))
              }
              onWorkspaceBrowserSearchChange={setWorkspaceBrowserSearch}
              onWorkspaceBrowserParent={() => {
                if (!workspaceParentDir) return;
                setWorkspaceBrowserDir(workspaceParentDir);
              }}
              onWorkspaceBrowserDirectory={(path) => setWorkspaceBrowserDir(path)}
              onSelectWorkspaceDirectory={() => {
                if (!workspaceCurrentBrowseDir) return;
                setBrief((current) => ({
                  ...current,
                  workspaceRoot: workspaceCurrentBrowseDir,
                }));
                setWorkspaceBrowserOpen(false);
                setWorkspaceBrowserSearch("");
                toast("ok", `Workspace selected: ${workspaceCurrentBrowseDir}`);
              }}
              onReset={() => {
                resetLocalState();
                toast("ok", "Started a new empty mission draft.");
              }}
              disabled={isPlanning}
            />
          </PanelCard>

          <PlannerDraftList
            storagePrefix={namedDraftPrefix}
            historyKey={historyKey}
            draftName={namedDraftName}
            onDraftNameChange={setNamedDraftName}
            onSaveCurrentDraft={saveCurrentNamedDraft}
            onOpenDraft={openNamedDraft}
            onDeleteDraft={deleteNamedDraft}
            onOpenHistory={openHistoryDraft}
            onDeleteHistory={deleteHistoryDraft}
          />
        </div>

        <div className="flex flex-col gap-4 min-h-0 h-full overflow-y-auto pr-1 pb-10">
          <PanelCard
            title="Planner conversation"
            subtitle="Shape the full workflow plan."
            className="flex flex-col shrink-0 h-[500px]"
            actions={
              <div className="flex flex-wrap gap-2">
                <button
                  type="button"
                  className="tcp-btn"
                  onClick={() => void startPlanning()}
                  disabled={isPlanning}
                >
                  <i data-lucide={hasPlan ? "refresh-cw" : "play"} className="mr-1 h-3 w-3"></i>
                  {hasPlan ? "Regenerate" : "Start planning"}
                </button>
                <button
                  type="button"
                  className="tcp-btn"
                  onClick={() => void resetPlan()}
                  title="Reset session"
                >
                  <i data-lucide="rotate-ccw"></i>
                </button>
              </div>
            }
          >
            <div className="tcp-chat-container flex-1">
              {plannerDraftUpdatedAtMs ? (
                <div className="mb-3 rounded-xl border border-white/5 bg-black/20 px-3 py-2 text-[10px] text-slate-500">
                  Last autosave: {new Date(plannerDraftUpdatedAtMs).toLocaleTimeString()}
                </div>
              ) : null}
              <div className="tcp-chat-fill flex flex-col">
                <ChatInterfacePanel
                  messages={plannerChatMessages}
                  emptyText="Describe your mission intent to start."
                  inputValue={plannerInput}
                  inputPlaceholder={
                    hasPlan ? "Refine the plan..." : "Describe the mission and outcome..."
                  }
                  sendLabel={hasPlan ? "Send to planner" : "Start plan"}
                  onInputChange={setPlannerInput}
                  onSend={() =>
                    void (hasPlan ? revisePlan(plannerInput) : startPlanning(plannerInput))
                  }
                  sendDisabled={isPlanning || !safeString(plannerInput)}
                  inputDisabled={isPlanning}
                  statusTitle={plannerStatusTitle}
                  statusDetail={isPlanning ? plannerStatusDetail : ""}
                  questionTitle="Planner question"
                  questionText={clarification.status === "waiting" ? clarification.question : ""}
                  quickReplies={
                    clarification.status === "waiting" ? clarification.options || [] : []
                  }
                  onQuickReply={(option) => void revisePlan(option.label)}
                  questionHint="Reply with more detail."
                  botIdentity={{
                    botName: `Tandem Planner`,
                    botAvatarUrl: identity.botAvatarUrl,
                  }}
                  showThinking={planningState === "generating" || planningState === "revising"}
                  thinkingText={planningState === "revising" ? "Revising plan" : "Drafting plan"}
                />
              </div>

              <div className="mt-4 flex flex-wrap gap-2">
                <button
                  type="button"
                  className="tcp-btn-primary"
                  onClick={() =>
                    void (hasPlan ? revisePlan(plannerInput || brief.goal) : startPlanning())
                  }
                  disabled={
                    isPlanning ||
                    (!hasPlan && !safeString(brief.goal)) ||
                    !safeString(brief.workspaceRoot)
                  }
                >
                  {hasPlan ? "Revise draft" : "Generate draft"}
                </button>
                <button
                  type="button"
                  className="tcp-btn"
                  onClick={() => setPlannerInput(brief.goal)}
                  disabled={isPlanning || !safeString(brief.goal)}
                >
                  Seed from brief
                </button>
              </div>
            </div>
          </PanelCard>

          {isPlanning ? (
            <PanelCard
              title="Generating plan flow"
              subtitle="The planner is synthesizing structure."
              className="shrink-0"
            >
              <PlanGenerationAnimation />
            </PanelCard>
          ) : null}

          {hasGeneratedDraft ? (
            <>
              <PlannerHandoffPanel
                brief={brief}
                planPreview={planPreview}
                planPackage={planPackage}
                planPackageBundle={planPackageBundle}
                validationReport={validationReport}
                overlapAnalysis={overlapAnalysis}
                teachingLibrary={teachingLibrary}
                navigate={navigate}
                toast={toast}
              />
              <PlanFormationPanel
                brief={brief}
                planPreview={planPreview}
                validationReport={validationReport}
                overlapAnalysis={overlapAnalysis}
              />
              <TimelinePlanPanel brief={brief} planPreview={planPreview} />
              <PlanSummaryPanel
                planPreview={planPreview}
                brief={brief}
                planPackage={planPackage}
                planPackageBundle={planPackageBundle}
              />
              <PlanValidationPanel validationReport={validationReport} />
              <PlanOverlapPanel overlapAnalysis={overlapAnalysis} />
              <PlanReplayPanel planPackageReplay={planPackageReplay} />
              <PlannerDiagnosticsPanel
                plannerDiagnostics={plannerDiagnostics}
                teachingLibrary={teachingLibrary}
                planningChangeSummary={planningChangeSummary}
              />
            </>
          ) : null}
        </div>
      </div>
    </AnimatedPage>
  );
}
