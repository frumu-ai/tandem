import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";
import YAML from "yaml";
import { renderIcons } from "../app/icons.js";
import { AutomationCalendar } from "../features/automations/AutomationCalendar";
import { AutomationsPageTabs } from "../features/automations/AutomationsPageTabs";
import { ScheduleBuilder } from "../features/automations/ScheduleBuilder";
import { SpawnApprovals } from "../features/automations/SpawnApprovals";
import {
  DeleteAutomationDialog,
  LegacyAutomationEditDialog,
  WorkflowAutomationEditDialog,
} from "../features/automations/MyAutomationsDialogs";
import { useAutomationsPageState } from "../features/automations/useAutomationsPageState";
import { useSelectedRunLifecycle } from "../features/automations/useSelectedRunLifecycle";
import { Step2Schedule } from "../features/automations/create/Step2Schedule";
import { Step3Mode } from "../features/automations/create/Step3Mode";
import { Step1Goal as Step1GoalExternal } from "../features/automations/create/Step1Goal";
import { Step4Review as Step4ReviewExternal } from "../features/automations/create/Step4Review";
import { CreateWizard as CreateWizardExternal } from "../features/automations/create/CreateWizard";
import { WorkflowTaskActionsPanel } from "../features/automations/WorkflowTaskActionsPanel";
import { WorkflowArtifactsPanel } from "../features/automations/WorkflowArtifactsPanel";
import { WorkflowBlockersPanel } from "../features/automations/WorkflowBlockersPanel";
import { WorkflowDebugHintsPanel } from "../features/automations/WorkflowDebugHintsPanel";
import { WorkflowLiveSessionLogPanel } from "../features/automations/WorkflowLiveSessionLogPanel";
import { WorkflowMissionObjectivePanel } from "../features/automations/WorkflowMissionObjectivePanel";
import { WorkflowRequiredActionsPanel } from "../features/automations/WorkflowRequiredActionsPanel";
import { WorkflowRunTelemetryPanel } from "../features/automations/WorkflowRunTelemetryPanel";
import { WorkflowRunSummaryPanel } from "../features/automations/WorkflowRunSummaryPanel";
import { WorkflowTaskSignalsPanel } from "../features/automations/WorkflowTaskSignalsPanel";
import { projectOrchestrationRun } from "../features/orchestrator/blackboardProjection";
import { TaskBoard } from "../features/orchestration/TaskBoard";
import {
  workflowActiveSessionCount,
  workflowArtifactCandidates,
  workflowArtifactValidation,
  workflowBlockedNodeCount,
  workflowBlockedNodeIds,
  workflowCompletedNodeCount,
  workflowCompletedNodeIds,
  workflowContextHistoryEntries,
  workflowDerivedRunStatus,
  workflowEventAt,
  workflowEventBlockers,
  workflowEventReason,
  workflowEventRunId,
  workflowEventSessionId,
  workflowEventSummary,
  workflowEventType,
  workflowFirstPendingTaskId,
  workflowLatestLifecycleTaskId,
  workflowNodeOutputEntries,
  workflowNodeOutputText,
  workflowNodeToolTelemetry,
  workflowNodeOutput,
  workflowPendingNodeCount,
  workflowPersistedHistoryEntries,
  workflowSessionLogEventEntries,
  workflowTaskInspectionDetails,
  workflowTelemetryDisplayEntries,
  workflowTelemetrySeedEvents,
  workflowNodeStability,
  workflowProjectionFromRunSnapshot,
  workflowRecentNodeEventSummaries,
  workflowSessionIds,
} from "../features/orchestration/workflowStability";
import { useEngineStream } from "../features/stream/useEngineStream";
import { useCapabilities } from "../features/system/queries.ts";
import { api } from "../lib/api";
import { renderMarkdownSafe } from "../lib/markdown";
import { ProviderModelSelector } from "../components/ProviderModelSelector";
import { describeScheduleValue } from "../features/automations/scheduleBuilder";
import { AdvancedMissionBuilderPanel } from "./AdvancedMissionBuilderPanel";
import { OptimizationCampaignsPanel } from "./OptimizationCampaignsPanel";
import { ScopeInspector } from "../features/automations/ScopeInspector";
import { PageCard, EmptyState, formatJson } from "./ui";
import type { AppPageProps } from "./pageTypes";
import agentCatalog from "../generated/agent-catalog.json";

type AgentCatalogCategory = {
  id: string;
  title: string;
  summary: string;
  source_path: string;
  count: number;
};

type AgentCatalogEntry = {
  id: string;
  name: string;
  summary: string;
  category_id: string;
  category_title: string;
  category_summary: string;
  source_path: string;
  source_file: string;
  sandbox_mode: string;
  target_surfaces: string[];
  instructions: string;
  tags: string[];
  requires: string[];
  role: string;
};

type AgentCatalogIndex = {
  generated_at: string;
  source_root: string;
  categories: AgentCatalogCategory[];
  agents: AgentCatalogEntry[];
};

const CATALOG = agentCatalog as AgentCatalogIndex;

// ─── Types ─────────────────────────────────────────────────────────────────

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

interface WorkflowEditDraft {
  automationId: string;
  name: string;
  description: string;
  scheduleKind: "manual" | "cron" | "interval";
  cronExpression: string;
  intervalSeconds: string;
  workspaceRoot: string;
  executionMode: ExecutionMode;
  maxParallelAgents: string;
  modelProvider: string;
  modelId: string;
  plannerModelProvider: string;
  plannerModelId: string;
  toolAccessMode: WorkflowToolAccessMode;
  customToolsText: string;
  selectedMcpServers: string[];
  nodes: WorkflowNodeEditDraft[];
  scopeSnapshot: any | null;
  planPackageBundle: any | null;
  planPackageReplay: any | null;
  scopeValidation: any | null;
  runtimeContext: any | null;
  approvedPlanMaterialization: any | null;
  connectorBindingsJson: string;
}

