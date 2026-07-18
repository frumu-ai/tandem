import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import YAML from "yaml";
import { Step1Goal } from "./Step1Goal";
import { Step2Schedule } from "./Step2Schedule";
import { Step3Mode } from "./Step3Mode";
import { Step4Review } from "./Step4Review";
import { detectBrowserTimezone, isValidTimezone } from "../timezone";
import {
  buildDefaultKnowledgeOperatorPreferences,
  buildPlannerProviderOptions,
} from "../../planner/plannerShared";
import type { ExecutionProfile } from "../AutomationsRunHelpers";
import type { NavigationLockState } from "../../../pages/pageTypes";
import { Icon } from "../../../ui/Icon";
import { useEngineStream } from "../../stream/useEngineStream";
import { preferredProviderModel } from "../../../components/ProviderModelSelector";

type ExecutionMode = "single" | "team" | "swarm";
type WizardExecutionProfile = "" | ExecutionProfile;
type WizardStep = 1 | 2 | 3 | 4;
type WorkflowToolAccessMode = "all" | "custom";

interface SchedulePreset {
  label: string;
  desc: string;
  icon: string;
  cron: string;
  intervalSeconds?: number;
}

interface WizardState {
  goal: string;
  workspaceRoot: string;
  timezone: string;
  schedulePreset: string;
  scheduleKind: "manual" | "cron" | "interval";
  cron: string;
  intervalSeconds: string;
  mode: ExecutionMode;
  executionProfile: WizardExecutionProfile;
  maxAgents: string;
  routedSkill: string;
  routingConfidence: string;
  modelProvider: string;
  modelId: string;
  plannerModelProvider: string;
  plannerModelId: string;
  roleModelsJson: string;
  toolAccessMode: WorkflowToolAccessMode;
  customToolsText: string;
  selectedMcpServers: string[];
  exportPackDraft: boolean;
  advancedMode: boolean;
  customSkillName: string;
  customSkillDescription: string;
  customWorkflowKind: "pack_builder_recipe" | "automation_v2_dag";
}

interface McpServerOption {
  name: string;
  connected: boolean;
  enabled: boolean;
  authKind: string;
  lastError: string;
  pendingAuth: boolean;
  authMessage: string;
  authorizationUrl: string;
  connectionCount: number;
}

type PlannerClarificationState = {
  status: "none" | "waiting";
  question: string;
};

type PlannerLiveProgress = {
  phase: string;
  providerID: string;
  modelID: string;
  responseChars: number;
  elapsedMs: number;
};

function plannerProgressStage(phase: string): string {
  switch (phase) {
    case "dispatch":
      return "Sending the request to the model";
    case "waiting":
      return "Waiting for the model's first response";
    case "thinking":
      return "The model is working on the plan";
    case "streaming":
      return "Receiving the model response";
    case "retrying":
      return "Retrying with a compatible response mode";
    case "validating":
      return "Validating and assembling the plan";
    case "failed":
      return "The planner request failed";
    default:
      return "Preparing the planner request";
  }
}

interface AutomationWizardConfig {
  defaults: {
    schedulePreset: string;
    mode: ExecutionMode;
    maxAgents: string;
  };
  steps: string[];
  schedulePresets: SchedulePreset[];
  executionModes: Array<{
    id: ExecutionMode;
    label: string;
    icon: string;
    desc: string;
    bestFor: string;
  }>;
  goalExamples: string[];
}

const AUTOMATION_WIZARD_SOURCES = import.meta.glob("../../../pages/automation-wizard.yaml", {
  eager: true,
  query: "?raw",
  import: "default",
}) as Record<string, string>;

function parseAutomationWizardConfig(source: string): AutomationWizardConfig {
  if (!String(source || "").trim()) {
    throw new Error("Automation wizard config file could not be loaded.");
  }
  const parsed = YAML.parse(source) as unknown;
  if (!parsed || typeof parsed !== "object") {
    throw new Error("Invalid automation wizard config: expected a YAML object.");
  }
  const config = parsed as Partial<AutomationWizardConfig>;
  const defaults = (config.defaults || {}) as Partial<AutomationWizardConfig["defaults"]>;
  const steps = config.steps;
  const schedulePresets = config.schedulePresets;
  const executionModes = config.executionModes;
  const goalExamples = config.goalExamples;
  if (!Array.isArray(steps) || !steps.length)
    throw new Error("Invalid automation wizard config: steps must be a non-empty array.");
  if (!Array.isArray(schedulePresets) || !schedulePresets.length)
    throw new Error("Invalid automation wizard config: schedulePresets must be a non-empty array.");
  if (!Array.isArray(executionModes) || !executionModes.length)
    throw new Error("Invalid automation wizard config: executionModes must be a non-empty array.");
  if (!Array.isArray(goalExamples) || !goalExamples.length)
    throw new Error("Invalid automation wizard config: goalExamples must be a non-empty array.");
  return {
    defaults: {
      schedulePreset: String(defaults.schedulePreset || "").trim() || "Every morning",
      mode:
        defaults.mode === "single" || defaults.mode === "team" || defaults.mode === "swarm"
          ? defaults.mode
          : "team",
      maxAgents: String(defaults.maxAgents || "").trim() || "4",
    },
    steps: steps.map((step) => String(step || "").trim()).filter(Boolean),
    schedulePresets: schedulePresets.map((preset: any) => ({
      label: String(preset?.label || "").trim(),
      desc: String(preset?.desc || "").trim(),
      icon: String(preset?.icon || "").trim(),
      cron: String(preset?.cron || "").trim(),
      intervalSeconds:
        preset?.intervalSeconds === undefined || preset?.intervalSeconds === null
          ? undefined
          : Number(preset.intervalSeconds),
    })),
    executionModes: executionModes.map((mode: any) => ({
      id: mode?.id === "single" || mode?.id === "team" || mode?.id === "swarm" ? mode.id : "team",
      label: String(mode?.label || "").trim(),
      icon: String(mode?.icon || "").trim(),
      desc: String(mode?.desc || "").trim(),
      bestFor: String(mode?.bestFor || "").trim(),
    })),
    goalExamples: goalExamples.map((example) => String(example || "").trim()).filter(Boolean),
  };
}

const AUTOMATION_WIZARD_SOURCE = Object.values(AUTOMATION_WIZARD_SOURCES).find(
  (value): value is string => typeof value === "string" && value.trim().length > 0
);

const AUTOMATION_IMPORT_FIELDS = [
  "automation_id",
  "name",
  "description",
  "status",
  "schedule",
  "agents",
  "flow",
  "execution",
  "output_targets",
  "creator_id",
  "workspace_root",
  "metadata",
  "scope_policy",
  "watch_conditions",
  "handoff_config",
] as const;

function safeString(value: unknown) {
  return String(value || "").trim();
}

function automationApplyIdempotencyKey(
  plan: any,
  overlapDecision: string,
  exportPackDraft: boolean
) {
  const serialized = JSON.stringify({ plan, overlapDecision, exportPackDraft });
  let hash = 2166136261;
  for (let index = 0; index < serialized.length; index += 1) {
    hash = Math.imul(hash ^ serialized.charCodeAt(index), 16777619);
  }
  const planID = safeString(plan?.plan_id || plan?.planId || "plan").slice(0, 96);
  return `automation-wizard:${planID}:${(hash >>> 0).toString(16).padStart(8, "0")}`;
}

