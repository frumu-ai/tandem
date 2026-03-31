import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useState } from "react";
import YAML from "yaml";
import { Step1Goal } from "./Step1Goal";
import { Step2Schedule } from "./Step2Schedule";
import { Step3Mode } from "./Step3Mode";
import { Step4Review } from "./Step4Review";

type ExecutionMode = "single" | "team" | "swarm";
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
  schedulePreset: string;
  scheduleKind: "manual" | "cron" | "interval";
  cron: string;
  intervalSeconds: string;
  mode: ExecutionMode;
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
  selectedAgentId: string;
}

interface ProviderOption {
  id: string;
  models: string[];
}

interface McpServerOption {
  name: string;
  connected: boolean;
  enabled: boolean;
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

const AUTOMATION_WIZARD_SOURCES = import.meta.glob("../../pages/automation-wizard.yaml", {
  eager: true,
  query: "?raw",
  import: "default",
}) as Record<string, string>;

function parseAutomationWizardConfig(source: string): AutomationWizardConfig {
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

const AUTOMATION_WIZARD_CONFIG = parseAutomationWizardConfig(
  Object.values(AUTOMATION_WIZARD_SOURCES)[0] || ""
);
const AUTOMATION_PLANNER_SEED_KEY = "tandem.automations.plannerSeed";

function createDefaultWizardState(
  defaultProvider: string,
  defaultModel: string,
  workspaceRoot = ""
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
    schedulePreset: AUTOMATION_WIZARD_CONFIG.defaults.schedulePreset,
    scheduleKind: defaultSchedule.scheduleKind,
    cron: defaultSchedule.cron,
    intervalSeconds: defaultSchedule.intervalSeconds,
    mode: AUTOMATION_WIZARD_CONFIG.defaults.mode,
    maxAgents: AUTOMATION_WIZARD_CONFIG.defaults.maxAgents,
    routedSkill: "",
    routingConfidence: "",
    modelProvider: String(defaultProvider || ""),
    modelId: String(defaultModel || ""),
    plannerModelProvider: "",
    plannerModelId: "",
    roleModelsJson: "",
    toolAccessMode: "all",
    customToolsText: "",
    selectedMcpServers: [],
    exportPackDraft: false,
    advancedMode: false,
    customSkillName: "",
    customSkillDescription: "",
    customWorkflowKind: "pack_builder_recipe",
    selectedAgentId: "",
  };
}

function normalizeMcpServers(raw: any): McpServerOption[] {
  if (Array.isArray(raw?.servers)) {
    return raw.servers
      .map((row: any) => {
        const name = String(row?.name || "").trim();
        if (!name) return null;
        return { name, connected: !!row?.connected, enabled: row?.enabled !== false };
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
        return {
          name: cleanName,
          connected: !!(cfg as any).connected,
          enabled: (cfg as any).enabled !== false,
        };
      })
      .filter((row): row is McpServerOption => !!row)
      .sort((a, b) => a.name.localeCompare(b.name));
  }
  return [];
}

function toSchedulePayload(wizard: WizardState) {
  if (wizard.scheduleKind === "manual") return { type: "manual" };
  if (wizard.scheduleKind === "interval") {
    return {
      interval_seconds: {
        seconds: Math.max(1, Number.parseInt(String(wizard.intervalSeconds || "3600"), 10) || 3600),
      },
    };
  }
  const customCron = String(wizard.cron || "").trim();
  if (customCron) return { cron: { expression: customCron } };
  const preset = AUTOMATION_WIZARD_CONFIG.schedulePresets.find(
    (p) => p.label === wizard.schedulePreset
  );
  if (preset?.intervalSeconds) return { interval_seconds: { seconds: preset.intervalSeconds } };
  if (preset?.cron) return { cron: { expression: preset.cron } };
  return { type: "manual" };
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
    wizard.mode === "swarm"
      ? Math.max(1, Math.min(16, Number.parseInt(String(wizard.maxAgents || "4"), 10) || 4))
      : 1;
  const payload: Record<string, unknown> = {
    execution_mode: wizard.mode,
    max_parallel_agents: maxParallelAgents,
  };
  if (String(wizard.modelProvider || "").trim())
    payload.model_provider = String(wizard.modelProvider).trim();
  if (String(wizard.modelId || "").trim()) payload.model_id = String(wizard.modelId).trim();
  if (roleModels && Object.keys(roleModels).length) payload.role_models = roleModels;
  return payload;
}

export function CreateWizard({
  client,
  api,
  toast,
  navigate,
  defaultProvider,
  defaultModel,
}: {
  client: any;
  api: (path: string, init?: RequestInit) => Promise<any>;
  toast: any;
  navigate: (route: string) => void;
  defaultProvider: string;
  defaultModel: string;
}) {
  const queryClient = useQueryClient();
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
  const [plannerError, setPlannerError] = useState<string>("");
  const [plannerDiagnostics, setPlannerDiagnostics] = useState<any>(null);
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
    const rows = Array.isArray(providersCatalogQuery.data?.all)
      ? providersCatalogQuery.data.all
      : [];
    return rows
      .map((provider: any) => ({
        id: String(provider?.id || "").trim(),
        models: Object.keys(provider?.models || {}),
      }))
      .filter((provider: ProviderOption) => !!provider.id)
      .sort((a: ProviderOption, b: ProviderOption) => a.id.localeCompare(b.id));
  }, [providersCatalogQuery.data]);
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

  useEffect(() => {
    const configDefaultProvider = String(
      providersConfigQuery.data?.default || defaultProvider || ""
    ).trim();
    if (!configDefaultProvider) return;
    const models =
      providerOptions.find((provider) => provider.id === configDefaultProvider)?.models || [];
    const configDefaultModel = String(
      providersConfigQuery.data?.providers?.[configDefaultProvider]?.default_model ||
        defaultModel ||
        models[0] ||
        ""
    ).trim();
    setWizard((current) => {
      if (current.modelProvider && current.modelId) return current;
      return {
        ...current,
        modelProvider: current.modelProvider || configDefaultProvider,
        modelId: current.modelId || configDefaultModel,
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
      return (
        (await client.workflowPlans.chatStart({
          prompt: wizard.goal,
          schedule: toSchedulePayload(wizard),
          plan_source: planSource,
          allowed_mcp_servers: wizard.selectedMcpServers,
          workspace_root: wizard.workspaceRoot,
          operator_preferences: buildOperatorPreferences(wizard),
        })) || null
      );
    },
    onSuccess: (res) => {
      setPlanPreview(res?.plan || null);
      setOverlapAnalysis(res?.overlap_analysis || res?.overlapAnalysis || null);
      setOverlapDecision("");
      setPlanningConversation(res?.conversation || null);
      setPlanningChangeSummary([]);
      setPlannerError("");
      setPlannerDiagnostics(res?.planner_diagnostics || res?.plannerDiagnostics || null);
    },
    onError: (error) => {
      setPlanPreview(null);
      setOverlapAnalysis(null);
      setOverlapDecision("");
      setPlanningConversation(null);
      setPlanningChangeSummary([]);
      setPlannerError(error instanceof Error ? error.message : String(error));
      setPlannerDiagnostics(null);
    },
  });
  const planningMessageMutation = useMutation({
    mutationFn: async (message: string) => {
      if (!client?.workflowPlans?.chatMessage || !planPreview?.plan_id) return null;
      return client.workflowPlans.chatMessage({ plan_id: planPreview.plan_id, message });
    },
    onSuccess: (res) => {
      setPlanPreview(res?.plan || null);
      setOverlapAnalysis(res?.overlap_analysis || res?.overlapAnalysis || null);
      setOverlapDecision("");
      setPlanningConversation(res?.conversation || null);
      setPlanningChangeSummary(
        Array.isArray(res?.change_summary)
          ? res.change_summary.map((row: any) => String(row || "").trim()).filter(Boolean)
          : []
      );
      setPlannerError(
        typeof res?.clarifier?.question === "string" ? String(res.clarifier.question) : ""
      );
      setPlannerDiagnostics(res?.planner_diagnostics || res?.plannerDiagnostics || null);
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
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
      setPlanPreview(res?.plan || null);
      setOverlapAnalysis(res?.overlap_analysis || res?.overlapAnalysis || null);
      setOverlapDecision("");
      setPlanningConversation(res?.conversation || null);
      setPlanningChangeSummary([]);
      setPlannerError("");
      setPlannerDiagnostics(res?.planner_diagnostics || res?.plannerDiagnostics || null);
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setPlannerError(message);
      toast("err", message);
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
      const preview =
        planPreview ||
        (await compileMutation.mutateAsync().catch((error: unknown) => {
          throw error instanceof Error ? error : new Error(String(error));
        }));
      const nextPlan = preview?.plan || preview;
      if (!nextPlan) throw new Error("Workflow plan preview failed.");
      if (
        (overlapAnalysis?.requires_user_confirmation ||
          overlapAnalysis?.requiresUserConfirmation) &&
        !overlapDecision.trim()
      ) {
        throw new Error("Select an overlap decision before creating the automation.");
      }
      return client.workflowPlans.apply({
        plan: nextPlan,
        creator_id: "control-panel",
        overlap_decision: overlapDecision.trim() || undefined,
        ...(wizard.exportPackDraft
          ? { pack_builder_export: { enabled: true, auto_apply: false } }
          : {}),
      });
    },
    onSuccess: async (res) => {
      const exportStatus = res?.pack_builder_export?.status;
      toast(
        "ok",
        exportStatus === "preview_pending"
          ? "🎉 Automation created and reusable pack draft exported. Check Pack Builder to continue."
          : "🎉 Automation created! Check 'My Automations' to see it running."
      );
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["automations"] }),
        queryClient.invalidateQueries({ queryKey: ["mcp"] }),
      ]);
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
      setPlannerError("");
      setValidationBadge("");
      setGeneratedSkill(null);
      setShowArtifactPreview(false);
      setArtifactPreviewKey("SKILL.md");
      setInstallStatus("");
      setStep(1);
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setPlannerError(message);
      toast("err", message);
    },
  });

  const workspaceRootError = validateWorkspaceRootInput(wizard.workspaceRoot);
  const plannerModelError = validatePlannerModelInput(
    wizard.plannerModelProvider,
    wizard.plannerModelId
  );
  const roleModelsError = validateRoleModelsJsonInput(wizard.roleModelsJson);
  const canAdvance =
    step === 1
      ? wizard.goal.trim().length > 8
      : step === 2
        ? wizard.scheduleKind === "manual" ||
          (wizard.scheduleKind === "cron" && !!wizard.cron.trim()) ||
          (wizard.scheduleKind === "interval" &&
            (Number.parseInt(String(wizard.intervalSeconds || "0"), 10) || 0) > 0) ||
          !!wizard.schedulePreset
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
      try {
        await compileMutation.mutateAsync();
      } catch {
        return;
      }
    }
    setStep(next);
  };

  useEffect(() => {
    if (step !== 1) return;
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
  }, [step]);

  return (
    <div className="grid gap-4">
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
                  className={`flex h-5 w-5 items-center justify-center rounded-full text-xs font-bold ${active ? "bg-amber-500 text-black" : done ? "bg-slate-600 text-white" : "bg-slate-800 text-slate-500"}`}
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
              selectedAgentId={wizard.selectedAgentId}
              onChangeSelectedAgentId={(v) => setWizard((s) => ({ ...s, selectedAgentId: v }))}
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
              onScheduleChange={(value) =>
                setWizard((s) => ({
                  ...s,
                  schedulePreset: "",
                  scheduleKind: value.scheduleKind,
                  cron: value.cronExpression,
                  intervalSeconds: value.intervalSeconds,
                }))
              }
            />
          ) : step === 3 ? (
            <Step3Mode
              selected={wizard.mode}
              onSelect={(mode) => setWizard((s) => ({ ...s, mode }))}
              executionModes={AUTOMATION_WIZARD_CONFIG.executionModes}
              maxAgents={wizard.maxAgents}
              onMaxAgents={(v) => setWizard((s) => ({ ...s, maxAgents: v }))}
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
              onPlannerProviderChange={(v) =>
                setWizard((s) => ({
                  ...s,
                  plannerModelProvider: v,
                  plannerModelId: v === s.plannerModelProvider ? s.plannerModelId : "",
                }))
              }
              onPlannerModelChange={(v) => setWizard((s) => ({ ...s, plannerModelId: v }))}
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
              onOpenMcpSettings={() => navigate("mcp")}
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
        <div className="flex justify-between gap-2">
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