interface WorkflowNodeEditDraft {
  nodeId: string;
  title: string;
  objective: string;
  agentId: string;
  modelProvider: string;
  modelId: string;
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

// ─── Constants ──────────────────────────────────────────────────────────────

const AUTOMATION_WIZARD_SOURCES = import.meta.glob("./automation-wizard.yaml", {
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

  if (!Array.isArray(steps) || !steps.length) {
    throw new Error("Invalid automation wizard config: steps must be a non-empty array.");
  }
  if (!Array.isArray(schedulePresets) || !schedulePresets.length) {
    throw new Error("Invalid automation wizard config: schedulePresets must be a non-empty array.");
  }
  if (!Array.isArray(executionModes) || !executionModes.length) {
    throw new Error("Invalid automation wizard config: executionModes must be a non-empty array.");
  }
  if (!Array.isArray(goalExamples) || !goalExamples.length) {
    throw new Error("Invalid automation wizard config: goalExamples must be a non-empty array.");
  }

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
        ? {
            scheduleKind: "cron" as const,
            cron: defaultPreset.cron,
            intervalSeconds: "3600",
          }
        : {
            scheduleKind: "manual" as const,
            cron: "",
            intervalSeconds: "3600",
          };
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

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function normalizeMcpServers(raw: any): McpServerOption[] {
  if (Array.isArray(raw?.servers)) {
    return raw.servers
      .map((row: any) => {
        const name = String(row?.name || "").trim();
        if (!name) return null;
        return {
          name,
          connected: !!row?.connected,
          enabled: row?.enabled !== false,
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

function formatAgentInstructionsMarkdown(raw: string) {
  const lines = String(raw || "")
    .split(/\r?\n/)
    .map((line) => line.trimEnd());
  const out: string[] = [];
  let inSectionList = false;
  const sectionHeaders = new Set(["Working mode", "Focus on", "Quality checks", "Return"]);

  for (const rawLine of lines) {
    const line = rawLine.trim();
    if (!line) {
      if (inSectionList && out[out.length - 1] !== "") out.push("");
      inSectionList = false;
      continue;
    }
    if (/^\d+\.\s+/.test(line) || /^[-*]\s+/.test(line)) {
      out.push(line);
      inSectionList = true;
      continue;
    }
    const headerMatch = line.match(/^([A-Za-z][A-Za-z\s]+):$/);
    if (headerMatch) {
      const heading = headerMatch[1].trim();
      out.push(`### ${heading}`);
      inSectionList = sectionHeaders.has(heading);
      continue;
    }
    if (inSectionList) {
      out.push(`- ${line}`);
      continue;
    }
    out.push(line);
  }

  return out
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function toSchedulePayload(wizard: WizardState) {
  if (wizard.scheduleKind === "manual") {
    return { type: "manual" };
  }
  if (wizard.scheduleKind === "interval") {
    return {
      interval_seconds: {
        seconds: Math.max(1, Number.parseInt(String(wizard.intervalSeconds || "3600"), 10) || 3600),
      },
    };
  }
  const customCron = String(wizard.cron || "").trim();
  if (customCron) {
    return { cron: { expression: customCron } };
  }
  const preset = AUTOMATION_WIZARD_CONFIG.schedulePresets.find(
    (p) => p.label === wizard.schedulePreset
  );
  if (preset?.intervalSeconds) {
    return { interval_seconds: { seconds: preset.intervalSeconds } };
  }
  if (preset?.cron) {
    return { cron: { expression: preset.cron } };
  }
  return { type: "manual" };
}

function formatScheduleLabel(schedule: any) {
  const cronExpr = String(schedule?.cron?.expression || schedule?.cron_expression || "").trim();
  if (cronExpr) {
    return describeScheduleValue({
      scheduleKind: "cron",
      cronExpression: cronExpr,
      intervalSeconds: "3600",
    });
  }
  const seconds = Number(schedule?.interval_seconds?.seconds);
  if (Number.isFinite(seconds) && seconds > 0) {
    return describeScheduleValue({
      scheduleKind: "interval",
      cronExpression: "",
      intervalSeconds: String(seconds),
    });
  }
  return "manual";
}

function formatAutomationV2ScheduleLabel(schedule: any) {
  const type = String(schedule?.type || "")
    .trim()
    .toLowerCase();
  if (type === "cron") {
    return describeScheduleValue({
      scheduleKind: "cron",
      cronExpression: String(schedule?.cron_expression || schedule?.cronExpression || ""),
      intervalSeconds: "3600",
    });
  }
  if (type === "interval") {
    const seconds = Number(schedule?.interval_seconds || schedule?.intervalSeconds || 0);
    if (!Number.isFinite(seconds) || seconds <= 0) return "interval";
    return describeScheduleValue({
      scheduleKind: "interval",
      cronExpression: "",
      intervalSeconds: String(seconds),
    });
  }
  return "manual";
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

function validateModelInput(provider: string, model: string) {
  const providerValue = String(provider || "").trim();
  const modelValue = String(model || "").trim();
  if (!providerValue && !modelValue) return "";
  if (!providerValue) return "Model provider is required when a model is set.";
  if (!modelValue) return "Model is required when a provider is set.";
  return "";
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
    roleModels.planner = {
      provider_id: plannerModelProvider,
      model_id: plannerModelId,
    };
  }
  const maxParallelAgents =
    wizard.mode === "swarm"
      ? Math.max(1, Math.min(16, Number.parseInt(String(wizard.maxAgents || "4"), 10) || 4))
      : 1;
  const payload: Record<string, unknown> = {
    execution_mode: wizard.mode,
    max_parallel_agents: maxParallelAgents,
  };
  if (String(wizard.modelProvider || "").trim()) {
    payload.model_provider = String(wizard.modelProvider).trim();
  }
  if (String(wizard.modelId || "").trim()) {
    payload.model_id = String(wizard.modelId).trim();
  }
  if (roleModels && Object.keys(roleModels).length) {
    payload.role_models = roleModels;
  }
  return payload;
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

function scheduleToEditor(schedule: any) {
  const type = String(schedule?.type || "")
    .trim()
    .toLowerCase();
  const cronExpression = String(
    schedule?.cron?.expression || schedule?.cron_expression || schedule?.cron || ""
  ).trim();
  const intervalValue = Number(
    schedule?.interval_seconds?.seconds ||
      schedule?.interval_seconds ||
      schedule?.intervalSeconds ||
      3600
  );
  const intervalSeconds =
    Number.isFinite(intervalValue) && intervalValue > 0 ? Math.round(intervalValue) : 3600;
  return {
    scheduleKind:
      type === "manual"
        ? ("manual" as const)
        : cronExpression
          ? ("cron" as const)
          : ("interval" as const),
    cronExpression,
    intervalSeconds,
  };
}

function normalizeMcpServerNamespace(raw: string) {
  let out = "";
  let previousUnderscore = false;
  for (const ch of String(raw || "").trim()) {
    if (/^[a-z0-9]$/i.test(ch)) {
      out += ch.toLowerCase();
      previousUnderscore = false;
    } else if (!previousUnderscore) {
      out += "_";
      previousUnderscore = true;
    }
  }
  return out.replace(/^_+|_+$/g, "") || "mcp";
}

function normalizeAllowedTools(raw: string[]) {
  const seen = new Set<string>();
  const values: string[] = [];
  for (const row of raw) {
    const value = String(row || "").trim();
    if (!value || seen.has(value)) continue;
    seen.add(value);
    values.push(value);
  }
  return values;
}

function parseCustomToolText(raw: string) {
  return normalizeAllowedTools(
    String(raw || "")
      .split(/[\n,]/g)
      .map((value) => String(value || "").trim())
      .filter(Boolean)
  );
}

function formatCustomToolText(raw: string[]) {
  return normalizeAllowedTools(raw).join("\n");
}

function compileWorkflowToolAllowlist(
  selectedMcpServers: string[],
  toolAccessMode: WorkflowToolAccessMode,
  customToolsText: string
) {
  if (toolAccessMode === "all") return ["*"];
  return normalizeAllowedTools([
    ...parseCustomToolText(customToolsText),
    ...selectedMcpServers.map((server) => `mcp.${normalizeMcpServerNamespace(server)}.*`),
  ]);
}

function extractAutomationOperatorPreferences(automation: any) {
  const metadataPrefs =
    automation?.metadata?.operator_preferences || automation?.metadata?.operatorPreferences;
  if (metadataPrefs && typeof metadataPrefs === "object") {
    return metadataPrefs as Record<string, any>;
  }
  const firstAgent = Array.isArray(automation?.agents) ? automation.agents[0] : null;
  const defaultModel =
    firstAgent?.model_policy?.default_model || firstAgent?.modelPolicy?.defaultModel || null;
  const roleModels =
    firstAgent?.model_policy?.role_models || firstAgent?.modelPolicy?.roleModels || null;
  const fallback: Record<string, any> = {};
  if (defaultModel?.provider_id || defaultModel?.providerId) {
    fallback.model_provider = defaultModel.provider_id || defaultModel.providerId;
  }
  if (defaultModel?.model_id || defaultModel?.modelId) {
    fallback.model_id = defaultModel.model_id || defaultModel.modelId;
  }
  if (roleModels && typeof roleModels === "object") {
    fallback.role_models = roleModels;
  }
  if (automation?.execution?.max_parallel_agents || automation?.execution?.maxParallelAgents) {
    fallback.max_parallel_agents =
      automation.execution.max_parallel_agents || automation.execution.maxParallelAgents;
  }
  return fallback;
}

function workflowToolAccessFromAutomation(automation: any) {
  const allowlist = normalizeAllowedTools(
    (Array.isArray(automation?.agents)
      ? automation.agents.flatMap((agent: any) =>
          Array.isArray(agent?.tool_policy?.allowlist) ? agent.tool_policy.allowlist : []
        )
      : []
    )
      .map((value: any) => String(value || "").trim())
      .filter(Boolean)
  );
  if (!allowlist.length || allowlist.includes("*")) {
    return { toolAccessMode: "all" as const, customToolsText: "" };
  }
  const customTools = allowlist.filter((tool) => !tool.startsWith("mcp."));
  return {
    toolAccessMode: "custom" as const,
    customToolsText: formatCustomToolText(customTools),
  };
}

function connectorBindingsJsonFromPlanPackage(planPackage: any | null) {
  return formatJson(
    Array.isArray(planPackage?.connector_bindings) ? planPackage.connector_bindings : []
  );
}

function cloneJsonValue<T>(value: T): T {
  if (value === undefined) return value;
  return JSON.parse(JSON.stringify(value));
}

function deriveConnectorBindingResolutionFromPlanPackage(
  planPackage: any | null,
  connectorBindings: Array<Record<string, any>>
) {
  const intents = Array.isArray(planPackage?.connector_intents)
    ? planPackage.connector_intents
    : [];
  const entriesByCapability = new Map<string, Record<string, any>>();

  for (const intent of intents) {
    const capability = String(intent?.capability || "").trim();
    if (!capability) continue;
    entriesByCapability.set(capability, {
      capability,
      why: String(intent?.why || "").trim() || null,
      required: intent?.required === true,
      degraded_mode_allowed: intent?.degraded_mode_allowed === true,
      resolved: false,
      status: intent?.required === true ? "unresolved_required" : "unresolved_optional",
      binding_type: null,
      binding_id: null,
      allowlist_pattern: null,
    });
  }

  for (const binding of connectorBindings) {
    const capability = String(binding?.capability || "").trim();
    if (!capability) continue;
    const resolved =
      String(binding?.status || "")
        .trim()
        .toLowerCase() === "mapped";
    const entry = entriesByCapability.get(capability) || {
      capability,
      why: null,
      required: false,
      degraded_mode_allowed: false,
      resolved: false,
      status: "unresolved_optional",
      binding_type: null,
      binding_id: null,
      allowlist_pattern: null,
    };
    entry.binding_type = String(binding?.binding_type || "").trim() || null;
    entry.binding_id = String(binding?.binding_id || "").trim() || null;
    entry.allowlist_pattern = String(binding?.allowlist_pattern || "").trim() || null;
    entry.resolved = resolved;
    entry.status = resolved
      ? "mapped"
      : entry.required
        ? "unresolved_required"
        : "unresolved_optional";
    entriesByCapability.set(capability, entry);
  }

  const entries = Array.from(entriesByCapability.values()).sort((left, right) =>
    left.capability.localeCompare(right.capability)
  );
  const mappedCount = entries.filter((entry) => entry.resolved).length;
  const unresolvedRequiredCount = entries.filter(
    (entry) => !entry.resolved && entry.required
  ).length;
  const unresolvedOptionalCount = entries.filter(
    (entry) => !entry.resolved && !entry.required
  ).length;

  return {
    mapped_count: mappedCount,
    unresolved_required_count: unresolvedRequiredCount,
    unresolved_optional_count: unresolvedOptionalCount,
    entries,
  };
}

function parseConnectorBindingsJson(text: string) {
  const raw = String(text || "").trim();
  if (!raw) return [];
  const parsed = JSON.parse(raw);
  if (!Array.isArray(parsed)) {
    throw new Error("Connector bindings must be a JSON array.");
  }
  const seen = new Set<string>();
  return parsed.map((binding: any, index: number) => {
    if (!binding || typeof binding !== "object") {
      throw new Error(`Connector binding ${index + 1} must be an object.`);
    }
    const capability = String(binding.capability || "").trim();
    if (!capability) {
      throw new Error(`Connector binding ${index + 1} is missing a capability.`);
    }
    if (seen.has(capability)) {
      throw new Error(`Connector binding capability \`${capability}\` is declared more than once.`);
    }
    seen.add(capability);
    const bindingType = String(binding.binding_type || binding.bindingType || "").trim();
    const bindingId = String(binding.binding_id || binding.bindingId || "").trim();
    const allowlistPattern = String(
      binding.allowlist_pattern || binding.allowlistPattern || ""
    ).trim();
    const status = String(binding.status || "")
      .trim()
      .toLowerCase();
    if (!status) {
      throw new Error(
        `Connector binding \`${capability}\` must include an explicit status of mapped, unresolved_required, or unresolved_optional.`
      );
    }
    const normalizedStatus =
      status === "mapped" || status === "unresolved_required" || status === "unresolved_optional"
        ? status
        : null;

    if (!normalizedStatus) {
      throw new Error(
        `Connector binding \`${capability}\` has unsupported status \`${status}\`. Use mapped, unresolved_required, or unresolved_optional.`
      );
    }

    if (normalizedStatus === "mapped" && (!bindingType || !bindingId)) {
      throw new Error(
        `Connector binding \`${capability}\` must include binding_type and binding_id when status is mapped.`
      );
    }

    return {
      capability,
      binding_type: bindingType,
      binding_id: bindingId,
      allowlist_pattern: allowlistPattern || null,
      status: normalizedStatus,
    };
  });
}

function workflowAutomationToEditDraft(automation: any): WorkflowEditDraft | null {
  const automationId = String(
    automation?.automation_id || automation?.automationId || automation?.id || ""
  ).trim();
  if (!automationId) return null;
  const scheduleEditor = scheduleToEditor(automation?.schedule);
  const prefs = extractAutomationOperatorPreferences(automation);
  const plannerRoleModel = prefs?.role_models?.planner || prefs?.roleModels?.planner || {};
  const maxParallelRaw = Number(
    prefs?.max_parallel_agents ??
      prefs?.maxParallelAgents ??
      automation?.execution?.max_parallel_agents ??
      automation?.execution?.maxParallelAgents ??
      1
  );
  const executionMode = String(prefs?.execution_mode || prefs?.executionMode || "").trim();
  const selectedMcpServers = Array.isArray(automation?.metadata?.allowed_mcp_servers)
    ? automation.metadata.allowed_mcp_servers
    : Array.isArray(automation?.agents?.[0]?.mcp_policy?.allowed_servers)
      ? automation.agents[0].mcp_policy.allowed_servers
      : [];
  const toolAccess = workflowToolAccessFromAutomation(automation);
  const workflowModelProvider = String(prefs?.model_provider || prefs?.modelProvider || "").trim();
  const workflowModelId = String(prefs?.model_id || prefs?.modelId || "").trim();
  const agentsById = new Map<string, any>(
    Array.isArray(automation?.agents)
      ? automation.agents.map((agent: any) => [
          String(agent?.agent_id || agent?.agentId || "").trim(),
          agent,
        ])
      : []
  );
  const nodes = Array.isArray(automation?.flow?.nodes)
    ? automation.flow.nodes.map((node: any, index: number) => ({
        nodeId: String(node?.node_id || node?.nodeId || node?.id || `node-${index}`).trim(),
        title: String(
          node?.title ||
            node?.name ||
            node?.objective ||
            node?.node_id ||
            node?.id ||
            "Workflow step"
        ).trim(),
        objective: String(node?.objective || "").trim(),
        agentId: String(node?.agent_id || node?.agentId || "").trim(),
        ...(() => {
          const agent = agentsById.get(String(node?.agent_id || node?.agentId || "").trim()) as
            | any
            | undefined;
          return workflowNodeModelPolicyDraft(
            agent?.model_policy || agent?.modelPolicy || null,
            workflowModelProvider,
            workflowModelId
          );
        })(),
      }))
    : [];
  const scopeSnapshot =
    automation?.metadata?.plan_package || automation?.metadata?.planPackage || null;
  const planPackageBundle =
    automation?.metadata?.plan_package_bundle || automation?.metadata?.planPackageBundle || null;
  const planPackageReplay =
    automation?.metadata?.plan_package_replay || automation?.metadata?.planPackageReplay || null;
  const scopeValidation =
    automation?.metadata?.plan_package_validation ||
    automation?.metadata?.planPackageValidation ||
    null;
  const runtimeContext =
    automation?.metadata?.context_materialization || automation?.runtime_context || null;
  const approvedPlanMaterialization =
    automation?.metadata?.approved_plan_materialization ||
    automation?.metadata?.approvedPlanMaterialization ||
    null;
  const connectorBindingsJson = connectorBindingsJsonFromPlanPackage(scopeSnapshot);
  return {
    automationId,
    name: String(automation?.name || automationId).trim(),
    description: String(automation?.description || "").trim(),
    scheduleKind: scheduleEditor.scheduleKind,
    cronExpression: scheduleEditor.cronExpression,
    intervalSeconds: String(scheduleEditor.intervalSeconds),
    workspaceRoot: String(
      automation?.workspace_root ||
        automation?.workspaceRoot ||
        automation?.metadata?.workspace_root ||
        ""
    ).trim(),
    executionMode:
      executionMode === "single" || executionMode === "swarm" || executionMode === "team"
        ? (executionMode as ExecutionMode)
        : maxParallelRaw > 1
          ? "swarm"
          : "team",
    maxParallelAgents: String(
      Number.isFinite(maxParallelRaw) && maxParallelRaw > 0 ? Math.round(maxParallelRaw) : 1
    ),
    modelProvider: workflowModelProvider,
    modelId: workflowModelId,
    plannerModelProvider: String(
      plannerRoleModel?.provider_id || plannerRoleModel?.providerId || ""
    ).trim(),
    plannerModelId: String(plannerRoleModel?.model_id || plannerRoleModel?.modelId || "").trim(),
    toolAccessMode: toolAccess.toolAccessMode,
    customToolsText: toolAccess.customToolsText,
    selectedMcpServers: selectedMcpServers
      .map((row: any) => String(row || "").trim())
      .filter(Boolean),
    nodes,
    scopeSnapshot,
    planPackageBundle,
    planPackageReplay,
    scopeValidation,
    runtimeContext,
    approvedPlanMaterialization,
    connectorBindingsJson,
  };
}

function isMissionBlueprintAutomation(automation: any) {
  const metadata =
    automation?.metadata && typeof automation.metadata === "object" ? automation.metadata : {};
  const builderKind = String(
    metadata.builder_kind || metadata.builderKind || automation?.builder_kind || ""
  )
    .trim()
    .toLowerCase();
  if (builderKind === "mission_blueprint") return true;
  return !!(
    metadata.mission_blueprint ||
    metadata.missionBlueprint ||
    metadata.mission_blueprint_v1 ||
    metadata.mission
  );
}

function workflowEditToSchedule(draft: WorkflowEditDraft) {
  const misfirePolicy = { type: "run_once" as const };
  if (draft.scheduleKind === "manual") {
    return {
      type: "manual",
      timezone: "UTC",
      misfire_policy: misfirePolicy,
    };
  }
  if (draft.scheduleKind === "cron") {
    return {
      type: "cron",
      cron_expression: String(draft.cronExpression || "").trim(),
      timezone: "UTC",
      misfire_policy: misfirePolicy,
    };
  }
  return {
    type: "interval",
    interval_seconds: Math.max(
      1,
      Number.parseInt(String(draft.intervalSeconds || "3600"), 10) || 3600
    ),
    timezone: "UTC",
    misfire_policy: misfirePolicy,
  };
}

const CALENDAR_DISPLAY_DURATION_MS = 30 * 60 * 1000;
const CALENDAR_SLOT_MS = 60 * 1000;

function getAutomationScheduleTimezone(automation: any) {
  return (
    String(
      automation?.schedule?.timezone ||
        automation?.timezone ||
        automation?.schedule?.timeZone ||
        "UTC"
    ).trim() || "UTC"
  );
}

function getAutomationCronExpression(schedule: any) {
  return String(
    schedule?.cron?.expression ||
      schedule?.cron_expression ||
      schedule?.cronExpression ||
      schedule?.cron ||
      ""
  ).trim();
}

function splitCronField(field: string) {
  return String(field || "")
    .trim()
    .split(",")
    .map((value) => value.trim())
    .filter(Boolean);
}

function matchesCronAtom(atom: string, value: number, min: number, max: number) {
  const trimmed = String(atom || "").trim();
  if (!trimmed || trimmed === "*") return true;
  const stepParts = trimmed.split("/");
  const base = stepParts[0] || "*";
  const step = stepParts[1] ? Number.parseInt(stepParts[1], 10) : 1;
  const normalizedStep = Number.isFinite(step) && step > 0 ? step : 1;
  const rangeParts = base.split("-");
  let start = min;
  let end = max;
  if (base !== "*") {
    if (rangeParts.length === 2) {
      start = Number.parseInt(rangeParts[0], 10);
      end = Number.parseInt(rangeParts[1], 10);
    } else {
      start = Number.parseInt(base, 10);
      end = start;
    }
  }
  if (!Number.isFinite(start) || !Number.isFinite(end)) return false;
  const clampedStart = Math.max(min, Math.min(max, start));
  const clampedEnd = Math.max(min, Math.min(max, end));
  if (value < clampedStart || value > clampedEnd) return false;
  return (value - clampedStart) % normalizedStep === 0;
}

function matchesCronField(field: string, value: number, min: number, max: number) {
  const atoms = splitCronField(field);
  if (!atoms.length) return true;
  return atoms.some((atom) => matchesCronAtom(atom, value, min, max));
}

function cronMatchesUtc(date: Date, expression: string) {
  const fields = String(expression || "")
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  if (fields.length !== 5) return false;
  const [minuteField, hourField, domField, monthField, dowField] = fields;
  const minute = date.getUTCMinutes();
  const hour = date.getUTCHours();
  const dom = date.getUTCDate();
  const month = date.getUTCMonth() + 1;
  const dow = date.getUTCDay();
  const minuteMatch = matchesCronField(minuteField, minute, 0, 59);
  const hourMatch = matchesCronField(hourField, hour, 0, 23);
  const monthMatch = matchesCronField(monthField, month, 1, 12);
  const domWildcard = !domField || domField === "*";
  const dowWildcard = !dowField || dowField === "*";
  const domMatch = domWildcard || matchesCronField(domField, dom, 1, 31);
  const dowMatch = dowWildcard || matchesCronField(dowField, dow === 0 ? 7 : dow, 0, 7);
  const dayMatch = domWildcard || dowWildcard ? domMatch && dowMatch : domMatch || dowMatch;
  return minuteMatch && hourMatch && monthMatch && dayMatch;
}

function expandCronOccurrences(expression: string, rangeStartMs: number, rangeEndMs: number) {
  const out: number[] = [];
  const start = Math.max(0, Math.min(rangeStartMs, rangeEndMs));
  const end = Math.max(rangeStartMs, rangeEndMs);
  const cursor = new Date(Math.floor(start / CALENDAR_SLOT_MS) * CALENDAR_SLOT_MS);
  while (cursor.getTime() < end) {
    if (cronMatchesUtc(cursor, expression)) {
      out.push(cursor.getTime());
    }
    cursor.setUTCMinutes(cursor.getUTCMinutes() + 1);
  }
  return out;
}

function isCalendarEditableCron(expression: string) {
  const fields = String(expression || "")
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  if (fields.length !== 5) return false;
  const [minuteField, hourField, domField, monthField, dowField] = fields;
  const minuteOk = /^\d+$/.test(minuteField);
  const hourOk = /^\d+$/.test(hourField);
  const domOk = domField === "*";
  const monthOk = monthField === "*";
  const dowOk = dowField === "*" || /^[0-7]$/.test(dowField);
  return minuteOk && hourOk && domOk && monthOk && dowOk;
}

function rewriteCronForDroppedStart(expression: string, start: Date) {
  const fields = String(expression || "")
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  if (fields.length !== 5) return null;
  const [minuteField, hourField, domField, monthField, dowField] = fields;
  if (domField !== "*" || monthField !== "*") return null;
  if (!/^\d+$/.test(minuteField) || !/^\d+$/.test(hourField)) return null;
  const minute = String(start.getUTCMinutes()).padStart(2, "0");
  const hour = String(start.getUTCHours());
  const weekday = String(start.getUTCDay());
  const nextDow = weekday === "0" ? "0" : weekday;
  const nextDowField = dowField === "*" ? "*" : nextDow;
  return `${minute} ${hour} ${domField} ${monthField} ${nextDowField}`;
}

function getAutomationCalendarTitle(automation: any) {
  return String(
    automation?.name ||
      automation?.mission?.objective ||
      automation?.description ||
      automation?.automation_id ||
      automation?.automationId ||
      "Automation"
  ).trim();
}

function getAutomationCalendarFamily(automation: any) {
  const automationId = String(
    automation?.automation_id || automation?.automationId || automation?.id || ""
  ).trim();
  return automationId.startsWith("automation-v2-") ? "v2" : "legacy";
}

function getAutomationCalendarScheduleStatus(automation: any) {
  return String(automation?.status || "active").trim() || "active";
}

function buildCalendarOccurrences({
  automation,
  family,
  rangeStartMs,
  rangeEndMs,
}: {
  automation: any;
  family: "legacy" | "v2";
  rangeStartMs: number;
  rangeEndMs: number;
}) {
  const automationId = String(
    automation?.automation_id || automation?.automationId || automation?.id || ""
  ).trim();
  if (!automationId) return [];
  const schedule = automation?.schedule || {};
  const scheduleType = String(schedule?.type || "")
    .trim()
    .toLowerCase();
  const cronExpression = getAutomationCronExpression(schedule);
  if (scheduleType !== "cron" || !cronExpression) return [];
  const title = getAutomationCalendarTitle(automation);
  const scheduleLabel =
    family === "legacy" ? formatScheduleLabel(schedule) : formatAutomationV2ScheduleLabel(schedule);
  const status = getAutomationCalendarScheduleStatus(automation);
  const timezone = getAutomationScheduleTimezone(automation);
  const lifecycleState = String(
    automation?.metadata?.approved_plan_materialization?.lifecycle_state ||
      automation?.metadata?.approvedPlanMaterialization?.lifecycleState ||
      automation?.metadata?.plan_package?.lifecycle_state ||
      automation?.metadata?.planPackage?.lifecycleState ||
      automation?.lifecycle_state ||
      automation?.lifecycleState ||
      ""
  )
    .trim()
    .toLowerCase();
  const approvalRequired =
    automation?.requires_approval === true ||
    automation?.policy?.approval?.requires_approval === true;
  const activationReady = automation?.metadata?.plan_package_validation?.ready_for_activation;
  const editable = isCalendarEditableCron(cronExpression);
  const starts = expandCronOccurrences(cronExpression, rangeStartMs, rangeEndMs);
  return starts.map((startMs) => ({
    id: `${automationId}:${startMs}`,
    title,
    start: new Date(startMs),
    end: new Date(startMs + CALENDAR_DISPLAY_DURATION_MS),
    allDay: false,
    editable,
    startEditable: editable,
    durationEditable: false,
    extendedProps: {
      automation,
      automationId,
      family,
      scheduleLabel,
      scheduleType,
      cronExpression,
      status,
      timezone,
      lifecycleState,
      approvalRequired,
      approvalState: approvalRequired ? "approval required" : "approval optional",
      activationReady,
    },
  }));
}

function isStandupAutomation(automation: any) {
  return String(automation?.metadata?.feature || "").trim() === "agent_standup";
}

function looksLikeEmailNode(node: WorkflowNodeEditDraft) {
  const haystack = `${node.title} ${node.objective} ${node.agentId}`.toLowerCase();
  return (
    haystack.includes("email") ||
    haystack.includes("gmail") ||
    haystack.includes("draft") ||
    haystack.includes("send")
  );
}

function appendNoAttachmentGuardrail(objective: string) {
  const rule =
    "Do not send or preserve attachments. If an attachment field is empty, null, or missing a valid upload or s3key, omit the attachment parameter entirely and send the email body inline instead.";
  const trimmed = String(objective || "").trim();
  if (trimmed.toLowerCase().includes("omit the attachment parameter entirely")) return trimmed;
  return trimmed ? `${trimmed}\n\n${rule}` : rule;
}

function workflowEditToOperatorPreferences(draft: WorkflowEditDraft) {
  const prefs: Record<string, any> = {
    execution_mode: draft.executionMode,
    max_parallel_agents:
      draft.executionMode === "swarm"
        ? Math.max(
            1,
            Math.min(16, Number.parseInt(String(draft.maxParallelAgents || "4"), 10) || 4)
          )
        : 1,
  };
  const modelProvider = String(draft.modelProvider || "").trim();
  const modelId = String(draft.modelId || "").trim();
  if (modelProvider) prefs.model_provider = modelProvider;
  if (modelId) prefs.model_id = modelId;
  const plannerProvider = String(draft.plannerModelProvider || "").trim();
  const plannerModel = String(draft.plannerModelId || "").trim();
  if (plannerProvider && plannerModel) {
    prefs.role_models = {
      planner: {
        provider_id: plannerProvider,
        model_id: plannerModel,
      },
    };
  }
  prefs.tool_access_mode = draft.toolAccessMode;
  if (draft.toolAccessMode === "custom") {
    prefs.tool_allowlist = parseCustomToolText(draft.customToolsText);
  }
  return prefs;
}

function compileWorkflowModelPolicy(operatorPreferences: Record<string, any>) {
  const payload: Record<string, any> = {};
  if (operatorPreferences.model_provider && operatorPreferences.model_id) {
    payload.default_model = {
      provider_id: operatorPreferences.model_provider,
      model_id: operatorPreferences.model_id,
    };
  }
  if (operatorPreferences.role_models && typeof operatorPreferences.role_models === "object") {
    payload.role_models = operatorPreferences.role_models;
  }
  return Object.keys(payload).length ? payload : null;
}

function workflowNodeModelPolicyDraft(
  agentModelPolicy: any | null,
  workflowModelProvider: string,
  workflowModelId: string
) {
  const defaultModel = agentModelPolicy?.default_model || agentModelPolicy?.defaultModel || null;
  const provider = String(defaultModel?.provider_id || defaultModel?.providerId || "").trim();
  const model = String(defaultModel?.model_id || defaultModel?.modelId || "").trim();
  if (
    provider &&
    model &&
    provider === String(workflowModelProvider || "").trim() &&
    model === String(workflowModelId || "").trim()
  ) {
    return {
      modelProvider: "",
      modelId: "",
    };
  }
  return {
    modelProvider: provider,
    modelId: model,
  };
}

function workflowNodeModelPolicyWithOverride(
  baseModelPolicy: Record<string, any> | null,
  provider: string,
  model: string
) {
  const providerValue = String(provider || "").trim();
  const modelValue = String(model || "").trim();
  if (!providerValue && !modelValue) {
    return baseModelPolicy ? cloneJsonValue(baseModelPolicy) : null;
  }
  const nextPolicy = baseModelPolicy
    ? (cloneJsonValue(baseModelPolicy) as Record<string, any>)
    : {};
  nextPolicy.default_model = {
    provider_id: providerValue,
    model_id: modelValue,
  };
  return nextPolicy;
}

function isActiveRunStatus(status: string) {
  const normalized = String(status || "")
    .trim()
    .toLowerCase();
  return [
    "queued",
    "running",
    "in_progress",
    "executing",
    "pending_approval",
    "awaiting_approval",
  ].includes(normalized);
}

function workflowQueueReason(run: any) {
  return String(
    run?.scheduler?.queue_reason || run?.scheduler?.queueReason || run?.scheduler?.reason || ""
  )
    .trim()
    .toLowerCase();
}

function workflowQueueResourceKey(run: any) {
  return String(run?.scheduler?.resource_key || run?.scheduler?.resourceKey || "").trim();
}

function workflowStatusDisplay(run: any) {
  const status = workflowDerivedRunStatus(run);
  if (status !== "queued") return status || "unknown";
  const reason = workflowQueueReason(run);
  if (reason === "workspace_lock") return "queued (workspace lock)";
  if (reason === "capacity") return "queued (capacity)";
  if (reason === "rate_limit") {
    const provider = String(
      run?.scheduler?.rate_limited_provider || run?.scheduler?.rateLimitedProvider || ""
    ).trim();
    return provider ? `queued (rate limit: ${provider})` : "queued (rate limit)";
  }
  return "queued";
}

function workflowStatusSubtleDetail(run: any) {
  const reason = workflowQueueReason(run);
  if (!reason) return "";
  if (reason === "workspace_lock") {
    const resourceKey = workflowQueueResourceKey(run);
    return resourceKey
      ? `Waiting for workspace lock: ${resourceKey}`
      : "Waiting for workspace lock";
  }
  if (reason === "capacity") return "Waiting for scheduler capacity";
  if (reason === "rate_limit") {
    const provider = String(
      run?.scheduler?.rate_limited_provider || run?.scheduler?.rateLimitedProvider || ""
    ).trim();
    return provider
      ? `Waiting for provider rate limit: ${provider}`
      : "Waiting for provider rate limit";
  }
  return "";
}

function runTimeLabel(run: any) {
  const started = Number(run?.started_at_ms || run?.fired_at_ms || run?.created_at_ms || 0);
  if (!Number.isFinite(started) || started <= 0) return "time unavailable";
  const deltaMs = Date.now() - started;
  const seconds = Math.max(0, Math.floor(deltaMs / 1000));
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  return `${Math.floor(seconds / 3600)}h`;
}

function deriveRunDebugHints(run: any, artifacts: any[]) {
  const hints: string[] = [];
  const status = workflowDerivedRunStatus(run);
  if (status === "pending_approval" || status === "awaiting_approval") {
    hints.push("Run is waiting for approval before external actions.");
  }
  if (status === "blocked_policy") {
    hints.push("Run was blocked by policy. Check tool allowlist and integration permissions.");
  }
  if (status === "failed" || status === "error") {
    hints.push("Run failed. Inspect detail/error fields for root cause.");
  }
  if ((status === "completed" || status === "done") && !artifacts.length) {
    hints.push("Run completed but produced no artifacts. Verify output target and tool actions.");
  }
  if (run?.requires_approval === true) {
    hints.push("Automation policy requires human approval. Disable it for fully automated runs.");
  }
  return hints;
}

function normalizeTimestamp(raw: any) {
  const value = Number(raw || 0);
  if (!Number.isFinite(value) || value <= 0) return Date.now();
  return value < 1_000_000_000_000 ? value * 1000 : value;
}

function timestampOrNull(raw: any) {
  const value = Number(raw || 0);
  if (!Number.isFinite(value) || value <= 0) return null;
  return value < 1_000_000_000_000 ? value * 1000 : value;
}

function formatTimestampLabel(raw: any) {
  const value = timestampOrNull(raw);
  return value ? new Date(value).toLocaleTimeString() : "time unavailable";
}

function compactIdentifier(raw: any, max = 28) {
  const value = String(raw || "").trim();
  if (!value) return "";
  if (value.length <= max) return value;
  const head = Math.max(8, Math.floor((max - 1) / 2));
  const tail = Math.max(6, max - head - 1);
  return `${value.slice(0, head)}…${value.slice(-tail)}`;
}

function shortText(raw: any, max = 88) {
  const text = String(raw || "")
    .replace(/\s+/g, " ")
    .trim();
  if (!text) return "";
  return text.length > max ? `${text.slice(0, max - 1).trimEnd()}…` : text;
}

function runObjectiveText(run: any) {
  return String(
    run?.mission_snapshot?.objective || run?.mission?.objective || run?.objective || run?.name || ""
  )
    .replace(/\s+/g, " ")
    .trim();
}

function runDisplayTitle(run: any) {
  const explicitName = String(run?.name || "").trim();
  if (explicitName) return explicitName;
  const objective = runObjectiveText(run);
  if (objective) return shortText(objective, 96);
  const automationId = String(run?.automation_id || run?.routine_id || "").trim();
  if (automationId) return automationId;
  return "Run";
}

function formatRunDateTime(raw: any) {
  const value = Number(raw || 0);
  if (!Number.isFinite(value) || value <= 0) return "";
  return new Date(normalizeTimestamp(value)).toLocaleString();
}

function uniqueStrings(values: Array<any>) {
  const seen = new Set<string>();
  const rows: string[] = [];
  for (const value of values) {
    const text = String(value || "").trim();
    if (!text || seen.has(text)) continue;
    seen.add(text);
    rows.push(text);
  }
  return rows;
}

function looksLikePath(text: string) {
  const value = String(text || "").trim();
  if (!value) return false;
  if (value.includes("/") || value.includes("\\")) return true;
  return /\.[a-z0-9]{1,8}$/i.test(value);
}

function collectPathStrings(value: any, keyHint = "", depth = 0): string[] {
  if (depth > 4 || value == null) return [];
  if (typeof value === "string") {
    const text = value.trim();
    if (!text) return [];
    if (/(path|file|artifact)/i.test(keyHint) || looksLikePath(text)) return [text];
    return [];
  }
  if (Array.isArray(value)) {
    return value.flatMap((item) => collectPathStrings(item, keyHint, depth + 1));
  }
  if (typeof value === "object") {
    return Object.entries(value).flatMap(([key, entry]) =>
      collectPathStrings(entry, key, depth + 1)
    );
  }
  return [];
}

function sessionMessageText(message: any) {
  const parts = Array.isArray(message?.parts) ? message.parts : [];
  const rows = parts
    .map((part: any) => {
      const type = String(part?.type || "").trim();
      if (type === "text" || type === "reasoning") return String(part?.text || "").trim();
      if (type === "tool") {
        const tool = String(part?.tool || "tool").trim();
        const error = String(part?.error || "").trim();
        const result = part?.result ? formatJson(part.result) : "";
        return [`tool: ${tool}`, error ? `error: ${error}` : "", result].filter(Boolean).join("\n");
      }
      return String(part?.text || "").trim();
    })
    .filter(Boolean);
  return rows.join("\n\n").trim();
}

function sessionMessageVariant(message: any) {
  const role = String(message?.info?.role || "")
    .trim()
    .toLowerCase();
  if (role === "user") return "user";
  if (role === "assistant") return "assistant";
  const body = sessionMessageText(message).toLowerCase();
  if (body.includes("engine_error") || body.includes("error")) return "error";
  return "system";
}

function sessionMessageParts(message: any) {
  return Array.isArray(message?.parts) ? message.parts : [];
}

function sessionMessageCreatedAt(message: any) {
  return normalizeTimestamp(
    message?.info?.time?.created || message?.info?.created_at_ms || message?.created_at_ms || 0
  );
}

function sessionMessageId(message: any, index: number) {
  return (
    String(message?.info?.id || message?.id || `message-${index}`).trim() || `message-${index}`
  );
}

function sessionLabel(sessionId: string) {
  const value = String(sessionId || "").trim();
  return value ? `session ${compactIdentifier(value, 18)}` : "session";
}

function normalizeWorkflowTaskId(raw: string) {
  const value = String(raw || "").trim();
  if (!value) return "";
  return value.startsWith("node-") ? value : `node-${value}`;
}

function workflowNodeIdFromText(raw: string) {
  const text = String(raw || "").trim();
  if (!text) return "";
  for (const pattern of [
    /node id:\s*([a-z0-9._-]+)/i,
    /step[_\s]id:\s*([a-z0-9._-]+)/i,
    /task[_\s]id:\s*(?:node-)?([a-z0-9._-]+)/i,
  ]) {
    const match = text.match(pattern);
    if (match?.[1]) return normalizeWorkflowTaskId(match[1]);
  }
  return "";
}

function workflowDescendantTaskIds(tasks: any[], rootTaskId: string) {
  const root = String(rootTaskId || "").trim();
  if (!root) return [];
  const descendants = new Set<string>([root]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const task of Array.isArray(tasks) ? tasks : []) {
      const taskId = String(task?.id || "").trim();
      if (!taskId || descendants.has(taskId)) continue;
      const deps = Array.isArray(task?.dependencies)
        ? task.dependencies.map((dep: any) => String(dep || "").trim()).filter(Boolean)
        : [];
      if (deps.some((dep) => descendants.has(dep))) {
        descendants.add(taskId);
        changed = true;
      }
    }
  }
  return Array.from(descendants);
}

function detectWorkflowActiveTaskId(
  run: any,
  sessionMessages: Array<{ sessionId: string; message: any }>,
  sessionEvents: Array<{ id: string; at: number; event: any }>
) {
  const status = String(run?.status || "")
    .trim()
    .toLowerCase();
  if (!["running", "pausing", "paused"].includes(status)) return "";
  const lifecycleTaskId = workflowLatestLifecycleTaskId(run);
  if (lifecycleTaskId) return lifecycleTaskId;
  for (let i = sessionEvents.length - 1; i >= 0; i -= 1) {
    const payload = sessionEvents[i]?.event?.properties || sessionEvents[i]?.event || {};
    const explicit = normalizeWorkflowTaskId(
      String(payload?.task_id || payload?.step_id || payload?.node_id || "").trim()
    );
    if (explicit) return explicit;
    const fromText = workflowNodeIdFromText(
      String(payload?.message || payload?.detail || payload?.reason || "")
    );
    if (fromText) return fromText;
  }
  for (let i = sessionMessages.length - 1; i >= 0; i -= 1) {
    const fromText = workflowNodeIdFromText(sessionMessageText(sessionMessages[i]?.message));
    if (fromText) return fromText;
  }
  return workflowFirstPendingTaskId(run);
}

function explainRunFailure(run: any) {
  const detail = String(run?.detail || "").trim();
  if (!detail) return "";
  if (detail.includes("BASH_COMMAND_MISSING")) {
    return "This workflow failed because the agent called the `bash` tool without providing a shell command. The tool was available, but the request payload was missing its required `command` field.";
  }
  if (detail.includes("WEBFETCH_URL_MISSING")) {
    return "This workflow failed because a web fetch tool call was made without a URL.";
  }
  if (detail.includes("No such file or directory")) {
    return "This workflow failed because the agent tried to read a path that does not exist from the configured workspace root.";
  }
  return detail;
}

function buildRunBlockers(run: any, sessionEvents: any[], runEvents: any[]) {
  const blockers: Array<{
    key: string;
    title: string;
    reason: string;
    source: string;
    at?: number;
  }> = [];
  const push = (key: string, title: string, reason: string, source: string, at?: number) => {
    if (!reason.trim()) return;
    if (blockers.some((row) => row.key === key)) return;
    blockers.push({ key, title, reason, source, at });
  };

  if (run?.requires_approval === true || String(run?.status || "").trim() === "pending_approval") {
    push(
      "approval-required",
      "Approval required",
      String(
        run?.approval_reason || "Manual approval required before external side effects."
      ).trim(),
      "policy"
    );
  }
  if (String(run?.denial_reason || "").trim()) {
    push("denied", "Run denied", String(run.denial_reason).trim(), "run");
  }
  if (String(run?.paused_reason || "").trim()) {
    push("paused", "Run paused", String(run.paused_reason).trim(), "run");
  }
  if (String(run?.detail || "").trim()) {
    const detail = String(run.detail).trim();
    if (
      detail.toLowerCase().includes("tool") ||
      detail.toLowerCase().includes("bash_command_missing") ||
      detail.toLowerCase().includes("command_missing") ||
      detail.toLowerCase().includes("permission") ||
      detail.toLowerCase().includes("approval") ||
      detail.toLowerCase().includes("mcp") ||
      detail.toLowerCase().includes("auth") ||
      detail.toLowerCase().includes("failed after")
    ) {
      push("detail", "Failure reason", explainRunFailure(run), "run");
    }
  }
  if (!workflowSessionIds(run).length) {
    push(
      "missing-session",
      "No linked session transcript",
      "This run does not expose a linked session transcript, so only telemetry/history are available.",
      "run"
    );
  }
  for (const output of workflowNodeOutputEntries(run)) {
    const body = workflowNodeOutputText(output.value);
    const telemetry = workflowNodeToolTelemetry(output.value);
    const artifactValidation = workflowArtifactValidation(output.value);
    if (
      String(output?.value?.status || "")
        .trim()
        .toLowerCase() === "blocked"
    ) {
      const executed = Array.isArray(telemetry?.executed_tools)
        ? telemetry.executed_tools.join(", ")
        : "";
      const requested = Array.isArray(telemetry?.requested_tools)
        ? telemetry.requested_tools.join(", ")
        : "";
      push(
        `node-status-${output.nodeId}`,
        `Node blocked: ${output.nodeId}`,
        [
          String(output?.value?.blocked_reason || output?.value?.blockedReason || "").trim(),
          String(output?.value?.blocker_category || output?.value?.blockerCategory || "").trim()
            ? `blocker category: ${String(
                output?.value?.blocker_category || output?.value?.blockerCategory || ""
              ).trim()}`
            : "",
          requested ? `offered tools: ${requested}` : "",
          executed ? `executed tools: ${executed}` : "",
          String(
            output?.value?.preflight?.budget_status || output?.value?.preflight?.budgetStatus || ""
          )
            ? `preflight budget: ${String(
                output?.value?.preflight?.budget_status ||
                  output?.value?.preflight?.budgetStatus ||
                  ""
              ).trim()}`
            : "",
          Array.isArray(output?.value?.capability_resolution?.missing_capabilities) &&
          output.value.capability_resolution.missing_capabilities.length
            ? `missing capabilities: ${output.value.capability_resolution.missing_capabilities.join(", ")}`
            : "",
          output?.value?.capability_resolution?.email_tool_diagnostics
            ? `selected mcp servers: ${
                Array.isArray(
                  output.value.capability_resolution.email_tool_diagnostics.selected_servers
                ) &&
                output.value.capability_resolution.email_tool_diagnostics.selected_servers.length
                  ? output.value.capability_resolution.email_tool_diagnostics.selected_servers.join(
                      ", "
                    )
                  : "none"
              }`
            : "",
          output?.value?.capability_resolution?.email_tool_diagnostics
            ? `email-like tools discovered: ${
                Array.isArray(
                  output.value.capability_resolution.email_tool_diagnostics.available_tools
                ) &&
                output.value.capability_resolution.email_tool_diagnostics.available_tools.length
                  ? output.value.capability_resolution.email_tool_diagnostics.available_tools.join(
                      ", "
                    )
                  : "none"
              }`
            : "",
          output?.value?.capability_resolution?.email_tool_diagnostics
            ? `email-like tools offered: ${
                Array.isArray(
                  output.value.capability_resolution.email_tool_diagnostics.offered_tools
                ) && output.value.capability_resolution.email_tool_diagnostics.offered_tools.length
                  ? output.value.capability_resolution.email_tool_diagnostics.offered_tools.join(
                      ", "
                    )
                  : "none"
              }`
            : "",
          output?.value?.capability_resolution?.mcp_tool_diagnostics
            ? `mcp remote tools: ${
                Array.isArray(
                  output.value.capability_resolution.mcp_tool_diagnostics.remote_tools
                ) && output.value.capability_resolution.mcp_tool_diagnostics.remote_tools.length
                  ? output.value.capability_resolution.mcp_tool_diagnostics.remote_tools.join(", ")
                  : "none"
              }`
            : "",
          output?.value?.capability_resolution?.mcp_tool_diagnostics
            ? `registered mcp tools: ${
                Array.isArray(
                  output.value.capability_resolution.mcp_tool_diagnostics.registered_tools
                ) && output.value.capability_resolution.mcp_tool_diagnostics.registered_tools.length
                  ? output.value.capability_resolution.mcp_tool_diagnostics.registered_tools.join(
                      ", "
                    )
                  : "none"
              }`
            : "",
          String(artifactValidation?.semantic_block_reason || "").trim()
            ? `research validation: ${String(artifactValidation?.semantic_block_reason || "").trim()}`
            : "",
          String(artifactValidation?.rejected_artifact_reason || "").trim()
            ? `artifact validation: ${String(artifactValidation?.rejected_artifact_reason || "").trim()}`
            : "",
          Array.isArray(artifactValidation?.unmet_requirements) &&
          artifactValidation.unmet_requirements.length
            ? `unmet requirements: ${artifactValidation.unmet_requirements.join(", ")}`
            : "",
          Array.isArray(artifactValidation?.undeclared_files_created) &&
          artifactValidation.undeclared_files_created.length
            ? `undeclared files created: ${artifactValidation.undeclared_files_created.join(", ")}`
            : "",
          artifactValidation?.auto_cleaned ? "artifact cleanup was applied" : "",
          telemetry && !telemetry?.web_research_used ? "web research was not used" : "",
          telemetry && !telemetry?.workspace_inspection_used
            ? "workspace inspection was not used"
            : "",
        ]
          .filter(Boolean)
          .join("\n"),
        output.nodeId
      );
    }
    if (!body) continue;
    const lower = body.toLowerCase();
    if (
      lower.includes("could not complete") ||
      lower.includes("invalid attachment") ||
      lower.includes("timed out") ||
      lower.includes("blocked") ||
      lower.includes("no email delivery tool") ||
      lower.includes("auth was not approved")
    ) {
      push(
        `node-output-${output.nodeId}`,
        `Node issue: ${output.nodeId}`,
        shortText(body, 360),
        output.nodeId,
        Number(output.value?.created_at_ms || output.value?.createdAtMs || 0)
      );
    }
  }

  workflowEventBlockers([...sessionEvents, ...runEvents]).forEach((blocker) => {
    push(blocker.key, blocker.title, blocker.reason, blocker.source, blocker.at);
  });

  return blockers.sort((a, b) => (b.at || 0) - (a.at || 0));
}

// ─── Wizard Steps ───────────────────────────────────────────────────────────

// moved Step1Goal to features/automations/create

// moved Step2Schedule and Step3Mode to features/automations/create

// moved Step4Review to features/automations/create

// ─── My Automations (combined routines + packs) ─────────────────────────────

function MyAutomations({
  client,
  toast,
  navigate,
  viewMode,
  selectedRunId,
  onSelectRunId,
  onOpenRunningView,
  onOpenAdvancedEdit,
}: {
  client: any;
  toast: any;
  navigate: (route: string) => void;
  viewMode: "calendar" | "list" | "running";
  selectedRunId: string;
  onSelectRunId: (runId: string) => void;
  onOpenRunningView: () => void;
  onOpenAdvancedEdit: (automation: any) => void;
}) {
  const queryClient = useQueryClient();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<{
    automationId: string;
    family: "legacy" | "v2";
    title: string;
  } | null>(null);
  const [editDraft, setEditDraft] = useState<{
    automationId: string;
    name: string;
    objective: string;
    mode: "standalone" | "orchestrated";
    requiresApproval: boolean;
    scheduleKind: "cron" | "interval";
    cronExpression: string;
    intervalSeconds: string;
  } | null>(null);
  const [selectedLogSource, setSelectedLogSource] = useState<
    "all" | "automations" | "context" | "global"
  >("all");
  const [runEvents, setRunEvents] = useState<
    Array<{ id: string; source: "automations" | "context" | "global"; at: number; event: any }>
  >([]);
  const [selectedSessionId, setSelectedSessionId] = useState<string>("");
  const [selectedSessionFilterId, setSelectedSessionFilterId] = useState<string>("all");
  const [selectedBoardTaskId, setSelectedBoardTaskId] = useState<string>("");
  const [selectedRunArtifactKey, setSelectedRunArtifactKey] = useState<string>("");
  const [sessionEvents, setSessionEvents] = useState<Array<{ id: string; at: number; event: any }>>(
    []
  );
  const boardDetailRef = useRef<HTMLDivElement | null>(null);
  const artifactsSectionRef = useRef<HTMLDivElement | null>(null);
  const sessionLogRef = useRef<HTMLDivElement | null>(null);
  const [sessionLogPinnedToBottom, setSessionLogPinnedToBottom] = useState(false);
  const [workflowEditDraft, setWorkflowEditDraft] = useState<WorkflowEditDraft | null>(null);
  const [calendarRange, setCalendarRange] = useState(() => {
    const now = new Date();
    const utcDay = now.getUTCDay();
    const start = new Date(
      Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() - utcDay, 0, 0, 0, 0)
    );
    return {
      startMs: start.getTime(),
      endMs: start.getTime() + 7 * 24 * 60 * 60 * 1000,
    };
  });
  const isWorkflowRun = selectedRunId.startsWith("automation-v2-run-");

  const automationsQuery = useQuery({
    queryKey: ["automations", "list"],
    queryFn: () =>
      client?.automations?.list?.().catch(() => ({ automations: [] })) ??
      Promise.resolve({ automations: [] }),
    refetchInterval: 20000,
  });
  const automationsV2Query = useQuery({
    queryKey: ["automations", "v2", "list"],
    queryFn: () =>
      client?.automationsV2?.list?.().catch(() => ({ automations: [] })) ??
      Promise.resolve({ automations: [] }),
    refetchInterval: 20000,
  });
  const automationsV2 = useMemo(() => {
    const rows = toArray(automationsV2Query.data, "automations");
    const byId = new Map<string, any>();
    for (const row of rows) {
      const id = String(row?.automation_id || row?.automationId || row?.id || "").trim();
      if (!id) continue;
      if (!byId.has(id)) byId.set(id, row);
    }
    return Array.from(byId.values());
  }, [automationsV2Query.data]);
  const overlapHistoryEntries = useMemo(() => {
    const rows: Array<Record<string, any>> = [];
    for (const automation of automationsV2) {
      const automationId = String(
        automation?.automation_id || automation?.automationId || automation?.id || ""
      ).trim();
      const automationName = String(automation?.name || "").trim();
      const planPackage = automation?.metadata?.plan_package || automation?.metadata?.planPackage;
      const overlapLog = toArray(planPackage?.overlap_policy, "overlap_log");
      const sourcePlanId = String(
        planPackage?.plan_id || planPackage?.planId || automationId || ""
      ).trim();
      const sourcePlanRevision = Number(
        planPackage?.plan_revision || planPackage?.planRevision || 0
      );
      const sourceLifecycleState = String(
        planPackage?.lifecycle_state || planPackage?.lifecycleState || automation?.status || ""
      ).trim();
      for (const entry of overlapLog) {
        rows.push({
          rowKey: [
            automationId || sourcePlanId || "automation",
            String(entry?.matched_plan_id || entry?.matchedPlanId || ""),
            String(entry?.matched_plan_revision || entry?.matchedPlanRevision || ""),
            String(entry?.decision || ""),
            String(entry?.decided_at || entry?.decidedAt || ""),
          ].join(":"),
          sourceLabel: automationName || automationId || sourcePlanId || "workflow plan",
          sourceAutomationId: automationId,
          sourcePlanId,
          sourcePlanRevision: Number.isFinite(sourcePlanRevision) ? sourcePlanRevision : 0,
          sourceLifecycleState,
          matchedPlanId: String(entry?.matched_plan_id || entry?.matchedPlanId || "").trim(),
          matchedPlanRevision: Number(
            entry?.matched_plan_revision || entry?.matchedPlanRevision || 0
          ),
          matchLayer: String(entry?.match_layer || entry?.matchLayer || "").trim(),
          similarityScore: entry?.similarity_score ?? entry?.similarityScore ?? null,
          decision: String(entry?.decision || "").trim(),
          decidedBy: String(entry?.decided_by || entry?.decidedBy || "").trim(),
          decidedAt: String(entry?.decided_at || entry?.decidedAt || "").trim(),
        });
      }
    }
    return rows.sort((left, right) => {
      const leftAt = Number(Date.parse(String(left.decidedAt || "")));
      const rightAt = Number(Date.parse(String(right.decidedAt || "")));
      if (Number.isFinite(leftAt) && Number.isFinite(rightAt) && leftAt !== rightAt) {
        return rightAt - leftAt;
      }
      return String(left.sourcePlanId || left.sourceAutomationId || left.rowKey).localeCompare(
        String(right.sourcePlanId || right.sourceAutomationId || right.rowKey)
      );
    });
  }, [automationsV2]);
  const providerCatalogQuery = useQuery({
    queryKey: ["providers", "catalog", "workflow-edit"],
    queryFn: () =>
      client?.providers?.catalog?.().catch(() => ({ all: [] })) ?? Promise.resolve({ all: [] }),
    refetchInterval: 30000,
  });
  const providersConfigQuery = useQuery({
    queryKey: ["providers", "config", "workflow-edit"],
    queryFn: () =>
      client?.providers?.config?.().catch(() => ({ providers: {} })) ??
      Promise.resolve({ providers: {} }),
    refetchInterval: 30000,
  });
  const mcpServersQuery = useQuery({
    queryKey: ["mcp", "servers", "workflow-edit"],
    queryFn: () =>
      client?.mcp?.list?.().catch(() => ({ servers: [] })) ?? Promise.resolve({ servers: [] }),
    refetchInterval: 15000,
  });
  const runsQuery = useQuery({
    queryKey: ["automations", "runs"],
    queryFn: () =>
      client?.automations?.listRuns?.({ limit: 20 }).catch(() => ({ runs: [] })) ??
      Promise.resolve({ runs: [] }),
    refetchInterval: 9000,
  });
  const workflowRunsQuery = useQuery({
    queryKey: ["automations", "v2", "runs", "all"],
    queryFn: () =>
      api("/api/engine/automations/v2/runs?limit=40").catch(() => ({ runs: [] as any[] })),
    refetchInterval: 9000,
  });
  const runDetailQuery = useQuery({
    queryKey: ["automations", "run", selectedRunId],
    enabled: !!selectedRunId,
    queryFn: () =>
      (isWorkflowRun
        ? client?.automationsV2?.getRun?.(selectedRunId)
        : client?.automations?.getRun?.(selectedRunId)
      )?.catch(() => ({ run: null })) ?? Promise.resolve({ run: null }),
    refetchInterval: selectedRunId ? 5000 : false,
  });
  const runArtifactsQuery = useQuery({
    queryKey: ["automations", "run", "artifacts", selectedRunId],
    enabled: !!selectedRunId && !isWorkflowRun,
    queryFn: () =>
      client?.automations?.listArtifacts?.(selectedRunId).catch(() => ({ artifacts: [] })),
    refetchInterval: selectedRunId ? 8000 : false,
  });
  const taskResetPreviewQuery = useQuery({
    queryKey: ["automations", "run", "task-reset-preview", selectedRunId, selectedBoardTaskId],
    enabled:
      !!selectedRunId &&
      isWorkflowRun &&
      String(selectedBoardTaskId || "").startsWith("node-") &&
      !!String(selectedBoardTaskId || "").trim() &&
      !!client?.automationsV2?.previewTaskReset,
    queryFn: () =>
      client?.automationsV2
        ?.previewTaskReset(
          selectedRunId,
          String(selectedBoardTaskId || "")
            .replace(/^node-/, "")
            .trim()
        )
        .catch(() => ({ preview: null })) ?? Promise.resolve({ preview: null }),
    refetchInterval: false,
  });
  const availableSessionIds = useMemo(
    () => workflowSessionIds((runDetailQuery.data as any)?.run),
    [runDetailQuery.data]
  );
  const sessionMessageQueries = useQueries({
    queries: availableSessionIds.map((sessionId) => ({
      queryKey: ["automations", "run", "session", selectedRunId, sessionId, "messages"],
      enabled: !!selectedRunId && !!sessionId,
      queryFn: () => client?.sessions?.messages?.(sessionId).catch(() => []) ?? Promise.resolve([]),
      refetchInterval:
        selectedRunId && sessionId && isActiveRunStatus((runDetailQuery.data as any)?.run?.status)
          ? 4000
          : false,
    })),
  });
  const selectedAutomationId = String(
    (runDetailQuery.data as any)?.run?.automation_id ||
      (runDetailQuery.data as any)?.run?.routine_id ||
      ""
  ).trim();
  const selectedContextRunId = String(
    (runDetailQuery.data as any)?.contextRunID ||
      (isWorkflowRun && selectedRunId ? `automation-v2-${selectedRunId}` : "")
  ).trim();
  const runHistoryQuery = useQuery({
    queryKey: ["automations", "history", selectedAutomationId],
    enabled: !!selectedAutomationId && !isWorkflowRun,
    queryFn: () =>
      client?.automations?.history?.(selectedAutomationId, 80).catch(() => ({ events: [] })),
    refetchInterval: selectedRunId ? 10000 : false,
  });
  const persistedRunEventsQuery = useQuery({
    queryKey: ["automations", "run", "events", selectedRunId],
    enabled: !!selectedRunId && !!client?.runEvents,
    queryFn: () => client.runEvents(selectedRunId, { tail: 400 }).catch(() => []),
    refetchInterval:
      selectedRunId && isActiveRunStatus((runDetailQuery.data as any)?.run?.status) ? 5000 : false,
  });
  const workflowContextRunQuery = useQuery({
    queryKey: ["automations", "run", "context", selectedContextRunId],
    enabled: !!selectedContextRunId,
    queryFn: () =>
      api(`/api/engine/context/runs/${encodeURIComponent(selectedContextRunId)}`).catch(() => ({
        run: null,
      })),
    refetchInterval:
      selectedContextRunId && isActiveRunStatus((runDetailQuery.data as any)?.run?.status)
        ? 5000
        : false,
  });
  const workflowContextBlackboardQuery = useQuery({
    queryKey: ["automations", "run", "context", selectedContextRunId, "blackboard"],
    enabled: !!selectedContextRunId,
    queryFn: () =>
      api(`/api/engine/context/runs/${encodeURIComponent(selectedContextRunId)}/blackboard`).catch(
        () => ({
          blackboard: null,
        })
      ),
    refetchInterval:
      selectedContextRunId && isActiveRunStatus((runDetailQuery.data as any)?.run?.status)
        ? 5000
        : false,
  });
  const workflowContextEventsQuery = useQuery({
    queryKey: ["automations", "run", "context", selectedContextRunId, "events"],
    enabled: !!selectedContextRunId,
    queryFn: () =>
      api(`/api/engine/context/runs/${encodeURIComponent(selectedContextRunId)}/events`).catch(
        () => ({ events: [] })
      ),
    refetchInterval:
      selectedContextRunId && isActiveRunStatus((runDetailQuery.data as any)?.run?.status)
        ? 5000
        : false,
  });
  const workflowContextPatchesQuery = useQuery({
    queryKey: ["automations", "run", "context", selectedContextRunId, "patches"],
    enabled: !!selectedContextRunId,
    queryFn: () =>
      api(
        `/api/engine/context/runs/${encodeURIComponent(selectedContextRunId)}/blackboard/patches`
      ).catch(() => ({ patches: [] })),
    refetchInterval:
      selectedContextRunId && isActiveRunStatus((runDetailQuery.data as any)?.run?.status)
        ? 5000
        : false,
  });
  const packsQuery = useQuery({
    queryKey: ["automations", "packs"],
    queryFn: () =>
      client?.packs?.list?.().catch(() => ({ packs: [] })) ?? Promise.resolve({ packs: [] }),
    refetchInterval: 30000,
  });

  const runNowMutation = useMutation({
    mutationFn: (id: string) => client?.automations?.runNow?.(id),
    onSuccess: async () => {
      toast("ok", "Automation triggered.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const runNowV2Mutation = useMutation({
    mutationFn: async ({ id, dryRun }: { id: string; dryRun?: boolean }) => {
      if (!client?.automationsV2?.runNow) {
        throw new Error("Workflow run now is not available in this client.");
      }
      return client.automationsV2.runNow(id, { dryRun: !!dryRun });
    },
    onSuccess: async (payload: any) => {
      const runId = String(payload?.run?.run_id || payload?.run?.runId || "").trim();
      const isDryRun = payload?.dry_run === true || payload?.dryRun === true;
      toast("ok", isDryRun ? "Workflow dry run recorded." : "Workflow automation triggered.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const runActionMutation = useMutation({
    mutationFn: async ({
      action,
      runId,
      family,
      reason,
    }: {
      action: "pause" | "resume";
      runId: string;
      family: "legacy" | "v2";
      reason?: string;
    }) => {
      if (family === "v2") {
        if (action === "pause") return client.automationsV2.pauseRun(runId, reason);
        return client.automationsV2.resumeRun(runId, reason);
      }
      if (action === "pause") return client.automations.pauseRun(runId, reason);
      return client.automations.resumeRun(runId, reason);
    },
    onSuccess: async () => {
      toast("ok", "Run action applied.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const workflowRepairMutation = useMutation({
    mutationFn: async ({
      runId,
      nodeId,
      reason,
    }: {
      runId: string;
      nodeId: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.repairRun) {
        throw new Error("Workflow repair is not available in this client.");
      }
      return client.automationsV2.repairRun(runId, {
        node_id: nodeId,
        reason: reason ?? "",
      });
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      toast("ok", "Workflow continued from blocked step.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const workflowRecoverMutation = useMutation({
    mutationFn: async ({ runId, reason }: { runId: string; reason?: string }) => {
      if (!client?.automationsV2?.recoverRun) {
        throw new Error("Workflow retry is not available in this client.");
      }
      return client.automationsV2.recoverRun(runId, reason);
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      toast("ok", "Workflow run queued again.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const workflowTaskRetryMutation = useMutation({
    mutationFn: async ({
      runId,
      nodeId,
      reason,
    }: {
      runId: string;
      nodeId: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.retryTask) {
        throw new Error("Task retry is not available in this client.");
      }
      return client.automationsV2.retryTask(runId, nodeId, reason);
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      toast("ok", "Task retried and subtree requeued.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const workflowTaskContinueMutation = useMutation({
    mutationFn: async ({
      runId,
      nodeId,
      reason,
    }: {
      runId: string;
      nodeId: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.continueTask) {
        throw new Error("Task continue is not available in this client.");
      }
      return client.automationsV2.continueTask(runId, nodeId, reason);
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      toast("ok", "Blocked task continued with minimal reset.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const workflowTaskRequeueMutation = useMutation({
    mutationFn: async ({
      runId,
      nodeId,
      reason,
    }: {
      runId: string;
      nodeId: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.requeueTask) {
        throw new Error("Task requeue is not available in this client.");
      }
      return client.automationsV2.requeueTask(runId, nodeId, reason);
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      toast("ok", "Task requeued and subtree reset.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const backlogTaskClaimMutation = useMutation({
    mutationFn: async ({
      runId,
      taskId,
      agentId,
      reason,
    }: {
      runId: string;
      taskId: string;
      agentId?: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.claimBacklogTask) {
        throw new Error("Backlog task claim is not available in this client.");
      }
      return client.automationsV2.claimBacklogTask(runId, taskId, {
        agent_id: agentId,
        reason,
      });
    },
    onSuccess: async () => {
      toast("ok", "Backlog task claimed.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (selectedRunId) {
        onSelectRunId(selectedRunId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const backlogTaskRequeueMutation = useMutation({
    mutationFn: async ({
      runId,
      taskId,
      reason,
    }: {
      runId: string;
      taskId: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.requeueBacklogTask) {
        throw new Error("Backlog task requeue is not available in this client.");
      }
      return client.automationsV2.requeueBacklogTask(runId, taskId, reason);
    },
    onSuccess: async () => {
      toast("ok", "Backlog task requeued.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (selectedRunId) {
        onSelectRunId(selectedRunId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const updateAutomationMutation = useMutation({
    mutationFn: async (draft: {
      automationId: string;
      name: string;
      objective: string;
      mode: "standalone" | "orchestrated";
      requiresApproval: boolean;
      scheduleKind: "cron" | "interval";
      cronExpression: string;
      intervalSeconds: string;
    }) => {
      const name = String(draft.name || "").trim();
      const objective = String(draft.objective || "").trim();
      const cronExpression = String(draft.cronExpression || "").trim();
      const intervalSeconds = Number(draft.intervalSeconds);
      if (!name) throw new Error("Automation name is required.");
      if (!objective) throw new Error("Objective is required.");
      if (draft.scheduleKind === "cron" && !cronExpression) {
        throw new Error("Cron expression is required.");
      }
      if (
        draft.scheduleKind === "interval" &&
        (!Number.isFinite(intervalSeconds) || intervalSeconds <= 0)
      ) {
        throw new Error("Interval seconds must be greater than zero.");
      }
      return client.automations.update(draft.automationId, {
        name,
        mode: draft.mode,
        mission: { objective },
        policy: { approval: { requires_approval: !!draft.requiresApproval } },
        schedule:
          draft.scheduleKind === "cron"
            ? { cron: { expression: cronExpression } }
            : { interval_seconds: { seconds: Math.round(intervalSeconds) } },
      });
    },
    onSuccess: async () => {
      toast("ok", "Automation updated.");
      setEditDraft(null);
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const updateWorkflowAutomationMutation = useMutation({
    mutationFn: async (draft: WorkflowEditDraft) => {
      const name = String(draft.name || "").trim();
      const description = String(draft.description || "").trim();
      const workspaceRoot = String(draft.workspaceRoot || "").trim();
      const modelError = validateModelInput(draft.modelProvider, draft.modelId);
      const plannerModelError = validatePlannerModelInput(
        draft.plannerModelProvider,
        draft.plannerModelId
      );
      const workspaceError = validateWorkspaceRootInput(workspaceRoot);
      if (!name) throw new Error("Automation name is required.");
      if (workspaceError) throw new Error(workspaceError);
      if (modelError) throw new Error(modelError);
      if (plannerModelError) throw new Error(plannerModelError);
      if (draft.scheduleKind === "cron" && !String(draft.cronExpression || "").trim()) {
        throw new Error("Cron expression is required.");
      }
      if (
        draft.scheduleKind === "interval" &&
        (!Number.isFinite(Number(draft.intervalSeconds)) || Number(draft.intervalSeconds) <= 0)
      ) {
        throw new Error("Interval seconds must be greater than zero.");
      }
      const operatorPreferences = workflowEditToOperatorPreferences(draft);
      const modelPolicy = compileWorkflowModelPolicy(operatorPreferences);
      const baseModelPolicy = modelPolicy
        ? (cloneJsonValue(modelPolicy) as Record<string, any>)
        : null;
      const selectedMcpServers = draft.selectedMcpServers
        .map((row) => String(row || "").trim())
        .filter(Boolean);
      const toolAllowlist = compileWorkflowToolAllowlist(
        selectedMcpServers,
        draft.toolAccessMode,
        draft.customToolsText
      );
      const connectorBindings = parseConnectorBindingsJson(draft.connectorBindingsJson);
      const stepModelPolicies = new Map<string, Record<string, any> | null>();
      for (const node of draft.nodes) {
        const nodeAgentId = String(node.agentId || "").trim();
        if (!nodeAgentId) continue;
        const nodeModelProvider = String(node.modelProvider || "").trim();
        const nodeModelId = String(node.modelId || "").trim();
        const nodeModelError = validateModelInput(nodeModelProvider, nodeModelId);
        if (nodeModelError) {
          throw new Error(`${node.title || node.nodeId || nodeAgentId}: ${nodeModelError}`);
        }
        stepModelPolicies.set(
          nodeAgentId,
          workflowNodeModelPolicyWithOverride(baseModelPolicy, nodeModelProvider, nodeModelId)
        );
      }
      const nextScopeSnapshot = draft.scopeSnapshot ? cloneJsonValue(draft.scopeSnapshot) : null;
      if (nextScopeSnapshot && typeof nextScopeSnapshot === "object") {
        nextScopeSnapshot.connector_bindings = connectorBindings;
        nextScopeSnapshot.connector_binding_resolution =
          deriveConnectorBindingResolutionFromPlanPackage(nextScopeSnapshot, connectorBindings);
      }
      const existing = automationsV2.find(
        (row: any) =>
          String(row?.automation_id || row?.automationId || row?.id || "").trim() ===
          draft.automationId
      );
      const agents = Array.isArray(existing?.agents)
        ? existing.agents.map((agent: any) => {
            const agentId = String(agent?.agent_id || agent?.agentId || "").trim();
            const nextModelPolicy = stepModelPolicies.has(agentId)
              ? stepModelPolicies.get(agentId)
              : agent?.model_policy || agent?.modelPolicy || modelPolicy;
            return {
              ...agent,
              model_policy: nextModelPolicy ? cloneJsonValue(nextModelPolicy) : null,
              modelPolicy: undefined,
              tool_policy: {
                ...(agent?.tool_policy || {}),
                allowlist: toolAllowlist,
                denylist: Array.isArray(agent?.tool_policy?.denylist)
                  ? agent.tool_policy.denylist
                  : [],
              },
              mcp_policy: {
                ...(agent?.mcp_policy || {}),
                allowed_servers: selectedMcpServers,
                allowed_tools: null,
              },
            };
          })
        : [];
      const flowNodes = Array.isArray(existing?.flow?.nodes)
        ? existing.flow.nodes.map((node: any, index: number) => {
            const nodeId = String(
              node?.node_id || node?.nodeId || node?.id || `node-${index}`
            ).trim();
            const draftNode = draft.nodes.find((row) => row.nodeId === nodeId);
            return draftNode
              ? {
                  ...node,
                  objective: String(draftNode.objective || "").trim(),
                }
              : node;
          })
        : [];
      const existingMetadata =
        existing?.metadata && typeof existing.metadata === "object" ? existing.metadata : {};
      const nextPlanPackage = nextScopeSnapshot
        ? {
            ...(cloneJsonValue(existingMetadata?.plan_package) || {}),
            ...nextScopeSnapshot,
          }
        : existingMetadata?.plan_package;
      const nextPlanPackageBundle =
        nextScopeSnapshot && existingMetadata?.plan_package_bundle
          ? {
              ...cloneJsonValue(existingMetadata.plan_package_bundle),
              scope_snapshot: nextScopeSnapshot,
            }
          : existingMetadata?.plan_package_bundle;
      return client.automationsV2.update(draft.automationId, {
        name,
        description: description || null,
        schedule: workflowEditToSchedule(draft),
        workspace_root: workspaceRoot,
        execution: {
          ...(existing?.execution || {}),
          max_parallel_agents:
            draft.executionMode === "swarm"
              ? Math.max(
                  1,
                  Math.min(16, Number.parseInt(String(draft.maxParallelAgents || "4"), 10) || 4)
                )
              : 1,
        },
        flow: existing?.flow
          ? {
              ...existing.flow,
              nodes: flowNodes,
            }
          : existing?.flow,
        agents,
        metadata: {
          ...existingMetadata,
          workspace_root: workspaceRoot,
          operator_preferences: operatorPreferences,
          allowed_mcp_servers: selectedMcpServers,
          ...(nextPlanPackage ? { plan_package: nextPlanPackage } : {}),
          ...(nextPlanPackageBundle ? { plan_package_bundle: nextPlanPackageBundle } : {}),
        },
      });
    },
    onSuccess: async () => {
      toast("ok", "Workflow automation updated.");
      setWorkflowEditDraft(null);
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const automationActionMutation = useMutation({
    mutationFn: async ({
      action,
      automationId,
      family,
    }: {
      action: "pause" | "resume" | "delete";
      automationId: string;
      family: "legacy" | "v2";
    }) => {
      if (family === "v2") {
        if (action === "delete") return client.automationsV2.delete(automationId);
        if (action === "pause") return client.automationsV2.pause(automationId);
        return client.automationsV2.resume(automationId);
      }
      if (action === "delete") return client.automations.delete(automationId);
      return client.automations.update(automationId, {
        status: action === "pause" ? "paused" : "enabled",
      });
    },
    onSuccess: async (_payload, vars) => {
      if (vars.action === "delete") toast("ok", "Automation removed.");
      if (vars.action === "pause") toast("ok", "Automation paused.");
      if (vars.action === "resume") toast("ok", "Automation resumed.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const automations = useMemo(() => {
    const merged = [
      ...toArray(automationsQuery.data, "automations"),
      ...toArray(automationsQuery.data, "routines"),
    ];
    const byId = new Map<string, any>();
    for (const row of merged) {
      const id = String(row?.automation_id || row?.routine_id || row?.id || "").trim();
      if (!id) continue;
      if (!byId.has(id)) byId.set(id, row);
    }
    return Array.from(byId.values());
  }, [automationsQuery.data]);
  const calendarEvents = useMemo(() => {
    const legacyEvents = automations.flatMap((automation: any) =>
      buildCalendarOccurrences({
        automation,
        family: "legacy",
        rangeStartMs: calendarRange.startMs,
        rangeEndMs: calendarRange.endMs,
      })
    );
    const workflowEvents = automationsV2.flatMap((automation: any) =>
      buildCalendarOccurrences({
        automation,
        family: "v2",
        rangeStartMs: calendarRange.startMs,
        rangeEndMs: calendarRange.endMs,
      })
    );
    return [...legacyEvents, ...workflowEvents];
  }, [automations, automationsV2, calendarRange.endMs, calendarRange.startMs]);
  const legacyRuns = toArray(runsQuery.data, "runs");
  const providerOptions = useMemo<ProviderOption[]>(() => {
    const rows = Array.isArray((providerCatalogQuery.data as any)?.all)
      ? (providerCatalogQuery.data as any).all
      : Array.isArray((providerCatalogQuery.data as any)?.providers)
        ? (providerCatalogQuery.data as any).providers
        : [];
    const configuredProviders = (providersConfigQuery.data as any)?.providers || {};
    return rows
      .map((provider: any) => ({
        id: String(provider?.id || "").trim(),
        models: Object.keys(provider?.models || {}).sort(),
        configured: !!configuredProviders[String(provider?.id || "").trim()],
      }))
      .filter((provider: ProviderOption) => provider.id)
      .sort((a, b) => a.id.localeCompare(b.id));
  }, [providerCatalogQuery.data, providersConfigQuery.data]);
  const mcpServers = useMemo(
    () => normalizeMcpServers(mcpServersQuery.data),
    [mcpServersQuery.data]
  );
  const workflowRuns = toArray(workflowRunsQuery.data, "runs");
  const runs = useMemo(() => {
    const all = [...legacyRuns, ...workflowRuns];
    const byId = new Map<string, any>();
    for (const run of all) {
      const runId = String(run?.run_id || run?.runId || run?.id || "").trim();
      if (!runId) continue;
      if (!byId.has(runId)) byId.set(runId, run);
    }
    return Array.from(byId.values()).sort((a: any, b: any) => {
      const aAt = normalizeTimestamp(
        a?.started_at_ms || a?.startedAtMs || a?.created_at_ms || a?.createdAtMs || 0
      );
      const bAt = normalizeTimestamp(
        b?.started_at_ms || b?.startedAtMs || b?.created_at_ms || b?.createdAtMs || 0
      );
      return bAt - aAt;
    });
  }, [legacyRuns, workflowRuns]);
  const packs = toArray(packsQuery.data, "packs");
  const activeRuns = runs.filter((run: any) => isActiveRunStatus(workflowDerivedRunStatus(run)));
  const workflowQueueCounts = useMemo(() => {
    let active = 0;
    let queuedCapacity = 0;
    let queuedWorkspaceLock = 0;
    let queuedOther = 0;
    workflowRuns.forEach((run: any) => {
      const status = workflowDerivedRunStatus(run);
      const reason = workflowQueueReason(run);
      if (status === "queued") {
        if (reason === "capacity") queuedCapacity += 1;
        else if (reason === "workspace_lock") queuedWorkspaceLock += 1;
        else queuedOther += 1;
        return;
      }
      if (isActiveRunStatus(status)) active += 1;
    });
    return { active, queuedCapacity, queuedWorkspaceLock, queuedOther };
  }, [workflowRuns]);
  const failedRuns = runs.filter((run: any) => {
    const status = workflowDerivedRunStatus(run);
    return status === "failed" || status === "error" || status === "blocked";
  });
  const selectedRun = (runDetailQuery.data as any)?.run || null;
  const workflowBlackboard = (workflowContextBlackboardQuery.data as any)?.blackboard || null;
  const workflowContextEvents = Array.isArray((workflowContextEventsQuery.data as any)?.events)
    ? (workflowContextEventsQuery.data as any).events
    : [];
  const workflowContextPatches = Array.isArray((workflowContextPatchesQuery.data as any)?.patches)
    ? (workflowContextPatchesQuery.data as any).patches
    : [];
  const workflowProjection = useMemo(() => {
    if (!isWorkflowRun) return { tasks: [], currentTaskId: "", taskSource: "empty" as const };
    const activeTaskId = detectWorkflowActiveTaskId(selectedRun, [], sessionEvents);
    const contextProjection = projectOrchestrationRun({
      run: (workflowContextRunQuery.data as any)?.run || null,
      tasks: Array.isArray((workflowContextRunQuery.data as any)?.run?.steps)
        ? (workflowContextRunQuery.data as any)?.run.steps
        : [],
      blackboard: workflowBlackboard,
      events: workflowContextEvents,
    });
    if (contextProjection.tasks.length) {
      const normalizedTasks = activeTaskId
        ? contextProjection.tasks.map((task) =>
            task.id === activeTaskId && ["pending", "runnable", "assigned"].includes(task.state)
              ? { ...task, state: "in_progress" as const }
              : task
          )
        : contextProjection.tasks;
      return {
        ...contextProjection,
        tasks: normalizedTasks,
        currentTaskId: contextProjection.currentTaskId || activeTaskId,
      };
    }
    return workflowProjectionFromRunSnapshot(selectedRun, activeTaskId);
  }, [
    isWorkflowRun,
    selectedRun,
    sessionEvents,
    workflowBlackboard,
    workflowContextEvents,
    workflowContextRunQuery.data,
  ]);
  const selectedBoardTask = useMemo(
    () => workflowProjection.tasks.find((task) => task.id === selectedBoardTaskId) || null,
    [selectedBoardTaskId, workflowProjection.tasks]
  );
  const firstBlockedWorkflowTask = useMemo(
    () =>
      workflowProjection.tasks.find(
        (task) => String(task.state || "").toLowerCase() === "blocked"
      ) || null,
    [workflowProjection.tasks]
  );
  const selectedBoardTaskOutput = useMemo(() => {
    if (!selectedBoardTask) return null;
    const nodeId = String(selectedBoardTask.id || "").replace(/^node-/, "");
    return workflowNodeOutput(selectedRun, nodeId);
  }, [selectedBoardTask, selectedRun]);
  const selectedBoardTaskTelemetry = useMemo(
    () => workflowNodeToolTelemetry(selectedBoardTaskOutput),
    [selectedBoardTaskOutput]
  );
  const selectedBoardTaskArtifactValidation = useMemo(
    () => workflowArtifactValidation(selectedBoardTaskOutput),
    [selectedBoardTaskOutput]
  );
  const selectedBoardTaskInspection = useMemo(
    () => workflowTaskInspectionDetails(selectedBoardTask, selectedBoardTaskOutput) || {},
    [selectedBoardTask, selectedBoardTaskOutput]
  );
  const {
    validationBasis: selectedBoardTaskValidationBasis = null,
    qualityMode: selectedBoardTaskQualityMode = "",
    requestedQualityMode: selectedBoardTaskRequestedQualityMode = "",
    emergencyRollbackEnabled: selectedBoardTaskEmergencyRollbackEnabled = null,
    blockerCategory: selectedBoardTaskBlockerCategory = "",
    receiptLedger: selectedBoardTaskReceiptLedger = null,
    receiptTimeline: selectedBoardTaskReceiptTimeline = [],
    touchedFiles: selectedBoardTaskTouchedFiles = [],
    undeclaredFiles: selectedBoardTaskUndeclaredFiles = [],
    researchReadPaths: selectedBoardTaskResearchReadPaths = [],
    discoveredRelevantPaths: selectedBoardTaskDiscoveredRelevantPaths = [],
    reviewedPathsBackedByRead: selectedBoardTaskReviewedPathsBackedByRead = [],
    unreviewedRelevantPaths: selectedBoardTaskUnreviewedRelevantPaths = [],
    unmetResearchRequirements: selectedBoardTaskUnmetResearchRequirements = [],
    verificationOutcome: selectedBoardTaskVerificationOutcome = "",
    verificationPassed: selectedBoardTaskVerificationPassed = null,
    verificationResults: selectedBoardTaskVerificationResults = [],
    failureDetail: selectedBoardTaskFailureDetail = "",
    workflowClass: selectedBoardTaskWorkflowClass = "",
    phase: selectedBoardTaskPhase = "",
    failureKind: selectedBoardTaskFailureKind = "",
    warningCount: selectedBoardTaskWarningCount = 0,
    warningRequirements: selectedBoardTaskWarningRequirements = [],
    validationOutcome: selectedBoardTaskValidationOutcome = "",
    artifactCandidates: selectedBoardTaskArtifactCandidates = [],
  } = selectedBoardTaskInspection as any;
  const continueBlockedTask =
    String(selectedBoardTask?.state || "").toLowerCase() === "blocked"
      ? selectedBoardTask
      : firstBlockedWorkflowTask;
  const continueBlockedNodeId = String(continueBlockedTask?.id || "")
    .replace(/^node-/, "")
    .trim();
  const rawRunStatus = String(selectedRun?.status || "")
    .trim()
    .toLowerCase();
  const runStatus = workflowDerivedRunStatus(selectedRun);
  const runStatusDerivedFromBlockedNodes =
    rawRunStatus !== runStatus &&
    (rawRunStatus === "completed" || rawRunStatus === "done") &&
    workflowBlockedNodeCount(selectedRun) > 0;
  const selectedBoardTaskNeedsWorkflowAction =
    String(selectedBoardTask?.id || "").startsWith("node-") &&
    ["blocked", "failed"].includes(
      String(selectedBoardTask?.state || "")
        .trim()
        .toLowerCase()
    );
  const canRecoverWorkflowRun =
    isWorkflowRun &&
    !!selectedRunId &&
    (["failed", "paused"].includes(runStatus) ||
      workflowBlockedNodeCount(selectedRun) > 0 ||
      selectedBoardTaskNeedsWorkflowAction);
  const canContinueBlockedWorkflow = isWorkflowRun && !!selectedRunId && !!continueBlockedNodeId;
  const runRepairGuidanceEntries = useMemo(() => {
    const direct = selectedRun?.nodeRepairGuidance;
    const directEntries =
      direct && typeof direct === "object" && !Array.isArray(direct)
        ? Object.entries(direct)
            .map(([nodeId, guidance]: [string, any]) => ({
              nodeId: String(nodeId || "").trim(),
              guidance: guidance || {},
            }))
            .filter((entry) => entry.nodeId)
        : [];
    if (directEntries.length) return directEntries;
    const outputs =
      selectedRun?.checkpoint?.node_outputs || selectedRun?.checkpoint?.nodeOutputs || {};
    return Object.entries(outputs)
      .map(([nodeId, output]: [string, any]) => {
        const artifactValidation = output?.artifact_validation || output?.artifactValidation || {};
        const validatorSummary = output?.validator_summary || output?.validatorSummary || {};
        const actions = Array.isArray(
          artifactValidation?.required_next_tool_actions ||
            artifactValidation?.requiredNextToolActions
        )
          ? artifactValidation.required_next_tool_actions ||
            artifactValidation.requiredNextToolActions
          : [];
        const unmet = Array.isArray(
          validatorSummary?.unmet_requirements || validatorSummary?.unmetRequirements
        )
          ? validatorSummary.unmet_requirements || validatorSummary.unmetRequirements
          : [];
        const reason = String(
          validatorSummary?.reason || output?.blocked_reason || output?.blockedReason || ""
        ).trim();
        const blockingClassification = String(
          artifactValidation?.blocking_classification ||
            artifactValidation?.blockingClassification ||
            ""
        ).trim();
        if (!actions.length && !unmet.length && !reason && !blockingClassification) return null;
        return {
          nodeId: String(nodeId || "").trim(),
          guidance: {
            status: output?.status || "",
            failureKind: output?.failure_kind || output?.failureKind || "",
            reason,
            unmetRequirements: unmet,
            blockingClassification,
            requiredNextToolActions: actions,
            repairAttempt:
              artifactValidation?.repair_attempt ?? artifactValidation?.repairAttempt ?? null,
            repairAttemptsRemaining:
              artifactValidation?.repair_attempts_remaining ??
              artifactValidation?.repairAttemptsRemaining ??
              null,
          },
        };
      })
      .filter(Boolean) as Array<{ nodeId: string; guidance: any }>;
  }, [selectedRun]);
  useEffect(() => {
    if (!selectedBoardTaskId || !boardDetailRef.current) return;
    boardDetailRef.current.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [selectedBoardTaskId]);
  useEffect(() => {
    setSelectedRunArtifactKey("");
  }, [selectedRunId, selectedBoardTaskId]);
  const runArtifacts = isWorkflowRun
    ? Array.isArray(workflowBlackboard?.artifacts)
      ? workflowBlackboard.artifacts
      : []
    : toArray(runArtifactsQuery.data, "artifacts");
  const runArtifactEntries = useMemo(
    () =>
      runArtifacts.map((artifact: any, index: number) => {
        const key = String(artifact?.id || artifact?.artifact_id || `artifact-${index + 1}`).trim();
        const name = String(
          artifact?.name ||
            artifact?.label ||
            artifact?.kind ||
            artifact?.type ||
            artifact?.path ||
            key
        ).trim();
        const kind = String(artifact?.kind || artifact?.type || artifact?.path || "").trim();
        const paths = uniqueStrings(collectPathStrings(artifact));
        return { key, name: name || key, kind, artifact, paths };
      }),
    [runArtifacts]
  );
  const selectedBoardTaskRelatedPaths = useMemo(() => {
    if (!selectedBoardTask) return [];
    return uniqueStrings([
      ...collectPathStrings(selectedBoardTaskOutput),
      ...collectPathStrings(selectedBoardTaskArtifactValidation),
      String((selectedBoardTask as any).output_path || "").trim(),
    ]);
  }, [selectedBoardTask, selectedBoardTaskArtifactValidation, selectedBoardTaskOutput]);
  const selectedBoardTaskRelatedArtifacts = useMemo(() => {
    if (!selectedBoardTaskRelatedPaths.length) return [];
    return runArtifactEntries.filter((entry) =>
      entry.paths.some((path) => selectedBoardTaskRelatedPaths.includes(path))
    );
  }, [runArtifactEntries, selectedBoardTaskRelatedPaths]);
  const selectedBoardTaskNodeId = useMemo(
    () =>
      String(selectedBoardTask?.id || "").startsWith("node-")
        ? String(selectedBoardTask?.id || "")
            .replace(/^node-/, "")
            .trim()
        : "",
    [selectedBoardTask]
  );
  const selectedBoardTaskIsWorkflowNode = useMemo(
    () => String(selectedBoardTask?.id || "").startsWith("node-"),
    [selectedBoardTask]
  );
  const selectedBoardTaskIsProjectedBacklogItem = useMemo(
    () => String((selectedBoardTask as any)?.task_type || "").trim() === "automation_backlog_item",
    [selectedBoardTask]
  );
  const selectedBoardTaskStateNormalized = useMemo(
    () =>
      String(selectedBoardTask?.state || "")
        .trim()
        .toLowerCase(),
    [selectedBoardTask]
  );
  const selectedBoardTaskLeaseExpiresAtMs = useMemo(
    () => Number((selectedBoardTask as any)?.lease_expires_at_ms || 0) || 0,
    [selectedBoardTask]
  );
  const selectedBoardTaskIsStale = useMemo(
    () =>
      Boolean((selectedBoardTask as any)?.is_stale) ||
      (selectedBoardTaskStateNormalized === "in_progress" &&
        selectedBoardTaskLeaseExpiresAtMs > 0 &&
        selectedBoardTaskLeaseExpiresAtMs <= Date.now()),
    [selectedBoardTask, selectedBoardTaskLeaseExpiresAtMs, selectedBoardTaskStateNormalized]
  );
  const selectedBoardTaskLifecycleEvents = useMemo(
    () => workflowRecentNodeEventSummaries(selectedRun, selectedBoardTaskNodeId, 8),
    [selectedBoardTaskNodeId, selectedRun]
  );
  const selectedBoardTaskResetTaskIds = useMemo(
    () => workflowDescendantTaskIds(workflowProjection.tasks, selectedBoardTask?.id || ""),
    [selectedBoardTask, workflowProjection.tasks]
  );
  const selectedBoardTaskResetTasks = useMemo(
    () =>
      selectedBoardTaskResetTaskIds
        .map((taskId) => workflowProjection.tasks.find((task) => task.id === taskId) || null)
        .filter(Boolean) as any[],
    [selectedBoardTaskResetTaskIds, workflowProjection.tasks]
  );
  const selectedBoardTaskResetNodeIds = useMemo(() => {
    const preview = (taskResetPreviewQuery.data as any)?.preview;
    const previewNodes = Array.isArray(preview?.reset_nodes)
      ? preview.reset_nodes.map((value: any) => String(value || "").trim()).filter(Boolean)
      : [];
    if (previewNodes.length) return previewNodes;
    return selectedBoardTaskResetTaskIds
      .map((taskId) => taskId.replace(/^node-/, "").trim())
      .filter(Boolean);
  }, [selectedBoardTaskResetTaskIds, taskResetPreviewQuery.data]);
  const selectedBoardTaskResetOutputPaths = useMemo(() => {
    const preview = (taskResetPreviewQuery.data as any)?.preview;
    const previewOutputs = Array.isArray(preview?.cleared_outputs)
      ? preview.cleared_outputs.map((value: any) => String(value || "").trim()).filter(Boolean)
      : [];
    if (previewOutputs.length) return uniqueStrings(previewOutputs);
    return uniqueStrings(
      selectedBoardTaskResetTasks.map((task) => String((task as any)?.output_path || "").trim())
    );
  }, [selectedBoardTaskResetTasks, taskResetPreviewQuery.data]);
  const focusArtifactEntry = (path: string) => {
    const targetPath = String(path || "").trim();
    const match = runArtifactEntries.find((entry) => entry.paths.includes(targetPath));
    setSelectedRunArtifactKey(match?.key || "");
    if (artifactsSectionRef.current) {
      artifactsSectionRef.current.scrollIntoView({ block: "nearest", behavior: "smooth" });
    }
  };
  const canTaskRetry =
    isWorkflowRun &&
    !!selectedRunId &&
    selectedBoardTaskIsWorkflowNode &&
    !!selectedBoardTaskNodeId &&
    ["blocked", "failed"].includes(selectedBoardTaskStateNormalized);
  const runDebuggerRetryNodeId =
    canTaskRetry && selectedBoardTaskIsWorkflowNode ? selectedBoardTaskNodeId : "";
  const canTaskContinue =
    isWorkflowRun &&
    !!selectedRunId &&
    selectedBoardTaskIsWorkflowNode &&
    !!selectedBoardTaskNodeId &&
    selectedBoardTaskStateNormalized === "blocked";
  const canTaskRequeue =
    isWorkflowRun &&
    !!selectedRunId &&
    selectedBoardTaskIsWorkflowNode &&
    !!selectedBoardTaskNodeId &&
    !["in_progress", "done", "blocked", "failed"].includes(selectedBoardTaskStateNormalized);
  const canBacklogTaskClaim =
    isWorkflowRun &&
    !!selectedRunId &&
    selectedBoardTaskIsProjectedBacklogItem &&
    !selectedBoardTaskIsWorkflowNode &&
    ["pending", "runnable"].includes(selectedBoardTaskStateNormalized);
  const canBacklogTaskRequeue =
    isWorkflowRun &&
    !!selectedRunId &&
    selectedBoardTaskIsProjectedBacklogItem &&
    !selectedBoardTaskIsWorkflowNode &&
    (["blocked", "failed"].includes(selectedBoardTaskStateNormalized) || selectedBoardTaskIsStale);
  const selectedBoardTaskImpactSummary = useMemo(() => {
    const preview = (taskResetPreviewQuery.data as any)?.preview;
    const rootTitle = String(selectedBoardTask?.title || selectedBoardTaskNodeId || "task").trim();
    const subtreeCount = selectedBoardTaskResetNodeIds.length;
    const descendantCount = Math.max(0, subtreeCount - (selectedBoardTaskNodeId ? 1 : 0));
    const outputCount = selectedBoardTaskResetOutputPaths.length;
    return {
      rootTitle,
      subtreeCount,
      descendantCount,
      outputCount,
      previewBacked: Boolean((taskResetPreviewQuery.data as any)?.preview),
      preservesUpstreamOutputs:
        typeof preview?.preserves_upstream_outputs === "boolean"
          ? preview.preserves_upstream_outputs
          : true,
    };
  }, [
    selectedBoardTask,
    selectedBoardTaskNodeId,
    selectedBoardTaskResetNodeIds.length,
    selectedBoardTaskResetOutputPaths.length,
    taskResetPreviewQuery.data,
  ]);
  const runHints = deriveRunDebugHints(selectedRun, runArtifacts);
  const runHistoryEvents = isWorkflowRun
    ? (() => {
        const contextHistory = workflowContextHistoryEntries(
          workflowContextEvents,
          workflowContextPatches
        );
        if (contextHistory.length) return contextHistory;
        return workflowPersistedHistoryEntries(
          Array.isArray(persistedRunEventsQuery.data) ? persistedRunEventsQuery.data : [],
          selectedRunId
        );
      })()
    : Array.isArray((runHistoryQuery.data as any)?.events)
      ? (runHistoryQuery.data as any).events
      : Array.isArray((runHistoryQuery.data as any)?.history)
        ? (runHistoryQuery.data as any).history
        : [];
  const telemetrySeedEvents = useMemo(() => {
    return workflowTelemetrySeedEvents(
      Array.isArray(persistedRunEventsQuery.data) ? persistedRunEventsQuery.data : [],
      workflowContextEvents,
      isWorkflowRun,
      selectedRunId
    );
  }, [isWorkflowRun, persistedRunEventsQuery.data, selectedRunId, workflowContextEvents]);
  const telemetryEvents = useMemo(() => {
    const all = [...telemetrySeedEvents, ...runEvents];
    const seen = new Set<string>();
    return all
      .filter((item) => {
        if (!item?.id || seen.has(item.id)) return false;
        seen.add(item.id);
        return true;
      })
      .sort((a, b) => Number(a.at || 0) - Number(b.at || 0));
  }, [telemetrySeedEvents, runEvents]);
  const filteredRunEvents = telemetryEvents.filter((item) =>
    selectedLogSource === "all" ? true : item.source === selectedLogSource
  );
  const filteredRunEventEntries = useMemo(
    () => workflowTelemetryDisplayEntries(filteredRunEvents),
    [filteredRunEvents]
  );
  const sessionMessages = useMemo(
    () =>
      sessionMessageQueries.flatMap((query, index) => {
        const sessionId = availableSessionIds[index] || "";
        const messages = Array.isArray(query.data) ? query.data : [];
        return messages.map((message: any) => ({
          sessionId,
          message,
        }));
      }),
    [availableSessionIds, sessionMessageQueries]
  );
  const runSummaryRows = useMemo(() => {
    const rows: Array<{ label: string; value: string }> = [];
    rows.push({ label: "status", value: runStatus || "unknown" });
    if (runStatusDerivedFromBlockedNodes) {
      rows.push({ label: "status note", value: "derived from blocked nodes" });
    }
    rows.push({ label: "artifacts", value: String(runArtifacts.length) });
    if (isWorkflowRun) {
      rows.push({ label: "tasks", value: String(workflowProjection.tasks.length) });
      rows.push({
        label: "context events",
        value: String(workflowContextEvents.length),
      });
      rows.push({
        label: "blackboard patches",
        value: String(workflowContextPatches.length),
      });
      rows.push({
        label: "completed nodes",
        value: String(workflowCompletedNodeCount(selectedRun)),
      });
      rows.push({
        label: "pending nodes",
        value: String(workflowPendingNodeCount(selectedRun)),
      });
      rows.push({
        label: "blocked nodes",
        value: String(workflowBlockedNodeCount(selectedRun)),
      });
    }
    if (String(selectedRun?.detail || "").trim()) {
      rows.push({ label: "detail", value: String(selectedRun.detail).trim() });
    }
    if (selectedRun?.requires_approval !== undefined) {
      rows.push({
        label: "requires approval",
        value: String(Boolean(selectedRun?.requires_approval)),
      });
    }
    if (String(selectedRun?.approval_reason || "").trim()) {
      rows.push({ label: "approval reason", value: String(selectedRun.approval_reason).trim() });
    }
    if (String(selectedRun?.denial_reason || "").trim()) {
      rows.push({ label: "denial reason", value: String(selectedRun.denial_reason).trim() });
    }
    if (String(selectedRun?.paused_reason || "").trim()) {
      rows.push({ label: "paused reason", value: String(selectedRun.paused_reason).trim() });
    }
    return rows;
  }, [
    isWorkflowRun,
    runArtifacts.length,
    runStatus,
    runStatusDerivedFromBlockedNodes,
    selectedRun,
    workflowContextEvents.length,
    workflowContextPatches.length,
    workflowProjection.tasks.length,
  ]);
  const failureReason = useMemo(() => explainRunFailure(selectedRun), [selectedRun]);

  useSelectedRunLifecycle({
    availableSessionIds,
    queryClient,
    selectedRunId,
    selectedContextRunId,
    onSelectRunId,
    setSelectedSessionId,
    setSelectedSessionFilterId,
    setRunEvents,
    setSelectedLogSource,
    setSelectedBoardTaskId,
    setSessionEvents,
    setSessionLogPinnedToBottom,
  });

  const prevAutoSelectRunId = useRef("");
  useEffect(() => {
    if (!selectedRunId || !workflowProjection.tasks.length) return;
    if (prevAutoSelectRunId.current === selectedRunId) return;
    prevAutoSelectRunId.current = selectedRunId;
    setSelectedBoardTaskId(
      workflowProjection.currentTaskId ||
        workflowProjection.tasks.find((task) =>
          ["in_progress", "blocked", "assigned", "runnable", "pending"].includes(
            String(task.state || "").toLowerCase()
          )
        )?.id ||
        workflowProjection.tasks[0]?.id ||
        ""
    );
  }, [selectedRunId, workflowProjection.currentTaskId, workflowProjection.tasks]);

  useEngineStream(
    selectedRunId
      ? isWorkflowRun
        ? `/api/engine/automations/v2/events?run_id=${encodeURIComponent(selectedRunId)}`
        : `/api/engine/automations/events?run_id=${encodeURIComponent(selectedRunId)}`
      : "",
    (msg) => {
      try {
        const payload = JSON.parse(String(msg?.data || "{}"));
        if (!payload || payload.status === "ready") return;
        const runId = workflowEventRunId(payload);
        if (!runId || runId !== selectedRunId) return;
        const type = workflowEventType(payload);
        const at = workflowEventAt(payload);
        const id = `automations:${runId}:${type}:${at}:${Math.random().toString(16).slice(2, 8)}`;
        setRunEvents((prev) => [
          ...prev.slice(-299),
          { id, source: "automations", at, event: payload },
        ]);
      } catch {
        return;
      }
    },
    { enabled: !!selectedRunId }
  );

  useEngineStream(
    selectedContextRunId
      ? `/api/engine/context/runs/${encodeURIComponent(selectedContextRunId)}/events/stream?tail=50`
      : "",
    (msg) => {
      try {
        const payload = JSON.parse(String(msg?.data || "{}"));
        if (!payload || payload.status === "ready") return;
        const id = `context:${String(payload?.seq || "")}:${String(payload?.event_type || "")}`;
        const at =
          timestampOrNull(
            payload?.created_at_ms || payload?.timestamp_ms || payload?.timestampMs
          ) || Date.now();
        setRunEvents((prev) => {
          if (prev.some((row) => row.id === id)) return prev;
          return [...prev.slice(-399), { id, source: "context", at, event: payload }];
        });
      } catch {
        return;
      }
    },
    { enabled: !!selectedContextRunId }
  );

  useEngineStream(
    selectedRunId && selectedSessionId
      ? `/api/engine/event?sessionID=${encodeURIComponent(selectedSessionId)}&runID=${encodeURIComponent(selectedRunId)}`
      : "",
    (msg) => {
      try {
        const payload = JSON.parse(String(msg?.data || "{}"));
        if (!payload) return;
        const type = workflowEventType(payload);
        const at = workflowEventAt(payload);
        const id = [
          type || "event",
          String(payload?.properties?.sessionID || payload?.sessionID || selectedSessionId || ""),
          String(payload?.properties?.runID || payload?.runID || selectedRunId || ""),
          String(payload?.properties?.messageID || payload?.messageID || ""),
          String(
            payload?.properties?.part?.id || payload?.properties?.seq || payload?.timestamp_ms || at
          ),
        ].join(":");
        setSessionEvents((prev) => {
          if (prev.some((row) => row.id === id)) return prev;
          return [...prev.slice(-499), { id, at, event: payload }];
        });
      } catch {
        return;
      }
    },
    { enabled: !!selectedRunId && !!selectedSessionId }
  );

  useEngineStream(
    selectedRunId ? "/api/global/event" : "",
    (msg) => {
      try {
        const payload = JSON.parse(String(msg?.data || "{}"));
        const runId = workflowEventRunId(payload);
        if (!runId || runId !== selectedRunId) return;
        const type = workflowEventType(payload);
        if (!type || type === "server.connected" || type === "engine.lifecycle.ready") return;
        const at = workflowEventAt(payload);
        const id = `global:${runId}:${type}:${at}:${Math.random().toString(16).slice(2, 8)}`;
        setRunEvents((prev) => [...prev.slice(-299), { id, source: "global", at, event: payload }]);
      } catch {
        return;
      }
    },
    { enabled: !!selectedRunId }
  );

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    renderIcons(root);
  }, [
    automations.length,
    automationsV2.length,
    runs.length,
    workflowRuns.length,
    packs.length,
    activeRuns.length,
    failedRuns.length,
    !!editDraft,
    !!selectedRunId,
    !!selectedSessionId,
    !!selectedBoardTask,
    runEvents.length,
    sessionEvents.length,
    updateAutomationMutation.isPending,
    runActionMutation.isPending,
    runNowMutation.isPending,
    runNowV2Mutation.isPending,
  ]);

  const statusColor = (status: string) => {
    const s = String(status || "").toLowerCase();
    if (s === "active" || s === "completed" || s === "done") return "tcp-badge-ok";
    if (s === "running" || s === "in_progress") return "tcp-badge-warn";
    if (s === "blocked") return "border border-emerald-400/60 bg-emerald-400/10 text-emerald-200";
    if (s === "failed" || s === "error") return "tcp-badge-err";
    return "tcp-badge-info";
  };

  const beginEdit = (automation: any) => {
    const automationId = String(
      automation?.automation_id || automation?.id || automation?.routine_id || ""
    ).trim();
    if (!automationId) {
      toast("err", "Cannot edit automation without an id.");
      return;
    }
    const scheduleEditor = scheduleToEditor(automation?.schedule);
    setEditDraft({
      automationId,
      name: String(automation?.name || automationId || "").trim(),
      objective: String(
        automation?.mission?.objective || automation?.mission_snapshot?.objective || ""
      ).trim(),
      mode:
        String(automation?.mode || "").toLowerCase() === "standalone"
          ? "standalone"
          : "orchestrated",
      requiresApproval:
        automation?.requires_approval === true ||
        automation?.policy?.approval?.requires_approval === true,
      scheduleKind: scheduleEditor.scheduleKind === "cron" ? "cron" : "interval",
      cronExpression: scheduleEditor.cronExpression,
      intervalSeconds: String(scheduleEditor.intervalSeconds),
    });
  };

  const isPausedAutomation = (automation: any) => {
    const status = String(automation?.status || "")
      .trim()
      .toLowerCase();
    return status === "paused" || status === "disabled";
  };

  const openCalendarAutomationEdit = (automation: any) => {
    if (!automation) return;
    if (isMissionBlueprintAutomation(automation)) {
      onOpenAdvancedEdit(automation);
      return;
    }
    const family = getAutomationCalendarFamily(automation);
    if (family === "legacy") {
      beginEdit(automation);
      return;
    }
    const draft = workflowAutomationToEditDraft(automation);
    if (!draft) {
      toast("err", "Cannot open this workflow automation for editing.");
      return;
    }
    setWorkflowEditDraft(draft);
  };

  const updateCalendarAutomationFromEvent = async (info: any) => {
    const event = info?.event;
    const automation = event?.extendedProps?.automation;
    const family =
      String(event?.extendedProps?.family || "legacy").trim() === "v2" ? "v2" : "legacy";
    const cronExpression = String(event?.extendedProps?.cronExpression || "").trim();
    const start = event?.start ? new Date(event.start) : null;
    const nextCron = start ? rewriteCronForDroppedStart(cronExpression, start) : null;
    if (!automation || !start || !nextCron) {
      info?.revert?.();
      toast("info", "That schedule cannot be moved from the calendar yet.");
      return;
    }
    try {
      if (family === "legacy") {
        const automationId = String(
          automation?.automation_id || automation?.id || automation?.routine_id || ""
        ).trim();
        const scheduleEditor = scheduleToEditor(automation?.schedule);
        await updateAutomationMutation.mutateAsync({
          automationId,
          name: String(automation?.name || automationId || "").trim(),
          objective: String(
            automation?.mission?.objective || automation?.mission_snapshot?.objective || ""
          ).trim(),
          mode:
            String(automation?.mode || "").toLowerCase() === "standalone"
              ? "standalone"
              : "orchestrated",
          requiresApproval:
            automation?.requires_approval === true ||
            automation?.policy?.approval?.requires_approval === true,
          scheduleKind: "cron",
          cronExpression: nextCron,
          intervalSeconds: String(scheduleEditor.intervalSeconds || 3600),
        });
        return;
      }
      const draft = workflowAutomationToEditDraft(automation);
      if (!draft) {
        throw new Error("Workflow automation draft could not be created.");
      }
      await updateWorkflowAutomationMutation.mutateAsync({
        ...draft,
        scheduleKind: "cron",
        cronExpression: nextCron,
        intervalSeconds: draft.intervalSeconds || "3600",
      });
    } catch (error) {
      info?.revert?.();
      toast("err", error instanceof Error ? error.message : String(error));
    }
  };

  const legacyAutomationCount = automations.length;
  const workflowAutomationCount = automationsV2.length;
  const totalSavedAutomations = legacyAutomationCount + workflowAutomationCount;
  const blockers = useMemo(
    () => buildRunBlockers(selectedRun, sessionEvents, runEvents),
    [selectedRun, sessionEvents, runEvents]
  );
  const sessionLogEntries = useMemo(() => {
    const messageEntries = sessionMessages.map(({ sessionId, message }: any, index: number) => ({
      id: `message:${sessionId}:${sessionMessageId(message, index)}`,
      kind: "message" as const,
      sessionId,
      at: sessionMessageCreatedAt(message),
      variant: sessionMessageVariant(message),
      label: String(message?.info?.role || "session").trim() || "session",
      body: sessionMessageText(message),
      raw: message,
      parts: sessionMessageParts(message),
      sessionLabel: sessionLabel(sessionId),
    }));
    const liveEntries = workflowSessionLogEventEntries(sessionEvents, selectedSessionId).map(
      (entry) => ({
        ...entry,
        sessionLabel: sessionLabel(workflowEventSessionId(entry.raw, selectedSessionId)),
      })
    );
    const rows = [...messageEntries, ...liveEntries].sort((a, b) => a.at - b.at);
    if (selectedSessionFilterId === "all") return rows;
    return rows.filter((entry) => entry.sessionId === selectedSessionFilterId);
  }, [selectedSessionFilterId, selectedSessionId, sessionMessages, sessionEvents]);

  useEffect(() => {
    const el = sessionLogRef.current;
    if (!el || !sessionLogPinnedToBottom) return;
    el.scrollTop = el.scrollHeight;
  }, [sessionLogEntries, sessionLogPinnedToBottom]);

  return (
    <div ref={rootRef} className="grid gap-4">
      {viewMode === "calendar" ? (
        <AutomationCalendar
          events={calendarEvents}
          onRangeChange={setCalendarRange}
          onOpenAutomation={openCalendarAutomationEdit}
          onRunAutomation={(
            automation: any,
            family: "legacy" | "v2",
            opts?: { dryRun?: boolean }
          ) => {
            const automationId = String(
              automation?.automation_id || automation?.automationId || automation?.id || ""
            ).trim();
            if (!automationId) return;
            if (opts?.dryRun) {
              runNowV2Mutation.mutate({ id: automationId, dryRun: true });
              return;
            }
            if (family === "v2") {
              runNowV2Mutation.mutate({ id: automationId });
              return;
            }
            runNowMutation.mutate(automationId);
          }}
          onEventDrop={updateCalendarAutomationFromEvent}
          statusColor={statusColor}
          runActionsDisabled={runNowMutation.isPending || runNowV2Mutation.isPending}
        />
      ) : null}

      {viewMode === "list" ? (
        <div className="space-y-4 mb-4">
          <div className="flex items-center justify-between gap-2">
            <p className="text-[11px] font-medium uppercase tracking-[0.24em] text-slate-500">
              Workflow Automations
            </p>
            <span className="tcp-badge-ghost text-xs tracking-wide">
              {workflowAutomationCount} items
            </span>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
            {automationsV2.length > 0 ? (
              automationsV2.map((automation: any) => {
                const id = String(
                  automation?.automation_id || automation?.automationId || ""
                ).trim();
                const status = String(automation?.status || "draft").trim();
                const paused = status.toLowerCase() === "paused";
                const standup = isStandupAutomation(automation);
                return (
                  <div key={id} className="tcp-card flex flex-col gap-3 group">
                    <div className="flex items-start justify-between gap-2">
                      <div className="flex items-center gap-2.5 min-w-0">
                        <span className="text-xl">🧩</span>
                        <div className="min-w-0">
                          <strong className="block truncate text-sm font-bold tracking-tight text-white mb-0.5">
                            {String(automation?.name || id || "Workflow automation")}
                          </strong>
                          {standup ? (
                            <span className="tcp-badge-ok text-[10px] py-0 px-1.5">Standup</span>
                          ) : null}
                        </div>
                      </div>
                      <div className="flex items-center gap-1.5 shrink-0">
                        <button
                          className="tcp-icon-btn h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity"
                          onClick={() => {
                            if (isMissionBlueprintAutomation(automation)) {
                              onOpenAdvancedEdit(automation);
                              return;
                            }
                            setWorkflowEditDraft(workflowAutomationToEditDraft(automation));
                          }}
                          disabled={!id}
                          title="Edit workflow automation"
                          aria-label="Edit workflow automation"
                        >
                          <i data-lucide="pencil" className="w-3.5 h-3.5"></i>
                        </button>
                        <span
                          className={`text-[10px] font-bold uppercase tracking-wider px-1.5 py-0.5 rounded ${statusColor(status)}`}
                        >
                          {status}
                        </span>
                      </div>
                    </div>

                    {String(automation?.description || "").trim() ? (
                      <div className="tcp-subtle text-xs line-clamp-2 leading-relaxed">
                        {String(automation.description)}
                      </div>
                    ) : (
                      <div className="tcp-subtle text-xs italic opacity-40">
                        No description provided
                      </div>
                    )}

                    {standup ? (
                      <div className="text-[10px] text-emerald-300/80 font-mono tracking-tight bg-emerald-500/10 p-1.5 rounded-md truncate">
                        report: {String(automation?.metadata?.standup?.report_path_template || "")}
                      </div>
                    ) : null}

                    <div className="tcp-subtle text-[11px] font-medium flex items-center gap-1.5">
                      <i data-lucide="calendar" className="w-3 h-3"></i>
                      {formatAutomationV2ScheduleLabel(automation?.schedule)}
                    </div>

                    <div className="mt-auto pt-3 flex flex-wrap gap-2 border-t border-white/5">
                      <button
                        className="tcp-btn-primary flex-1 h-8 px-2 text-[11px]"
                        onClick={() => runNowV2Mutation.mutate({ id })}
                        disabled={!id || runNowV2Mutation.isPending}
                      >
                        <i data-lucide="play" className="w-3 h-3"></i>
                        {runNowV2Mutation.isPending ? "Starting..." : "Run"}
                      </button>
                      <button
                        className="tcp-btn h-8 px-2 text-[11px]"
                        onClick={() => runNowV2Mutation.mutate({ id, dryRun: true })}
                        disabled={!id || runNowV2Mutation.isPending}
                      >
                        <i data-lucide="flask-conical" className="w-3 h-3"></i>
                        Dry
                      </button>
                      <button
                        className="tcp-btn h-8 px-2 text-[11px]"
                        onClick={() =>
                          automationActionMutation.mutate({
                            action: paused ? "resume" : "pause",
                            automationId: id,
                            family: "v2",
                          })
                        }
                        disabled={!id || automationActionMutation.isPending}
                      >
                        <i data-lucide={paused ? "play" : "pause"} className="w-3 h-3"></i>
                        {paused ? "Resume" : "Pause"}
                      </button>
                      <button
                        className="tcp-btn-danger h-8 w-8 px-0 flex items-center justify-center"
                        onClick={() =>
                          setDeleteConfirm({
                            automationId: id,
                            family: "v2",
                            title: String(automation?.name || id || "workflow automation"),
                          })
                        }
                        disabled={!id || automationActionMutation.isPending}
                        title="Remove"
                      >
                        <i data-lucide="trash-2" className="w-3.5 h-3.5"></i>
                      </button>
                    </div>
                  </div>
                );
              })
            ) : (
              <div className="tcp-list-item">
                <div className="font-medium">No workflow automations saved yet</div>
                <div className="tcp-subtle mt-1 text-xs">
                  This section is separate from run history and only shows workflow automation
                  definitions.
                </div>
              </div>
            )}
          </div>
        </div>
      ) : null}

      {viewMode === "list" ? (
        <div className="grid gap-2 mb-4">
          <div className="flex items-center justify-between gap-2">
            <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
              Saved Automations
            </p>
            <span className="tcp-badge-info">{totalSavedAutomations} saved</span>
          </div>

          <div className="grid gap-2">
            <div className="flex items-center justify-between gap-2">
              <p className="text-[11px] font-medium uppercase tracking-[0.24em] text-slate-500">
                Scheduled Automations
              </p>
              <span className="tcp-subtle text-xs">{legacyAutomationCount} items</span>
            </div>
            {automations.length > 0 ? (
              automations.map((automation: any) => {
                const id = String(
                  automation?.automation_id || automation?.id || automation?.routine_id || ""
                );
                return (
                  <div key={id} className="tcp-list-item">
                    <div className="mb-1 flex items-center justify-between gap-2">
                      <div className="flex items-center gap-2">
                        <span>⏰</span>
                        <strong>{String(automation?.name || id || "Automation")}</strong>
                      </div>
                      <div className="flex items-center gap-2">
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() => beginEdit(automation)}
                        >
                          <i data-lucide="pencil"></i>
                        </button>
                        <span className={statusColor(automation?.status)}>
                          {String(automation?.status || "active")}
                        </span>
                      </div>
                    </div>
                    <div className="tcp-subtle text-xs">
                      {formatScheduleLabel(automation?.schedule)}
                    </div>
                    <div className="mt-2">
                      <div className="flex flex-wrap gap-2">
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() => runNowMutation.mutate(id)}
                        >
                          <i data-lucide="play"></i>
                          Run now
                        </button>
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() =>
                            automationActionMutation.mutate({
                              action: isPausedAutomation(automation) ? "resume" : "pause",
                              automationId: id,
                              family: "legacy",
                            })
                          }
                          disabled={!id || automationActionMutation.isPending}
                        >
                          <i data-lucide={isPausedAutomation(automation) ? "play" : "pause"}></i>
                          {isPausedAutomation(automation) ? "Resume" : "Pause"}
                        </button>
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() => {
                            const latestForAutomation = runs.find((run: any) => {
                              const automationId = String(
                                run?.automation_id || run?.routine_id || run?.id || ""
                              ).trim();
                              return automationId === id;
                            });
                            const runId = String(
                              latestForAutomation?.run_id || latestForAutomation?.id || ""
                            ).trim();
                            if (runId) {
                              onSelectRunId(runId);
                              onOpenRunningView();
                            } else {
                              toast("info", "No runs yet for this automation.");
                            }
                          }}
                        >
                          <i data-lucide="info"></i>
                          Debug latest
                        </button>
                        <button
                          className="tcp-btn-danger h-7 px-2 text-xs"
                          onClick={() =>
                            setDeleteConfirm({
                              automationId: id,
                              family: "legacy",
                              title: String(
                                automation?.name || automation?.label || id || "automation"
                              ),
                            })
                          }
                          disabled={!id || automationActionMutation.isPending}
                        >
                          <i data-lucide="trash-2"></i>
                          Remove
                        </button>
                      </div>
                    </div>
                  </div>
                );
              })
            ) : (
              <div className="tcp-list-item">
                <div className="font-medium">No scheduled automations saved yet</div>
                <div className="tcp-subtle mt-1 text-xs">
                  This section shows automation definitions, not execution history.
                </div>
              </div>
            )}
          </div>
        </div>
      ) : null}

      {/* Installed packs from pack_builder */}
      {viewMode === "list" && packs.length > 0 ? (
        <div className="mt-12 pt-8 border-t border-white/5 opacity-60 hover:opacity-100 transition-opacity">
          <p className="text-[10px] text-slate-500 uppercase tracking-widest font-bold mb-3">
            System: Installed Packs
          </p>
          <div className="grid gap-2">
            {packs.map((pack: any, i: number) => (
              <div key={String(pack?.id || pack?.name || i)} className="tcp-list-item py-2">
                <div className="flex items-center justify-between gap-2">
                  <div className="flex items-center gap-2">
                    <span className="text-sm opacity-70">📦</span>
                    <strong className="text-xs">{String(pack?.name || pack?.id || "Pack")}</strong>
                  </div>
                  <span className="text-[10px] text-slate-500">
                    {String(pack?.version || "1.0.0")}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </div>
      ) : null}

      {viewMode === "running" ? (
        activeRuns.length > 0 ? (
          <div className="grid gap-2">
            <div className="flex items-center justify-between gap-2">
              <div className="grid gap-1">
                <p className="text-xs font-medium uppercase tracking-wide text-slate-500">
                  Active Running Tasks
                </p>
                <div className="flex flex-wrap gap-2">
                  <span className="tcp-badge-warn">{workflowQueueCounts.active} active</span>
                  <span className="tcp-badge-info">
                    {workflowQueueCounts.queuedCapacity} queued for capacity
                  </span>
                  <span className="tcp-badge-info">
                    {workflowQueueCounts.queuedWorkspaceLock} queued for workspace lock
                  </span>
                  {workflowQueueCounts.queuedOther > 0 ? (
                    <span className="tcp-badge-info">
                      {workflowQueueCounts.queuedOther} other queued
                    </span>
                  ) : null}
                </div>
              </div>
            </div>
            {activeRuns.slice(0, 14).map((run: any, index: number) => {
              const runId = String(run?.run_id || run?.id || index).trim();
              const runStatus = workflowStatusDisplay(run);
              const startedAt =
                run?.started_at_ms || run?.startedAtMs || run?.created_at_ms || run?.createdAtMs;
              const runStatusDetail = workflowStatusSubtleDetail(run);
              return (
                <div key={runId || index} className="tcp-list-item">
                  <div className="flex items-center justify-between gap-2">
                    <div className="grid gap-0.5">
                      <span className="font-medium text-sm">{runDisplayTitle(run)}</span>
                      <span className="tcp-subtle text-xs">
                        {runId || "unknown run"} · running for {runTimeLabel(run)}
                      </span>
                      {formatRunDateTime(startedAt) ? (
                        <span className="tcp-subtle text-xs">
                          Started: {formatRunDateTime(startedAt)}
                        </span>
                      ) : null}
                      {runObjectiveText(run) ? (
                        <span className="text-xs text-slate-400">
                          {shortText(runObjectiveText(run), 160)}
                        </span>
                      ) : null}
                      {runStatusDetail ? (
                        <span className="tcp-subtle text-xs">{runStatusDetail}</span>
                      ) : null}
                    </div>
                    <span className={statusColor(runStatus)}>{runStatus || "unknown"}</span>
                  </div>
                  <div className="mt-2 flex flex-wrap gap-2">
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() => onSelectRunId(runId)}
                    >
                      <i data-lucide="bug"></i>
                      Inspect
                    </button>
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() =>
                        runActionMutation.mutate({
                          action: "pause",
                          runId,
                          family: runId.startsWith("automation-v2-run-") ? "v2" : "legacy",
                        })
                      }
                      disabled={!runId || runActionMutation.isPending}
                    >
                      <i data-lucide="pause"></i>
                      Pause
                    </button>
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() =>
                        runActionMutation.mutate({
                          action: "resume",
                          runId,
                          family: runId.startsWith("automation-v2-run-") ? "v2" : "legacy",
                        })
                      }
                      disabled={!runId || runActionMutation.isPending}
                    >
                      <i data-lucide="play"></i>
                      Resume
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="tcp-list-item">
            <div className="font-medium">Active Running Tasks</div>
            <div className="tcp-subtle mt-1 text-xs">
              No active runs right now. Start a run to inspect live task execution.
            </div>
          </div>
        )
      ) : null}

      {viewMode === "running" && failedRuns.length > 0 ? (
        <div className="grid gap-2">
          <div className="flex items-center justify-between gap-2">
            <p className="text-xs font-medium uppercase tracking-wide text-slate-500">
              Recently Blocked Or Failed Runs
            </p>
            <span className="tcp-badge-err">{failedRuns.length} issues</span>
          </div>
          {failedRuns.slice(0, 10).map((run: any, index: number) => {
            const runId = String(run?.run_id || run?.id || index).trim();
            const runStatus = workflowStatusDisplay(run);
            const runStatusDetail = workflowStatusSubtleDetail(run);
            return (
              <div key={`failed-${runId || index}`} className="tcp-list-item">
                <div className="flex items-center justify-between gap-2">
                  <div className="grid gap-0.5">
                    <span className="font-medium text-sm">{runDisplayTitle(run)}</span>
                    <span className="tcp-subtle text-xs">{runId || "unknown run"}</span>
                    {formatRunDateTime(
                      run?.finished_at_ms ||
                        run?.finishedAtMs ||
                        run?.updated_at_ms ||
                        run?.updatedAtMs
                    ) ? (
                      <span className="tcp-subtle text-xs">
                        Finished:{" "}
                        {formatRunDateTime(
                          run?.finished_at_ms ||
                            run?.finishedAtMs ||
                            run?.updated_at_ms ||
                            run?.updatedAtMs
                        )}
                      </span>
                    ) : null}
                    {runObjectiveText(run) ? (
                      <span className="text-xs text-slate-400">
                        {shortText(runObjectiveText(run), 160)}
                      </span>
                    ) : null}
                    {runStatusDetail ? (
                      <span className="tcp-subtle text-xs">{runStatusDetail}</span>
                    ) : null}
                  </div>
                  <div className="flex items-center gap-2">
                    <span className={statusColor(runStatus)}>{runStatus || "failed"}</span>
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() => onSelectRunId(runId)}
                    >
                      <i data-lucide="bug"></i>
                      Inspect
                    </button>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      ) : null}

      {/* Recent run history */}
      {runs.length > 0 && viewMode === "running" ? (
        <div className="grid gap-2">
          <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
            {viewMode === "running" ? "Run Log Explorer" : "Recent Runs"}
          </p>
          {runs.slice(0, 12).map((run: any, index: number) => (
            <div key={String(run?.run_id || run?.id || index)} className="tcp-list-item">
              <div className="flex items-center justify-between gap-2">
                <span className="font-medium text-sm">{runDisplayTitle(run)}</span>
                <span className={statusColor(workflowStatusDisplay(run))}>
                  {workflowStatusDisplay(run) || "unknown"}
                </span>
              </div>
              <div className="mt-1 flex items-center justify-between gap-2">
                <div className="grid gap-0.5">
                  <span className="tcp-subtle text-xs">{String(run?.run_id || run?.id || "")}</span>
                  {formatRunDateTime(
                    run?.started_at_ms || run?.startedAtMs || run?.created_at_ms || run?.createdAtMs
                  ) ? (
                    <span className="tcp-subtle text-xs">
                      Started:{" "}
                      {formatRunDateTime(
                        run?.started_at_ms ||
                          run?.startedAtMs ||
                          run?.created_at_ms ||
                          run?.createdAtMs
                      )}
                    </span>
                  ) : null}
                  {run?.finished_at_ms || run?.finishedAtMs ? (
                    <span className="tcp-subtle text-xs">
                      Finished: {formatRunDateTime(run?.finished_at_ms || run?.finishedAtMs)}
                    </span>
                  ) : null}
                  {runObjectiveText(run) ? (
                    <span className="text-xs text-slate-400">
                      {shortText(runObjectiveText(run), 160)}
                    </span>
                  ) : null}
                  {workflowStatusSubtleDetail(run) ? (
                    <span className="tcp-subtle text-xs">{workflowStatusSubtleDetail(run)}</span>
                  ) : null}
                </div>
                <button
                  className="tcp-btn h-7 px-2 text-xs"
                  onClick={() => {
                    onSelectRunId(String(run?.run_id || run?.id || "").trim());
                    onOpenRunningView();
                  }}
                >
                  <i data-lucide="info"></i>
                  {viewMode === "running" ? "Logs" : "Details"}
                </button>
              </div>
            </div>
          ))}
        </div>
      ) : null}

      {!runs.length && viewMode === "running" ? (
        <EmptyState text="Run one automation, then use Logs to inspect full execution events." />
      ) : null}
      {!totalSavedAutomations && !packs.length && !runs.length && viewMode === "list" ? (
        <EmptyState text="No automations yet. Create your first one with the wizard!" />
      ) : null}
      {selectedRunId ? (
        <motion.div
          className="tcp-confirm-overlay"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          onClick={() => onSelectRunId("")}
        >
          <motion.div
            className="tcp-confirm-dialog tcp-run-debugger-modal"
            initial={{ opacity: 0, y: 8, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 6, scale: 0.98 }}
            onClick={(event) => event.stopPropagation()}
          >
            <div className="mb-3 flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
              <div className="grid gap-1">
                <h3 className="tcp-confirm-title">Run Debugger</h3>
                <div className="tcp-subtle text-xs">
                  automation:{" "}
                  {String(selectedRun?.automation_id || selectedRun?.routine_id || "unknown")}
                  {" · "}run: {selectedRunId}
                  {" · "}running for {runTimeLabel(selectedRun)}
                </div>
                {isWorkflowRun ? (
                  <div className="tcp-subtle text-xs">
                    completed nodes: {workflowCompletedNodeCount(selectedRun)}
                    {" · "}blocked nodes: {workflowBlockedNodeCount(selectedRun)}
                    {" · "}active sessions: {workflowActiveSessionCount(selectedRun)}
                  </div>
                ) : null}
              </div>
              <div className="flex w-full flex-col gap-2 sm:flex-row sm:flex-wrap sm:items-center lg:w-auto">
                <span className={statusColor(runStatus)}>{runStatus || "unknown"}</span>
                {runStatusDerivedFromBlockedNodes ? (
                  <span className="tcp-subtle">derived from blocked nodes</span>
                ) : null}
                {canContinueBlockedWorkflow ? (
                  <button
                    type="button"
                    className="tcp-btn h-8 w-full px-2 text-xs sm:w-auto"
                    onClick={() =>
                      workflowTaskContinueMutation.mutate({
                        runId: selectedRunId,
                        nodeId: continueBlockedNodeId,
                        reason: `continued blocked task ${continueBlockedNodeId} from run debugger`,
                      })
                    }
                    disabled={
                      !continueBlockedNodeId ||
                      workflowTaskContinueMutation.isPending ||
                      runActionMutation.isPending
                    }
                    title={
                      continueBlockedNodeId
                        ? `Continue blocked task ${continueBlockedNodeId} with minimal reset`
                        : "Select a blocked node to continue"
                    }
                  >
                    <i data-lucide="skip-forward"></i>
                    {workflowTaskContinueMutation.isPending ? "Continuing..." : "Continue"}
                  </button>
                ) : null}
                {canRecoverWorkflowRun ? (
                  <button
                    type="button"
                    className="tcp-btn h-8 w-full px-2 text-xs sm:w-auto"
                    onClick={() =>
                      runDebuggerRetryNodeId
                        ? workflowTaskRetryMutation.mutate({
                            runId: selectedRunId,
                            nodeId: runDebuggerRetryNodeId,
                            reason: `retried task ${runDebuggerRetryNodeId} from run debugger`,
                          })
                        : workflowRecoverMutation.mutate({
                            runId: selectedRunId,
                            reason: "retried from run debugger",
                          })
                    }
                    disabled={
                      !selectedRunId ||
                      workflowRecoverMutation.isPending ||
                      workflowTaskRetryMutation.isPending ||
                      runActionMutation.isPending
                    }
                    title={
                      runDebuggerRetryNodeId
                        ? `Retry selected task ${runDebuggerRetryNodeId}`
                        : "Retry the whole run"
                    }
                  >
                    <i data-lucide="rotate-ccw"></i>
                    {runDebuggerRetryNodeId
                      ? workflowTaskRetryMutation.isPending
                        ? "Retrying task..."
                        : "Retry Task"
                      : workflowRecoverMutation.isPending
                        ? "Retrying..."
                        : "Retry"}
                  </button>
                ) : null}
                {selectedRunId ? (
                  <button
                    type="button"
                    className="tcp-btn h-8 w-full px-2 text-xs sm:w-auto"
                    onClick={() =>
                      runActionMutation.mutate({
                        action: runStatus === "paused" ? "resume" : "pause",
                        runId: selectedRunId,
                        family: isWorkflowRun ? "v2" : "legacy",
                      })
                    }
                    disabled={
                      !selectedRunId ||
                      runActionMutation.isPending ||
                      !(runStatus === "paused" || isActiveRunStatus(runStatus))
                    }
                  >
                    <i data-lucide={runStatus === "paused" ? "play" : "pause"}></i>
                    {runStatus === "paused" ? "Resume" : "Pause"}
                  </button>
                ) : null}
                <button
                  className="tcp-btn h-8 w-full px-2 text-xs sm:w-auto"
                  onClick={() => {
                    void Promise.all([
                      queryClient.invalidateQueries({
                        queryKey: ["automations", "run", selectedRunId],
                      }),
                      queryClient.invalidateQueries({
                        queryKey: ["automations", "run", "artifacts", selectedRunId],
                      }),
                      selectedContextRunId
                        ? queryClient.invalidateQueries({
                            queryKey: ["automations", "run", "context", selectedContextRunId],
                          })
                        : Promise.resolve(),
                      selectedContextRunId
                        ? queryClient.invalidateQueries({
                            queryKey: [
                              "automations",
                              "run",
                              "context",
                              selectedContextRunId,
                              "blackboard",
                            ],
                          })
                        : Promise.resolve(),
                      selectedContextRunId
                        ? queryClient.invalidateQueries({
                            queryKey: [
                              "automations",
                              "run",
                              "context",
                              selectedContextRunId,
                              "events",
                            ],
                          })
                        : Promise.resolve(),
                      selectedContextRunId
                        ? queryClient.invalidateQueries({
                            queryKey: [
                              "automations",
                              "run",
                              "context",
                              selectedContextRunId,
                              "patches",
                            ],
                          })
                        : Promise.resolve(),
                      selectedRunId
                        ? queryClient.invalidateQueries({
                            queryKey: ["automations", "run", "session", selectedRunId],
                          })
                        : Promise.resolve(),
                    ]);
                  }}
                >
                  <i data-lucide="refresh-cw"></i>
                  Refresh
                </button>
                <button
                  className="tcp-btn h-8 w-full px-2 text-xs sm:w-auto"
                  onClick={() => {
                    setSelectedBoardTaskId("");
                    onSelectRunId("");
                  }}
                >
                  <i data-lucide="x"></i>
                  Close
                </button>
              </div>
            </div>
            <div className="flex-1 min-h-0 overflow-y-auto pr-1">
              <div className="grid min-h-full content-start gap-3">
                <WorkflowRunSummaryPanel runSummaryRows={runSummaryRows} />
                {isWorkflowRun ? (
                  <div className="tcp-list-item overflow-visible">
                    <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                      <div>
                        <div className="font-medium">Workflow Board</div>
                        <div className="tcp-subtle text-xs">
                          context run: {compactIdentifier(selectedContextRunId || "unlinked", 44)}
                          {" · "}tasks: {workflowProjection.tasks.length}
                          {" · "}artifacts: {runArtifacts.length}
                        </div>
                      </div>
                      <span className="tcp-badge-info">
                        {workflowProjection.taskSource === "hybrid"
                          ? "blackboard + context"
                          : workflowProjection.taskSource === "checkpoint"
                            ? "run checkpoint"
                            : workflowProjection.taskSource}
                      </span>
                    </div>
                    <TaskBoard
                      tasks={workflowProjection.tasks}
                      currentTaskId={workflowProjection.currentTaskId}
                      selectedTaskId={selectedBoardTaskId}
                      onTaskSelect={(task) =>
                        setSelectedBoardTaskId((current) => (current === task.id ? "" : task.id))
                      }
                    />
                  </div>
                ) : null}
                <div className="grid min-h-0 items-start gap-3 xl:grid-cols-[1.62fr_1fr]">
                  <div className="grid min-h-0 gap-3">
                    {selectedBoardTask ? (
                      <div
                        ref={boardDetailRef}
                        className="tcp-list-item relative max-h-[56vh] overflow-y-auto sm:max-h-[28rem]"
                      >
                        <div className="sticky -top-3 z-10 -mx-3 -mt-3 mb-2 flex items-center justify-between gap-2 rounded-t-xl border-b border-slate-800/80 bg-[color:color-mix(in_srgb,var(--color-surface-elevated)_96%,#000_4%)] px-3 py-3 backdrop-blur-sm">
                          <div className="font-medium">Task Details</div>
                          <button
                            type="button"
                            className="chat-icon-btn h-7 w-7"
                            aria-label="Close task details"
                            onClick={() => setSelectedBoardTaskId("")}
                          >
                            <i data-lucide="x-circle"></i>
                          </button>
                        </div>
                        <div className="grid gap-2 pr-1 text-sm text-slate-200">
                          <div className="whitespace-pre-wrap break-words font-medium leading-snug">
                            {selectedBoardTask.title}
                          </div>
                          {selectedBoardTask.description ? (
                            <div className="tcp-subtle whitespace-pre-wrap break-words">
                              {selectedBoardTask.description}
                            </div>
                          ) : null}
                          <div className="flex flex-wrap gap-2 text-xs">
                            <span className="tcp-badge-info">{selectedBoardTask.state}</span>
                            {selectedBoardTask.assigned_role ? (
                              <span className="tcp-badge-info">
                                agent: {selectedBoardTask.assigned_role}
                              </span>
                            ) : null}
                            {String((selectedBoardTask as any).task_kind || "").trim() ? (
                              <span className="tcp-badge-info">
                                task: {String((selectedBoardTask as any).task_kind).trim()}
                              </span>
                            ) : null}
                            {String((selectedBoardTask as any).backlog_task_id || "").trim() ? (
                              <span className="tcp-badge-info">
                                backlog: {String((selectedBoardTask as any).backlog_task_id).trim()}
                              </span>
                            ) : null}
                            {String((selectedBoardTask as any).task_owner || "").trim() ? (
                              <span className="tcp-badge-info">
                                owner: {String((selectedBoardTask as any).task_owner).trim()}
                              </span>
                            ) : null}
                            {selectedBoardTask.session_id ? (
                              <span className="tcp-badge-info">
                                {sessionLabel(selectedBoardTask.session_id)}
                              </span>
                            ) : null}
                          </div>
                          {selectedBoardTaskOutput ? (
                            <div className="flex flex-wrap gap-2 text-xs">
                              {String(selectedBoardTaskOutput?.status || "").trim() ? (
                                <span
                                  className={
                                    String(selectedBoardTaskOutput?.status || "")
                                      .trim()
                                      .toLowerCase() === "blocked"
                                      ? "tcp-badge-blocked"
                                      : selectedBoardTaskValidationOutcome ===
                                          "accepted_with_warnings"
                                        ? "tcp-badge-warn"
                                        : "tcp-badge-ok"
                                  }
                                >
                                  status:{" "}
                                  {selectedBoardTaskValidationOutcome === "accepted_with_warnings"
                                    ? "completed with warnings"
                                    : String(selectedBoardTaskOutput?.status || "").trim()}
                                </span>
                              ) : null}
                              {selectedBoardTaskWarningCount > 0 ? (
                                <span className="tcp-badge-warn">
                                  {selectedBoardTaskWarningCount} warning
                                  {selectedBoardTaskWarningCount === 1 ? "" : "s"}
                                </span>
                              ) : null}
                              {typeof selectedBoardTaskOutput?.approved === "boolean" ? (
                                <span
                                  className={
                                    selectedBoardTaskOutput.approved
                                      ? "tcp-badge-ok"
                                      : "tcp-badge-warn"
                                  }
                                >
                                  approved: {String(selectedBoardTaskOutput.approved)}
                                </span>
                              ) : null}
                              {selectedBoardTaskTelemetry?.workspace_inspection_used ? (
                                <span className="tcp-badge-info">workspace inspected</span>
                              ) : null}
                              {selectedBoardTaskTelemetry?.web_research_used ? (
                                <span className="tcp-badge-info">web research used</span>
                              ) : null}
                              {String(
                                selectedBoardTaskArtifactValidation?.rejected_artifact_reason || ""
                              ).trim() ? (
                                <span className="tcp-badge-warn">artifact rejected</span>
                              ) : null}
                            </div>
                          ) : null}
                          {!selectedBoardTaskIsWorkflowNode ? (
                            <div className="rounded-lg border border-slate-700/60 bg-slate-950/20 p-3 text-xs text-slate-300">
                              {selectedBoardTaskIsProjectedBacklogItem
                                ? "This is a projected backlog task derived from workflow output. You can claim or requeue it here without resetting the source workflow node."
                                : "This is a projected backlog task derived from workflow output, not a direct automation node. Inspect it here, but use the source workflow stage for retry, continue, or requeue actions."}
                            </div>
                          ) : null}
                          {selectedBoardTask.runtime_detail ? (
                            <div className="whitespace-pre-wrap break-words rounded-lg border border-slate-700/60 bg-slate-950/30 p-3 text-xs text-slate-300">
                              {selectedBoardTask.runtime_detail}
                            </div>
                          ) : null}
                          {selectedBoardTaskWorkflowClass ||
                          selectedBoardTaskPhase ||
                          selectedBoardTaskFailureKind ||
                          selectedBoardTaskWarningCount ||
                          selectedBoardTaskArtifactCandidates.length ? (
                            <div className="rounded-lg border border-slate-700/60 bg-slate-950/20 p-3 text-xs text-slate-300">
                              <div className="font-medium text-slate-200">Stability Contract</div>
                              <div className="mt-3 grid gap-2 sm:grid-cols-2">
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">workflow class</div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {selectedBoardTaskWorkflowClass || "n/a"}
                                  </div>
                                </div>
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">phase</div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {selectedBoardTaskPhase || "n/a"}
                                  </div>
                                </div>
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">failure kind</div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {selectedBoardTaskFailureKind || "n/a"}
                                  </div>
                                </div>
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">validation outcome</div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {selectedBoardTaskValidationOutcome || "n/a"}
                                  </div>
                                </div>
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">quality mode</div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {selectedBoardTaskQualityMode || "n/a"}
                                  </div>
                                </div>
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">rollback</div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {selectedBoardTaskEmergencyRollbackEnabled === null
                                      ? "n/a"
                                      : selectedBoardTaskEmergencyRollbackEnabled
                                        ? "enabled"
                                        : "disabled"}
                                  </div>
                                </div>
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">blocker category</div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {selectedBoardTaskBlockerCategory || "n/a"}
                                  </div>
                                </div>
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2 sm:col-span-2">
                                  <div className="tcp-subtle">validation basis</div>
                                  <div className="mt-1 whitespace-pre-wrap break-words font-medium text-slate-100">
                                    {selectedBoardTaskValidationBasis
                                      ? formatJson(selectedBoardTaskValidationBasis)
                                      : "n/a"}
                                  </div>
                                </div>
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2 sm:col-span-2">
                                  <div className="tcp-subtle">receipt ledger</div>
                                  <div className="mt-1 whitespace-pre-wrap break-words font-medium text-slate-100">
                                    {selectedBoardTaskReceiptLedger
                                      ? formatJson(selectedBoardTaskReceiptLedger)
                                      : "n/a"}
                                  </div>
                                </div>
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">artifact candidates</div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {selectedBoardTaskArtifactCandidates.length}
                                  </div>
                                </div>
                              </div>
                              {selectedBoardTaskWarningRequirements.length ? (
                                <div className="mt-3 rounded-md border border-amber-500/30 bg-amber-500/10 p-3">
                                  <div className="mb-2 font-medium text-amber-100">
                                    Non-blocking warnings
                                  </div>
                                  <div className="flex flex-wrap gap-2">
                                    {selectedBoardTaskWarningRequirements.map((item: string) => (
                                      <span key={item} className="tcp-badge-warn">
                                        {item.replace(/_/g, " ")}
                                      </span>
                                    ))}
                                  </div>
                                </div>
                              ) : null}
                              {selectedBoardTaskArtifactCandidates.length ? (
                                <div className="mt-3 grid gap-2">
                                  {selectedBoardTaskArtifactCandidates.map(
                                    (candidate: any, index: number) => (
                                      <div
                                        key={`${String(candidate?.source || "candidate")}-${index}`}
                                        className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2"
                                      >
                                        <div className="flex flex-wrap items-center gap-2">
                                          <span className="tcp-badge-info">
                                            {String(candidate?.source || "candidate")}
                                          </span>
                                          {candidate?.accepted ? (
                                            <span className="tcp-badge-ok">accepted</span>
                                          ) : null}
                                          {candidate?.substantive ? (
                                            <span className="tcp-badge-ok">substantive</span>
                                          ) : (
                                            <span className="tcp-badge-warn">non-substantive</span>
                                          )}
                                          {candidate?.placeholder_like ? (
                                            <span className="tcp-badge-warn">placeholder-like</span>
                                          ) : null}
                                        </div>
                                        <div className="mt-1 tcp-subtle">
                                          {Number(candidate?.length || 0)} chars
                                        </div>
                                      </div>
                                    )
                                  )}
                                </div>
                              ) : null}
                              {selectedBoardTaskReceiptTimeline.length ? (
                                <div className="mt-3 grid gap-2">
                                  <div className="tcp-subtle">receipt timeline</div>
                                  {selectedBoardTaskReceiptTimeline.map(
                                    (receipt: any, index: number) => (
                                      <div
                                        key={`${String(receipt?.seq || index)}:${String(
                                          receipt?.eventType || receipt?.event_type || ""
                                        )}`}
                                        className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2"
                                      >
                                        <div className="flex flex-wrap items-center gap-2">
                                          <span className="tcp-badge-info">
                                            seq {String(receipt?.seq || index + 1)}
                                          </span>
                                          {String(receipt?.eventType || "").trim() ? (
                                            <span className="tcp-badge-info">
                                              {String(receipt.eventType).trim()}
                                            </span>
                                          ) : null}
                                          {String(receipt?.receiptKind || "").trim() ? (
                                            <span className="tcp-badge-info">
                                              {String(receipt.receiptKind).trim()}
                                            </span>
                                          ) : null}
                                          {String(receipt?.attempt || "").trim() ? (
                                            <span className="tcp-badge-info">
                                              attempt {String(receipt.attempt).trim()}
                                            </span>
                                          ) : null}
                                          {Number(receipt?.at || 0) > 0 ? (
                                            <span className="tcp-subtle text-[11px]">
                                              {formatTimestampLabel(receipt.at)}
                                            </span>
                                          ) : null}
                                        </div>
                                        <div className="mt-1 text-slate-300">
                                          {String(receipt?.detail || "").trim() || "receipt"}
                                        </div>
                                        <details className="mt-2">
                                          <summary className="cursor-pointer text-xs text-slate-400">
                                            raw record
                                          </summary>
                                          <pre className="tcp-code mt-2 max-h-40 overflow-auto text-[11px]">
                                            {formatJson(receipt?.raw || receipt)}
                                          </pre>
                                        </details>
                                      </div>
                                    )
                                  )}
                                </div>
                              ) : null}
                              {selectedBoardTaskLifecycleEvents.length ? (
                                <div className="mt-3 grid gap-2">
                                  <div className="tcp-subtle">recent workflow events</div>
                                  {selectedBoardTaskLifecycleEvents.map(
                                    (summary: any, index: number) => {
                                      return (
                                        <div
                                          key={`${summary.event}-${String(summary.recordedAtMs || index)}`}
                                          className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2"
                                        >
                                          <div className="flex flex-wrap items-center gap-2">
                                            <span className="tcp-badge-info">{summary.event}</span>
                                            {summary.phase ? (
                                              <span className="tcp-badge-info">
                                                {summary.phase}
                                              </span>
                                            ) : null}
                                            {summary.failureKind ? (
                                              <span className="tcp-badge-warn">
                                                {summary.failureKind}
                                              </span>
                                            ) : null}
                                          </div>
                                          <div className="mt-1 text-slate-300">
                                            {summary.reason}
                                          </div>
                                        </div>
                                      );
                                    }
                                  )}
                                </div>
                              ) : null}
                            </div>
                          ) : null}
                          {String((selectedBoardTask as any).write_scope || "").trim() ||
                          String((selectedBoardTask as any).repo_root || "").trim() ||
                          String((selectedBoardTask as any).acceptance_criteria || "").trim() ||
                          String((selectedBoardTask as any).task_dependencies || "").trim() ||
                          String((selectedBoardTask as any).verification_state || "").trim() ||
                          String((selectedBoardTask as any).verification_command || "").trim() ||
                          String((selectedBoardTask as any).output_path || "").trim() ? (
                            <div className="rounded-lg border border-slate-700/60 bg-slate-950/20 p-3 text-xs text-slate-300">
                              <div className="font-medium text-slate-200">Coding Task Context</div>
                              <div className="mt-2 space-y-1">
                                <div>
                                  backlog task:{" "}
                                  {String(
                                    (selectedBoardTask as any).backlog_task_id || ""
                                  ).trim() || "n/a"}
                                </div>
                                <div>
                                  repo root:{" "}
                                  {String((selectedBoardTask as any).repo_root || "").trim() ||
                                    "n/a"}
                                </div>
                                <div>
                                  output path:{" "}
                                  {String((selectedBoardTask as any).output_path || "").trim() ||
                                    "n/a"}
                                </div>
                                <div>
                                  write scope:{" "}
                                  {String((selectedBoardTask as any).write_scope || "").trim() ||
                                    "n/a"}
                                </div>
                                <div>
                                  acceptance criteria:{" "}
                                  {String(
                                    (selectedBoardTask as any).acceptance_criteria || ""
                                  ).trim() || "n/a"}
                                </div>
                                <div>
                                  task dependencies:{" "}
                                  {String(
                                    (selectedBoardTask as any).task_dependencies || ""
                                  ).trim() || "n/a"}
                                </div>
                                <div>
                                  verification state:{" "}
                                  {String(
                                    (selectedBoardTask as any).verification_state || ""
                                  ).trim() || "n/a"}
                                </div>
                                <div>
                                  owner:{" "}
                                  {String((selectedBoardTask as any).task_owner || "").trim() ||
                                    "n/a"}
                                </div>
                                <div>
                                  lease owner:{" "}
                                  {String((selectedBoardTask as any).lease_owner || "").trim() ||
                                    "n/a"}
                                </div>
                                <div>
                                  lease expires:{" "}
                                  {selectedBoardTaskLeaseExpiresAtMs
                                    ? formatRunDateTime(selectedBoardTaskLeaseExpiresAtMs)
                                    : "n/a"}
                                </div>
                                <div>stale lease: {selectedBoardTaskIsStale ? "yes" : "no"}</div>
                                <div>
                                  verification:{" "}
                                  {String(
                                    (selectedBoardTask as any).verification_command || ""
                                  ).trim() || "n/a"}
                                </div>
                              </div>
                            </div>
                          ) : null}
                          {selectedBoardTaskResearchReadPaths.length ||
                          selectedBoardTaskDiscoveredRelevantPaths.length ||
                          selectedBoardTaskUnmetResearchRequirements.length ||
                          selectedBoardTaskArtifactValidation?.repair_attempted ? (
                            <div className="rounded-lg border border-slate-700/60 bg-slate-950/20 p-3 text-xs text-slate-300">
                              <div className="font-medium text-slate-200">
                                Research Requirement Status
                              </div>
                              <div className="mt-3 grid gap-2 sm:grid-cols-2">
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">discovered relevant files</div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {selectedBoardTaskDiscoveredRelevantPaths.length}
                                  </div>
                                </div>
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">actual read calls backed by path</div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {selectedBoardTaskResearchReadPaths.length}
                                  </div>
                                </div>
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">reviewed paths backed by read</div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {selectedBoardTaskReviewedPathsBackedByRead.length}
                                  </div>
                                </div>
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">web research</div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {selectedBoardTaskArtifactValidation?.web_research_attempted
                                      ? selectedBoardTaskArtifactValidation?.web_research_succeeded
                                        ? "attempted and succeeded"
                                        : "attempted but not successful"
                                      : "not attempted"}
                                  </div>
                                </div>
                              </div>
                              <div className="mt-3 grid gap-2 sm:grid-cols-2">
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">repair pass</div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {selectedBoardTaskArtifactValidation?.repair_attempted
                                      ? selectedBoardTaskArtifactValidation?.repair_succeeded
                                        ? "attempted and satisfied"
                                        : selectedBoardTaskArtifactValidation?.repair_exhausted
                                          ? "attempted and exhausted"
                                          : "attempted and still active"
                                      : "not needed or not attempted"}
                                  </div>
                                  {selectedBoardTaskArtifactValidation?.repair_attempted ? (
                                    <div className="mt-1 tcp-subtle">
                                      attempt{" "}
                                      {Number(
                                        selectedBoardTaskArtifactValidation?.repair_attempt || 0
                                      )}{" "}
                                      of{" "}
                                      {Number(
                                        selectedBoardTaskArtifactValidation?.repair_attempt || 0
                                      ) +
                                        Number(
                                          selectedBoardTaskArtifactValidation?.repair_attempts_remaining ||
                                            0
                                        )}
                                    </div>
                                  ) : null}
                                </div>
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">
                                    missing / unreviewed relevant files
                                  </div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {selectedBoardTaskUnreviewedRelevantPaths.length}
                                  </div>
                                </div>
                              </div>
                              {selectedBoardTaskUnmetResearchRequirements.length ? (
                                <div className="mt-3">
                                  <div className="tcp-subtle mb-1">unmet requirements</div>
                                  <div className="flex flex-wrap gap-1">
                                    {selectedBoardTaskUnmetResearchRequirements.map((item) => (
                                      <span
                                        key={item}
                                        className="rounded-full border border-emerald-500/30 bg-emerald-950/20 px-2 py-1 text-[11px] text-emerald-100/90"
                                      >
                                        {item}
                                      </span>
                                    ))}
                                  </div>
                                </div>
                              ) : null}
                              {selectedBoardTaskUnreviewedRelevantPaths.length ? (
                                <div className="mt-3">
                                  <div className="tcp-subtle mb-1">unreviewed relevant files</div>
                                  <div className="flex flex-wrap gap-1">
                                    {selectedBoardTaskUnreviewedRelevantPaths.map((path) => (
                                      <span
                                        key={path}
                                        className="rounded-full border border-slate-700/70 bg-slate-950/30 px-2 py-1 font-mono text-[11px] text-slate-300"
                                      >
                                        {path}
                                      </span>
                                    ))}
                                  </div>
                                </div>
                              ) : null}
                            </div>
                          ) : null}
                          {selectedBoardTaskOutput ||
                          selectedBoardTaskRelatedPaths.length ||
                          selectedBoardTaskFailureDetail ? (
                            <div className="rounded-lg border border-slate-700/60 bg-slate-950/20 p-3 text-xs text-slate-300">
                              <div className="font-medium text-slate-200">
                                Coding Verification & Failures
                              </div>
                              <div className="mt-3 grid gap-2 sm:grid-cols-2">
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">verification outcome</div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {selectedBoardTaskVerificationOutcome}
                                  </div>
                                </div>
                                <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle">verification passed</div>
                                  <div className="mt-1 font-medium text-slate-100">
                                    {typeof selectedBoardTaskVerificationPassed === "boolean"
                                      ? selectedBoardTaskVerificationPassed
                                        ? "yes"
                                        : "no"
                                      : "n/a"}
                                  </div>
                                </div>
                              </div>
                              {selectedBoardTaskTelemetry?.verification_expected ? (
                                <div className="mt-3 rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                  <div className="tcp-subtle mb-2">verification plan</div>
                                  <div className="mb-2 text-slate-200/80">
                                    {Number(
                                      selectedBoardTaskTelemetry?.verification_completed || 0
                                    )}{" "}
                                    / {Number(selectedBoardTaskTelemetry?.verification_total || 0)}{" "}
                                    checks ran
                                  </div>
                                  {selectedBoardTaskVerificationResults.length ? (
                                    <div className="grid gap-2">
                                      {selectedBoardTaskVerificationResults.map(
                                        (result: any, index: number) => (
                                          <div
                                            key={`${String(result?.command || index)}-${index}`}
                                            className="rounded-md border border-slate-800/80 bg-slate-950/40 p-2"
                                          >
                                            <div className="flex flex-wrap items-center gap-2">
                                              <span className="tcp-badge-info">
                                                {String(result?.kind || "verify")}
                                              </span>
                                              <span
                                                className={
                                                  result?.failed
                                                    ? "tcp-badge-warn"
                                                    : result?.ran
                                                      ? "tcp-badge-ok"
                                                      : "tcp-badge-info"
                                                }
                                              >
                                                {result?.failed
                                                  ? "failed"
                                                  : result?.ran
                                                    ? "passed"
                                                    : "not run"}
                                              </span>
                                            </div>
                                            <div className="mt-1 break-words font-mono text-[11px] text-slate-200">
                                              {String(result?.command || "").trim() || "n/a"}
                                            </div>
                                            {String(result?.failure || "").trim() ? (
                                              <div className="mt-1 whitespace-pre-wrap break-words text-[11px] text-emerald-100/90">
                                                {String(result?.failure || "").trim()}
                                              </div>
                                            ) : null}
                                          </div>
                                        )
                                      )}
                                    </div>
                                  ) : null}
                                </div>
                              ) : null}
                              {selectedBoardTaskFailureDetail ? (
                                <div className="mt-3 rounded-md border border-emerald-500/30 bg-emerald-950/20 p-2 text-emerald-100/90">
                                  <div className="tcp-subtle mb-1 text-emerald-100/70">
                                    failure detail
                                  </div>
                                  <div className="whitespace-pre-wrap break-words">
                                    {selectedBoardTaskFailureDetail}
                                  </div>
                                </div>
                              ) : null}
                              {selectedBoardTaskRelatedPaths.length ? (
                                <div className="mt-3">
                                  <div className="tcp-subtle mb-1">related artifacts</div>
                                  <div className="flex flex-wrap gap-2">
                                    {selectedBoardTaskRelatedPaths.map((path) => (
                                      <button
                                        key={path}
                                        type="button"
                                        className="rounded-full border border-slate-700/70 bg-slate-950/30 px-2 py-1 font-mono text-[11px] text-slate-200 transition hover:border-sky-500/40 hover:text-sky-100"
                                        onClick={() => focusArtifactEntry(path)}
                                        title={path}
                                      >
                                        Open {compactIdentifier(path, 44)}
                                      </button>
                                    ))}
                                  </div>
                                  {selectedBoardTaskRelatedArtifacts.length ? (
                                    <div className="mt-2 tcp-subtle">
                                      matched run artifacts:{" "}
                                      {selectedBoardTaskRelatedArtifacts
                                        .map((entry) => entry.name)
                                        .join(", ")}
                                    </div>
                                  ) : (
                                    <div className="mt-2 tcp-subtle">
                                      No matching run artifact found yet. The button will still jump
                                      to the artifacts section.
                                    </div>
                                  )}
                                </div>
                              ) : null}
                              <WorkflowTaskActionsPanel
                                selectedBoardTask={selectedBoardTask}
                                selectedBoardTaskIsWorkflowNode={selectedBoardTaskIsWorkflowNode}
                                selectedBoardTaskIsProjectedBacklogItem={
                                  selectedBoardTaskIsProjectedBacklogItem
                                }
                                selectedBoardTaskNodeId={selectedBoardTaskNodeId}
                                selectedBoardTaskStateNormalized={selectedBoardTaskStateNormalized}
                                selectedBoardTaskImpactSummary={selectedBoardTaskImpactSummary}
                                selectedBoardTaskResetOutputPaths={
                                  selectedBoardTaskResetOutputPaths
                                }
                                selectedRunId={selectedRunId}
                                canTaskContinue={canTaskContinue}
                                canTaskRetry={canTaskRetry}
                                canTaskRequeue={canTaskRequeue}
                                canBacklogTaskClaim={canBacklogTaskClaim}
                                canBacklogTaskRequeue={canBacklogTaskRequeue}
                                canRecoverWorkflowRun={canRecoverWorkflowRun}
                                continueBlockedNodeId={continueBlockedNodeId}
                                backlogTaskClaimMutation={backlogTaskClaimMutation}
                                backlogTaskRequeueMutation={backlogTaskRequeueMutation}
                                workflowTaskContinueMutation={workflowTaskContinueMutation}
                                workflowTaskRetryMutation={workflowTaskRetryMutation}
                                workflowTaskRequeueMutation={workflowTaskRequeueMutation}
                                workflowRepairMutation={workflowRepairMutation}
                                workflowRecoverMutation={workflowRecoverMutation}
                                taskResetPreviewQuery={taskResetPreviewQuery}
                              />
                            </div>
                          ) : null}
                          <WorkflowTaskSignalsPanel
                            selectedBoardTask={selectedBoardTask}
                            selectedBoardTaskTelemetry={selectedBoardTaskTelemetry}
                            selectedBoardTaskArtifactValidation={
                              selectedBoardTaskArtifactValidation
                            }
                            selectedBoardTaskTouchedFiles={selectedBoardTaskTouchedFiles}
                            selectedBoardTaskUndeclaredFiles={selectedBoardTaskUndeclaredFiles}
                            selectedBoardTaskRequestedQualityMode={
                              selectedBoardTaskRequestedQualityMode
                            }
                            selectedBoardTaskEmergencyRollbackEnabled={
                              selectedBoardTaskEmergencyRollbackEnabled
                            }
                            selectedBoardTaskBlockerCategory={selectedBoardTaskBlockerCategory}
                            selectedBoardTaskValidationBasis={selectedBoardTaskValidationBasis}
                            selectedBoardTaskReceiptTimeline={selectedBoardTaskReceiptTimeline}
                          />
                          {selectedBoardTask.error_message ? (
                            <div className="whitespace-pre-wrap break-words rounded-lg border border-rose-500/30 bg-rose-950/20 p-3 text-xs text-rose-200">
                              {selectedBoardTask.error_message}
                            </div>
                          ) : null}
                          {selectedBoardTask.dependencies.length ? (
                            <div className="flex flex-wrap gap-1 text-xs">
                              {selectedBoardTask.dependencies.map((dep) => (
                                <span key={dep} className="tcp-badge-info">
                                  depends on {dep}
                                </span>
                              ))}
                            </div>
                          ) : null}
                        </div>
                      </div>
                    ) : null}
                    <WorkflowLiveSessionLogPanel
                      selectedSessionId={selectedSessionId}
                      selectedSessionFilterId={selectedSessionFilterId}
                      availableSessionIds={availableSessionIds}
                      sessionLogEntries={sessionLogEntries}
                      sessionLogRef={sessionLogRef}
                      compactIdentifier={compactIdentifier}
                      sessionLabel={sessionLabel}
                      onSessionFilterChange={setSelectedSessionFilterId}
                      onCopySessionLog={async () => {
                        try {
                          await navigator.clipboard.writeText(
                            sessionLogEntries
                              .map((entry) => {
                                const ts = new Date(entry.at).toLocaleTimeString();
                                const sessionTag = entry.sessionId
                                  ? ` · ${entry.sessionLabel}`
                                  : "";
                                return `[${ts}] ${entry.label}${sessionTag}\n${entry.body || formatJson(entry.raw)}`;
                              })
                              .join("\n\n")
                          );
                          toast("ok", "Copied session log.");
                        } catch (error) {
                          toast("err", error instanceof Error ? error.message : "Copy failed.");
                        }
                      }}
                      onJumpToLatest={() => {
                        setSessionLogPinnedToBottom(true);
                        const el = sessionLogRef.current;
                        if (el) el.scrollTop = el.scrollHeight;
                      }}
                      onPinnedStateChange={setSessionLogPinnedToBottom}
                    />
                    <WorkflowRunTelemetryPanel
                      selectedLogSource={selectedLogSource}
                      telemetryEvents={telemetryEvents}
                      isWorkflowRun={isWorkflowRun}
                      filteredRunEventEntries={filteredRunEventEntries}
                      formatTimestampLabel={formatTimestampLabel}
                      onSelectLogSource={setSelectedLogSource}
                    />
                  </div>
                  <div className="grid min-h-0 content-start gap-3 overflow-visible">
                    <WorkflowBlockersPanel blockers={blockers} />
                    <WorkflowDebugHintsPanel runHints={runHints} />
                    <WorkflowRequiredActionsPanel
                      runRepairGuidanceEntries={runRepairGuidanceEntries}
                      isWorkflowRun={isWorkflowRun}
                      selectedRunId={selectedRunId}
                      hasActiveSessions={workflowActiveSessionCount(selectedRun) > 0}
                      onFocusNode={(nodeId) => setSelectedBoardTaskId(`node-${nodeId}`)}
                      workflowTaskRetryMutation={workflowTaskRetryMutation}
                      workflowTaskContinueMutation={workflowTaskContinueMutation}
                      workflowTaskRequeueMutation={workflowTaskRequeueMutation}
                    />
                    <WorkflowMissionObjectivePanel
                      objective={String(selectedRun?.mission_snapshot?.objective || "n/a")}
                    />
                    <div ref={artifactsSectionRef}>
                      <WorkflowArtifactsPanel
                        artifactCount={runArtifacts.length}
                        artifactEntries={runArtifactEntries}
                        selectedArtifactKey={selectedRunArtifactKey}
                        isWorkflowRun={isWorkflowRun}
                        onToggleArtifact={(key) =>
                          setSelectedRunArtifactKey((current) => (current === key ? "" : key))
                        }
                      />
                    </div>
                    <div className="tcp-list-item min-h-0">
                      <div className="font-medium">
                        {isWorkflowRun ? "Run History" : "Persisted History"}
                      </div>
                      {runHistoryEvents.length ? (
                        <div className="mt-2 grid gap-2 overflow-auto pr-1 sm:max-h-[14rem]">
                          {runHistoryEvents.map((event: any, index: number) => (
                            <details
                              key={`${String(event?.id || event?.event || event?.type || "history")}-${index}`}
                              className="rounded-lg border border-slate-700/40 bg-slate-900/25 p-2"
                            >
                              <summary className="cursor-pointer list-none">
                                <div className="flex items-center justify-between gap-2">
                                  <span className="text-xs font-medium text-slate-200">
                                    {String(
                                      event?.type || event?.event || event?.status || "history"
                                    )}
                                  </span>
                                  <span className="tcp-subtle text-[11px]">
                                    {formatTimestampLabel(
                                      event?.ts_ms ||
                                        event?.tsMs ||
                                        event?.at ||
                                        event?.timestamp_ms
                                    )}
                                  </span>
                                </div>
                                <div className="tcp-subtle mt-1 text-xs">
                                  {String(
                                    event?.detail ||
                                      event?.reason ||
                                      event?.family ||
                                      event?.status ||
                                      "No summary available."
                                  )}
                                </div>
                              </summary>
                              <pre className="tcp-code mt-2 max-h-32 overflow-auto text-[11px]">
                                {formatJson(event)}
                              </pre>
                            </details>
                          ))}
                        </div>
                      ) : (
                        <div className="tcp-subtle mt-2 text-xs">
                          {isWorkflowRun
                            ? "No context-run history has been persisted for this workflow run yet."
                            : "No persisted history rows returned for this automation."}
                        </div>
                      )}
                    </div>
                    <div className="tcp-list-item min-h-0">
                      <div className="mb-2 flex items-center justify-between gap-2">
                        <div className="font-medium">Raw Run Payload</div>
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={async () => {
                            try {
                              await navigator.clipboard.writeText(
                                [
                                  "=== RUN ===",
                                  formatJson(selectedRun),
                                  "=== ARTIFACTS ===",
                                  formatJson(runArtifacts),
                                  "=== TELEMETRY ===",
                                  formatJson(filteredRunEvents.map((row) => row.event)),
                                  "=== CONTEXT RUN ===",
                                  formatJson((workflowContextRunQuery.data as any)?.run || null),
                                  "=== BLACKBOARD ===",
                                  formatJson(workflowBlackboard),
                                  "=== HISTORY ===",
                                  formatJson(runHistoryEvents),
                                  "=== SESSION MESSAGES ===",
                                  formatJson(sessionMessages),
                                ].join("\n\n")
                              );
                              toast("ok", "Copied full debug context.");
                            } catch (error) {
                              toast("err", error instanceof Error ? error.message : "Copy failed.");
                            }
                          }}
                        >
                          <i data-lucide="copy-plus"></i>
                          Copy all debug context
                        </button>
                      </div>
                      <pre className="tcp-code overflow-auto sm:max-h-[18rem]">
                        {formatJson({
                          run: selectedRun,
                          contextRun: (workflowContextRunQuery.data as any)?.run || null,
                          blackboard: workflowBlackboard,
                        })}
                      </pre>
                    </div>
                  </div>
                </div>
              </div>
            </div>
            <div className="tcp-confirm-actions mt-3">
              <button className="tcp-btn" onClick={() => navigate("feed")}>
                <i data-lucide="radio"></i>
                Open Live Feed
              </button>
              <button className="tcp-btn" onClick={() => onSelectRunId("")}>
                <i data-lucide="x"></i>
                Close
              </button>
            </div>
          </motion.div>
        </motion.div>
      ) : null}
      <LegacyAutomationEditDialog
        editDraft={editDraft}
        setEditDraft={setEditDraft}
        updateAutomationMutation={updateAutomationMutation}
      />
      <WorkflowAutomationEditDialog
        workflowEditDraft={workflowEditDraft}
        setWorkflowEditDraft={setWorkflowEditDraft}
        validateWorkspaceRootInput={validateWorkspaceRootInput}
        validateModelInput={validateModelInput}
        validatePlannerModelInput={validatePlannerModelInput}
        automationWizardConfig={AUTOMATION_WIZARD_CONFIG}
        providerOptions={providerOptions}
        mcpServers={mcpServers}
        overlapHistoryEntries={overlapHistoryEntries}
        runNowV2Mutation={runNowV2Mutation}
        updateWorkflowAutomationMutation={updateWorkflowAutomationMutation}
      />
      <DeleteAutomationDialog
        deleteConfirm={deleteConfirm}
        setDeleteConfirm={setDeleteConfirm}
        automationActionMutation={automationActionMutation}
      />
    </div>
  );
}

// ─── Spawn Approvals ────────────────────────────────────────────────────────

// ─── Main Page ──────────────────────────────────────────────────────────────

export function AutomationsPage({ client, api, toast, navigate, providerStatus }: AppPageProps) {
  const caps = useCapabilities();
  const {
    tab,
    setTab,
    createMode,
    setCreateMode,
    selectedRunId,
    setSelectedRunId,
    advancedEditAutomation,
    setAdvancedEditAutomation,
  } = useAutomationsPageState();

  return (
    <AutomationsPageTabs
      tab={tab}
      setTab={setTab}
      createMode={createMode}
      setCreateMode={setCreateMode}
      selectedRunId={selectedRunId}
      setSelectedRunId={setSelectedRunId}
      advancedEditAutomation={advancedEditAutomation}
      setAdvancedEditAutomation={setAdvancedEditAutomation}
      client={client}
      api={api}
      toast={toast}
      navigate={navigate}
      providerStatus={providerStatus}
      PageCardComponent={PageCard}
      CreateWizardComponent={CreateWizardExternal}
      MyAutomationsComponent={MyAutomations}
      AdvancedMissionBuilderPanelComponent={AdvancedMissionBuilderPanel}
      OptimizationCampaignsPanelComponent={OptimizationCampaignsPanel}
      SpawnApprovalsComponent={SpawnApprovals}
    />
  );
}