function createPlannerProgressID() {
  const uuid = globalThis.crypto?.randomUUID?.();
  if (uuid) return `automation-wizard-${uuid}`;
  return `automation-wizard-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

function describePlannerFallback(diagnostics: any) {
  const reason = safeString(diagnostics?.fallback_reason || diagnostics?.fallbackReason);
  if (!reason) return "";
  const detail = safeString(diagnostics?.detail);
  const reasonLabel = reason.replace(/_/g, " ");
  return detail
    ? `Automation was not created. Plan generation failed (${reasonLabel}): ${detail}`
    : `Automation was not created. Plan generation failed: ${reasonLabel}.`;
}

function objectValue(value: unknown): Record<string, any> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, any>)
    : null;
}

function automationSpecFromImportedJson(parsed: unknown) {
  const root = objectValue(parsed);
  const candidate =
    objectValue(root?.automation) ||
    objectValue(root?.source_automation) ||
    objectValue(root?.spec) ||
    root;
  if (!candidate) throw new Error("Import file must contain a Tandem automation JSON object.");
  const payload: Record<string, any> = {};
  for (const field of AUTOMATION_IMPORT_FIELDS) {
    if (candidate[field] !== undefined) payload[field] = candidate[field];
  }
  if (!safeString(payload.name)) {
    throw new Error("Automation import is missing a name.");
  }
  if (!objectValue(payload.schedule)) {
    throw new Error("Automation import is missing a schedule.");
  }
  if (!objectValue(payload.flow)) {
    throw new Error("Automation import is missing a workflow flow.");
  }
  if (payload.agents !== undefined && !Array.isArray(payload.agents)) {
    throw new Error("Automation import agents must be an array.");
  }
  return payload;
}

function findScrollableParent(node: HTMLElement | null): HTMLElement | null {
  let current = node?.parentElement || null;
  while (current) {
    const style = window.getComputedStyle(current);
    const overflowY = style.overflowY.toLowerCase();
    const canScroll =
      (overflowY === "auto" || overflowY === "scroll" || overflowY === "overlay") &&
      current.scrollHeight > current.clientHeight;
    if (canScroll) return current;
    current = current.parentElement;
  }
  return null;
}

function summarizePlannerClarification(response: any): PlannerClarificationState {
  const question = safeString(response?.clarifier?.question);
  if (!question) return { status: "none", question: "" };
  return { status: "waiting", question };
}

function buildPlannerLatencyAdvisory(wizard: WizardState) {
  const goal = safeString(wizard.goal);
  if (!goal) return "";
  const lowered = goal.toLowerCase();
  const connectorMentions = [
    "reddit",
    "notion",
    "github",
    "slack",
    "jira",
    "airtable",
    "gmail",
    "mcp",
  ].filter((needle) => lowered.includes(needle)).length;
  const looksLongRunning =
    goal.length >= 2500 ||
    wizard.selectedMcpServers.length >= 2 ||
    connectorMentions >= 2 ||
    (wizard.scheduleKind !== "manual" &&
      (goal.length >= 1500 || wizard.selectedMcpServers.length >= 1));
  if (!looksLongRunning) return "";
  return "This workflow is connector-heavy or unusually detailed, so plan generation can take a few minutes.";
}

export const AUTOMATION_WIZARD_CONFIG = parseAutomationWizardConfig(AUTOMATION_WIZARD_SOURCE || "");
const AUTOMATION_PLANNER_SEED_KEY = "tandem.automations.plannerSeed";
const AUTOMATION_WIZARD_DRAFT_KEY = "tandem.automations.createWizardDraft";

function createDefaultWizardState(
  defaultProvider: string,
  defaultModel: string,
  workspaceRoot = "",
  timezone = detectBrowserTimezone()
): WizardState {
  const defaultPreset = AUTOMATION_WIZARD_CONFIG.schedulePresets.find(
    (preset) => preset.label === AUTOMATION_WIZARD_CONFIG.defaults.schedulePreset
  );
  const defaultSchedule =
    defaultPreset?.intervalSeconds !== undefined && defaultPreset.intervalSeconds !== null
      ? {
          scheduleKind: "interval" as const,
          cron: "",
          intervalSeconds: String(defaultPreset.intervalSeconds),
        }
      : defaultPreset?.cron
        ? { scheduleKind: "cron" as const, cron: defaultPreset.cron, intervalSeconds: "3600" }
        : { scheduleKind: "manual" as const, cron: "", intervalSeconds: "3600" };
  return {
    goal: "",
    workspaceRoot,
    timezone: String(timezone || "").trim() || detectBrowserTimezone(),
    schedulePreset: AUTOMATION_WIZARD_CONFIG.defaults.schedulePreset,
    scheduleKind: defaultSchedule.scheduleKind,
    cron: defaultSchedule.cron,
    intervalSeconds: defaultSchedule.intervalSeconds,
    mode: AUTOMATION_WIZARD_CONFIG.defaults.mode,
    executionProfile: "",
    maxAgents: AUTOMATION_WIZARD_CONFIG.defaults.maxAgents,
    routedSkill: "",
    routingConfidence: "",
    modelProvider: String(defaultProvider || ""),
    modelId: String(defaultModel || ""),
    plannerModelProvider: String(defaultProvider || ""),
    plannerModelId: String(defaultModel || ""),
    roleModelsJson: "",
    toolAccessMode: "all",
    customToolsText: "",
    selectedMcpServers: [],
    exportPackDraft: false,
    advancedMode: false,
    customSkillName: "",
    customSkillDescription: "",
    customWorkflowKind: "pack_builder_recipe",
  };
}

function normalizeMcpServers(raw: any): McpServerOption[] {
  if (Array.isArray(raw?.servers)) {
    return raw.servers
      .map((row: any) => {
        const name = String(row?.name || "").trim();
        if (!name) return null;
        const lastAuthChallenge = row?.last_auth_challenge || row?.lastAuthChallenge || null;
        const authorizationUrl = safeString(
          lastAuthChallenge?.authorization_url ||
            lastAuthChallenge?.authorizationUrl ||
            row?.authorization_url ||
            row?.authorizationUrl
        );
        const authMessage = safeString(lastAuthChallenge?.message);
        return {
          name,
          connected: !!row?.connected,
          enabled: row?.enabled !== false,
          authKind: safeString(row?.auth_kind || row?.authKind).toLowerCase(),
          lastError: safeString(row?.last_error || row?.lastError),
          pendingAuth: !!lastAuthChallenge || !!authorizationUrl,
          authMessage,
          authorizationUrl,
          connectionCount: Array.isArray(row?.connections) ? row.connections.length : 0,
        };
      })
      .filter((row): row is McpServerOption => !!row)
      .sort((a, b) => a.name.localeCompare(b.name));
  }
  if (raw && typeof raw === "object") {
    return Object.entries(raw)
      .map(([name, row]) => {
        const cleanName = String(name || "").trim();
        if (!cleanName) return null;
        const cfg = row && typeof row === "object" ? row : {};
        const lastAuthChallenge =
          (cfg as any)?.last_auth_challenge || (cfg as any)?.lastAuthChallenge || null;
        const authorizationUrl = safeString(
          lastAuthChallenge?.authorization_url ||
            lastAuthChallenge?.authorizationUrl ||
            (cfg as any)?.authorization_url ||
            (cfg as any)?.authorizationUrl
        );
        const authMessage = safeString(lastAuthChallenge?.message);
        return {
          name: cleanName,
          connected: !!(cfg as any).connected,
          enabled: (cfg as any).enabled !== false,
          authKind: safeString((cfg as any).auth_kind || (cfg as any).authKind).toLowerCase(),
          lastError: safeString((cfg as any).last_error || (cfg as any).lastError),
          pendingAuth: !!lastAuthChallenge || !!authorizationUrl,
          authMessage,
          authorizationUrl,
          connectionCount: Array.isArray((cfg as any).connections)
            ? (cfg as any).connections.length
            : 0,
        };
      })
      .filter((row): row is McpServerOption => !!row)
      .sort((a, b) => a.name.localeCompare(b.name));
  }
  return [];
}

function saveAutomationWizardDraft(payload: {
  wizard: WizardState;
  step: WizardStep;
  planSource: string;
}) {
  try {
    sessionStorage.setItem(
      AUTOMATION_WIZARD_DRAFT_KEY,
      JSON.stringify({
        wizard: payload.wizard,
        step: payload.step,
        plan_source: safeString(payload.planSource) || "automations_page",
        saved_at: Date.now(),
      })
    );
  } catch {
    // ignore storage failures
  }
}

function toSchedulePayload(wizard: WizardState) {
  const timezone = String(wizard.timezone || "").trim() || "UTC";
  if (wizard.scheduleKind === "manual") return { type: "manual", timezone };
  if (wizard.scheduleKind === "interval") {
    return {
      interval_seconds: {
        seconds: Math.max(1, Number.parseInt(String(wizard.intervalSeconds || "3600"), 10) || 3600),
      },
      timezone,
    };
  }
  const customCron = String(wizard.cron || "").trim();
  if (customCron) return { cron: { expression: customCron }, timezone };
  const preset = AUTOMATION_WIZARD_CONFIG.schedulePresets.find(
    (p) => p.label === wizard.schedulePreset
  );
  if (preset?.intervalSeconds)
    return { interval_seconds: { seconds: preset.intervalSeconds }, timezone };
  if (preset?.cron) return { cron: { expression: preset.cron }, timezone };
  return { type: "manual", timezone };
}

function normalizeWizardExecutionProfile(value: unknown): ExecutionProfile | null {
  const normalized = String(value || "")
    .trim()
    .toLowerCase();
  if (normalized === "strict" || normalized === "guided" || normalized === "yolo") {
    return normalized;
  }
  return null;
}

function planWithWizardSettings(plan: any, wizard: WizardState) {
  if (!plan || typeof plan !== "object") return plan;
  const timezone = String(wizard.timezone || "").trim();
  const executionProfile = normalizeWizardExecutionProfile(wizard.executionProfile);
  const nextPlan = { ...plan };
  if (timezone && nextPlan.schedule && typeof nextPlan.schedule === "object") {
    nextPlan.schedule = {
      ...nextPlan.schedule,
      timezone,
    };
  }
  if (executionProfile) {
    nextPlan.execution = {
      ...(nextPlan.execution && typeof nextPlan.execution === "object" ? nextPlan.execution : {}),
      profile: executionProfile,
    };
  }
  return nextPlan;
}

function validateWorkspaceRootInput(raw: string) {
  const value = String(raw || "").trim();
  if (!value) return "Workspace root is required.";
  if (!value.startsWith("/")) return "Workspace root must be an absolute path.";
  return "";
}

function validatePlannerModelInput(provider: string, model: string) {
  const providerValue = String(provider || "").trim();
  const modelValue = String(model || "").trim();
  if (!providerValue && !modelValue) return "";
  if (!providerValue) return "Planner model provider is required when a planner model is set.";
  if (!modelValue) return "Planner model is required when a planner provider is set.";
  return "";
}

function validateRoleModelsJsonInput(raw: string) {
  const value = String(raw || "").trim();
  if (!value) return "";
  try {
    const parsed = JSON.parse(value);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return "Role model overrides must be a JSON object.";
    }
    return "";
  } catch {
    return "Role model overrides must be valid JSON.";
  }
}

function buildOperatorPreferences(wizard: WizardState) {
  let roleModels: Record<string, unknown> | undefined;
  const rawRoleModels = String(wizard.roleModelsJson || "").trim();
  if (rawRoleModels) {
    try {
      const parsed = JSON.parse(rawRoleModels);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        roleModels = parsed as Record<string, unknown>;
      }
    } catch {
      roleModels = undefined;
    }
  }
  const plannerModelProvider = String(wizard.plannerModelProvider || "").trim();
  const plannerModelId = String(wizard.plannerModelId || "").trim();
  if (plannerModelProvider && plannerModelId) {
    roleModels = { ...(roleModels || {}) };
    roleModels.planner = { provider_id: plannerModelProvider, model_id: plannerModelId };
  }
  const maxParallelAgents =
    wizard.mode === "single"
      ? 1
      : Math.max(2, Math.min(16, Number.parseInt(String(wizard.maxAgents || "4"), 10) || 4));
  const payload: Record<string, unknown> = {
    execution_mode: wizard.mode,
    max_parallel_agents: maxParallelAgents,
  };
  const executionProfile = normalizeWizardExecutionProfile(wizard.executionProfile);
  if (executionProfile) payload.execution_profile = executionProfile;
  if (String(wizard.modelProvider || "").trim())
    payload.model_provider = String(wizard.modelProvider).trim();
  if (String(wizard.modelId || "").trim()) payload.model_id = String(wizard.modelId).trim();
  if (roleModels && Object.keys(roleModels).length) payload.role_models = roleModels;
  Object.assign(payload, buildDefaultKnowledgeOperatorPreferences(wizard.goal));
  return payload;
}

export function CreateWizard({
  client,
  api,
  toast,
  navigate,
  defaultProvider,
  defaultModel,
  onNavigationLockChange,
  onCreated,
}: {
  client: any;
  api: (path: string, init?: RequestInit) => Promise<any>;
  toast: any;
  navigate: (route: string) => void;
  defaultProvider: string;
  defaultModel: string;
  onNavigationLockChange?: (lock: NavigationLockState | null) => void;
  onCreated?: () => void;
}) {
  const queryClient = useQueryClient();
  const wizardRootRef = useRef<HTMLDivElement | null>(null);
  const importAutomationFileRef = useRef<HTMLInputElement | null>(null);
  const plannerProgressIDRef = useRef<string>("");
  const [step, setStep] = useState<WizardStep>(1);
  const [planSource, setPlanSource] = useState<string>("automations_page");
  const [routerMatches, setRouterMatches] = useState<
    Array<{ skill_name?: string; confidence?: number }>
  >([]);
  const [planPreview, setPlanPreview] = useState<any>(null);
  const [overlapAnalysis, setOverlapAnalysis] = useState<any>(null);
  const [overlapDecision, setOverlapDecision] = useState<string>("");
  const [planningConversation, setPlanningConversation] = useState<any>(null);
  const [planningChangeSummary, setPlanningChangeSummary] = useState<string[]>([]);
  const [planningClarification, setPlanningClarification] = useState<PlannerClarificationState>({
    status: "none",
    question: "",
  });
  const [plannerError, setPlannerError] = useState<string>("");
  const [plannerDiagnostics, setPlannerDiagnostics] = useState<any>(null);
  const [plannerLiveProgress, setPlannerLiveProgress] = useState<PlannerLiveProgress | null>(null);
  const [plannerElapsedSeconds, setPlannerElapsedSeconds] = useState(0);
  const [workspaceBrowserOpen, setWorkspaceBrowserOpen] = useState(false);
  const [workspaceBrowserDir, setWorkspaceBrowserDir] = useState("");
  const [workspaceBrowserSearch, setWorkspaceBrowserSearch] = useState("");
  const [validationBadge, setValidationBadge] = useState<string>("");
  const [generatedSkill, setGeneratedSkill] = useState<any>(null);
  const [showArtifactPreview, setShowArtifactPreview] = useState<boolean>(false);
  const [artifactPreviewKey, setArtifactPreviewKey] = useState<string>("SKILL.md");
  const [installStatus, setInstallStatus] = useState<string>("");
  const [wizard, setWizard] = useState<WizardState>(() =>
    createDefaultWizardState(defaultProvider, defaultModel)
  );
  const plannerDefaultInitializedRef = useRef(
    Boolean(String(defaultProvider || "").trim() && String(defaultModel || "").trim())
  );

  const providersCatalogQuery = useQuery({
    queryKey: ["settings", "providers", "catalog"],
    queryFn: () => client.providers.catalog().catch(() => ({ all: [] })),
    refetchInterval: 30000,
  });
  const providersConfigQuery = useQuery({
    queryKey: ["settings", "providers", "config"],
    queryFn: () => client.providers.config().catch(() => ({})),
    refetchInterval: 30000,
  });
  const mcpServersQuery = useQuery({
    queryKey: ["mcp", "servers"],
    queryFn: () => client.mcp.list().catch(() => ({})),
    refetchInterval: 12000,
  });
  const healthQuery = useQuery({
    queryKey: ["global", "health"],
    queryFn: () => client.health().catch(() => ({})),
    refetchInterval: 30000,
  });
  const workspaceBrowserQuery = useQuery({
    queryKey: ["automations", "workspace-browser", workspaceBrowserDir],
    enabled: workspaceBrowserOpen && !!workspaceBrowserDir,
    queryFn: () =>
      api(`/api/orchestrator/workspaces/list?dir=${encodeURIComponent(workspaceBrowserDir)}`, {
        method: "GET",
      }),
  });

  const providerOptions = useMemo(() => {
    return buildPlannerProviderOptions({
      providerCatalog: providersCatalogQuery.data,
      providerConfig: providersConfigQuery.data,
      defaultProvider: String(providersConfigQuery.data?.default || defaultProvider || "").trim(),
      defaultModel: String(
        providersConfigQuery.data?.providers?.[
          String(providersConfigQuery.data?.default || defaultProvider || "").trim()
        ]?.default_model ||
          providersConfigQuery.data?.providers?.[
            String(providersConfigQuery.data?.default || defaultProvider || "").trim()
          ]?.defaultModel ||
          defaultModel ||
          ""
      ).trim(),
    });
  }, [defaultModel, defaultProvider, providersCatalogQuery.data, providersConfigQuery.data]);
  const mcpServers = useMemo(
    () => normalizeMcpServers(mcpServersQuery.data),
    [mcpServersQuery.data]
  );
  const workspaceDirectories = Array.isArray(workspaceBrowserQuery.data?.directories)
    ? workspaceBrowserQuery.data.directories
    : [];
  const workspaceParentDir = String(workspaceBrowserQuery.data?.parent || "").trim();
  const workspaceCurrentBrowseDir = String(
    workspaceBrowserQuery.data?.dir || workspaceBrowserDir || ""
  ).trim();
  const workspaceSearchQuery = String(workspaceBrowserSearch || "")
    .trim()
    .toLowerCase();
  const filteredWorkspaceDirectories = useMemo(() => {
    if (!workspaceSearchQuery) return workspaceDirectories;
    return workspaceDirectories.filter((entry: any) => {
      const name = String(entry?.name || entry?.path || "")
        .trim()
        .toLowerCase();
      return name.includes(workspaceSearchQuery);
    });
  }, [workspaceDirectories, workspaceSearchQuery]);
  const plannerLatencyAdvisory = useMemo(() => buildPlannerLatencyAdvisory(wizard), [wizard]);
  const invalidateMcp = async () => {
    await queryClient.invalidateQueries({ queryKey: ["mcp"] });
  };
  const reportPlannerDiagnostics = (response: any) => {
    const diagnostics = response?.planner_diagnostics || response?.plannerDiagnostics || null;
    setPlannerDiagnostics(diagnostics);
    const fallbackError = describePlannerFallback(diagnostics);
    if (fallbackError) toast("err", fallbackError);
  };

  useEffect(() => {
    const configDefaultProvider = String(
      providersConfigQuery.data?.default || defaultProvider || ""
    ).trim();
    if (!configDefaultProvider) return;
    const selectedProvider =
      providerOptions.find((provider) => provider.id === configDefaultProvider)?.id ||
      providerOptions[0]?.id ||
      "";
    if (!selectedProvider) return;
    const models =
      providerOptions.find((provider) => provider.id === selectedProvider)?.models || [];
    const configuredDefaultModel = String(
      providersConfigQuery.data?.providers?.[selectedProvider]?.default_model ||
        providersConfigQuery.data?.providers?.[selectedProvider]?.defaultModel ||
        defaultModel ||
        ""
    ).trim();
    const configDefaultModel = preferredProviderModel(
      selectedProvider,
      models,
      configuredDefaultModel
    );
    const initializePlanner = !plannerDefaultInitializedRef.current;
    if (initializePlanner) plannerDefaultInitializedRef.current = true;
    setWizard((current) => {
      return {
        ...current,
        modelProvider: current.modelProvider || selectedProvider,
        modelId: current.modelId || configDefaultModel,
        plannerModelProvider: initializePlanner
          ? current.plannerModelProvider || selectedProvider
          : current.plannerModelProvider,
        plannerModelId: initializePlanner
          ? current.plannerModelId || configDefaultModel
          : current.plannerModelId,
      };
    });
  }, [defaultModel, defaultProvider, providerOptions, providersConfigQuery.data]);

  useEffect(() => {
    const defaultWorkspaceRoot = String(
      (healthQuery.data as any)?.workspaceRoot || (healthQuery.data as any)?.workspace_root || ""
    ).trim();
    if (!defaultWorkspaceRoot) return;
    setWizard((current) => {
      if (String(current.workspaceRoot || "").trim()) return current;
      return { ...current, workspaceRoot: defaultWorkspaceRoot };
    });
  }, [healthQuery.data]);

  const matchMutation = useMutation({
    mutationFn: async (goal: string) => {
      if (!goal.trim() || !client?.skills?.match) return null;
      return client.skills.match({ goal, maxMatches: 3, threshold: 0.35 });
    },
  });
  const compileMutation = useMutation({
    mutationFn: async () => {
      if (!client?.workflowPlans?.chatStart)
        throw new Error(
          "This control panel build is missing workflow planner client support. Rebuild the control panel against the local tandem client package."
        );
      const progressID = createPlannerProgressID();
      plannerProgressIDRef.current = progressID;
      return (
        (await client.workflowPlans.chatStart({
          prompt: wizard.goal,
          schedule: toSchedulePayload(wizard),
          plan_source: planSource,
          progress_id: progressID,
          allowed_mcp_servers: wizard.selectedMcpServers,
          workspace_root: wizard.workspaceRoot,
          operator_preferences: buildOperatorPreferences(wizard),
        })) || null
      );
    },
    onSuccess: (res) => {
      const clarification = summarizePlannerClarification(res);
      setPlanPreview(res?.plan || null);
      setOverlapAnalysis(res?.overlap_analysis || res?.overlapAnalysis || null);
      setOverlapDecision("");
      setPlanningConversation(res?.conversation || null);
      setPlanningChangeSummary([]);
      setPlanningClarification(clarification);
      setPlannerError("");
      reportPlannerDiagnostics(res);
    },
    onError: (error) => {
      setPlanPreview(null);
      setOverlapAnalysis(null);
      setOverlapDecision("");
      setPlanningConversation(null);
      setPlanningChangeSummary([]);
      setPlanningClarification({ status: "none", question: "" });
      const message = error instanceof Error ? error.message : String(error);
      setPlannerError(message);
      setPlannerDiagnostics(null);
      toast("err", message);
    },
  });
  const planningMessageMutation = useMutation({
    mutationFn: async (message: string) => {
      if (!client?.workflowPlans?.chatMessage || !planPreview?.plan_id) return null;
      return client.workflowPlans.chatMessage({ plan_id: planPreview.plan_id, message });
    },
    onSuccess: (res) => {
      const clarification = summarizePlannerClarification(res);
      setPlanPreview(res?.plan || null);
      setOverlapAnalysis(res?.overlap_analysis || res?.overlapAnalysis || null);
      setOverlapDecision("");
      setPlanningConversation(res?.conversation || null);
      setPlanningChangeSummary(
        Array.isArray(res?.change_summary)
          ? res.change_summary.map((row: any) => String(row || "").trim()).filter(Boolean)
          : []
      );
      setPlanningClarification(clarification);
      setPlannerError("");
      reportPlannerDiagnostics(res);
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setPlanningClarification({ status: "none", question: "" });
      setPlannerError(message);
      toast("err", message);
    },
  });
  const planningResetMutation = useMutation({
    mutationFn: async () => {
      if (!client?.workflowPlans?.chatReset || !planPreview?.plan_id) return null;
      return client.workflowPlans.chatReset({ plan_id: planPreview.plan_id });
    },
    onSuccess: (res) => {
      const clarification = summarizePlannerClarification(res);
      setPlanPreview(res?.plan || null);
      setOverlapAnalysis(res?.overlap_analysis || res?.overlapAnalysis || null);
      setOverlapDecision("");
      setPlanningConversation(res?.conversation || null);
      setPlanningChangeSummary([]);
      setPlanningClarification(clarification);
      setPlannerError("");
      reportPlannerDiagnostics(res);
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setPlanningClarification({ status: "none", question: "" });
      setPlannerError(message);
      toast("err", message);
    },
  });
  const plannerProviderPending = compileMutation.isPending || planningMessageMutation.isPending;
  useEffect(() => {
    if (!plannerProviderPending) return;
    const startedAt = Date.now();
    setPlannerLiveProgress(null);
    setPlannerElapsedSeconds(0);
    const interval = window.setInterval(() => {
      setPlannerElapsedSeconds(Math.max(0, Math.floor((Date.now() - startedAt) / 1_000)));
    }, 1_000);
    return () => window.clearInterval(interval);
  }, [plannerProviderPending]);
  useEngineStream(
    "/api/engine/global/event",
    (event) => {
      if (!plannerProviderPending) return;
      try {
        const payload = JSON.parse(String(event.data || "{}"));
        if (payload?.type !== "workflow_planner.progress") return;
        const properties = payload?.properties || {};
        const runID = String(properties.runID || "").trim();
        const expectedRunID = compileMutation.isPending
          ? plannerProgressIDRef.current
            ? `workflow-plan-build:${plannerProgressIDRef.current}`
            : ""
          : planningMessageMutation.isPending && planPreview?.plan_id
            ? `workflow-plan-revision:${String(planPreview.plan_id).trim()}`
            : "";
        if (!expectedRunID || runID !== expectedRunID) return;
        const phase = String(properties.phase || "")
          .trim()
          .toLowerCase();
        const responseChars = Math.max(0, Number(properties.responseChars) || 0);
        const elapsedMs = Math.max(0, Number(properties.elapsedMs) || 0);
        setPlannerLiveProgress((current) => ({
          phase,
          providerID: String(properties.providerID || current?.providerID || "").trim(),
          modelID: String(properties.modelID || current?.modelID || "").trim(),
          responseChars:
            phase === "dispatch" || phase === "retrying"
              ? responseChars
              : Math.max(current?.responseChars || 0, responseChars),
          elapsedMs: Math.max(current?.elapsedMs || 0, elapsedMs),
        }));
      } catch {
        // Ignore malformed or unrelated global events.
      }
    },
    { enabled: true }
  );
  const mcpActionMutation = useMutation({
    mutationFn: async ({
      action,
      server,
    }: {
      action: "connect" | "authenticate";
      server: McpServerOption;
    }) => {
      if (action === "connect") return client.mcp.connect(server.name);
      return api(`/api/engine/mcp/${encodeURIComponent(server.name)}/auth/authenticate`, {
        method: "POST",
      });
    },
    onSuccess: async (result, vars) => {
      await invalidateMcp();
      const pendingAuth =
        !!(result as any)?.pendingAuth ||
        !!(result as any)?.lastAuthChallenge ||
        !!(result as any)?.authorizationUrl;
      const actionOk = (result as any)?.ok !== false;
      const challenge = (result as any)?.lastAuthChallenge || {};
      const challengeUrl = safeString(
        challenge?.authorization_url ||
          challenge?.authorizationUrl ||
          (result as any)?.authorizationUrl ||
          vars.server.authorizationUrl
      );
      const message = safeString(challenge?.message || vars.server.authMessage);

      if (vars.action === "connect") {
        if (pendingAuth || (!actionOk && vars.server.authKind === "oauth")) {
          if (challengeUrl) {
            window.open(challengeUrl, "_blank", "noopener,noreferrer");
          }
          toast(
            "warn",
            message
              ? `OAuth authorization required for ${vars.server.name}: ${message}`
              : `OAuth authorization required for ${vars.server.name}. Finish sign-in in your browser, then click Mark sign-in complete.`
          );
          return;
        }
        if (!actionOk) {
          const errorMessage = safeString(
            (result as any)?.error?.message || (result as any)?.error
          );
          toast(
            "err",
            errorMessage
              ? `Failed to connect ${vars.server.name}: ${errorMessage}`
              : `Failed to connect ${vars.server.name}.`
          );
          return;
        }
        toast("ok", `Connected ${vars.server.name}.`);
        return;
      }

      if (!actionOk && !pendingAuth) {
        const errorMessage = safeString((result as any)?.error?.message || (result as any)?.error);
        toast(
          "err",
          errorMessage
            ? `OAuth authorization check failed for ${vars.server.name}: ${errorMessage}`
            : `OAuth authorization check failed for ${vars.server.name}.`
        );
        return;
      }
      if (pendingAuth) {
        if (challengeUrl) {
          window.open(challengeUrl, "_blank", "noopener,noreferrer");
        }
        toast(
          "warn",
          message
            ? `OAuth authorization still pending for ${vars.server.name}: ${message}`
            : `OAuth authorization still pending for ${vars.server.name}.`
        );
        return;
      }
      toast("ok", `Marked ${vars.server.name} as signed in.`);
    },
    onError: (error) => {
      toast("err", error instanceof Error ? error.message : String(error));
    },
  });
  const validateSkillMutation = useMutation({
    mutationFn: async (skillName: string) => {
      if (!client?.skills?.get || !client?.skills?.validate) return null;
      const loaded = await client.skills.get(skillName);
      const content = (loaded as any)?.content;
      if (!content) return null;
      return client.skills.validate({ content });
    },
    onSuccess: (res) =>
      setValidationBadge(!res ? "" : res.invalid > 0 ? "not_validated" : "validated"),
    onError: () => setValidationBadge("not_validated"),
  });
  const generateSkillMutation = useMutation({
    mutationFn: async () => {
      if (!client?.skills?.generate || !wizard.goal.trim()) return null;
      const prompt = wizard.advancedMode
        ? [
            wizard.goal.trim(),
            wizard.customSkillName ? `Skill name: ${wizard.customSkillName}` : "",
            wizard.customSkillDescription ? `Description: ${wizard.customSkillDescription}` : "",
            `Workflow kind: ${wizard.customWorkflowKind}`,
          ]
            .filter(Boolean)
            .join("\n")
        : wizard.goal;
      return client.skills.generate({ prompt });
    },
    onSuccess: (res) => {
      setGeneratedSkill(res);
      const firstKey = Object.keys((res as any)?.artifacts || {})[0];
      setArtifactPreviewKey(firstKey || "SKILL.md");
      setShowArtifactPreview(false);
      setInstallStatus("");
    },
    onError: () => {
      setGeneratedSkill(null);
      setShowArtifactPreview(false);
      setInstallStatus("Optional skill generation failed.");
    },
  });
  const installGeneratedSkillMutation = useMutation({
    mutationFn: async () => {
      if (!client?.skills?.generateInstall) return null;
      const artifacts = generatedSkill?.artifacts as Record<string, string> | undefined;
      if (!artifacts || !artifacts["SKILL.md"])
        throw new Error("No generated artifacts available to install.");
      return client.skills.generateInstall({
        location: "project",
        conflictPolicy: "rename",
        artifacts: {
          "SKILL.md": artifacts["SKILL.md"],
          "workflow.yaml": artifacts["workflow.yaml"],
          "automation.example.yaml": artifacts["automation.example.yaml"],
        },
      });
    },
    onSuccess: (res) => {
      const name = (res as any)?.skill?.name;
      setInstallStatus(
        name
          ? `Installed optional skill as '${String(name)}' in project skills.`
          : "Installed optional skill in project skills."
      );
      void queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) =>
      setInstallStatus(`Install failed: ${error instanceof Error ? error.message : String(error)}`),
  });
  const deployMutation = useMutation({
    mutationFn: async () => {
      if (!wizard.goal.trim()) throw new Error("Please describe your goal first.");
      if (planningClarification.status === "waiting") {
        throw new Error(
          planningClarification.question ||
            "Planner needs clarification before this automation can be created."
        );
      }
      const fallbackReason = safeString(
        plannerDiagnostics?.fallback_reason || plannerDiagnostics?.fallbackReason
      );
      if (fallbackReason) {
        throw new Error(
          describePlannerFallback(plannerDiagnostics) ||
            "Planner returned a fallback draft instead of a real workflow plan. Regenerate the plan before creating the automation."
        );
      }
      const preview =
        planPreview ||
        (await compileMutation.mutateAsync().catch((error: unknown) => {
          throw error instanceof Error ? error : new Error(String(error));
        }));
      const nextPlan = planWithWizardSettings(preview?.plan || preview, wizard);
      if (!nextPlan) throw new Error("Workflow plan preview failed.");
      if (
        (overlapAnalysis?.requires_user_confirmation ||
          overlapAnalysis?.requiresUserConfirmation) &&
        !overlapDecision.trim()
      ) {
        throw new Error("Select an overlap decision before creating the automation.");
      }
      return api("/api/engine/workflow-plans/apply", {
        method: "POST",
        body: JSON.stringify({
          plan: nextPlan,
          creator_id: "control-panel",
          overlap_decision: overlapDecision.trim() || undefined,
          idempotency_key: automationApplyIdempotencyKey(
            nextPlan,
            overlapDecision.trim(),
            wizard.exportPackDraft
          ),
          ...(wizard.exportPackDraft
            ? { pack_builder_export: { enabled: true, auto_apply: false } }
            : {}),
        }),
      });
    },
    onSuccess: async (res) => {
      try {
        sessionStorage.removeItem(AUTOMATION_WIZARD_DRAFT_KEY);
      } catch {
        // ignore storage failures
      }
      const exportStatus = res?.pack_builder_export?.status;
      toast(
        "ok",
        exportStatus === "preview_pending"
          ? "🎉 Automation created and reusable pack draft exported. Check Pack Builder to continue."
          : "🎉 Automation created! Check 'Library' to see it running."
      );
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["automations"] }),
        queryClient.invalidateQueries({ queryKey: ["mcp"] }),
      ]);
      plannerDefaultInitializedRef.current = Boolean(
        String(defaultProvider || "").trim() && String(defaultModel || "").trim()
      );
      setWizard(
        createDefaultWizardState(
          defaultProvider,
          defaultModel,
          String(
            (healthQuery.data as any)?.workspaceRoot ||
              (healthQuery.data as any)?.workspace_root ||
              ""
          ).trim()
        )
      );
      setRouterMatches([]);
      setPlanSource("automations_page");
      setPlanPreview(null);
      setOverlapAnalysis(null);
      setOverlapDecision("");
      setPlanningConversation(null);
      setPlanningChangeSummary([]);
      setPlanningClarification({ status: "none", question: "" });
      setPlannerError("");
      setValidationBadge("");
      setGeneratedSkill(null);
      setShowArtifactPreview(false);
      setArtifactPreviewKey("SKILL.md");
      setInstallStatus("");
      setStep(1);
      onCreated?.();
    },
    onError: async (error) => {
      const message = error instanceof Error ? error.message : String(error);
      const errorCode = String((error as any)?.code || "").trim();
      const operationApplied = (error as any)?.details?.operationApplied === true;
      const auditOutcomeUncertain =
        errorCode === "PROTECTED_AUDIT_PERSISTENCE_FAILED" && operationApplied;
      const displayedMessage = auditOutcomeUncertain
        ? "The automation was saved, but its required audit record failed. Tandem opened the Library so you can verify it. Do not click Create again while audit storage is unhealthy."
        : message;
      setPlannerError(displayedMessage);
      toast("err", displayedMessage);
      if (auditOutcomeUncertain) {
        await queryClient.invalidateQueries({ queryKey: ["automations"] });
        onCreated?.();
      }
    },
  });
  const importAutomationMutation = useMutation({
    mutationFn: async (file: File) => {
      const text = await file.text();
      let parsed: unknown;
      try {
        parsed = JSON.parse(text);
      } catch {
        throw new Error("Import file must be valid JSON.");
      }
      const payload = automationSpecFromImportedJson(parsed);
      const automationId = safeString(payload.automation_id);
      let existing = false;
      if (automationId) {
        try {
          await api(`/api/engine/automations/v2/${encodeURIComponent(automationId)}`);
          existing = true;
        } catch (error) {
          if ((error as any)?.status && (error as any).status !== 404) {
            throw error;
          }
        }
      }
      if (
        existing &&
        !window.confirm(
          `Replace the existing automation "${safeString(payload.name)}" with this imported JSON?`
        )
      ) {
        return { cancelled: true, automation: null, name: safeString(payload.name) };
      }
      const response = await api("/api/engine/automations/v2", {
        method: "POST",
        body: JSON.stringify(payload),
      });
      return {
        cancelled: false,
        automation: response?.automation || response,
        name: safeString(payload.name),
      };
    },
    onSuccess: async (result) => {
      if (result?.cancelled) return;
      const importedName =
        safeString(result?.automation?.name) || safeString(result?.name) || "Automation";
      toast("ok", `Imported "${importedName}".`);
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      onCreated?.();
    },
    onError: (error) => {
      toast("err", error instanceof Error ? error.message : String(error));
    },
    onSettled: () => {
      if (importAutomationFileRef.current) {
        importAutomationFileRef.current.value = "";
      }
    },
  });

  const workspaceRootError = validateWorkspaceRootInput(wizard.workspaceRoot);
  const plannerModelError = validatePlannerModelInput(
    wizard.plannerModelProvider,
    wizard.plannerModelId
  );
  const roleModelsError = validateRoleModelsJsonInput(wizard.roleModelsJson);
  const navigationLock = useMemo<NavigationLockState | null>(() => {
    if (compileMutation.isPending) {
      return {
        title: "Generating mission plan",
        message: "Tandem is drafting the workflow plan. Live planner activity appears below.",
        progress: {
          stage: plannerProgressStage(plannerLiveProgress?.phase || "preparing"),
          provider: plannerLiveProgress?.providerID || wizard.plannerModelProvider,
          model: plannerLiveProgress?.modelID || wizard.plannerModelId,
          elapsedSeconds: Math.max(
            plannerElapsedSeconds,
            Math.ceil((plannerLiveProgress?.elapsedMs || 0) / 1_000)
          ),
          responseChars: plannerLiveProgress?.responseChars || 0,
        },
      };
    }
    if (generateSkillMutation.isPending || installGeneratedSkillMutation.isPending) {
      return {
        title: "Generating reusable skill",
        message: "Tandem is drafting the reusable skill. Stay on this page until it finishes.",
      };
    }
    if (planningMessageMutation.isPending || planningResetMutation.isPending) {
      return {
        title: "Updating planning draft",
        message: planningMessageMutation.isPending
          ? "Tandem is revising the draft. Live planner activity appears below."
          : "Tandem is resetting the planning draft. Stay on this page until it finishes.",
        progress: planningMessageMutation.isPending
          ? {
              stage: plannerProgressStage(plannerLiveProgress?.phase || "preparing"),
              provider: plannerLiveProgress?.providerID || wizard.plannerModelProvider,
              model: plannerLiveProgress?.modelID || wizard.plannerModelId,
              elapsedSeconds: Math.max(
                plannerElapsedSeconds,
                Math.ceil((plannerLiveProgress?.elapsedMs || 0) / 1_000)
              ),
              responseChars: plannerLiveProgress?.responseChars || 0,
            }
          : undefined,
      };
    }
    if (deployMutation.isPending) {
      return {
        title: "Creating automation",
        message: "Tandem is creating the automation. Stay on this page until it finishes.",
      };
    }
    if (importAutomationMutation.isPending) {
      return {
        title: "Importing automation",
        message: "Tandem is importing the automation JSON. Stay on this page until it finishes.",
      };
    }
    return null;
  }, [
    compileMutation.isPending,
    deployMutation.isPending,
    generateSkillMutation.isPending,
    importAutomationMutation.isPending,
    installGeneratedSkillMutation.isPending,
    plannerElapsedSeconds,
    plannerLiveProgress,
    planningMessageMutation.isPending,
    planningResetMutation.isPending,
    wizard.plannerModelId,
    wizard.plannerModelProvider,
  ]);
  useLayoutEffect(() => {
    onNavigationLockChange?.(navigationLock);
    return () => {
      onNavigationLockChange?.(null);
    };
  }, [navigationLock, onNavigationLockChange]);
  useLayoutEffect(() => {
    const root = wizardRootRef.current;
    if (!root) return;
    root.scrollIntoView({ block: "start", behavior: "auto" });
    const scrollParent = findScrollableParent(root);
    if (scrollParent) {
      scrollParent.scrollTo({ top: 0, behavior: "auto" });
      return;
    }
    window.scrollTo({ top: 0, behavior: "auto" });
  }, [step]);
  const timezoneError =
    String(wizard.timezone || "").trim().length > 0 && !isValidTimezone(wizard.timezone)
      ? "Timezone must be a valid IANA timezone like Europe/Berlin."
      : "";
  const canAdvance =
    step === 1
      ? wizard.goal.trim().length > 8
      : step === 2
        ? (wizard.scheduleKind === "manual" ||
            (wizard.scheduleKind === "cron" && !!wizard.cron.trim()) ||
            (wizard.scheduleKind === "interval" &&
              (Number.parseInt(String(wizard.intervalSeconds || "0"), 10) || 0) > 0) ||
            !!wizard.schedulePreset) &&
          !timezoneError
        : step === 3
          ? !!wizard.mode && !workspaceRootError && !plannerModelError && !roleModelsError
          : true;

  const goToNextStep = async () => {
    if (step === 1) {
      const result = await matchMutation.mutateAsync(wizard.goal);
      if (result && result.decision === "match" && result.skill_name) {
        void validateSkillMutation.mutateAsync(String(result.skill_name));
        setWizard((s) => ({
          ...s,
          routedSkill: String(result.skill_name),
          routingConfidence:
            typeof result.confidence === "number" ? `${Math.round(result.confidence * 100)}%` : "",
        }));
      } else {
        setValidationBadge("");
        setWizard((s) => ({ ...s, routedSkill: "", routingConfidence: "" }));
      }
      const top = Array.isArray((result as any)?.top_matches) ? (result as any).top_matches : [];
      setRouterMatches(top);
    }
    const next = (step + 1) as WizardStep;
    if (next === 4) {
      setPlannerError("");
      setPlanPreview(null);
      setPlanningConversation(null);
      setPlanningChangeSummary([]);
      setPlanningClarification({ status: "none", question: "" });
      setPlannerDiagnostics(null);
      if (plannerLatencyAdvisory) {
        toast("info", plannerLatencyAdvisory);
      }
      try {
        await compileMutation.mutateAsync();
      } catch {
        return;
      }
    }
    setStep(next);
  };

  useEffect(() => {
    try {
      const rawDraft = sessionStorage.getItem(AUTOMATION_WIZARD_DRAFT_KEY);
      if (rawDraft) {
        sessionStorage.removeItem(AUTOMATION_WIZARD_DRAFT_KEY);
        const saved = JSON.parse(rawDraft);
        const savedWizard = saved?.wizard;
        if (savedWizard && typeof savedWizard === "object") {
          if (
            Object.prototype.hasOwnProperty.call(savedWizard, "plannerModelProvider") ||
            Object.prototype.hasOwnProperty.call(savedWizard, "plannerModelId")
          ) {
            plannerDefaultInitializedRef.current = true;
          }
          setWizard((current) => ({ ...current, ...(savedWizard as Partial<WizardState>) }));
          const savedStep = Number(saved?.step);
          if (savedStep >= 1 && savedStep <= 4) {
            setStep(savedStep as WizardStep);
          }
          const nextPlanSource = safeString(saved?.plan_source);
          if (nextPlanSource) {
            setPlanSource(nextPlanSource);
          }
          toast("info", "Restored your automation draft after returning from MCP setup.");
          return;
        }
      }
    } catch {
      // ignore invalid draft payloads
    }
    try {
      const raw = sessionStorage.getItem(AUTOMATION_PLANNER_SEED_KEY);
      if (!raw) return;
      sessionStorage.removeItem(AUTOMATION_PLANNER_SEED_KEY);
      const seed = JSON.parse(raw);
      const prompt = String(seed?.prompt || "").trim();
      if (!prompt) return;
      const nextPlanSource = String(seed?.plan_source || "chat_setup").trim() || "chat_setup";
      setPlanSource(nextPlanSource);
      setWizard((current) => ({ ...current, goal: prompt }));
    } catch {
      return;
    }
  }, [toast]);

  return (
    <div ref={wizardRootRef} className="flex flex-col h-full gap-4 min-h-0 relative">
      <input
        ref={importAutomationFileRef}
        type="file"
        accept=".json,application/json"
        className="hidden"
        onChange={(event) => {
          const file = event.currentTarget.files?.[0];
          if (!file) return;
          importAutomationMutation.mutate(file);
        }}
      />
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-3 border border-slate-800/70 bg-slate-950/30 px-3 py-3">
        <div className="min-w-0">
          <div className="text-xs font-medium uppercase tracking-[0.18em] text-slate-500">
            Import Automation JSON
          </div>
          <div className="mt-1 text-xs text-slate-400">
            Restore a workflow automation exported from another Tandem control panel.
          </div>
        </div>
        <button
          type="button"
          className="tcp-btn h-9 px-3 text-sm"
          onClick={() => importAutomationFileRef.current?.click()}
          disabled={importAutomationMutation.isPending}
        >
          <Icon name="upload" />
          {importAutomationMutation.isPending ? "Importing..." : "Import JSON"}
        </button>
      </div>
      <div className="flex items-center gap-2">
        {AUTOMATION_WIZARD_CONFIG.steps.map((label, i) => {
          const num = (i + 1) as WizardStep;
          const active = num === step;
          const done = num < step;
          return (
            <div key={label} className="flex-1">
              <button
                className={`mb-1 flex w-full items-center gap-1.5 rounded-lg px-2 py-1 text-xs font-medium transition-all ${active ? "bg-amber-500/20 text-amber-300" : done ? "text-slate-400" : "text-slate-600"}`}
                onClick={() => done && setStep(num)}
              >
                <span
                  className={`flex h-5 w-5 items-center justify-center rounded-full text-xs font-bold ${active ? "bg-amber-500 text-black" : done ? "bg-slate-600 text-tcp-text-primary" : "bg-slate-800 text-slate-500"}`}
                >
                  {done ? "✓" : num}
                </span>
                {label}
              </button>
              <div className="h-0.5 w-full rounded-full bg-slate-800">
                <div
                  className="h-full rounded-full bg-amber-500 transition-all"
                  style={{ width: done ? "100%" : active ? "50%" : "0%" }}
                />
              </div>
            </div>
          );
        })}
      </div>

      <AnimatePresence mode="wait">
        <motion.div
          key={step}
          className="flex-1 flex flex-col min-h-0 overflow-hidden"
          initial={{ opacity: 0, x: 16 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0, x: -16 }}
          transition={{ duration: 0.18 }}
        >
          {step === 1 ? (
            <Step1Goal
              value={wizard.goal}
              onChange={(v) => setWizard((s) => ({ ...s, goal: v }))}
              routedSkill={wizard.routedSkill}
              routingConfidence={wizard.routingConfidence}
              validationBadge={validationBadge}
              generatedSkill={generatedSkill}
              advancedMode={wizard.advancedMode}
              customSkillName={wizard.customSkillName}
              customSkillDescription={wizard.customSkillDescription}
              customWorkflowKind={wizard.customWorkflowKind}
              onToggleAdvancedMode={() =>
                setWizard((s) => ({ ...s, advancedMode: !s.advancedMode }))
              }
              onChangeCustomSkillName={(v) => setWizard((s) => ({ ...s, customSkillName: v }))}
              onChangeCustomSkillDescription={(v) =>
                setWizard((s) => ({ ...s, customSkillDescription: v }))
              }
              onChangeCustomWorkflowKind={(v) =>
                setWizard((s) => ({ ...s, customWorkflowKind: v }))
              }
              showArtifactPreview={showArtifactPreview}
              onToggleArtifactPreview={() => setShowArtifactPreview((v) => !v)}
              artifactPreviewKey={artifactPreviewKey}
              onSelectArtifactPreviewKey={(v) => setArtifactPreviewKey(v)}
              onGenerateSkill={() => void generateSkillMutation.mutateAsync()}
              onInstallGeneratedSkill={() => void installGeneratedSkillMutation.mutateAsync()}
              isGeneratingSkill={generateSkillMutation.isPending}
              isInstallingSkill={installGeneratedSkillMutation.isPending}
              installStatus={installStatus}
              topMatches={routerMatches}
              isMatching={matchMutation.isPending}
              goalPlaceholder={AUTOMATION_WIZARD_CONFIG.goalExamples[0]}
            />
          ) : step === 2 ? (
            <Step2Schedule
              selected={wizard.schedulePreset}
              presets={AUTOMATION_WIZARD_CONFIG.schedulePresets}
              onSelect={(preset) =>
                setWizard((s) => ({
                  ...s,
                  schedulePreset: preset.label,
                  scheduleKind:
                    preset.intervalSeconds !== undefined && preset.intervalSeconds !== null
                      ? "interval"
                      : preset.cron
                        ? "cron"
                        : "manual",
                  cron: preset.cron,
                  intervalSeconds:
                    preset.intervalSeconds !== undefined && preset.intervalSeconds !== null
                      ? String(preset.intervalSeconds)
                      : s.intervalSeconds,
                }))
              }
              scheduleValue={{
                scheduleKind: wizard.scheduleKind,
                cronExpression: wizard.cron,
                intervalSeconds: wizard.intervalSeconds,
              }}
              timezone={wizard.timezone}
              timezoneError={timezoneError}
              onScheduleChange={(value) =>
                setWizard((s) => ({
                  ...s,
                  schedulePreset: "",
                  scheduleKind: value.scheduleKind,
                  cron: value.cronExpression,
                  intervalSeconds: value.intervalSeconds,
                }))
              }
              onTimezoneChange={(value) => setWizard((s) => ({ ...s, timezone: value }))}
            />
          ) : step === 3 ? (
            <Step3Mode
              selected={wizard.mode}
              onSelect={(mode) => setWizard((s) => ({ ...s, mode }))}
              executionModes={AUTOMATION_WIZARD_CONFIG.executionModes}
              maxAgents={wizard.maxAgents}
              onMaxAgents={(v) => setWizard((s) => ({ ...s, maxAgents: v }))}
              executionProfile={wizard.executionProfile}
              onExecutionProfileChange={(executionProfile) =>
                setWizard((s) => ({ ...s, executionProfile }))
              }
              workspaceRoot={wizard.workspaceRoot}
              onWorkspaceRootChange={(v) => setWizard((s) => ({ ...s, workspaceRoot: v }))}
              providerOptions={providerOptions}
              providerId={wizard.modelProvider}
              modelId={wizard.modelId}
              plannerProviderId={wizard.plannerModelProvider}
              plannerModelId={wizard.plannerModelId}
              onProviderChange={(v) =>
                setWizard((s) => ({
                  ...s,
                  modelProvider: v,
                  modelId: v === s.modelProvider ? s.modelId : "",
                }))
              }
              onModelChange={(v) => setWizard((s) => ({ ...s, modelId: v }))}
              onPlannerProviderChange={(v) => {
                plannerDefaultInitializedRef.current = true;
                setWizard((s) => ({
                  ...s,
                  plannerModelProvider: v,
                  plannerModelId: v === s.plannerModelProvider ? s.plannerModelId : "",
                }));
              }}
              onPlannerModelChange={(v) => {
                plannerDefaultInitializedRef.current = true;
                setWizard((s) => ({ ...s, plannerModelId: v }));
              }}
              roleModelsJson={wizard.roleModelsJson}
              onRoleModelsChange={(v) => setWizard((s) => ({ ...s, roleModelsJson: v }))}
              roleModelsError={roleModelsError}
              toolAccessMode={wizard.toolAccessMode}
              customToolsText={wizard.customToolsText}
              onToolAccessModeChange={(toolAccessMode) =>
                setWizard((s) => ({ ...s, toolAccessMode }))
              }
              onCustomToolsTextChange={(customToolsText) =>
                setWizard((s) => ({ ...s, customToolsText }))
              }
              mcpServers={mcpServers}
              selectedMcpServers={wizard.selectedMcpServers}
              onToggleMcpServer={(name) =>
                setWizard((s) => ({
                  ...s,
                  selectedMcpServers: s.selectedMcpServers.includes(name)
                    ? s.selectedMcpServers.filter((row) => row !== name)
                    : [...s.selectedMcpServers, name],
                }))
              }
              onConnectMcpServer={(name) => {
                const server = mcpServers.find((row) => row.name === name);
                if (!server) return;
                void mcpActionMutation.mutateAsync({ action: "connect", server });
              }}
              onCompleteMcpSignIn={(name) => {
                const server = mcpServers.find((row) => row.name === name);
                if (!server) return;
                void mcpActionMutation.mutateAsync({ action: "authenticate", server });
              }}
              onRefreshMcpServers={() => {
                void invalidateMcp();
              }}
              onOpenMcpSettings={() => {
                saveAutomationWizardDraft({ wizard, step, planSource });
                toast(
                  "info",
                  "Saved this automation draft. After connecting your MCPs, return to Automations and the wizard will restore where you left off."
                );
                navigate("mcp");
              }}
              activeMcpAction={
                mcpActionMutation.isPending
                  ? {
                      name: mcpActionMutation.variables?.server?.name || "",
                      action: mcpActionMutation.variables?.action || "",
                    }
                  : null
              }
              workspaceRootError={workspaceRootError}
              plannerModelError={plannerModelError}
              workspaceBrowserOpen={workspaceBrowserOpen}
              workspaceBrowserDir={workspaceBrowserDir}
              workspaceBrowserSearch={workspaceBrowserSearch}
              onWorkspaceBrowserSearchChange={setWorkspaceBrowserSearch}
              onOpenWorkspaceBrowser={() => {
                const seed = String(
                  wizard.workspaceRoot ||
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
              onBrowseWorkspaceParent={() => {
                if (!workspaceParentDir) return;
                setWorkspaceBrowserDir(workspaceParentDir);
              }}
              onBrowseWorkspaceDirectory={(path) => setWorkspaceBrowserDir(path)}
              onSelectWorkspaceDirectory={() => {
                if (!workspaceCurrentBrowseDir) return;
                setWizard((s) => ({ ...s, workspaceRoot: workspaceCurrentBrowseDir }));
                setWorkspaceBrowserOpen(false);
                setWorkspaceBrowserSearch("");
                toast("ok", `Workspace selected: ${workspaceCurrentBrowseDir}`);
              }}
              workspaceBrowserParentDir={workspaceParentDir}
              workspaceCurrentBrowseDir={workspaceCurrentBrowseDir}
              filteredWorkspaceDirectories={filteredWorkspaceDirectories}
            />
          ) : (
            <Step4Review
              wizard={wizard}
              onToggleExportPackDraft={() =>
                setWizard((s) => ({ ...s, exportPackDraft: !s.exportPackDraft }))
              }
              onSubmit={() => deployMutation.mutate()}
              overlapAnalysis={overlapAnalysis}
              overlapDecision={overlapDecision}
              onSelectOverlapDecision={setOverlapDecision}
              isPending={deployMutation.isPending}
              planPreview={planPreview}
              isPreviewing={compileMutation.isPending}
              planningConversation={planningConversation}
              planningChangeSummary={planningChangeSummary}
              onSendPlanningMessage={(message) => {
                void planningMessageMutation.mutateAsync(message);
              }}
              isSendingPlanningMessage={planningMessageMutation.isPending}
              onResetPlanningChat={() => {
                void planningResetMutation.mutateAsync();
              }}
              isResettingPlanningChat={planningResetMutation.isPending}
              planningClarification={planningClarification}
              plannerLatencyAdvisory={plannerLatencyAdvisory}
              plannerError={plannerError}
              plannerDiagnostics={plannerDiagnostics}
              generatedSkill={generatedSkill}
              installStatus={installStatus}
              executionModes={AUTOMATION_WIZARD_CONFIG.executionModes}
            />
          )}
        </motion.div>
      </AnimatePresence>

      {step < 4 ? (
        <div className="flex justify-between gap-2 shrink-0">
          <button
            className="tcp-btn"
            disabled={step === 1 || compileMutation.isPending}
            onClick={() => setStep((s) => (s - 1) as WizardStep)}
          >
            ← Back
          </button>
          <button
            className="tcp-btn-primary"
            disabled={!canAdvance || compileMutation.isPending}
            onClick={() => void goToNextStep()}
          >
            {compileMutation.isPending ? "Generating Plan..." : "Next →"}
          </button>
        </div>
      ) : null}
    </div>
  );
}
