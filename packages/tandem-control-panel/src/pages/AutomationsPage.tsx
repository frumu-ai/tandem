import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import FullCalendar from "@fullcalendar/react";
import interactionPlugin from "@fullcalendar/interaction";
import timeGridPlugin from "@fullcalendar/timegrid";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";
import YAML from "yaml";
import { renderIcons } from "../app/icons.js";
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
import { AdvancedMissionBuilderPanel } from "./AdvancedMissionBuilderPanel";
import { OptimizationCampaignsPanel } from "./OptimizationCampaignsPanel";
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
type ActiveTab = "create" | "calendar" | "list" | "running" | "optimize" | "approvals";
type CreateMode = "simple" | "advanced";
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
  cron: string;
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
}

interface WorkflowNodeEditDraft {
  nodeId: string;
  title: string;
  objective: string;
  agentId: string;
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
  const defaults = config.defaults || {};
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
const AUTOMATIONS_STUDIO_HANDOFF_KEY = "tandem.automations.studioHandoff";

function createDefaultWizardState(
  defaultProvider: string,
  defaultModel: string,
  workspaceRoot = ""
): WizardState {
  return {
    goal: "",
    workspaceRoot,
    schedulePreset: AUTOMATION_WIZARD_CONFIG.defaults.schedulePreset,
    cron: "",
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

function toSchedulePayload(wizard: WizardState) {
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
  if (cronExpr) return cronExpr;
  const seconds = Number(schedule?.interval_seconds?.seconds);
  if (Number.isFinite(seconds) && seconds > 0) {
    if (seconds % 3600 === 0) return `Every ${seconds / 3600}h`;
    if (seconds % 60 === 0) return `Every ${seconds / 60}m`;
    return `Every ${seconds}s`;
  }
  return "manual";
}

function formatAutomationV2ScheduleLabel(schedule: any) {
  const type = String(schedule?.type || "")
    .trim()
    .toLowerCase();
  if (type === "cron")
    return String(schedule?.cron_expression || schedule?.cronExpression || "cron");
  if (type === "interval") {
    const seconds = Number(schedule?.interval_seconds || schedule?.intervalSeconds || 0);
    if (!Number.isFinite(seconds) || seconds <= 0) return "interval";
    if (seconds % 3600 === 0) return `Every ${seconds / 3600}h`;
    if (seconds % 60 === 0) return `Every ${seconds / 60}m`;
    return `Every ${seconds}s`;
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
      }))
    : [];
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
    modelProvider: String(prefs?.model_provider || prefs?.modelProvider || "").trim(),
    modelId: String(prefs?.model_id || prefs?.modelId || "").trim(),
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
          requested ? `offered tools: ${requested}` : "",
          executed ? `executed tools: ${executed}` : "",
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

function Step1Goal({
  value,
  onChange,
  routedSkill,
  routingConfidence,
  validationBadge,
  generatedSkill,
  advancedMode,
  customSkillName,
  customSkillDescription,
  customWorkflowKind,
  onToggleAdvancedMode,
  onChangeCustomSkillName,
  onChangeCustomSkillDescription,
  onChangeCustomWorkflowKind,
  showArtifactPreview,
  onToggleArtifactPreview,
  artifactPreviewKey,
  onSelectArtifactPreviewKey,
  onGenerateSkill,
  onInstallGeneratedSkill,
  isGeneratingSkill,
  isInstallingSkill,
  installStatus,
  topMatches,
  isMatching,
  selectedAgentId,
  onChangeSelectedAgentId,
}: {
  value: string;
  onChange: (v: string) => void;
  routedSkill: string;
  routingConfidence: string;
  validationBadge: string;
  generatedSkill: any;
  advancedMode: boolean;
  customSkillName: string;
  customSkillDescription: string;
  customWorkflowKind: "pack_builder_recipe" | "automation_v2_dag";
  onToggleAdvancedMode: () => void;
  onChangeCustomSkillName: (v: string) => void;
  onChangeCustomSkillDescription: (v: string) => void;
  onChangeCustomWorkflowKind: (v: "pack_builder_recipe" | "automation_v2_dag") => void;
  showArtifactPreview: boolean;
  onToggleArtifactPreview: () => void;
  artifactPreviewKey: string;
  onSelectArtifactPreviewKey: (v: string) => void;
  onGenerateSkill: () => void;
  onInstallGeneratedSkill: () => void;
  isGeneratingSkill: boolean;
  isInstallingSkill: boolean;
  installStatus: string;
  topMatches: Array<{ skill_name?: string; confidence?: number }>;
  isMatching: boolean;
  selectedAgentId: string;
  onChangeSelectedAgentId: (v: string) => void;
}) {
  const [agentSearch, setAgentSearch] = useState("");
  const generatedArtifactKeys = Object.keys(
    (generatedSkill?.artifacts as Record<string, string>) || {}
  );

  const filteredAgents = useMemo(() => {
    const query = agentSearch.toLowerCase().trim();
    if (!query) return CATALOG.agents.slice(0, 20);
    return CATALOG.agents
      .filter(
        (agent) =>
          agent.name.toLowerCase().includes(query) ||
          agent.summary.toLowerCase().includes(query) ||
          agent.tags.some((tag) => tag.toLowerCase().includes(query)) ||
          agent.category_title.toLowerCase().includes(query)
      )
      .slice(0, 20);
  }, [agentSearch]);

  const selectedAgent = useMemo(
    () => CATALOG.agents.find((a) => a.id === selectedAgentId) || null,
    [selectedAgentId]
  );
  return (
    <div className="grid gap-4">
      <p className="text-sm text-slate-400">
        Describe what you want the AI to do — in plain English. No technical knowledge needed.
      </p>
      <textarea
        className="tcp-input min-h-[120px] text-base"
        placeholder={`e.g. "${AUTOMATION_WIZARD_CONFIG.goalExamples[0]}"`}
        value={value}
        onInput={(e) => onChange((e.target as HTMLTextAreaElement).value)}
        autoFocus
      />
      <div className="grid gap-2">
        <div className="flex items-center justify-between">
          <p className="text-xs text-slate-500">Search agents from catalog:</p>
          {selectedAgentId && (
            <button
              className="tcp-btn h-6 px-2 text-xs"
              onClick={() => onChangeSelectedAgentId("")}
            >
              Clear
            </button>
          )}
        </div>
        <input
          className="tcp-input text-xs"
          placeholder="Search agents by name, tag, or description..."
          value={agentSearch}
          onInput={(e) => setAgentSearch((e.target as HTMLInputElement).value)}
        />
        <div className="max-h-[200px] overflow-y-auto rounded border border-slate-700/50">
          {filteredAgents.length === 0 ? (
            <p className="p-3 text-xs text-slate-500">No agents found</p>
          ) : (
            <div className="grid gap-1 p-2">
              {filteredAgents.map((agent) => (
                <button
                  key={agent.id}
                  className={`tcp-list-item flex flex-col items-start gap-1 text-left transition-all ${
                    selectedAgentId === agent.id ? "border-amber-400/60 bg-amber-400/10" : ""
                  }`}
                  onClick={() => {
                    onChangeSelectedAgentId(agent.id);
                    setAgentSearch("");
                  }}
                >
                  <div className="flex items-center gap-2 w-full">
                    <span className="font-medium text-xs truncate">{agent.name}</span>
                    <span className="tcp-badge-info text-[10px]">{agent.category_title}</span>
                    <span className="tcp-badge-info text-[10px]">{agent.role}</span>
                  </div>
                  <span className="text-[10px] text-slate-400 line-clamp-2">{agent.summary}</span>
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
      {selectedAgent && (
        <div className="rounded-xl border border-amber-400/30 bg-amber-400/5 p-3 text-xs">
          <div className="flex items-center justify-between gap-2 mb-2">
            <span className="font-medium text-amber-300">Selected: {selectedAgent.name}</span>
            <span className="tcp-badge-info">{selectedAgent.role}</span>
          </div>
          <div
            className="prose prose-sm prose-invert max-w-none text-slate-300"
            dangerouslySetInnerHTML={{
              __html: renderMarkdownSafe(selectedAgent.instructions || selectedAgent.summary),
            }}
          />
        </div>
      )}
      <div className="rounded-xl border border-slate-700/50 bg-slate-900/30 p-3 text-xs text-slate-300">
        <div className="flex items-center justify-between gap-2">
          <span className="uppercase tracking-wide text-slate-500">Reusable Flows</span>
          <span className="text-slate-500">{isMatching ? "Analyzing…" : "Ready"}</span>
        </div>
        {routedSkill ? (
          <p className="mt-1">
            Reusable flow match: <strong>{routedSkill}</strong>{" "}
            {routingConfidence ? `(${routingConfidence})` : ""}
            {validationBadge ? (
              <span
                className={`ml-2 ${validationBadge === "validated" ? "tcp-badge-ok" : "tcp-badge-warn"}`}
              >
                {validationBadge === "validated" ? "Validated" : "Not validated"}
              </span>
            ) : null}
          </p>
        ) : (
          <p className="mt-1 text-slate-400">
            No reusable flow selected. Tandem will create and run a workflow plan in the engine.
          </p>
        )}
        {topMatches.length ? (
          <div className="mt-2 flex flex-wrap gap-1">
            {topMatches.slice(0, 3).map((m, idx) => (
              <span key={`${String(m?.skill_name || "match")}-${idx}`} className="tcp-badge-info">
                {String(m?.skill_name || "unknown")}{" "}
                {typeof m?.confidence === "number" ? `${Math.round(m.confidence * 100)}%` : ""}
              </span>
            ))}
          </div>
        ) : null}
      </div>
      <div className="rounded-xl border border-slate-700/50 bg-slate-900/30 p-3 text-xs text-slate-300">
        <div className="flex items-center justify-between gap-2">
          <span className="uppercase tracking-wide text-slate-500">
            Optional: Reusable Skill Export
          </span>
          <div className="flex items-center gap-2">
            <button className="tcp-btn h-7 px-2 text-xs" onClick={onToggleAdvancedMode}>
              {advancedMode ? "Hide Export Options" : "Show Export Options"}
            </button>
            <button
              className="tcp-btn h-7 px-2 text-xs"
              onClick={onGenerateSkill}
              disabled={!value.trim() || isGeneratingSkill}
            >
              {isGeneratingSkill ? "Generating…" : "Generate Reusable Skill Draft"}
            </button>
            <button
              className="tcp-btn h-7 px-2 text-xs"
              onClick={onInstallGeneratedSkill}
              disabled={!generatedSkill?.artifacts || isInstallingSkill}
            >
              {isInstallingSkill ? "Installing…" : "Save Reusable Skill"}
            </button>
          </div>
        </div>
        <p className="mt-1 text-slate-400">
          This is a secondary prompt-based export path. It does not power the default automation
          flow, and it does not automatically track planner-chat revisions to the workflow plan.
        </p>
        <div className="mt-2 rounded-lg border border-slate-800/70 bg-slate-950/30 px-3 py-2 text-xs text-slate-400">
          Recommended flow: review and finalize the workflow plan first, then generate or regenerate
          the reusable skill draft if you want a reusable export from the current prompt.
        </div>
        {advancedMode ? (
          <div className="mt-2 grid gap-2">
            <input
              className="tcp-input text-xs"
              placeholder="skill-name"
              value={customSkillName}
              onInput={(e) => onChangeCustomSkillName((e.target as HTMLInputElement).value)}
            />
            <input
              className="tcp-input text-xs"
              placeholder="Short skill description"
              value={customSkillDescription}
              onInput={(e) => onChangeCustomSkillDescription((e.target as HTMLInputElement).value)}
            />
            <select
              className="tcp-input text-xs"
              value={customWorkflowKind}
              onInput={(e) =>
                onChangeCustomWorkflowKind(
                  (e.target as HTMLSelectElement).value as
                    | "pack_builder_recipe"
                    | "automation_v2_dag"
                )
              }
            >
              <option value="pack_builder_recipe">pack_builder_recipe</option>
              <option value="automation_v2_dag">automation_v2_dag</option>
            </select>
          </div>
        ) : null}
        {generatedSkill ? (
          <div className="mt-2 grid gap-1">
            <p>
              Optional scaffold status:{" "}
              <strong>{String(generatedSkill?.status || "generated")}</strong>
            </p>
            <p className="text-amber-200">
              This draft was generated from the prompt and export options. If you revise the
              workflow plan later in review, regenerate this draft before saving it.
            </p>
            <p>
              Suggested skill:{" "}
              <strong>{String(generatedSkill?.router?.skill_name || "new optional skill")}</strong>
            </p>
            <p className="text-slate-400">
              Artifacts:{" "}
              {generatedArtifactKeys.join(", ") ||
                "SKILL.md, workflow.yaml, automation.example.yaml"}
            </p>
            <div className="mt-1 flex items-center gap-2">
              <button className="tcp-btn h-7 px-2 text-xs" onClick={onToggleArtifactPreview}>
                {showArtifactPreview ? "Hide Raw" : "Show Raw"}
              </button>
              {showArtifactPreview ? (
                <select
                  className="tcp-input h-7 text-xs"
                  value={artifactPreviewKey}
                  onInput={(e) => onSelectArtifactPreviewKey((e.target as HTMLSelectElement).value)}
                >
                  {Object.keys((generatedSkill?.artifacts as Record<string, string>) || {}).map(
                    (key) => (
                      <option key={key} value={key}>
                        {key}
                      </option>
                    )
                  )}
                </select>
              ) : null}
            </div>
            {showArtifactPreview ? (
              <textarea
                className="tcp-input min-h-[140px] font-mono text-[11px]"
                readOnly
                value={String(
                  (generatedSkill?.artifacts as Record<string, string>)?.[artifactPreviewKey] || ""
                )}
              />
            ) : null}
          </div>
        ) : (
          <p className="mt-1 text-slate-400">
            Generate a reusable skill draft from this prompt if you want to save it for later reuse.
          </p>
        )}
        {installStatus ? <p className="mt-2 text-slate-300">{installStatus}</p> : null}
      </div>
    </div>
  );
}

function Step2Schedule({
  selected,
  onSelect,
  customCron,
  onCustomCron,
}: {
  selected: string;
  onSelect: (preset: SchedulePreset) => void;
  customCron: string;
  onCustomCron: (v: string) => void;
}) {
  return (
    <div className="grid gap-3">
      <p className="text-sm text-slate-400">When should this automation run?</p>
      <div className="grid gap-2 sm:grid-cols-2">
        {AUTOMATION_WIZARD_CONFIG.schedulePresets.map((preset) => (
          <button
            key={preset.label}
            onClick={() => onSelect(preset)}
            className={`tcp-list-item flex flex-col items-start gap-1 text-left transition-all ${
              selected === preset.label ? "border-amber-400/60 bg-amber-400/10" : ""
            }`}
          >
            <div className="flex items-center gap-2 font-medium">
              <span>{preset.icon}</span>
              <span>{preset.label}</span>
            </div>
            <span className="tcp-subtle text-xs">{preset.desc}</span>
            {preset.cron ? (
              <code className="rounded bg-slate-800/60 px-1.5 py-0.5 text-xs text-slate-400">
                {preset.cron}
              </code>
            ) : null}
          </button>
        ))}
      </div>
      <div className="grid gap-1">
        <label className="text-xs text-slate-500">Custom cron expression (advanced)</label>
        <input
          className="tcp-input font-mono text-sm"
          placeholder="e.g. 30 8 * * 1-5  (8:30am weekdays)"
          value={customCron}
          onInput={(e) => onCustomCron((e.target as HTMLInputElement).value)}
        />
      </div>
    </div>
  );
}

function Step3Mode({
  selected,
  onSelect,
  maxAgents,
  onMaxAgents,
  workspaceRoot,
  onWorkspaceRootChange,
  providerOptions,
  providerId,
  modelId,
  plannerProviderId,
  plannerModelId,
  onProviderChange,
  onModelChange,
  onPlannerProviderChange,
  onPlannerModelChange,
  roleModelsJson,
  onRoleModelsChange,
  roleModelsError,
  toolAccessMode,
  customToolsText,
  onToolAccessModeChange,
  onCustomToolsTextChange,
  mcpServers,
  selectedMcpServers,
  onToggleMcpServer,
  onOpenMcpSettings,
  workspaceRootError,
  plannerModelError,
  workspaceBrowserOpen,
  workspaceBrowserDir,
  workspaceBrowserSearch,
  onWorkspaceBrowserSearchChange,
  onOpenWorkspaceBrowser,
  onCloseWorkspaceBrowser,
  onBrowseWorkspaceParent,
  onBrowseWorkspaceDirectory,
  onSelectWorkspaceDirectory,
  workspaceBrowserParentDir,
  workspaceCurrentBrowseDir,
  filteredWorkspaceDirectories,
}: {
  selected: ExecutionMode;
  onSelect: (mode: ExecutionMode) => void;
  maxAgents: string;
  onMaxAgents: (v: string) => void;
  workspaceRoot: string;
  onWorkspaceRootChange: (v: string) => void;
  providerOptions: ProviderOption[];
  providerId: string;
  modelId: string;
  plannerProviderId: string;
  plannerModelId: string;
  onProviderChange: (v: string) => void;
  onModelChange: (v: string) => void;
  onPlannerProviderChange: (v: string) => void;
  onPlannerModelChange: (v: string) => void;
  roleModelsJson: string;
  onRoleModelsChange: (v: string) => void;
  roleModelsError: string;
  toolAccessMode: WorkflowToolAccessMode;
  customToolsText: string;
  onToolAccessModeChange: (mode: WorkflowToolAccessMode) => void;
  onCustomToolsTextChange: (value: string) => void;
  mcpServers: McpServerOption[];
  selectedMcpServers: string[];
  onToggleMcpServer: (name: string) => void;
  onOpenMcpSettings: () => void;
  workspaceRootError: string;
  plannerModelError: string;
  workspaceBrowserOpen: boolean;
  workspaceBrowserDir: string;
  workspaceBrowserSearch: string;
  onWorkspaceBrowserSearchChange: (value: string) => void;
  onOpenWorkspaceBrowser: () => void;
  onCloseWorkspaceBrowser: () => void;
  onBrowseWorkspaceParent: () => void;
  onBrowseWorkspaceDirectory: (path: string) => void;
  onSelectWorkspaceDirectory: () => void;
  workspaceBrowserParentDir: string;
  workspaceCurrentBrowseDir: string;
  filteredWorkspaceDirectories: any[];
}) {
  const modelOptions = providerOptions.find((p) => p.id === providerId)?.models || [];
  const plannerModelOptions = providerOptions.find((p) => p.id === plannerProviderId)?.models || [];
  const workspaceSearchQuery = String(workspaceBrowserSearch || "")
    .trim()
    .toLowerCase();
  return (
    <div className="grid gap-4">
      <p className="text-sm text-slate-400">
        How should the AI handle this task? (You can always change this later.)
      </p>
      <div className="grid gap-3">
        {AUTOMATION_WIZARD_CONFIG.executionModes.map((m) => (
          <button
            key={m.id}
            onClick={() => onSelect(m.id)}
            className={`tcp-list-item flex items-start gap-4 text-left transition-all ${
              selected === m.id ? "border-amber-400/60 bg-amber-400/10" : ""
            }`}
          >
            <span className="mt-0.5 text-2xl">{m.icon}</span>
            <div className="grid gap-1">
              <div className="flex items-center gap-2">
                <span className="font-semibold">{m.label}</span>
                {m.id === "team" ? (
                  <span className="rounded-full bg-amber-500/20 px-2 py-0.5 text-xs text-amber-300">
                    Recommended
                  </span>
                ) : null}
              </div>
              <span className="text-sm text-slate-300">{m.desc}</span>
              <span className="tcp-subtle text-xs">Best for: {m.bestFor}</span>
            </div>
            <div
              className="ml-auto mt-1 h-4 w-4 shrink-0 rounded-full border-2 border-slate-600 transition-all data-[checked]:border-amber-400 data-[checked]:bg-amber-400/30"
              data-checked={selected === m.id ? true : undefined}
            />
          </button>
        ))}
      </div>
      {selected === "swarm" ? (
        <div className="grid gap-1">
          <label className="text-xs text-slate-400">Max parallel agents</label>
          <input
            type="number"
            min="2"
            max="16"
            className="tcp-input w-24"
            value={maxAgents}
            onInput={(e) => onMaxAgents((e.target as HTMLInputElement).value)}
          />
        </div>
      ) : null}
      <div className="grid gap-2 rounded-xl border border-slate-700/50 bg-slate-900/30 p-3">
        <div className="text-xs uppercase tracking-wide text-slate-500">Execution Directory</div>
        <div className="grid gap-1">
          <label className="text-xs text-slate-400">Workspace root</label>
          <div className="grid gap-2 md:grid-cols-[auto_1fr_auto]">
            <button className="tcp-btn" type="button" onClick={onOpenWorkspaceBrowser}>
              Browse
            </button>
            <input
              className={`tcp-input text-sm ${workspaceRootError ? "border-red-500/70 text-red-100" : ""}`}
              value={workspaceRoot}
              readOnly
              placeholder="No local directory selected. Use Browse."
            />
            <button
              className="tcp-btn"
              type="button"
              onClick={() => onWorkspaceRootChange("")}
              disabled={!workspaceRoot}
            >
              Clear
            </button>
          </div>
          <div className="text-xs text-slate-500">
            Tandem will run this automation from this workspace directory.
          </div>
          {workspaceRootError ? (
            <div className="text-xs text-red-300">{workspaceRootError}</div>
          ) : null}
        </div>
      </div>
      <AnimatePresence>
        {workspaceBrowserOpen ? (
          <motion.div
            className="fixed inset-0 z-50 flex items-center justify-center p-4"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
          >
            <button
              type="button"
              className="tcp-confirm-backdrop"
              aria-label="Close workspace directory dialog"
              onClick={onCloseWorkspaceBrowser}
            />
            <motion.div
              className="tcp-confirm-dialog max-w-2xl"
              initial={{ opacity: 0, y: 8, scale: 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 6, scale: 0.98 }}
            >
              <h3 className="tcp-confirm-title">Select Workspace Folder</h3>
              <p className="tcp-confirm-message">Current: {workspaceCurrentBrowseDir || "n/a"}</p>
              <div className="mb-2 flex flex-wrap gap-2">
                <button
                  className="tcp-btn"
                  type="button"
                  onClick={onBrowseWorkspaceParent}
                  disabled={!workspaceBrowserParentDir}
                >
                  Up
                </button>
                <button
                  className="tcp-btn-primary"
                  type="button"
                  onClick={onSelectWorkspaceDirectory}
                  disabled={!workspaceCurrentBrowseDir}
                >
                  Select This Folder
                </button>
                <button className="tcp-btn" type="button" onClick={onCloseWorkspaceBrowser}>
                  Close
                </button>
              </div>
              <div className="mb-2">
                <input
                  className="tcp-input"
                  placeholder="Type to filter folders..."
                  value={workspaceBrowserSearch}
                  onInput={(e) =>
                    onWorkspaceBrowserSearchChange((e.target as HTMLInputElement).value)
                  }
                />
              </div>
              <div className="max-h-[360px] overflow-auto rounded-lg border border-slate-700/60 bg-slate-900/20 p-2">
                {filteredWorkspaceDirectories.length ? (
                  filteredWorkspaceDirectories.map((entry: any) => (
                    <button
                      key={String(entry?.path || entry?.name)}
                      className="tcp-list-item mb-1 w-full text-left"
                      type="button"
                      onClick={() => onBrowseWorkspaceDirectory(String(entry?.path || ""))}
                    >
                      {String(entry?.name || entry?.path || "")}
                    </button>
                  ))
                ) : (
                  <EmptyState
                    text={
                      workspaceSearchQuery
                        ? "No folders match your search."
                        : "No subdirectories in this folder."
                    }
                  />
                )}
              </div>
            </motion.div>
          </motion.div>
        ) : null}
      </AnimatePresence>
      <div className="grid gap-2 rounded-xl border border-slate-700/50 bg-slate-900/30 p-3">
        <div className="text-xs uppercase tracking-wide text-slate-500">Model Selection</div>
        <div className="grid gap-2 sm:grid-cols-2">
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Provider</label>
            <select
              className="tcp-input text-sm"
              value={providerId}
              onInput={(e) => onProviderChange((e.target as HTMLSelectElement).value)}
            >
              <option value="">Use workspace default</option>
              {providerOptions.map((provider) => (
                <option key={provider.id} value={provider.id}>
                  {provider.id}
                </option>
              ))}
            </select>
          </div>
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Model</label>
            <input
              className="tcp-input text-sm"
              value={modelId}
              list={providerId ? `models-${providerId}` : undefined}
              onInput={(e) => onModelChange((e.target as HTMLInputElement).value)}
              placeholder="Use workspace default model"
            />
            {providerId ? (
              <datalist id={`models-${providerId}`}>
                {modelOptions.map((model) => (
                  <option key={model} value={model} />
                ))}
              </datalist>
            ) : null}
          </div>
        </div>
        <div className="grid gap-2 rounded-lg border border-slate-800/70 bg-slate-950/30 p-3">
          <div className="text-xs uppercase tracking-wide text-slate-500">
            Planner fallback model
          </div>
          <div className="text-xs text-slate-400">
            Optional. Use this only if you want planning chat to try broader engine-side revisions
            beyond the built-in deterministic edits.
          </div>
          <div className="grid gap-2 sm:grid-cols-2">
            <div className="grid gap-1">
              <label className="text-xs text-slate-400">Planner provider</label>
              <select
                className="tcp-input text-sm"
                value={plannerProviderId}
                onInput={(e) => onPlannerProviderChange((e.target as HTMLSelectElement).value)}
              >
                <option value="">Disabled</option>
                {providerOptions.map((provider) => (
                  <option key={`planner-${provider.id}`} value={provider.id}>
                    {provider.id}
                  </option>
                ))}
              </select>
            </div>
            <div className="grid gap-1">
              <label className="text-xs text-slate-400">Planner model</label>
              <input
                className="tcp-input text-sm"
                value={plannerModelId}
                list={plannerProviderId ? `planner-models-${plannerProviderId}` : undefined}
                onInput={(e) => onPlannerModelChange((e.target as HTMLInputElement).value)}
                placeholder="Disabled unless provider and model are set"
              />
              {plannerProviderId ? (
                <datalist id={`planner-models-${plannerProviderId}`}>
                  {plannerModelOptions.map((model) => (
                    <option key={model} value={model} />
                  ))}
                </datalist>
              ) : null}
            </div>
          </div>
          {plannerModelError ? (
            <div className="text-xs text-red-300">{plannerModelError}</div>
          ) : null}
        </div>
        <div className="grid gap-1">
          <label className="text-xs text-slate-400">Role model overrides (advanced JSON)</label>
          <textarea
            className={`tcp-input min-h-[72px] font-mono text-xs ${roleModelsError ? "border-red-500/70 text-red-100" : ""}`}
            value={roleModelsJson}
            onInput={(e) => onRoleModelsChange((e.target as HTMLTextAreaElement).value)}
            placeholder={`{"planner":{"provider_id":"openai","model_id":"gpt-5"},"worker":{"provider_id":"anthropic","model_id":"claude-sonnet-4"}}`}
          />
          {roleModelsError ? <div className="text-xs text-red-300">{roleModelsError}</div> : null}
        </div>
      </div>
      <div className="grid gap-3 rounded-xl border border-slate-700/50 bg-slate-900/30 p-3">
        <div className="text-xs uppercase tracking-wide text-slate-500">Tool Access</div>
        <div className="grid gap-2 sm:grid-cols-2">
          <button
            type="button"
            className={`tcp-list-item text-left ${toolAccessMode === "all" ? "border-amber-400/60 bg-amber-400/10" : ""}`}
            onClick={() => onToolAccessModeChange("all")}
          >
            <div className="font-medium">All tools</div>
            <div className="tcp-subtle text-xs">
              Grant full built-in tool access. Use this for autonomous coding and file operations.
            </div>
          </button>
          <button
            type="button"
            className={`tcp-list-item text-left ${toolAccessMode === "custom" ? "border-amber-400/60 bg-amber-400/10" : ""}`}
            onClick={() => onToolAccessModeChange("custom")}
          >
            <div className="font-medium">Custom allowlist</div>
            <div className="tcp-subtle text-xs">
              Restrict built-in tools manually. MCP tools still come from the selected servers
              below.
            </div>
          </button>
        </div>
        {toolAccessMode === "custom" ? (
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Allowed built-in tools</label>
            <textarea
              className="tcp-input min-h-[96px] font-mono text-xs"
              value={customToolsText}
              onInput={(e) => onCustomToolsTextChange((e.target as HTMLTextAreaElement).value)}
              placeholder={`read\nwrite\nedit\nbash\nls\nglob\nwebsearch`}
            />
            <div className="text-xs text-slate-500">
              Enter one tool per line or comma-separated. Example: `read`, `write`, `edit`, `bash`,
              `ls`, `glob`, `websearch`.
            </div>
          </div>
        ) : (
          <div className="text-xs text-slate-500">
            All built-in tools are allowed. MCP tools are additionally controlled by the server
            selection below.
          </div>
        )}
      </div>
      <div className="grid gap-2 rounded-xl border border-slate-700/50 bg-slate-900/30 p-3">
        <div className="flex items-center justify-between gap-2">
          <div className="text-xs uppercase tracking-wide text-slate-500">MCP Servers</div>
          <button className="tcp-btn h-7 px-2 text-xs" onClick={onOpenMcpSettings}>
            Add MCP Server
          </button>
        </div>
        {mcpServers.length ? (
          <div className="flex flex-wrap gap-2">
            {mcpServers.map((server) => {
              const isSelected = selectedMcpServers.includes(server.name);
              return (
                <button
                  key={server.name}
                  className={`tcp-btn h-7 px-2 text-xs ${isSelected ? "border-amber-400/60 bg-amber-400/10 text-amber-300" : ""}`}
                  onClick={() => onToggleMcpServer(server.name)}
                >
                  {server.name} {server.connected ? "• connected" : "• disconnected"}
                </button>
              );
            })}
          </div>
        ) : (
          <div className="text-xs text-slate-400">
            No MCP servers configured yet. Add one to allow external tools in this automation.
          </div>
        )}
      </div>
    </div>
  );
}

function Step4Review({
  wizard,
  onToggleExportPackDraft,
  onSubmit,
  isPending,
  planPreview,
  isPreviewing,
  planningConversation,
  planningChangeSummary,
  onSendPlanningMessage,
  isSendingPlanningMessage,
  onResetPlanningChat,
  isResettingPlanningChat,
  plannerError,
  plannerDiagnostics,
  generatedSkill,
  installStatus,
}: {
  wizard: WizardState;
  onToggleExportPackDraft: () => void;
  onSubmit: () => void;
  isPending: boolean;
  planPreview: any;
  isPreviewing: boolean;
  planningConversation: any;
  planningChangeSummary: string[];
  onSendPlanningMessage: (message: string) => void;
  isSendingPlanningMessage: boolean;
  onResetPlanningChat: () => void;
  isResettingPlanningChat: boolean;
  plannerError: string;
  plannerDiagnostics: any;
  generatedSkill: any;
  installStatus: string;
}) {
  const [planningNote, setPlanningNote] = useState("");
  const [goalExpanded, setGoalExpanded] = useState(false);
  const [descriptionExpanded, setDescriptionExpanded] = useState(false);
  const [expandedStepIds, setExpandedStepIds] = useState<Record<string, boolean>>({});
  const wizardSchedule = wizard.cron
    ? wizard.cron
    : AUTOMATION_WIZARD_CONFIG.schedulePresets.find((p) => p.label === wizard.schedulePreset)
          ?.intervalSeconds
      ? `Every ${
          AUTOMATION_WIZARD_CONFIG.schedulePresets.find((p) => p.label === wizard.schedulePreset)!
            .intervalSeconds! / 3600
        }h`
      : wizard.schedulePreset || "Manual";
  const planOperatorPreferences =
    planPreview && typeof planPreview === "object"
      ? planPreview.operator_preferences || planPreview.operatorPreferences || {}
      : {};
  const effectiveMode = String(
    (planOperatorPreferences as any)?.execution_mode || wizard.mode || "team"
  ).trim() as ExecutionMode;
  const modeInfo = AUTOMATION_WIZARD_CONFIG.executionModes.find((m) => m.id === effectiveMode);
  const effectiveMaxParallel = Number(
    (planOperatorPreferences as any)?.max_parallel_agents ??
      (planOperatorPreferences as any)?.maxParallelAgents ??
      (effectiveMode === "swarm" ? wizard.maxAgents : 1)
  );
  const hasPlanPreview = !!planPreview;
  const effectiveModelProvider = String(
    hasPlanPreview
      ? (planOperatorPreferences as any)?.model_provider ||
          (planOperatorPreferences as any)?.modelProvider ||
          ""
      : wizard.modelProvider || ""
  ).trim();
  const effectiveModelId = String(
    hasPlanPreview
      ? (planOperatorPreferences as any)?.model_id ||
          (planOperatorPreferences as any)?.modelId ||
          ""
      : wizard.modelId || ""
  ).trim();
  const effectivePlannerRoleModel =
    planOperatorPreferences &&
    typeof planOperatorPreferences === "object" &&
    ((planOperatorPreferences as any)?.role_models?.planner ||
      (planOperatorPreferences as any)?.roleModels?.planner);
  const effectivePlannerModelProvider = String(
    hasPlanPreview
      ? effectivePlannerRoleModel?.provider_id || effectivePlannerRoleModel?.providerId || ""
      : wizard.plannerModelProvider || ""
  ).trim();
  const effectivePlannerModelId = String(
    hasPlanPreview
      ? effectivePlannerRoleModel?.model_id || effectivePlannerRoleModel?.modelId || ""
      : wizard.plannerModelId || ""
  ).trim();
  const plannerFallbackEnabled = !!(
    (effectivePlannerModelProvider && effectivePlannerModelId) ||
    (effectiveModelProvider && effectiveModelId)
  );
  const effectiveWorkspaceRoot = String(
    planPreview?.workspace_root || planPreview?.workspaceRoot || wizard.workspaceRoot || ""
  ).trim();
  const effectiveMcpServers = Array.isArray(
    planPreview?.allowed_mcp_servers || planPreview?.allowedMcpServers
  )
    ? ((planPreview?.allowed_mcp_servers || planPreview?.allowedMcpServers || []) as string[])
    : wizard.selectedMcpServers;
  const effectiveToolAccessMode = String(
    hasPlanPreview
      ? (planOperatorPreferences as any)?.tool_access_mode ||
          (planOperatorPreferences as any)?.toolAccessMode ||
          "all"
      : wizard.toolAccessMode || "all"
  ).trim();
  const effectiveCustomTools = hasPlanPreview
    ? normalizeAllowedTools(
        (
          (planOperatorPreferences as any)?.tool_allowlist ||
          (planOperatorPreferences as any)?.toolAllowlist ||
          []
        ).map((value: any) => String(value || "").trim())
      )
    : parseCustomToolText(wizard.customToolsText);
  const effectiveSchedule = planPreview?.schedule
    ? formatAutomationV2ScheduleLabel(planPreview.schedule)
    : wizardSchedule;
  const effectivePlanTitle = String(planPreview?.title || "").trim();
  const plannerFallbackReason = String(
    plannerDiagnostics?.fallback_reason || plannerDiagnostics?.fallbackReason || ""
  ).trim();
  const plannerFallbackDetail = String(plannerDiagnostics?.detail || "").trim();
  const toggleStepExpanded = (stepId: string) =>
    setExpandedStepIds((current) => ({ ...current, [stepId]: !current[stepId] }));

  return (
    <div className="grid gap-4">
      <p className="text-sm text-slate-400">Review your automation before deploying.</p>

      {/* Summary card */}
      <div className="rounded-xl border border-slate-700/60 bg-slate-900/40 p-4 grid gap-3">
        {effectivePlanTitle ? (
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">Plan Title</span>
            <span className="text-sm font-semibold text-slate-100">{effectivePlanTitle}</span>
          </div>
        ) : null}
        <div className="grid gap-1">
          <span className="text-xs text-slate-500 uppercase tracking-wide">Goal</span>
          <div className="rounded-lg border border-slate-800 bg-slate-950/40">
            <button
              type="button"
              className="flex w-full items-center justify-between gap-3 px-3 py-2 text-left"
              onClick={() => setGoalExpanded((current) => !current)}
            >
              <span className="text-sm text-slate-100 italic">
                {goalExpanded ? "Hide goal" : "View goal"}
              </span>
              <span className="tcp-subtle text-xs">{goalExpanded ? "Collapse" : "Expand"}</span>
            </button>
            <div className="border-t border-slate-800 px-3 py-3">
              <div
                className={`tcp-markdown tcp-markdown-ai text-sm ${goalExpanded ? "" : "max-h-28 overflow-hidden"}`}
                dangerouslySetInnerHTML={{ __html: renderMarkdownSafe(wizard.goal || "") }}
              />
            </div>
          </div>
        </div>
        <div className="grid grid-cols-2 gap-3">
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">Schedule</span>
            <span className="text-sm font-medium text-slate-200">{effectiveSchedule}</span>
          </div>
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">Execution Mode</span>
            <span className="text-sm font-medium text-slate-200">
              {modeInfo?.icon} {modeInfo?.label || effectiveMode}
              {Number.isFinite(effectiveMaxParallel) && effectiveMaxParallel > 1
                ? ` · ${effectiveMaxParallel} agents`
                : ""}
            </span>
          </div>
        </div>
        {hasPlanPreview || effectiveModelProvider || effectiveModelId ? (
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">Model Override</span>
            <span className="text-sm font-medium text-slate-200">
              {effectiveModelProvider || effectiveModelId
                ? `${effectiveModelProvider || "default provider"} / ${effectiveModelId || "default model"}`
                : "Workspace default"}
            </span>
          </div>
        ) : null}
        {hasPlanPreview || effectivePlannerModelProvider || effectivePlannerModelId ? (
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">Planner Model</span>
            <span className="text-sm font-medium text-slate-200">
              {effectivePlannerModelProvider || effectivePlannerModelId
                ? `${effectivePlannerModelProvider || "default provider"} / ${effectivePlannerModelId || "default model"}`
                : effectiveModelProvider || effectiveModelId
                  ? `Using model override: ${effectiveModelProvider || "default provider"} / ${effectiveModelId || "default model"}`
                  : "Workspace default"}
            </span>
          </div>
        ) : null}
        <div className="grid gap-1">
          <span className="text-xs text-slate-500 uppercase tracking-wide">
            Broader Planner Revisions
          </span>
          <span className="text-sm font-medium text-slate-200">
            {plannerFallbackEnabled
              ? "Enabled. Tandem will use the planner model if set, otherwise it falls back to the selected model override."
              : "Disabled unless a planner model, model override, or workspace default model is configured"}
          </span>
        </div>
        <div className="grid gap-1">
          <span className="text-xs text-slate-500 uppercase tracking-wide">Workspace Root</span>
          <code className="rounded bg-slate-800/60 px-2 py-1 text-xs text-slate-300">
            {effectiveWorkspaceRoot || "engine workspace root"}
          </code>
        </div>
        {hasPlanPreview || effectiveMcpServers.length ? (
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">MCP Servers</span>
            {effectiveMcpServers.length ? (
              <div className="flex flex-wrap gap-1">
                {effectiveMcpServers.map((name) => (
                  <span key={name} className="tcp-badge-info">
                    {name}
                  </span>
                ))}
              </div>
            ) : (
              <span className="text-sm font-medium text-slate-400">None</span>
            )}
          </div>
        ) : null}
        <div className="grid gap-1">
          <span className="text-xs text-slate-500 uppercase tracking-wide">Tool Access</span>
          {effectiveToolAccessMode === "custom" ? (
            effectiveCustomTools.length ? (
              <div className="flex flex-wrap gap-1">
                {effectiveCustomTools.map((tool) => (
                  <span key={tool} className="tcp-badge-info">
                    {tool}
                  </span>
                ))}
              </div>
            ) : (
              <span className="text-sm font-medium text-slate-400">Custom allowlist</span>
            )
          ) : (
            <span className="text-sm font-medium text-slate-200">All tools</span>
          )}
        </div>
        {wizard.routedSkill ? (
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">
              Reusable Flow Match
            </span>
            <span className="text-sm font-medium text-slate-200">
              {wizard.routedSkill}
              {wizard.routingConfidence ? ` (${wizard.routingConfidence})` : ""}
            </span>
          </div>
        ) : null}
        {wizard.cron ? (
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">Cron</span>
            <code className="rounded bg-slate-800/60 px-2 py-1 text-xs text-slate-300">
              {wizard.cron}
            </code>
          </div>
        ) : null}
        <div className="grid gap-1">
          <span className="text-xs text-slate-500 uppercase tracking-wide">Workflow Plan</span>
          {isPreviewing ? (
            <span className="text-sm text-slate-300">Planning workflow…</span>
          ) : planPreview ? (
            <div className="grid gap-1 text-sm text-slate-300">
              <span>
                Confidence: <strong>{String(planPreview?.confidence || "unknown")}</strong>
              </span>
              <span>
                Execution target:{" "}
                <strong>{String(planPreview?.execution_target || "automation_v2")}</strong>
              </span>
              {effectivePlanTitle ? (
                <span>
                  Title: <strong>{effectivePlanTitle}</strong>
                </span>
              ) : null}
              <span>
                Steps:{" "}
                <strong>{Array.isArray(planPreview?.steps) ? planPreview.steps.length : 0}</strong>
              </span>
              {Array.isArray(planPreview?.steps) && planPreview.steps.length ? (
                <div className="mt-1 grid gap-1">
                  {planPreview.steps.map((step: any, index: number) =>
                    (() => {
                      const stepId = String(step?.step_id || step?.stepId || `step-${index + 1}`);
                      const expanded = !!expandedStepIds[stepId];
                      return (
                        <div
                          key={`${stepId}-${index}`}
                          className="rounded-lg border border-slate-800 bg-slate-950/40"
                        >
                          <button
                            type="button"
                            className="flex w-full items-center justify-between gap-3 px-3 py-2 text-left"
                            onClick={() => toggleStepExpanded(stepId)}
                          >
                            <div className="min-w-0">
                              <div className="text-xs font-medium text-slate-200">
                                {stepId}
                                {step?.kind ? (
                                  <span className="ml-2 text-[11px] uppercase tracking-wide text-slate-500">
                                    {String(step.kind)}
                                  </span>
                                ) : null}
                              </div>
                              {typeof step?.objective === "string" && step.objective.trim() ? (
                                <div className="mt-1 line-clamp-2 text-xs text-slate-400">
                                  {step.objective}
                                </div>
                              ) : null}
                            </div>
                            <span className="tcp-subtle shrink-0 text-xs">
                              {expanded ? "Hide" : "Details"}
                            </span>
                          </button>
                          {expanded &&
                          typeof step?.objective === "string" &&
                          step.objective.trim() ? (
                            <div className="border-t border-slate-800 px-3 py-3">
                              <div
                                className="tcp-markdown tcp-markdown-ai text-sm"
                                dangerouslySetInnerHTML={{
                                  __html: renderMarkdownSafe(step.objective || ""),
                                }}
                              />
                            </div>
                          ) : null}
                        </div>
                      );
                    })()
                  )}
                </div>
              ) : null}
              {typeof planPreview?.description === "string" && planPreview.description.trim() ? (
                <div className="rounded-lg border border-slate-800 bg-slate-950/40">
                  <button
                    type="button"
                    className="flex w-full items-center justify-between gap-3 px-3 py-2 text-left"
                    onClick={() => setDescriptionExpanded((current) => !current)}
                  >
                    <span className="text-sm text-slate-200">Plan description</span>
                    <span className="tcp-subtle text-xs">
                      {descriptionExpanded ? "Collapse" : "Expand"}
                    </span>
                  </button>
                  <div className="border-t border-slate-800 px-3 py-3">
                    <div
                      className={`tcp-markdown tcp-markdown-ai text-sm ${
                        descriptionExpanded ? "" : "max-h-24 overflow-hidden"
                      }`}
                      dangerouslySetInnerHTML={{
                        __html: renderMarkdownSafe(String(planPreview.description || "")),
                      }}
                    />
                  </div>
                </div>
              ) : null}
            </div>
          ) : (
            <span className="text-sm text-slate-400">
              Workflow preview has not been generated yet.
            </span>
          )}
        </div>
      </div>

      {plannerError ? (
        <div className="rounded-xl border border-red-500/40 bg-red-950/30 p-3 text-sm text-red-200">
          {plannerError}
        </div>
      ) : null}

      {plannerFallbackReason ? (
        <div className="rounded-xl border border-amber-500/40 bg-amber-950/30 p-3 text-sm text-amber-100">
          <div className="font-medium text-amber-200">Planner fallback</div>
          <div className="mt-1">
            Reason: <code className="text-xs">{plannerFallbackReason}</code>
          </div>
          {plannerFallbackDetail ? (
            <div className="mt-2 text-xs text-amber-200/90 whitespace-pre-wrap">
              {plannerFallbackDetail}
            </div>
          ) : null}
        </div>
      ) : null}

      {planningChangeSummary.length ? (
        <div className="rounded-xl border border-emerald-500/30 bg-emerald-950/20 p-3">
          <div className="text-xs uppercase tracking-wide text-emerald-300">
            Latest Plan Changes
          </div>
          <div className="mt-2 flex flex-wrap gap-2">
            {planningChangeSummary.map((item, index) => (
              <span key={`${item}-${index}`} className="tcp-badge-ok">
                {item}
              </span>
            ))}
          </div>
        </div>
      ) : null}

      {planPreview ? (
        <div className="rounded-xl border border-slate-700/60 bg-slate-900/40 p-4 grid gap-3">
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs text-slate-500 uppercase tracking-wide">Planning Chat</span>
            <button
              className="tcp-btn h-7 px-2 text-xs"
              disabled={isResettingPlanningChat || !planPreview?.plan_id}
              onClick={onResetPlanningChat}
            >
              {isResettingPlanningChat ? "Resetting…" : "Reset Plan"}
            </button>
          </div>
          <div className="rounded-lg border border-amber-500/30 bg-amber-950/20 px-3 py-2 text-xs text-amber-200">
            {plannerFallbackEnabled
              ? "With a planner model or model override configured, planning chat can attempt broader natural-language workflow rewrites across the allowed fixed step ids. Deterministic edits still act as the safety net for schedule, workspace root, title, MCP servers, execution mode, model overrides, safe workflow shapes, small workflow-step changes, and terminal output-style changes. Custom step types are still not supported in this slice."
              : "Planning chat is currently limited to deterministic edits like schedule, workspace root, title, MCP servers, execution mode, model overrides, switching between safe workflow shapes, small workflow-step changes like adding or removing input collection, analysis, or notifications, and terminal output-style changes like JSON, markdown, summary, URLs, or citations. Broader workflow rewrites require a planner model or model override, and custom step types are still not supported in this slice."}
          </div>
          <div className="max-h-56 overflow-auto rounded-lg border border-slate-800 bg-slate-950/50 p-3">
            {Array.isArray(planningConversation?.messages) &&
            planningConversation.messages.length ? (
              <div className="grid gap-3">
                {planningConversation.messages.map((message: any, index: number) => (
                  <div key={`${message?.created_at_ms || index}-${index}`} className="grid gap-1">
                    <span className="text-[11px] uppercase tracking-wide text-slate-500">
                      {String(message?.role || "assistant")}
                    </span>
                    <div className="text-sm text-slate-200">
                      {String(message?.text || "").trim()}
                    </div>
                  </div>
                ))}
              </div>
            ) : (
              <div className="text-sm text-slate-400">
                Add planning notes here to revise the workflow before creating it.
              </div>
            )}
          </div>
          <textarea
            className="tcp-input min-h-[84px] text-sm"
            value={planningNote}
            onInput={(e) => setPlanningNote((e.target as HTMLTextAreaElement).value)}
            placeholder='Example: "Make this weekly, run it from /srv/acme/app, and remove notifications."'
          />
          <div className="flex justify-end">
            <button
              className="tcp-btn-primary"
              disabled={isSendingPlanningMessage || !planningNote.trim() || !planPreview?.plan_id}
              onClick={() => {
                const note = planningNote.trim();
                if (!note) return;
                onSendPlanningMessage(note);
                setPlanningNote("");
              }}
            >
              {isSendingPlanningMessage ? "Updating plan…" : "Update Plan"}
            </button>
          </div>
        </div>
      ) : null}

      <div className="rounded-xl border border-slate-700/40 bg-slate-800/20 p-3 text-xs text-slate-400">
        <label className="flex items-start gap-3 rounded-lg border border-slate-700/50 bg-slate-900/30 p-3 text-sm text-slate-300">
          <input
            type="checkbox"
            className="mt-0.5"
            checked={wizard.exportPackDraft}
            onChange={onToggleExportPackDraft}
          />
          <span className="grid gap-1">
            <span className="font-medium text-slate-200">Also export a reusable pack draft</span>
            <span className="text-xs text-slate-400">
              After creating the automation, Tandem will also create a Pack Builder draft so this
              workflow can be saved and reused later.
            </span>
          </span>
        </label>
      </div>

      {generatedSkill || installStatus ? (
        <div className="rounded-xl border border-slate-700/40 bg-slate-800/20 p-3 text-xs text-slate-400">
          <div className="text-xs uppercase tracking-wide text-slate-500">
            Reusable Skill Export
          </div>
          <div className="mt-1 grid gap-1">
            {generatedSkill ? (
              <>
                <span>
                  Draft status:{" "}
                  <strong className="text-slate-300">
                    {String(generatedSkill?.status || "generated")}
                  </strong>
                </span>
                <span className="text-amber-200">
                  This draft is prompt-based and may be stale if you changed the workflow plan in
                  planning chat. Regenerate it from Step 1 before saving if you want it to reflect
                  the latest plan direction.
                </span>
              </>
            ) : null}
            {installStatus ? <span>{installStatus}</span> : null}
          </div>
        </div>
      ) : null}

      <div className="rounded-xl border border-slate-700/40 bg-slate-800/20 p-3 text-xs text-slate-400">
        💡 Tandem will save this automation and schedule a{" "}
        <strong className="text-slate-300">{modeInfo?.label || effectiveMode}</strong> that runs{" "}
        <strong className="text-slate-300">{effectiveSchedule}</strong>. You can pause, edit or
        delete it anytime.
      </div>

      <button
        className="tcp-btn-primary"
        disabled={isPending || isPreviewing || !wizard.goal.trim() || !planPreview}
        onClick={onSubmit}
      >
        {isPending ? "Creating automation…" : "🚀 Create Automation"}
      </button>
    </div>
  );
}

// ─── Wizard Container ───────────────────────────────────────────────────────

function CreateWizard({
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
      return {
        ...current,
        workspaceRoot: defaultWorkspaceRoot,
      };
    });
  }, [healthQuery.data]);

  const matchMutation = useMutation({
    mutationFn: async (goal: string) => {
      if (!goal.trim() || !client?.skills?.match) {
        return null;
      }
      return client.skills.match({ goal, maxMatches: 3, threshold: 0.35 });
    },
    onError: () => {
      // Keep routing non-blocking.
    },
  });

  const compileMutation = useMutation({
    mutationFn: async () => {
      if (!client?.workflowPlans?.chatStart) {
        throw new Error(
          "This control panel build is missing workflow planner client support. Rebuild the control panel against the local tandem client package."
        );
      }
      const response = await client.workflowPlans.chatStart({
        prompt: wizard.goal,
        schedule: toSchedulePayload(wizard),
        plan_source: planSource,
        allowed_mcp_servers: wizard.selectedMcpServers,
        workspace_root: wizard.workspaceRoot,
        operator_preferences: buildOperatorPreferences(wizard),
      });
      return response || null;
    },
    onSuccess: (res) => {
      setPlanPreview(res?.plan || null);
      setPlanningConversation(res?.conversation || null);
      setPlanningChangeSummary([]);
      setPlannerError("");
      setPlannerDiagnostics(res?.planner_diagnostics || res?.plannerDiagnostics || null);
    },
    onError: (error) => {
      setPlanPreview(null);
      setPlanningConversation(null);
      setPlanningChangeSummary([]);
      setPlannerError(error instanceof Error ? error.message : String(error));
      setPlannerDiagnostics(null);
    },
  });

  const planningMessageMutation = useMutation({
    mutationFn: async (message: string) => {
      if (!client?.workflowPlans?.chatMessage || !planPreview?.plan_id) {
        return null;
      }
      return client.workflowPlans.chatMessage({
        plan_id: planPreview.plan_id,
        message,
      });
    },
    onSuccess: (res) => {
      setPlanPreview(res?.plan || null);
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
      if (!client?.workflowPlans?.chatReset || !planPreview?.plan_id) {
        return null;
      }
      return client.workflowPlans.chatReset({
        plan_id: planPreview.plan_id,
      });
    },
    onSuccess: (res) => {
      setPlanPreview(res?.plan || null);
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
      if (!client?.skills?.get || !client?.skills?.validate) {
        return null;
      }
      const loaded = await client.skills.get(skillName);
      const content = (loaded as any)?.content;
      if (!content) {
        return null;
      }
      return client.skills.validate({ content });
    },
    onSuccess: (res) => {
      if (!res) {
        setValidationBadge("");
        return;
      }
      setValidationBadge(res.invalid > 0 ? "not_validated" : "validated");
    },
    onError: () => setValidationBadge("not_validated"),
  });

  const generateSkillMutation = useMutation({
    mutationFn: async () => {
      if (!client?.skills?.generate || !wizard.goal.trim()) {
        return null;
      }
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
      if (!client?.skills?.generateInstall) {
        return null;
      }
      const artifacts = generatedSkill?.artifacts as Record<string, string> | undefined;
      if (!artifacts || !artifacts["SKILL.md"]) {
        throw new Error("No generated artifacts available to install.");
      }
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
      if (!nextPlan) {
        throw new Error("Workflow plan preview failed.");
      }
      return client.workflowPlans.apply({
        plan: nextPlan,
        creator_id: "control-panel",
        ...(wizard.exportPackDraft
          ? {
              pack_builder_export: {
                enabled: true,
                auto_apply: false,
              },
            }
          : {}),
      });
    },
    onSuccess: async (res) => {
      const exportStatus = res?.pack_builder_export?.status;
      if (exportStatus === "preview_pending") {
        toast(
          "ok",
          "🎉 Automation created and reusable pack draft exported. Check Pack Builder to continue."
        );
      } else {
        toast("ok", "🎉 Automation created! Check 'My Automations' to see it running.");
      }
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
        ? !!wizard.schedulePreset || !!wizard.cron.trim()
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
        setWizard((s) => ({
          ...s,
          routedSkill: "",
          routingConfidence: "",
        }));
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
      setWizard((current) => ({
        ...current,
        goal: prompt,
      }));
    } catch {
      // ignore
    }
  }, [step]);

  return (
    <div className="grid gap-4">
      {/* Progress Bar */}
      <div className="flex items-center gap-2">
        {AUTOMATION_WIZARD_CONFIG.steps.map((label, i) => {
          const num = (i + 1) as WizardStep;
          const active = num === step;
          const done = num < step;
          return (
            <div key={label} className="flex-1">
              <button
                className={`mb-1 flex w-full items-center gap-1.5 rounded-lg px-2 py-1 text-xs font-medium transition-all ${
                  active
                    ? "bg-amber-500/20 text-amber-300"
                    : done
                      ? "text-slate-400"
                      : "text-slate-600"
                }`}
                onClick={() => done && setStep(num)}
              >
                <span
                  className={`flex h-5 w-5 items-center justify-center rounded-full text-xs font-bold ${
                    active
                      ? "bg-amber-500 text-black"
                      : done
                        ? "bg-slate-600 text-white"
                        : "bg-slate-800 text-slate-500"
                  }`}
                >
                  {done ? "✓" : num}
                </span>
                {label}
              </button>
              {/* Progress line */}
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

      {/* Step content */}
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
              onGenerateSkill={() => {
                void generateSkillMutation.mutateAsync();
              }}
              onInstallGeneratedSkill={() => {
                void installGeneratedSkillMutation.mutateAsync();
              }}
              isGeneratingSkill={generateSkillMutation.isPending}
              isInstallingSkill={installGeneratedSkillMutation.isPending}
              installStatus={installStatus}
              topMatches={routerMatches}
              isMatching={matchMutation.isPending}
              selectedAgentId={wizard.selectedAgentId}
              onChangeSelectedAgentId={(v) => setWizard((s) => ({ ...s, selectedAgentId: v }))}
            />
          ) : step === 2 ? (
            <Step2Schedule
              selected={wizard.schedulePreset}
              onSelect={(preset) =>
                setWizard((s) => ({
                  ...s,
                  schedulePreset: preset.label,
                  cron: preset.cron,
                }))
              }
              customCron={wizard.cron}
              onCustomCron={(v) => setWizard((s) => ({ ...s, cron: v, schedulePreset: "" }))}
            />
          ) : step === 3 ? (
            <Step3Mode
              selected={wizard.mode}
              onSelect={(mode) => setWizard((s) => ({ ...s, mode }))}
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
            />
          )}
        </motion.div>
      </AnimatePresence>

      {/* Navigation */}
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
            onClick={() => {
              void goToNextStep();
            }}
          >
            {compileMutation.isPending ? "Generating Plan..." : "Next →"}
          </button>
        </div>
      ) : null}
    </div>
  );
}

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
    Array<{ id: string; source: "automations" | "global"; at: number; event: any }>
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
  const [sessionLogPinnedToBottom, setSessionLogPinnedToBottom] = useState(true);
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
    mutationFn: (id: string) => client?.automationsV2?.runNow?.(id),
    onSuccess: async (payload: any) => {
      const runId = String(payload?.run?.run_id || payload?.run?.runId || "").trim();
      toast("ok", "Workflow automation triggered.");
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
      const selectedMcpServers = draft.selectedMcpServers
        .map((row) => String(row || "").trim())
        .filter(Boolean);
      const toolAllowlist = compileWorkflowToolAllowlist(
        selectedMcpServers,
        draft.toolAccessMode,
        draft.customToolsText
      );
      const existing = automationsV2.find(
        (row: any) =>
          String(row?.automation_id || row?.automationId || row?.id || "").trim() ===
          draft.automationId
      );
      const agents = Array.isArray(existing?.agents)
        ? existing.agents.map((agent: any) => ({
            ...agent,
            model_policy: modelPolicy,
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
          }))
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
    () => workflowTaskInspectionDetails(selectedBoardTask, selectedBoardTaskOutput),
    [selectedBoardTask, selectedBoardTaskOutput]
  );
  const selectedBoardTaskTouchedFiles = selectedBoardTaskInspection.touchedFiles;
  const selectedBoardTaskUndeclaredFiles = selectedBoardTaskInspection.undeclaredFiles;
  const selectedBoardTaskResearchReadPaths = selectedBoardTaskInspection.researchReadPaths;
  const selectedBoardTaskDiscoveredRelevantPaths =
    selectedBoardTaskInspection.discoveredRelevantPaths;
  const selectedBoardTaskReviewedPathsBackedByRead =
    selectedBoardTaskInspection.reviewedPathsBackedByRead;
  const selectedBoardTaskUnreviewedRelevantPaths =
    selectedBoardTaskInspection.unreviewedRelevantPaths;
  const selectedBoardTaskUnmetResearchRequirements =
    selectedBoardTaskInspection.unmetResearchRequirements;
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
  const canRecoverWorkflowRun =
    isWorkflowRun && ["failed", "paused"].includes(runStatus) && !!selectedRunId;
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
    if (!selectedBoardTask || !boardDetailRef.current) return;
    boardDetailRef.current.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [selectedBoardTask]);
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
  const selectedBoardTaskVerificationOutcome = selectedBoardTaskInspection.verificationOutcome;
  const selectedBoardTaskVerificationPassed = selectedBoardTaskInspection.verificationPassed;
  const selectedBoardTaskVerificationResults = selectedBoardTaskInspection.verificationResults;
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
  const selectedBoardTaskFailureDetail = selectedBoardTaskInspection.failureDetail;
  const selectedBoardTaskWorkflowClass = selectedBoardTaskInspection.workflowClass;
  const selectedBoardTaskPhase = selectedBoardTaskInspection.phase;
  const selectedBoardTaskFailureKind = selectedBoardTaskInspection.failureKind;
  const selectedBoardTaskWarningCount = selectedBoardTaskInspection.warningCount;
  const selectedBoardTaskWarningRequirements = selectedBoardTaskInspection.warningRequirements;
  const selectedBoardTaskValidationOutcome = selectedBoardTaskInspection.validationOutcome;
  const selectedBoardTaskArtifactCandidates = selectedBoardTaskInspection.artifactCandidates;
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
          workflowEventType,
          workflowEventReason,
          workflowEventAt,
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
      workflowEventType,
      workflowEventAt,
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

  useEffect(() => {
    setSelectedSessionId((current) => {
      if (current && availableSessionIds.includes(current)) return current;
      return availableSessionIds[0] || "";
    });
  }, [availableSessionIds]);

  useEffect(() => {
    setSelectedSessionFilterId((current) => {
      if (current === "all") return current;
      if (current && availableSessionIds.includes(current)) return current;
      return "all";
    });
  }, [availableSessionIds]);

  useEffect(() => {
    setRunEvents([]);
    setSelectedLogSource("all");
    setSelectedBoardTaskId("");
    setSessionEvents([]);
    setSessionLogPinnedToBottom(true);
  }, [selectedRunId, selectedContextRunId]);

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
      ? `/api/event?sessionID=${encodeURIComponent(selectedSessionId)}&runID=${encodeURIComponent(selectedRunId)}`
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

  const renderCalendarEventContent = (arg: any) => {
    const status = String(arg?.event?.extendedProps?.status || "active").trim() || "active";
    const scheduleLabel = String(arg?.event?.extendedProps?.scheduleLabel || "").trim();
    return (
      <div className="flex h-full min-h-0 flex-col gap-0.5 overflow-hidden rounded-lg border border-slate-700/60 bg-slate-950/90 px-2 py-1 text-xs shadow-sm">
        <div className="flex items-center justify-between gap-2">
          <span className="truncate font-medium text-slate-100">
            {String(arg?.event?.title || "")}
          </span>
          <span className={statusColor(status)}>{status}</span>
        </div>
        <div className="truncate text-[11px] text-slate-400">{scheduleLabel}</div>
      </div>
    );
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
        <div className="grid gap-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="grid gap-1">
              <p className="text-xs font-medium uppercase tracking-wide text-slate-500">
                Cron schedules
              </p>
              <div className="tcp-subtle text-xs">
                Drag a card to change when that automation fires. Only cron-based automations are
                shown here for now.
              </div>
            </div>
            <span className="tcp-badge-info">{calendarEvents.length} scheduled items</span>
          </div>
          <div className="overflow-hidden rounded-2xl border border-slate-800/80 bg-slate-950/35 p-2">
            <FullCalendar
              plugins={[timeGridPlugin, interactionPlugin]}
              initialView="timeGridWeek"
              timeZone="UTC"
              firstDay={0}
              height="auto"
              expandRows
              nowIndicator
              editable
              eventStartEditable
              eventDurationEditable={false}
              eventOverlap
              slotEventOverlap
              allDaySlot={false}
              slotMinTime="00:00:00"
              slotMaxTime="24:00:00"
              stickyHeaderDates
              headerToolbar={{
                left: "prev,next today",
                center: "title",
                right: "",
              }}
              events={calendarEvents}
              datesSet={(arg: any) => {
                setCalendarRange({
                  startMs: arg.start.getTime(),
                  endMs: arg.end.getTime(),
                });
              }}
              eventClick={(arg: any) => {
                arg.jsEvent?.preventDefault?.();
                openCalendarAutomationEdit(arg.event?.extendedProps?.automation);
              }}
              eventDrop={updateCalendarAutomationFromEvent}
              eventContent={renderCalendarEventContent}
              eventClassNames={(arg: any) => [
                String(arg?.event?.extendedProps?.family || "legacy") === "v2"
                  ? "tcp-calendar-event tcp-calendar-event-v2"
                  : "tcp-calendar-event tcp-calendar-event-legacy",
              ]}
            />
          </div>
        </div>
      ) : null}

      {/* Installed packs from pack_builder */}
      {viewMode === "list" && packs.length > 0 ? (
        <div className="grid gap-2">
          <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
            Installed Packs
          </p>
          {packs.map((pack: any, i: number) => (
            <div key={String(pack?.id || pack?.name || i)} className="tcp-list-item">
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  <span>📦</span>
                  <strong>{String(pack?.name || pack?.id || "Pack")}</strong>
                </div>
                <span className="tcp-badge-info">{String(pack?.version || "1.0.0")}</span>
              </div>
              <div className="tcp-subtle text-xs mt-1">
                {String(pack?.description || pack?.path || "")}
              </div>
            </div>
          ))}
        </div>
      ) : null}

      {viewMode === "list" ? (
        <div className="grid gap-2">
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

      {viewMode === "list" ? (
        <div className="grid gap-2">
          <div className="flex items-center justify-between gap-2">
            <p className="text-[11px] font-medium uppercase tracking-[0.24em] text-slate-500">
              Workflow Automations
            </p>
            <span className="tcp-subtle text-xs">{workflowAutomationCount} items</span>
          </div>
          {automationsV2.length > 0 ? (
            automationsV2.map((automation: any) => {
              const id = String(automation?.automation_id || automation?.automationId || "").trim();
              const status = String(automation?.status || "draft").trim();
              const paused = status.toLowerCase() === "paused";
              const standup = isStandupAutomation(automation);
              return (
                <div key={id} className="tcp-list-item">
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <div className="flex items-center gap-2">
                      <span>🧩</span>
                      <strong>{String(automation?.name || id || "Workflow automation")}</strong>
                      {standup ? <span className="tcp-badge-ok">Standup</span> : null}
                    </div>
                    <div className="flex items-center gap-2">
                      <button
                        className="tcp-btn h-7 px-2 text-xs"
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
                        <i data-lucide="pencil"></i>
                      </button>
                      <span className={statusColor(status)}>{status}</span>
                    </div>
                  </div>
                  {String(automation?.description || "").trim() ? (
                    <div className="tcp-subtle text-xs">{String(automation.description)}</div>
                  ) : null}
                  {standup ? (
                    <div className="mt-1 text-xs text-emerald-200">
                      report: {String(automation?.metadata?.standup?.report_path_template || "")}
                    </div>
                  ) : null}
                  <div className="tcp-subtle mt-1 text-xs">
                    {formatAutomationV2ScheduleLabel(automation?.schedule)}
                  </div>
                  <div className="mt-2 flex flex-wrap gap-2">
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() => runNowV2Mutation.mutate(id)}
                      disabled={!id || runNowV2Mutation.isPending}
                    >
                      <i data-lucide="play"></i>
                      {runNowV2Mutation.isPending ? "Starting..." : "Run now"}
                    </button>
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() =>
                        automationActionMutation.mutate({
                          action: paused ? "resume" : "pause",
                          automationId: id,
                          family: "v2",
                        })
                      }
                      disabled={!id || automationActionMutation.isPending}
                    >
                      <i data-lucide={paused ? "play" : "pause"}></i>
                      {paused ? "Resume" : "Pause"}
                    </button>
                    <button
                      className="tcp-btn-danger h-7 px-2 text-xs"
                      onClick={() =>
                        setDeleteConfirm({
                          automationId: id,
                          family: "v2",
                          title: String(automation?.name || id || "workflow automation"),
                        })
                      }
                      disabled={!id || automationActionMutation.isPending}
                    >
                      <i data-lucide="trash-2"></i>
                      Remove
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
      ) : null}

      {viewMode === "running" ? (
        activeRuns.length > 0 ? (
          <div className="grid gap-2">
            <div className="flex items-center justify-between gap-2">
              <p className="text-xs font-medium uppercase tracking-wide text-slate-500">
                Active Running Tasks
              </p>
              <span className="tcp-badge-warn">{activeRuns.length} active</span>
            </div>
            {activeRuns.slice(0, 14).map((run: any, index: number) => {
              const runId = String(run?.run_id || run?.id || index).trim();
              const runStatus = workflowDerivedRunStatus(run);
              const startedAt =
                run?.started_at_ms || run?.startedAtMs || run?.created_at_ms || run?.createdAtMs;
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
            const runStatus = workflowDerivedRunStatus(run);
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
      {runs.length > 0 ? (
        <div className="grid gap-2">
          <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
            {viewMode === "running" ? "Run Log Explorer" : "Recent Runs"}
          </p>
          {runs.slice(0, 12).map((run: any, index: number) => (
            <div key={String(run?.run_id || run?.id || index)} className="tcp-list-item">
              <div className="flex items-center justify-between gap-2">
                <span className="font-medium text-sm">{runDisplayTitle(run)}</span>
                <span className={statusColor(workflowDerivedRunStatus(run))}>
                  {workflowDerivedRunStatus(run) || "unknown"}
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
      <AnimatePresence>
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
                        workflowRecoverMutation.mutate({
                          runId: selectedRunId,
                          reason: "retried from run debugger",
                        })
                      }
                      disabled={
                        !selectedRunId ||
                        workflowRecoverMutation.isPending ||
                        runActionMutation.isPending
                      }
                    >
                      <i data-lucide="rotate-ccw"></i>
                      {workflowRecoverMutation.isPending ? "Retrying..." : "Retry"}
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
                    onClick={() => onSelectRunId("")}
                  >
                    <i data-lucide="x"></i>
                    Close
                  </button>
                </div>
              </div>
              <div className="flex-1 min-h-0 overflow-y-auto pr-1">
                <div className="grid min-h-full content-start gap-3">
                  <div className="tcp-list-item overflow-visible">
                    <div className="font-medium">Run Summary</div>
                    <div className="mt-2 grid gap-2 text-xs text-slate-300 sm:grid-cols-2 xl:grid-cols-4">
                      {runSummaryRows.map((row) => (
                        <div key={row.label} className="break-words">
                          {row.label}: {row.value}
                        </div>
                      ))}
                    </div>
                  </div>
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
                                  backlog:{" "}
                                  {String((selectedBoardTask as any).backlog_task_id).trim()}
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
                                      selectedBoardTaskValidationOutcome ===
                                        "accepted_with_warnings" ||
                                      String(selectedBoardTaskOutput?.status || "")
                                        .trim()
                                        .toLowerCase() === "blocked"
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
                                  selectedBoardTaskArtifactValidation?.rejected_artifact_reason ||
                                    ""
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
                                              <span className="tcp-badge-warn">
                                                non-substantive
                                              </span>
                                            )}
                                            {candidate?.placeholder_like ? (
                                              <span className="tcp-badge-warn">
                                                placeholder-like
                                              </span>
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
                                              <span className="tcp-badge-info">
                                                {summary.event}
                                              </span>
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
                                <div className="font-medium text-slate-200">
                                  Coding Task Context
                                </div>
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
                                    <div className="tcp-subtle">
                                      actual read calls backed by path
                                    </div>
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
                                      /{" "}
                                      {Number(selectedBoardTaskTelemetry?.verification_total || 0)}{" "}
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
                                        No matching run artifact found yet. The button will still
                                        jump to the artifacts section.
                                      </div>
                                    )}
                                  </div>
                                ) : null}
                                {(canTaskContinue ||
                                  canTaskRetry ||
                                  canTaskRequeue ||
                                  canBacklogTaskClaim ||
                                  canBacklogTaskRequeue ||
                                  canRecoverWorkflowRun) &&
                                selectedRunId ? (
                                  <div className="mt-3 space-y-3">
                                    {selectedBoardTaskIsWorkflowNode ? (
                                      <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2 text-[11px] text-slate-300">
                                        <div className="font-medium text-slate-100">
                                          Action impact
                                        </div>
                                        <div className="mt-1 tcp-subtle">
                                          {taskResetPreviewQuery.isLoading
                                            ? "Loading engine preview..."
                                            : selectedBoardTaskImpactSummary.previewBacked
                                              ? "Engine preview"
                                              : "UI-estimated preview"}
                                        </div>
                                        <div className="mt-1">
                                          Selected task: {selectedBoardTaskImpactSummary.rootTitle}
                                        </div>
                                        <div>
                                          Reset scope:{" "}
                                          {canTaskContinue
                                            ? "minimal reset of the blocked task"
                                            : `${selectedBoardTaskImpactSummary.subtreeCount} task${
                                                selectedBoardTaskImpactSummary.subtreeCount === 1
                                                  ? ""
                                                  : "s"
                                              }${
                                                selectedBoardTaskImpactSummary.descendantCount > 0
                                                  ? ` (${selectedBoardTaskImpactSummary.descendantCount} descendant${
                                                      selectedBoardTaskImpactSummary.descendantCount ===
                                                      1
                                                        ? ""
                                                        : "s"
                                                    })`
                                                  : ""
                                              }`}
                                        </div>
                                        <div>
                                          Preserves:{" "}
                                          {selectedBoardTaskImpactSummary.preservesUpstreamOutputs
                                            ? "completed upstream outputs outside this subtree"
                                            : "n/a"}
                                        </div>
                                        <div>
                                          Clears: stale outputs for{" "}
                                          {selectedBoardTaskImpactSummary.outputCount} declared
                                          artifact
                                          {selectedBoardTaskImpactSummary.outputCount === 1
                                            ? ""
                                            : "s"}
                                        </div>
                                        {selectedBoardTaskResetOutputPaths.length ? (
                                          <div className="mt-2 flex flex-wrap gap-1">
                                            {selectedBoardTaskResetOutputPaths.map((path) => (
                                              <span
                                                key={path}
                                                className="rounded-full border border-slate-700/70 bg-slate-950/30 px-2 py-1 font-mono text-[11px] text-slate-300"
                                              >
                                                {path}
                                              </span>
                                            ))}
                                          </div>
                                        ) : null}
                                      </div>
                                    ) : selectedBoardTaskIsProjectedBacklogItem ? (
                                      <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2 text-[11px] text-slate-300">
                                        <div className="font-medium text-slate-100">
                                          Action impact
                                        </div>
                                        <div className="mt-1">
                                          Claiming assigns this backlog task to an agent without
                                          resetting any workflow nodes.
                                        </div>
                                        <div>
                                          Requeueing clears stale lease state and returns the task
                                          to the runnable queue.
                                        </div>
                                        <div>
                                          Upstream workflow artifacts are preserved because this
                                          acts on the projected backlog task only.
                                        </div>
                                      </div>
                                    ) : null}
                                    <div className="flex flex-wrap gap-2">
                                      {canBacklogTaskClaim ? (
                                        <div className="space-y-1">
                                          <button
                                            type="button"
                                            className="tcp-btn h-8 px-3 text-xs"
                                            onClick={() =>
                                              backlogTaskClaimMutation.mutate({
                                                runId: selectedRunId,
                                                taskId: String(selectedBoardTask.id || ""),
                                                agentId:
                                                  String(
                                                    (selectedBoardTask as any).task_owner || ""
                                                  ).trim() || undefined,
                                                reason: `claimed backlog task ${String(selectedBoardTask.id || "")} from debugger`,
                                              })
                                            }
                                            disabled={
                                              backlogTaskClaimMutation.isPending ||
                                              backlogTaskRequeueMutation.isPending
                                            }
                                          >
                                            {backlogTaskClaimMutation.isPending
                                              ? "Claiming..."
                                              : "Claim Task"}
                                          </button>
                                          <div className="tcp-subtle text-[11px]">
                                            Assign this projected coding task and start its lease.
                                          </div>
                                        </div>
                                      ) : null}
                                      {canBacklogTaskRequeue ? (
                                        <div className="space-y-1">
                                          <button
                                            type="button"
                                            className="tcp-btn h-8 px-3 text-xs"
                                            onClick={() =>
                                              backlogTaskRequeueMutation.mutate({
                                                runId: selectedRunId,
                                                taskId: String(selectedBoardTask.id || ""),
                                                reason: `requeued backlog task ${String(selectedBoardTask.id || "")} from debugger`,
                                              })
                                            }
                                            disabled={
                                              backlogTaskClaimMutation.isPending ||
                                              backlogTaskRequeueMutation.isPending
                                            }
                                          >
                                            {backlogTaskRequeueMutation.isPending
                                              ? "Requeueing..."
                                              : "Requeue Backlog Task"}
                                          </button>
                                          <div className="tcp-subtle text-[11px]">
                                            Use when the task is blocked, failed, or its lease went
                                            stale.
                                          </div>
                                        </div>
                                      ) : null}
                                      {canTaskContinue ? (
                                        <div className="space-y-1">
                                          <button
                                            type="button"
                                            className="tcp-btn h-8 px-3 text-xs"
                                            onClick={() =>
                                              workflowTaskContinueMutation.mutate({
                                                runId: selectedRunId,
                                                nodeId: selectedBoardTaskNodeId,
                                                reason: `continued blocked task ${selectedBoardTaskNodeId} from debugger`,
                                              })
                                            }
                                            disabled={
                                              workflowTaskContinueMutation.isPending ||
                                              workflowTaskRetryMutation.isPending ||
                                              workflowTaskRequeueMutation.isPending ||
                                              backlogTaskClaimMutation.isPending ||
                                              backlogTaskRequeueMutation.isPending
                                            }
                                          >
                                            {workflowTaskContinueMutation.isPending
                                              ? "Continuing..."
                                              : "Continue Task"}
                                          </button>
                                          <div className="tcp-subtle text-[11px]">
                                            Minimal reset: reruns the blocked task itself and
                                            preserves descendants unless they need to rerun later.
                                          </div>
                                        </div>
                                      ) : null}
                                      {canTaskRetry ? (
                                        <div className="space-y-1">
                                          <button
                                            type="button"
                                            className="tcp-btn h-8 px-3 text-xs"
                                            onClick={() =>
                                              workflowTaskRetryMutation.mutate({
                                                runId: selectedRunId,
                                                nodeId: selectedBoardTaskNodeId,
                                                reason: `retried task ${selectedBoardTaskNodeId} from debugger`,
                                              })
                                            }
                                            disabled={
                                              workflowTaskContinueMutation.isPending ||
                                              workflowTaskRetryMutation.isPending ||
                                              workflowTaskRequeueMutation.isPending ||
                                              backlogTaskClaimMutation.isPending ||
                                              backlogTaskRequeueMutation.isPending
                                            }
                                          >
                                            {workflowTaskRetryMutation.isPending
                                              ? "Retrying task..."
                                              : "Retry Task"}
                                          </button>
                                          <div className="tcp-subtle text-[11px]">
                                            Best for blocked or failed work that should rerun from
                                            this task downward.
                                          </div>
                                        </div>
                                      ) : null}
                                      {canTaskRequeue ? (
                                        <div className="space-y-1">
                                          <button
                                            type="button"
                                            className="tcp-btn h-8 px-3 text-xs"
                                            onClick={() =>
                                              workflowTaskRequeueMutation.mutate({
                                                runId: selectedRunId,
                                                nodeId: selectedBoardTaskNodeId,
                                                reason: `requeued task ${selectedBoardTaskNodeId} from debugger`,
                                              })
                                            }
                                            disabled={
                                              workflowTaskContinueMutation.isPending ||
                                              workflowTaskRetryMutation.isPending ||
                                              workflowTaskRequeueMutation.isPending ||
                                              backlogTaskClaimMutation.isPending ||
                                              backlogTaskRequeueMutation.isPending
                                            }
                                          >
                                            {workflowTaskRequeueMutation.isPending
                                              ? "Requeueing..."
                                              : "Requeue Task"}
                                          </button>
                                          <div className="tcp-subtle text-[11px]">
                                            Use when this task should go back onto the queue with
                                            its descendants reset.
                                          </div>
                                        </div>
                                      ) : null}
                                      {selectedBoardTaskStateNormalized === "blocked" &&
                                      continueBlockedNodeId ? (
                                        <div className="space-y-1">
                                          <button
                                            type="button"
                                            className="tcp-btn h-8 px-3 text-xs"
                                            onClick={() =>
                                              workflowRepairMutation.mutate({
                                                runId: selectedRunId,
                                                nodeId: continueBlockedNodeId,
                                                reason: `continued from blocked node ${continueBlockedNodeId}`,
                                              })
                                            }
                                            disabled={
                                              workflowTaskContinueMutation.isPending ||
                                              workflowRepairMutation.isPending ||
                                              !continueBlockedNodeId
                                            }
                                          >
                                            {workflowRepairMutation.isPending
                                              ? "Repairing..."
                                              : "Repair Blocked Step"}
                                          </button>
                                          <div className="tcp-subtle text-[11px]">
                                            Heavier reset/repair flow for blocked nodes when minimal
                                            continue is not enough.
                                          </div>
                                        </div>
                                      ) : null}
                                      {canRecoverWorkflowRun ? (
                                        <div className="space-y-1">
                                          <button
                                            type="button"
                                            className="tcp-btn h-8 px-3 text-xs"
                                            onClick={() =>
                                              workflowRecoverMutation.mutate({
                                                runId: selectedRunId,
                                                reason: `retried from task ${String(selectedBoardTask.id || "").replace(/^node-/, "")}`,
                                              })
                                            }
                                            disabled={workflowRecoverMutation.isPending}
                                          >
                                            {workflowRecoverMutation.isPending
                                              ? "Retrying..."
                                              : "Retry Workflow"}
                                          </button>
                                          <div className="tcp-subtle text-[11px]">
                                            Recover the whole run, not just this task subtree.
                                          </div>
                                        </div>
                                      ) : null}
                                    </div>
                                  </div>
                                ) : null}
                              </div>
                            ) : null}
                            {selectedBoardTaskArtifactValidation ||
                            selectedBoardTaskTelemetry ||
                            selectedBoardTaskTouchedFiles.length ||
                            selectedBoardTaskUndeclaredFiles.length ? (
                              <div className="rounded-lg border border-slate-700/60 bg-slate-950/20 p-3 text-xs text-slate-300">
                                <div className="font-medium text-slate-200">Coding Signals</div>
                                <div className="mt-3 grid gap-2 sm:grid-cols-2">
                                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                    <div className="tcp-subtle">execution mode</div>
                                    <div className="mt-1 font-medium text-slate-100">
                                      {String(
                                        selectedBoardTaskArtifactValidation?.execution_policy
                                          ?.mode || ""
                                      ).trim() || "n/a"}
                                    </div>
                                  </div>
                                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                    <div className="tcp-subtle">git diff</div>
                                    <div className="mt-1 font-medium text-slate-100">
                                      {String(
                                        selectedBoardTaskArtifactValidation?.git_diff_summary
                                          ?.stat || ""
                                      ).trim() || "n/a"}
                                    </div>
                                  </div>
                                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                    <div className="tcp-subtle">workspace inspection</div>
                                    <div className="mt-1 font-medium text-slate-100">
                                      {selectedBoardTaskTelemetry?.workspace_inspection_used
                                        ? "yes"
                                        : "no"}
                                    </div>
                                  </div>
                                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                                    <div className="tcp-subtle">web research</div>
                                    <div className="mt-1 font-medium text-slate-100">
                                      {selectedBoardTaskTelemetry?.web_research_used ? "yes" : "no"}
                                    </div>
                                  </div>
                                </div>
                                <div className="mt-3 space-y-2">
                                  <div>
                                    <div className="tcp-subtle mb-1">touched files</div>
                                    {selectedBoardTaskTouchedFiles.length ? (
                                      <div className="flex flex-wrap gap-1">
                                        {selectedBoardTaskTouchedFiles.map((file) => (
                                          <span
                                            key={file}
                                            className="rounded-full border border-slate-700/70 bg-slate-950/30 px-2 py-1 font-mono text-[11px] text-slate-300"
                                          >
                                            {file}
                                          </span>
                                        ))}
                                      </div>
                                    ) : (
                                      <div className="tcp-subtle">none</div>
                                    )}
                                  </div>
                                  <div>
                                    <div className="tcp-subtle mb-1">undeclared files</div>
                                    {selectedBoardTaskUndeclaredFiles.length ? (
                                      <div className="flex flex-wrap gap-1">
                                        {selectedBoardTaskUndeclaredFiles.map((file) => (
                                          <span
                                            key={file}
                                            className="rounded-full border border-amber-500/30 bg-amber-950/20 px-2 py-1 font-mono text-[11px] text-amber-100"
                                          >
                                            {file}
                                          </span>
                                        ))}
                                      </div>
                                    ) : (
                                      <div className="tcp-subtle">none</div>
                                    )}
                                  </div>
                                </div>
                              </div>
                            ) : null}
                            {String(selectedBoardTask?.state || "").toLowerCase() === "blocked" ? (
                              <div className="rounded-lg border border-slate-700/60 bg-slate-950/30 p-3 text-xs text-slate-300">
                                Continue resets this blocked step and its descendants, preserves
                                valid upstream outputs, and clears stale descendant artifacts before
                                requeue.
                              </div>
                            ) : null}
                            {selectedBoardTaskTelemetry ? (
                              <div className="rounded-lg border border-slate-700/60 bg-slate-950/30 p-3 text-xs text-slate-300">
                                <div className="mb-2 font-medium text-slate-100">Node Tooling</div>
                                <div className="grid gap-1">
                                  <div>
                                    offered:{" "}
                                    {Array.isArray(selectedBoardTaskTelemetry?.requested_tools)
                                      ? selectedBoardTaskTelemetry.requested_tools.join(", ") ||
                                        "n/a"
                                      : "n/a"}
                                  </div>
                                  <div>
                                    executed:{" "}
                                    {Array.isArray(selectedBoardTaskTelemetry?.executed_tools)
                                      ? selectedBoardTaskTelemetry.executed_tools.join(", ") ||
                                        "none"
                                      : "none"}
                                  </div>
                                  <div>
                                    workspace inspection:{" "}
                                    {selectedBoardTaskTelemetry?.workspace_inspection_used
                                      ? "yes"
                                      : "no"}
                                  </div>
                                  <div>
                                    web research:{" "}
                                    {selectedBoardTaskTelemetry?.web_research_used ? "yes" : "no"}
                                  </div>
                                </div>
                              </div>
                            ) : null}
                            {selectedBoardTaskArtifactValidation ? (
                              <div className="rounded-lg border border-slate-700/60 bg-slate-950/30 p-3 text-xs text-slate-300">
                                <div className="mb-2 font-medium text-slate-100">
                                  Artifact Validation
                                </div>
                                <div className="grid gap-1">
                                  <div>
                                    accepted path:{" "}
                                    {String(
                                      selectedBoardTaskArtifactValidation?.accepted_artifact_path ||
                                        ""
                                    ).trim() || "n/a"}
                                  </div>
                                  <div>
                                    rejected reason:{" "}
                                    {String(
                                      selectedBoardTaskArtifactValidation?.rejected_artifact_reason ||
                                        ""
                                    ).trim() || "none"}
                                  </div>
                                  <div>
                                    auto-cleaned:{" "}
                                    {String(
                                      Boolean(selectedBoardTaskArtifactValidation?.auto_cleaned)
                                    )}
                                  </div>
                                  <div>
                                    undeclared files:{" "}
                                    {selectedBoardTaskUndeclaredFiles.length
                                      ? selectedBoardTaskUndeclaredFiles.join(", ")
                                      : "none"}
                                  </div>
                                  <div>
                                    execution policy:{" "}
                                    {String(
                                      selectedBoardTaskArtifactValidation?.execution_policy?.mode ||
                                        ""
                                    ).trim() || "n/a"}
                                  </div>
                                  <div>
                                    touched files:{" "}
                                    {selectedBoardTaskTouchedFiles.length
                                      ? selectedBoardTaskTouchedFiles.join(", ")
                                      : "none"}
                                  </div>
                                  <div>
                                    git diff:{" "}
                                    {String(
                                      selectedBoardTaskArtifactValidation?.git_diff_summary?.stat ||
                                        ""
                                    ).trim() || "n/a"}
                                  </div>
                                </div>
                              </div>
                            ) : null}
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
                      <div className="tcp-list-item min-h-0 xl:order-2">
                        <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                          <div>
                            <div className="font-medium">Live Session Log</div>
                            <div className="tcp-subtle text-xs">
                              {selectedSessionId
                                ? selectedSessionFilterId === "all"
                                  ? `Merged timeline across ${availableSessionIds.length || 1} session${availableSessionIds.length === 1 ? "" : "s"}`
                                  : `Filtered to ${selectedSessionFilterId}`
                                : "This run does not expose a session transcript."}
                            </div>
                          </div>
                          <div className="flex flex-wrap gap-2">
                            {availableSessionIds.length > 1 ? (
                              <select
                                className="tcp-select h-7 min-w-[12rem] max-w-full shrink-0 text-xs sm:min-w-[14rem]"
                                value={selectedSessionFilterId}
                                onInput={(e) =>
                                  setSelectedSessionFilterId((e.target as HTMLSelectElement).value)
                                }
                              >
                                <option value="all">All sessions</option>
                                {availableSessionIds.map((sessionId) => (
                                  <option key={sessionId} value={sessionId} title={sessionId}>
                                    {sessionLabel(sessionId)}
                                  </option>
                                ))}
                              </select>
                            ) : selectedSessionId ? (
                              <span className="tcp-badge-info" title={selectedSessionId}>
                                {sessionLabel(selectedSessionId)}
                              </span>
                            ) : null}
                            {selectedSessionId ? (
                              <span className="tcp-badge-info" title={selectedSessionId}>
                                live: {compactIdentifier(selectedSessionId, 24)}
                              </span>
                            ) : null}
                            <button
                              className="tcp-btn h-7 px-2 text-xs"
                              disabled={!sessionLogEntries.length}
                              onClick={async () => {
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
                                  toast(
                                    "err",
                                    error instanceof Error ? error.message : "Copy failed."
                                  );
                                }
                              }}
                            >
                              <i data-lucide="copy"></i>
                              Copy session log
                            </button>
                            <button
                              className="tcp-btn h-7 px-2 text-xs"
                              onClick={() => {
                                setSessionLogPinnedToBottom(true);
                                const el = sessionLogRef.current;
                                if (el) el.scrollTop = el.scrollHeight;
                              }}
                            >
                              <i data-lucide="arrow-down"></i>
                              Jump to latest
                            </button>
                          </div>
                        </div>
                        <div
                          ref={sessionLogRef}
                          className="grid min-h-[12rem] gap-2 overflow-auto pr-1 sm:min-h-[14rem] sm:max-h-[18rem]"
                          onScroll={(event) => {
                            const el = event.currentTarget;
                            const pinned = el.scrollHeight - (el.scrollTop + el.clientHeight) < 48;
                            setSessionLogPinnedToBottom(pinned);
                          }}
                        >
                          {sessionLogEntries.length ? (
                            sessionLogEntries.map((entry) => (
                              <div
                                key={entry.id}
                                className={`rounded-lg border p-3 ${
                                  entry.variant === "assistant"
                                    ? "border-sky-500/30 bg-sky-950/10"
                                    : entry.variant === "user"
                                      ? "border-slate-600/60 bg-slate-900/35"
                                      : entry.variant === "error"
                                        ? "border-rose-500/35 bg-rose-950/20"
                                        : "border-slate-700/50 bg-slate-900/25"
                                }`}
                              >
                                <div className="mb-1 flex flex-wrap items-center justify-between gap-2">
                                  <div className="flex flex-wrap items-center gap-2">
                                    <span className="text-xs font-medium uppercase tracking-wide text-slate-200">
                                      {entry.label}
                                    </span>
                                    {entry.sessionId ? (
                                      <span className="tcp-badge-info text-[10px]">
                                        {entry.sessionLabel}
                                      </span>
                                    ) : null}
                                  </div>
                                  <span className="tcp-subtle text-[11px]">
                                    {new Date(entry.at).toLocaleTimeString()}
                                  </span>
                                </div>
                                {entry.body ? (
                                  <div className="whitespace-pre-wrap break-words text-sm text-slate-100">
                                    {entry.body}
                                  </div>
                                ) : (
                                  <div className="tcp-subtle text-xs">No textual body.</div>
                                )}
                                {entry.kind === "message" &&
                                entry.parts.some(
                                  (part: any) => String(part?.type || "") === "tool"
                                ) ? (
                                  <details className="mt-2">
                                    <summary className="cursor-pointer text-xs text-slate-400">
                                      Tool payloads
                                    </summary>
                                    <pre className="tcp-code mt-2 max-h-40 overflow-auto text-[11px]">
                                      {formatJson(entry.parts)}
                                    </pre>
                                  </details>
                                ) : null}
                                {entry.kind === "event" ? (
                                  <details className="mt-2">
                                    <summary className="cursor-pointer text-xs text-slate-400">
                                      Raw event
                                    </summary>
                                    <pre className="tcp-code mt-2 max-h-40 overflow-auto text-[11px]">
                                      {formatJson(entry.raw)}
                                    </pre>
                                  </details>
                                ) : null}
                              </div>
                            ))
                          ) : (
                            <div className="tcp-subtle text-xs">
                              {availableSessionIds.length
                                ? "Waiting for session transcript or live session events."
                                : "This run does not expose a session transcript."}
                            </div>
                          )}
                        </div>
                      </div>
                      <div className="tcp-list-item min-h-0 xl:order-3">
                        <div className="mb-2 flex items-center justify-between gap-2">
                          <div className="font-medium">Run Telemetry</div>
                          <div className="flex w-full flex-wrap gap-1 sm:w-auto">
                            <button
                              className={`tcp-btn h-7 flex-1 px-2 text-[11px] sm:flex-none ${selectedLogSource === "all" ? "border-amber-400/60 bg-amber-400/10 text-amber-300" : ""}`}
                              onClick={() => setSelectedLogSource("all")}
                            >
                              all ({telemetryEvents.length})
                            </button>
                            <button
                              className={`tcp-btn h-7 flex-1 px-2 text-[11px] sm:flex-none ${selectedLogSource === "automations" ? "border-amber-400/60 bg-amber-400/10 text-amber-300" : ""}`}
                              onClick={() => setSelectedLogSource("automations")}
                            >
                              automations
                            </button>
                            {isWorkflowRun ? (
                              <button
                                className={`tcp-btn h-7 flex-1 px-2 text-[11px] sm:flex-none ${selectedLogSource === "context" ? "border-amber-400/60 bg-amber-400/10 text-amber-300" : ""}`}
                                onClick={() => setSelectedLogSource("context")}
                              >
                                context
                              </button>
                            ) : null}
                            <button
                              className={`tcp-btn h-7 flex-1 px-2 text-[11px] sm:flex-none ${selectedLogSource === "global" ? "border-amber-400/60 bg-amber-400/10 text-amber-300" : ""}`}
                              onClick={() => setSelectedLogSource("global")}
                            >
                              global
                            </button>
                          </div>
                        </div>
                        {filteredRunEventEntries.length ? (
                          <div className="grid gap-2 overflow-auto pr-1 sm:max-h-[12rem]">
                            {filteredRunEventEntries
                              .slice(-40)
                              .reverse()
                              .map((item) => (
                                <details
                                  key={item.id}
                                  className="rounded-lg border border-slate-700/40 bg-slate-900/30 p-2"
                                >
                                  <summary className="cursor-pointer list-none">
                                    <div className="flex items-center justify-between gap-2">
                                      <span className="text-xs font-medium text-slate-200">
                                        {item.label}
                                      </span>
                                      <span className="tcp-subtle text-[11px]">
                                        {formatTimestampLabel(item.at)} · {item.source}
                                      </span>
                                    </div>
                                    <div className="tcp-subtle mt-1 text-xs">{item.detail}</div>
                                  </summary>
                                  <pre className="tcp-code mt-2 max-h-40 overflow-auto text-[11px]">
                                    {formatJson(item.raw)}
                                  </pre>
                                </details>
                              ))}
                          </div>
                        ) : (
                          <div className="tcp-subtle text-xs">
                            {isWorkflowRun
                              ? "No workflow, context, or global telemetry has been captured for this run yet."
                              : "No automation/global telemetry captured for this run yet."}
                          </div>
                        )}
                      </div>
                    </div>
                    <div className="grid min-h-0 content-start gap-3 overflow-visible">
                      {blockers.length ? (
                        <div className="tcp-list-item overflow-visible">
                          <div className="mb-2 font-medium">Blockers</div>
                          <div className="grid gap-2">
                            {blockers.map((blocker) => (
                              <div
                                key={blocker.key}
                                className="rounded-lg border border-emerald-500/30 bg-emerald-950/20 p-3"
                              >
                                <div className="mb-1 flex flex-wrap items-center gap-2">
                                  <strong>{blocker.title}</strong>
                                  <span className="border border-emerald-400/60 bg-emerald-400/10 text-emerald-200 tcp-badge">
                                    {blocker.source}
                                  </span>
                                  {blocker.at ? (
                                    <span className="tcp-subtle text-[11px]">
                                      {new Date(blocker.at).toLocaleTimeString()}
                                    </span>
                                  ) : null}
                                </div>
                                <div className="whitespace-pre-wrap break-words text-sm text-emerald-100/90">
                                  {blocker.reason}
                                </div>
                              </div>
                            ))}
                          </div>
                        </div>
                      ) : null}
                      {runHints.length ? (
                        <div className="tcp-list-item overflow-visible">
                          <div className="mb-1 font-medium">Debug hints</div>
                          <div className="grid gap-1 text-xs text-slate-300">
                            {runHints.map((hint) => (
                              <div key={hint}>{hint}</div>
                            ))}
                          </div>
                        </div>
                      ) : null}
                      {runRepairGuidanceEntries.length ? (
                        <div className="tcp-list-item overflow-visible">
                          <div className="mb-2 flex items-center justify-between gap-2">
                            <div className="font-medium">Next Required Actions</div>
                            <span className="border border-emerald-400/60 bg-emerald-400/10 text-emerald-200 tcp-badge">
                              {runRepairGuidanceEntries.length} node
                              {runRepairGuidanceEntries.length === 1 ? "" : "s"}
                            </span>
                          </div>
                          <div className="grid gap-2">
                            {runRepairGuidanceEntries.map(({ nodeId, guidance }) => {
                              const actions = Array.isArray(guidance?.requiredNextToolActions)
                                ? guidance.requiredNextToolActions
                                : [];
                              const unmet = Array.isArray(guidance?.unmetRequirements)
                                ? guidance.unmetRequirements
                                : [];
                              const reason = String(guidance?.reason || "").trim();
                              const blockingClassification = String(
                                guidance?.blockingClassification || ""
                              ).trim();
                              const failureKind = String(guidance?.failureKind || "").trim();
                              const status = String(guidance?.status || "").trim();
                              const normalizedStatus = status.toLowerCase();
                              const hasActiveSessions = workflowActiveSessionCount(selectedRun) > 0;
                              const canGuidanceRetry =
                                isWorkflowRun &&
                                !!selectedRunId &&
                                !hasActiveSessions &&
                                (["blocked", "failed", "needs_repair"].includes(normalizedStatus) ||
                                  // Also allow retry on completed nodes that appear in guidance
                                  // because they have unmet requirements (upstream quality issues)
                                  (normalizedStatus === "completed" &&
                                    (actions.length > 0 || unmet.length > 0)));
                              const canGuidanceContinue =
                                isWorkflowRun &&
                                !!selectedRunId &&
                                !hasActiveSessions &&
                                ["blocked", "needs_repair"].includes(normalizedStatus);
                              return (
                                <div
                                  key={nodeId}
                                  className="rounded-lg border border-emerald-500/30 bg-emerald-950/20 p-3"
                                >
                                  <div className="mb-2 flex flex-wrap items-center gap-2">
                                    <strong>{nodeId}</strong>
                                    {status ? (
                                      <span className="border border-emerald-400/60 bg-emerald-400/10 text-emerald-200 tcp-badge">
                                        {status}
                                      </span>
                                    ) : null}
                                    {blockingClassification ? (
                                      <span className="tcp-subtle text-[11px]">
                                        {blockingClassification.replace(/_/g, " ")}
                                      </span>
                                    ) : null}
                                    {guidance?.repairAttemptsRemaining !== null &&
                                    guidance?.repairAttemptsRemaining !== undefined ? (
                                      <span className="tcp-subtle text-[11px]">
                                        {String(guidance.repairAttemptsRemaining)} repair attempt
                                        {Number(guidance.repairAttemptsRemaining) === 1
                                          ? ""
                                          : "s"}{" "}
                                        left
                                      </span>
                                    ) : null}
                                  </div>
                                  <div className="mb-2 flex flex-wrap gap-2">
                                    <button
                                      type="button"
                                      className="tcp-btn h-7 px-2 text-xs"
                                      onClick={() => setSelectedBoardTaskId(`node-${nodeId}`)}
                                    >
                                      Focus
                                    </button>
                                    {canGuidanceRetry ? (
                                      <button
                                        type="button"
                                        className="tcp-btn h-7 px-2 text-xs"
                                        onClick={() =>
                                          workflowTaskRetryMutation.mutate({
                                            runId: selectedRunId,
                                            nodeId,
                                            reason: `retried task ${nodeId} from repair guidance`,
                                          })
                                        }
                                        disabled={
                                          workflowTaskRetryMutation.isPending ||
                                          workflowTaskContinueMutation.isPending ||
                                          workflowTaskRequeueMutation.isPending
                                        }
                                      >
                                        {workflowTaskRetryMutation.isPending
                                          ? "Retrying..."
                                          : "Retry"}
                                      </button>
                                    ) : null}
                                    {canGuidanceContinue ? (
                                      <button
                                        type="button"
                                        className="tcp-btn h-7 px-2 text-xs"
                                        onClick={() =>
                                          workflowTaskContinueMutation.mutate({
                                            runId: selectedRunId,
                                            nodeId,
                                            reason: `continued task ${nodeId} from repair guidance`,
                                          })
                                        }
                                        disabled={
                                          workflowTaskRetryMutation.isPending ||
                                          workflowTaskContinueMutation.isPending ||
                                          workflowTaskRequeueMutation.isPending
                                        }
                                      >
                                        {workflowTaskContinueMutation.isPending
                                          ? "Continuing..."
                                          : "Continue"}
                                      </button>
                                    ) : null}
                                  </div>
                                  {reason ? (
                                    <div className="mb-2 whitespace-pre-wrap break-words text-sm text-emerald-100/90">
                                      {reason}
                                    </div>
                                  ) : null}
                                  {actions.length ? (
                                    <div className="grid gap-1">
                                      {actions.map((action: any, index: number) => (
                                        <div
                                          key={`${nodeId}-action-${index}`}
                                          className="rounded-md border border-emerald-400/20 bg-black/20 px-2 py-1 text-xs text-emerald-50"
                                        >
                                          {String(action || "")}
                                        </div>
                                      ))}
                                    </div>
                                  ) : null}
                                  {!actions.length && unmet.length ? (
                                    <div className="mt-2 flex flex-wrap gap-1">
                                      {unmet.map((item: any) => (
                                        <span
                                          key={`${nodeId}-${String(item)}`}
                                          className="rounded-full border border-emerald-400/25 bg-black/20 px-2 py-1 text-[11px] text-emerald-100/90"
                                        >
                                          {String(item || "").replace(/_/g, " ")}
                                        </span>
                                      ))}
                                    </div>
                                  ) : null}
                                  {failureKind ? (
                                    <div className="tcp-subtle mt-2 text-[11px]">
                                      failure kind: {failureKind}
                                    </div>
                                  ) : null}
                                </div>
                              );
                            })}
                          </div>
                        </div>
                      ) : null}
                      <div className="tcp-list-item overflow-visible">
                        <div className="font-medium">Mission Objective</div>
                        <pre className="tcp-code mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-words">
                          {String(selectedRun?.mission_snapshot?.objective || "n/a")}
                        </pre>
                      </div>
                      <div ref={artifactsSectionRef} className="tcp-list-item overflow-visible">
                        <div className="font-medium">Artifacts ({runArtifacts.length})</div>
                        {runArtifacts.length ? (
                          <div className="mt-2 grid gap-2 overflow-auto pr-1 sm:max-h-40">
                            {runArtifactEntries.map((entry) => (
                              <details
                                key={entry.key}
                                open={selectedRunArtifactKey === entry.key ? true : undefined}
                                className={
                                  selectedRunArtifactKey === entry.key
                                    ? "rounded-lg border border-sky-500/40 bg-sky-950/10 p-2"
                                    : "rounded-lg border border-slate-700/40 bg-slate-900/25 p-2"
                                }
                              >
                                <summary
                                  className="cursor-pointer list-none"
                                  onClick={() =>
                                    setSelectedRunArtifactKey((current) =>
                                      current === entry.key ? "" : entry.key
                                    )
                                  }
                                >
                                  <div className="flex items-center justify-between gap-2">
                                    <span className="text-xs font-medium text-slate-200">
                                      {entry.name}
                                    </span>
                                    <span className="tcp-subtle text-[11px]">
                                      {entry.kind || "artifact"}
                                    </span>
                                  </div>
                                </summary>
                                {entry.paths.length ? (
                                  <div className="mt-2 flex flex-wrap gap-1">
                                    {entry.paths.map((path) => (
                                      <span
                                        key={path}
                                        className="rounded-full border border-slate-700/70 bg-slate-950/30 px-2 py-1 font-mono text-[11px] text-slate-300"
                                      >
                                        {path}
                                      </span>
                                    ))}
                                  </div>
                                ) : null}
                                <pre className="tcp-code mt-2 max-h-32 overflow-auto text-[11px]">
                                  {formatJson(entry.artifact)}
                                </pre>
                              </details>
                            ))}
                          </div>
                        ) : (
                          <div className="tcp-subtle mt-2 text-xs">
                            {isWorkflowRun
                              ? "No blackboard artifacts have been recorded for this workflow run yet."
                              : "No run artifacts were persisted for this automation."}
                          </div>
                        )}
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
                                toast(
                                  "err",
                                  error instanceof Error ? error.message : "Copy failed."
                                );
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
        {editDraft ? (
          <motion.div
            className="tcp-confirm-overlay"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={() => setEditDraft(null)}
          >
            <motion.div
              className="tcp-confirm-dialog w-[min(40rem,96vw)]"
              initial={{ opacity: 0, y: 8, scale: 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 6, scale: 0.98 }}
              onClick={(event) => event.stopPropagation()}
            >
              <h3 className="tcp-confirm-title">Edit automation</h3>
              <div className="grid gap-3">
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Name</label>
                  <input
                    className="tcp-input"
                    value={editDraft.name}
                    onInput={(e) =>
                      setEditDraft((current) =>
                        current
                          ? { ...current, name: (e.target as HTMLInputElement).value }
                          : current
                      )
                    }
                  />
                </div>
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Objective</label>
                  <textarea
                    className="tcp-input min-h-[96px]"
                    value={editDraft.objective}
                    onInput={(e) =>
                      setEditDraft((current) =>
                        current
                          ? { ...current, objective: (e.target as HTMLTextAreaElement).value }
                          : current
                      )
                    }
                  />
                </div>
                <div className="grid gap-1 sm:grid-cols-2 sm:gap-2">
                  <div className="grid gap-1">
                    <label className="text-xs text-slate-400">Mode</label>
                    <select
                      className="tcp-input"
                      value={editDraft.mode}
                      onInput={(e) =>
                        setEditDraft((current) =>
                          current
                            ? {
                                ...current,
                                mode: (e.target as HTMLSelectElement).value as
                                  | "standalone"
                                  | "orchestrated",
                              }
                            : current
                        )
                      }
                    >
                      <option value="standalone">standalone</option>
                      <option value="orchestrated">orchestrated</option>
                    </select>
                  </div>
                  <div className="grid gap-1">
                    <label className="text-xs text-slate-400">Approval policy</label>
                    <button
                      className={`tcp-input flex h-10 items-center justify-between px-3 text-xs ${
                        editDraft.requiresApproval ? "border-amber-400/60 bg-amber-400/10" : ""
                      }`}
                      role="switch"
                      aria-checked={editDraft.requiresApproval}
                      onClick={() =>
                        setEditDraft((current) =>
                          current
                            ? { ...current, requiresApproval: !current.requiresApproval }
                            : current
                        )
                      }
                    >
                      <span className="flex items-center gap-2">
                        <i
                          data-lucide={editDraft.requiresApproval ? "shield-alert" : "shield-check"}
                        ></i>
                        {editDraft.requiresApproval
                          ? "Manual approvals enabled"
                          : "Fully automated enabled"}
                      </span>
                      <span
                        className={`relative h-5 w-9 rounded-full transition ${
                          editDraft.requiresApproval ? "bg-amber-500/40" : "bg-emerald-500/30"
                        }`}
                      >
                        <span
                          className={`absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-slate-100 transition ${
                            editDraft.requiresApproval ? "" : "translate-x-4"
                          }`}
                        />
                      </span>
                    </button>
                  </div>
                </div>
                <div className="grid gap-1 sm:grid-cols-2 sm:gap-2">
                  <div className="grid gap-1">
                    <label className="text-xs text-slate-400">Schedule type</label>
                    <select
                      className="tcp-input"
                      value={editDraft.scheduleKind}
                      onInput={(e) =>
                        setEditDraft((current) =>
                          current
                            ? {
                                ...current,
                                scheduleKind: (e.target as HTMLSelectElement).value as
                                  | "cron"
                                  | "interval",
                              }
                            : current
                        )
                      }
                    >
                      <option value="interval">interval</option>
                      <option value="cron">cron</option>
                    </select>
                  </div>
                </div>
                {editDraft.scheduleKind === "cron" ? (
                  <div className="grid gap-1">
                    <label className="text-xs text-slate-400">Cron expression</label>
                    <input
                      className="tcp-input font-mono"
                      value={editDraft.cronExpression}
                      onInput={(e) =>
                        setEditDraft((current) =>
                          current
                            ? { ...current, cronExpression: (e.target as HTMLInputElement).value }
                            : current
                        )
                      }
                      placeholder="0 9 * * *"
                    />
                  </div>
                ) : (
                  <div className="grid gap-1">
                    <label className="text-xs text-slate-400">Interval seconds</label>
                    <input
                      type="number"
                      min="1"
                      className="tcp-input"
                      value={editDraft.intervalSeconds}
                      onInput={(e) =>
                        setEditDraft((current) =>
                          current
                            ? { ...current, intervalSeconds: (e.target as HTMLInputElement).value }
                            : current
                        )
                      }
                    />
                  </div>
                )}
              </div>
              <div className="tcp-confirm-actions mt-3">
                <button className="tcp-btn" onClick={() => setEditDraft(null)}>
                  <i data-lucide="x-circle"></i>
                  Cancel
                </button>
                <button
                  className="tcp-btn-primary"
                  onClick={() => editDraft && updateAutomationMutation.mutate(editDraft)}
                  disabled={updateAutomationMutation.isPending}
                >
                  <i data-lucide="check"></i>
                  Save
                </button>
              </div>
            </motion.div>
          </motion.div>
        ) : null}
        {workflowEditDraft ? (
          <motion.div
            className="tcp-confirm-overlay"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={() => setWorkflowEditDraft(null)}
          >
            <motion.div
              className="tcp-confirm-dialog tcp-workflow-editor-modal"
              initial={{ opacity: 0, y: 8, scale: 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 6, scale: 0.98 }}
              onClick={(event) => event.stopPropagation()}
            >
              <div className="flex items-start justify-between gap-3 border-b border-slate-800/70 px-4 py-4">
                <div>
                  <h3 className="tcp-confirm-title">Edit workflow automation</h3>
                  <div className="mt-1 text-sm text-slate-400">
                    Update scheduling, model routing, MCP access, and the actual step prompts.
                  </div>
                </div>
                <button className="tcp-btn h-9 w-9 px-0" onClick={() => setWorkflowEditDraft(null)}>
                  <i data-lucide="x"></i>
                </button>
              </div>
              <div className="grid flex-1 gap-4 overflow-y-auto px-4 py-4 xl:grid-cols-[minmax(22rem,0.92fr)_minmax(0,1.35fr)]">
                <div className="grid content-start gap-4">
                  <div className="grid gap-3 rounded-xl border border-slate-700/50 bg-slate-900/30 p-4">
                    <div className="grid gap-1">
                      <label className="text-xs text-slate-400">Automation name</label>
                      <input
                        className="tcp-input"
                        value={workflowEditDraft.name}
                        onInput={(e) =>
                          setWorkflowEditDraft((current) =>
                            current
                              ? { ...current, name: (e.target as HTMLInputElement).value }
                              : current
                          )
                        }
                      />
                    </div>
                    <div className="grid gap-1">
                      <label className="text-xs text-slate-400">Notes / description</label>
                      <textarea
                        className="tcp-input min-h-[120px]"
                        value={workflowEditDraft.description}
                        onInput={(e) =>
                          setWorkflowEditDraft((current) =>
                            current
                              ? { ...current, description: (e.target as HTMLTextAreaElement).value }
                              : current
                          )
                        }
                        placeholder="Add notes, delivery expectations, or operator guidance."
                      />
                    </div>
                    <div className="grid gap-1">
                      <label className="text-xs text-slate-400">Workspace root</label>
                      <input
                        className={`tcp-input ${
                          validateWorkspaceRootInput(workflowEditDraft.workspaceRoot)
                            ? "border-red-500/70 text-red-100"
                            : ""
                        }`}
                        value={workflowEditDraft.workspaceRoot}
                        onInput={(e) =>
                          setWorkflowEditDraft((current) =>
                            current
                              ? { ...current, workspaceRoot: (e.target as HTMLInputElement).value }
                              : current
                          )
                        }
                      />
                      {validateWorkspaceRootInput(workflowEditDraft.workspaceRoot) ? (
                        <div className="text-xs text-red-300">
                          {validateWorkspaceRootInput(workflowEditDraft.workspaceRoot)}
                        </div>
                      ) : null}
                    </div>
                  </div>

                  <div className="grid gap-3 rounded-xl border border-slate-700/50 bg-slate-900/30 p-4">
                    <div className="text-xs uppercase tracking-wide text-slate-500">Execution</div>
                    <div className="grid gap-2 sm:grid-cols-3">
                      <div className="grid gap-1">
                        <label className="text-xs text-slate-400">Schedule type</label>
                        <select
                          className="tcp-select"
                          value={workflowEditDraft.scheduleKind}
                          onInput={(e) =>
                            setWorkflowEditDraft((current) =>
                              current
                                ? {
                                    ...current,
                                    scheduleKind: (e.target as HTMLSelectElement).value as
                                      | "manual"
                                      | "cron"
                                      | "interval",
                                  }
                                : current
                            )
                          }
                        >
                          <option value="manual">manual</option>
                          <option value="cron">cron</option>
                          <option value="interval">interval</option>
                        </select>
                      </div>
                      <div className="grid gap-1">
                        <label className="text-xs text-slate-400">Execution mode</label>
                        <select
                          className="tcp-select"
                          value={workflowEditDraft.executionMode}
                          onInput={(e) =>
                            setWorkflowEditDraft((current) =>
                              current
                                ? {
                                    ...current,
                                    executionMode: (e.target as HTMLSelectElement)
                                      .value as ExecutionMode,
                                  }
                                : current
                            )
                          }
                        >
                          {AUTOMATION_WIZARD_CONFIG.executionModes.map((mode) => (
                            <option key={mode.id} value={mode.id}>
                              {mode.label}
                            </option>
                          ))}
                        </select>
                      </div>
                      <div className="grid gap-1">
                        <label className="text-xs text-slate-400">Max parallel agents</label>
                        <input
                          type="number"
                          min="1"
                          max="16"
                          className="tcp-input"
                          value={workflowEditDraft.maxParallelAgents}
                          onInput={(e) =>
                            setWorkflowEditDraft((current) =>
                              current
                                ? {
                                    ...current,
                                    maxParallelAgents: (e.target as HTMLInputElement).value,
                                  }
                                : current
                            )
                          }
                          disabled={workflowEditDraft.executionMode !== "swarm"}
                        />
                      </div>
                    </div>
                    {workflowEditDraft.scheduleKind === "cron" ? (
                      <div className="grid gap-1">
                        <label className="text-xs text-slate-400">Cron expression</label>
                        <input
                          className="tcp-input font-mono"
                          value={workflowEditDraft.cronExpression}
                          onInput={(e) =>
                            setWorkflowEditDraft((current) =>
                              current
                                ? {
                                    ...current,
                                    cronExpression: (e.target as HTMLInputElement).value,
                                  }
                                : current
                            )
                          }
                        />
                      </div>
                    ) : workflowEditDraft.scheduleKind === "interval" ? (
                      <div className="grid gap-1">
                        <label className="text-xs text-slate-400">Interval seconds</label>
                        <input
                          type="number"
                          min="1"
                          className="tcp-input"
                          value={workflowEditDraft.intervalSeconds}
                          onInput={(e) =>
                            setWorkflowEditDraft((current) =>
                              current
                                ? {
                                    ...current,
                                    intervalSeconds: (e.target as HTMLInputElement).value,
                                  }
                                : current
                            )
                          }
                        />
                      </div>
                    ) : null}
                  </div>

                  <div className="grid gap-3 rounded-xl border border-slate-700/50 bg-slate-900/30 p-4">
                    <div className="text-xs uppercase tracking-wide text-slate-500">
                      Model Selection
                    </div>
                    <ProviderModelSelector
                      providerLabel="Model provider"
                      modelLabel="Model"
                      draft={{
                        provider: workflowEditDraft.modelProvider,
                        model: workflowEditDraft.modelId,
                      }}
                      providers={providerOptions}
                      onChange={(draft) =>
                        setWorkflowEditDraft((current) =>
                          current
                            ? {
                                ...current,
                                modelProvider: draft.provider,
                                modelId: draft.model,
                              }
                            : current
                        )
                      }
                      inheritLabel="Workspace default"
                    />
                    {validateModelInput(
                      workflowEditDraft.modelProvider,
                      workflowEditDraft.modelId
                    ) ? (
                      <div className="text-xs text-red-300">
                        {validateModelInput(
                          workflowEditDraft.modelProvider,
                          workflowEditDraft.modelId
                        )}
                      </div>
                    ) : null}
                    <div className="grid gap-2 rounded-lg border border-slate-800/70 bg-slate-950/30 p-3">
                      <div className="text-xs uppercase tracking-wide text-slate-500">
                        Planner fallback model
                      </div>
                      <div className="text-xs text-slate-400">
                        Optional. Override the workflow model only for planning and revisions.
                      </div>
                      <ProviderModelSelector
                        providerLabel="Planner provider"
                        modelLabel="Planner model"
                        draft={{
                          provider: workflowEditDraft.plannerModelProvider,
                          model: workflowEditDraft.plannerModelId,
                        }}
                        providers={providerOptions}
                        onChange={(draft) =>
                          setWorkflowEditDraft((current) =>
                            current
                              ? {
                                  ...current,
                                  plannerModelProvider: draft.provider,
                                  plannerModelId: draft.model,
                                }
                              : current
                          )
                        }
                        inheritLabel="Use workflow model"
                      />
                      {validatePlannerModelInput(
                        workflowEditDraft.plannerModelProvider,
                        workflowEditDraft.plannerModelId
                      ) ? (
                        <div className="text-xs text-red-300">
                          {validatePlannerModelInput(
                            workflowEditDraft.plannerModelProvider,
                            workflowEditDraft.plannerModelId
                          )}
                        </div>
                      ) : null}
                    </div>
                  </div>

                  <div className="grid gap-3 rounded-xl border border-slate-700/50 bg-slate-900/30 p-4">
                    <div className="text-xs uppercase tracking-wide text-slate-500">
                      Tool Access
                    </div>
                    <div className="grid gap-2 sm:grid-cols-2">
                      <button
                        type="button"
                        className={`tcp-list-item text-left ${workflowEditDraft.toolAccessMode === "all" ? "border-amber-400/60 bg-amber-400/10" : ""}`}
                        onClick={() =>
                          setWorkflowEditDraft((current) =>
                            current ? { ...current, toolAccessMode: "all" } : current
                          )
                        }
                      >
                        <div className="font-medium">All tools</div>
                        <div className="tcp-subtle text-xs">
                          Grant full built-in tool access to workflow agents.
                        </div>
                      </button>
                      <button
                        type="button"
                        className={`tcp-list-item text-left ${workflowEditDraft.toolAccessMode === "custom" ? "border-amber-400/60 bg-amber-400/10" : ""}`}
                        onClick={() =>
                          setWorkflowEditDraft((current) =>
                            current ? { ...current, toolAccessMode: "custom" } : current
                          )
                        }
                      >
                        <div className="font-medium">Custom allowlist</div>
                        <div className="tcp-subtle text-xs">
                          Restrict built-in tools manually. MCP tools still follow the selected
                          servers.
                        </div>
                      </button>
                    </div>
                    {workflowEditDraft.toolAccessMode === "custom" ? (
                      <div className="grid gap-1">
                        <label className="text-xs text-slate-400">Allowed built-in tools</label>
                        <textarea
                          className="tcp-input min-h-[96px] font-mono text-xs"
                          value={workflowEditDraft.customToolsText}
                          onInput={(e) =>
                            setWorkflowEditDraft((current) =>
                              current
                                ? {
                                    ...current,
                                    customToolsText: (e.target as HTMLTextAreaElement).value,
                                  }
                                : current
                            )
                          }
                          placeholder={`read\nwrite\nedit\nbash\nls\nglob\nwebsearch`}
                        />
                      </div>
                    ) : (
                      <div className="text-xs text-slate-500">
                        All built-in tools are allowed for this automation.
                      </div>
                    )}
                  </div>

                  <div className="grid gap-2 rounded-xl border border-slate-700/50 bg-slate-900/30 p-4">
                    <div className="text-xs uppercase tracking-wide text-slate-500">
                      MCP Servers
                    </div>
                    {mcpServers.length ? (
                      <div className="flex flex-wrap gap-2">
                        {mcpServers.map((server) => {
                          const isSelected = workflowEditDraft.selectedMcpServers.includes(
                            server.name
                          );
                          return (
                            <button
                              key={server.name}
                              className={`tcp-btn h-7 px-2 text-xs ${
                                isSelected
                                  ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                                  : ""
                              }`}
                              onClick={() =>
                                setWorkflowEditDraft((current) =>
                                  current
                                    ? {
                                        ...current,
                                        selectedMcpServers: isSelected
                                          ? current.selectedMcpServers.filter(
                                              (name) => name !== server.name
                                            )
                                          : [...current.selectedMcpServers, server.name].sort(),
                                      }
                                    : current
                                )
                              }
                            >
                              {server.name} {server.connected ? "• connected" : "• disconnected"}
                            </button>
                          );
                        })}
                      </div>
                    ) : (
                      <div className="text-xs text-slate-400">No MCP servers configured yet.</div>
                    )}
                  </div>
                </div>

                <div className="grid content-start gap-4">
                  <div className="grid gap-2 rounded-xl border border-slate-700/50 bg-slate-900/30 p-4">
                    <div>
                      <div>
                        <div className="text-xs uppercase tracking-wide text-slate-500">
                          Prompt Editor
                        </div>
                        <div className="mt-1 text-xs text-slate-400">
                          Edit the actual prompts Tandem sends for each workflow step. These
                          objectives control what every node does at runtime.
                        </div>
                      </div>
                    </div>
                    {workflowEditDraft.nodes.length ? (
                      <div className="grid gap-3">
                        {workflowEditDraft.nodes.map((node, index) => (
                          <div
                            key={node.nodeId || index}
                            className="rounded-lg border border-slate-700/60 bg-slate-950/30 p-3"
                          >
                            <div className="mb-2 flex flex-wrap items-center gap-2">
                              <strong className="text-sm text-slate-100">
                                {node.nodeId || node.title || `Step ${index + 1}`}
                              </strong>
                              {node.agentId ? (
                                <span className="tcp-badge-info">agent: {node.agentId}</span>
                              ) : null}
                            </div>
                            <textarea
                              className="tcp-input min-h-[180px] text-sm leading-6"
                              value={node.objective}
                              onInput={(e) =>
                                setWorkflowEditDraft((current) =>
                                  current
                                    ? {
                                        ...current,
                                        nodes: current.nodes.map((row) =>
                                          row.nodeId === node.nodeId
                                            ? {
                                                ...row,
                                                objective: (e.target as HTMLTextAreaElement).value,
                                              }
                                            : row
                                        ),
                                      }
                                    : current
                                )
                              }
                              placeholder="Describe exactly what this step should do."
                            />
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="text-xs text-slate-400">
                        This workflow does not currently expose editable node objectives.
                      </div>
                    )}
                  </div>
                </div>
              </div>
              <div className="tcp-confirm-actions border-t border-slate-800/70 px-4 py-3">
                <button className="tcp-btn" onClick={() => setWorkflowEditDraft(null)}>
                  <i data-lucide="x-circle"></i>
                  Cancel
                </button>
                <button
                  className="tcp-btn-primary"
                  onClick={() =>
                    workflowEditDraft && updateWorkflowAutomationMutation.mutate(workflowEditDraft)
                  }
                  disabled={updateWorkflowAutomationMutation.isPending}
                >
                  <i data-lucide="check"></i>
                  {updateWorkflowAutomationMutation.isPending ? "Saving..." : "Save"}
                </button>
              </div>
            </motion.div>
          </motion.div>
        ) : null}
        {deleteConfirm ? (
          <motion.div
            className="tcp-confirm-overlay"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={() => setDeleteConfirm(null)}
          >
            <motion.div
              className="tcp-confirm-dialog w-[min(34rem,96vw)]"
              initial={{ opacity: 0, y: 8, scale: 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 6, scale: 0.98 }}
              onClick={(event) => event.stopPropagation()}
            >
              <h3 className="tcp-confirm-title">Delete automation</h3>
              <p className="tcp-confirm-message">
                This will permanently remove <strong>{deleteConfirm.title}</strong>.
              </p>
              <div className="tcp-confirm-actions mt-3">
                <button className="tcp-btn" onClick={() => setDeleteConfirm(null)}>
                  <i data-lucide="x"></i>
                  Cancel
                </button>
                <button
                  className="tcp-btn-danger"
                  disabled={automationActionMutation.isPending}
                  onClick={() =>
                    automationActionMutation.mutate(
                      {
                        action: "delete",
                        automationId: deleteConfirm.automationId,
                        family: deleteConfirm.family,
                      },
                      {
                        onSettled: () => setDeleteConfirm(null),
                      }
                    )
                  }
                >
                  <i data-lucide="trash-2"></i>
                  {automationActionMutation.isPending ? "Deleting..." : "Delete automation"}
                </button>
              </div>
            </motion.div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}

// ─── Spawn Approvals ────────────────────────────────────────────────────────

function SpawnApprovals({ client, toast }: { client: any; toast: any }) {
  const queryClient = useQueryClient();

  const approvalsQuery = useQuery({
    queryKey: ["automations", "approvals"],
    queryFn: () =>
      client?.agentTeams?.listApprovals?.().catch(() => ({ spawnApprovals: [] })) ??
      Promise.resolve({ spawnApprovals: [] }),
    refetchInterval: 6000,
  });

  const instancesQuery = useQuery({
    queryKey: ["automations", "instances"],
    queryFn: () =>
      client?.agentTeams?.listInstances?.().catch(() => ({ instances: [] })) ??
      Promise.resolve({ instances: [] }),
    refetchInterval: 8000,
  });

  const replyMutation = useMutation({
    mutationFn: ({ requestId, decision }: { requestId: string; decision: "approve" | "deny" }) =>
      client?.agentTeams?.replyApproval?.(requestId, decision),
    onSuccess: async () => {
      toast("ok", "Approval updated.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const approvals = toArray(approvalsQuery.data, "spawnApprovals");
  const instances = toArray(instancesQuery.data, "instances");

  return (
    <div className="grid gap-4">
      {approvals.length > 0 ? (
        <div className="grid gap-2">
          <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
            Pending Approvals
          </p>
          {approvals.map((approval: any, index: number) => {
            const requestId = String(approval?.request_id || approval?.id || `request-${index}`);
            return (
              <div key={requestId} className="tcp-list-item border-amber-500/40">
                <div className="mb-1 font-medium text-amber-300">
                  ⚠️ {String(approval?.reason || approval?.title || "Spawn request")}
                </div>
                <div className="tcp-subtle text-xs">{requestId}</div>
                <div className="mt-2 flex gap-2">
                  <button
                    className="tcp-btn-primary h-7 px-2 text-xs"
                    onClick={() => replyMutation.mutate({ requestId, decision: "approve" })}
                  >
                    <i data-lucide="badge-check"></i>
                    Approve
                  </button>
                  <button
                    className="tcp-btn-danger h-7 px-2 text-xs"
                    onClick={() => replyMutation.mutate({ requestId, decision: "deny" })}
                  >
                    <i data-lucide="x"></i>
                    Deny
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      ) : null}

      {instances.length > 0 ? (
        <div className="grid gap-2">
          <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">Active Teams</p>
          {instances.map((instance: any, index: number) => (
            <div
              key={String(instance?.instance_id || instance?.id || index)}
              className="tcp-list-item"
            >
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  <span>👥</span>
                  <strong>
                    {String(
                      instance?.name || instance?.template_id || instance?.instance_id || "Instance"
                    )}
                  </strong>
                </div>
                <span className="tcp-badge-info">{String(instance?.status || "active")}</span>
              </div>
              <div className="tcp-subtle text-xs mt-1">
                Mission: {String(instance?.mission_id || "—")}
              </div>
            </div>
          ))}
        </div>
      ) : null}

      {!approvals.length && !instances.length ? (
        <EmptyState text="No active teams or pending approvals right now." />
      ) : null}
    </div>
  );
}

// ─── Main Page ──────────────────────────────────────────────────────────────

export function AutomationsPage({ client, api, toast, navigate, providerStatus }: AppPageProps) {
  const caps = useCapabilities();
  const [tab, setTab] = useState<ActiveTab>("calendar");
  const [createMode, setCreateMode] = useState<CreateMode>("simple");
  const [selectedRunId, setSelectedRunId] = useState<string>("");
  const [advancedEditAutomation, setAdvancedEditAutomation] = useState<any | null>(null);

  useEffect(() => {
    try {
      const raw = sessionStorage.getItem(AUTOMATIONS_STUDIO_HANDOFF_KEY);
      if (!raw) return;
      sessionStorage.removeItem(AUTOMATIONS_STUDIO_HANDOFF_KEY);
      const handoff = JSON.parse(raw || "{}");
      if (handoff?.tab === "running") setTab("running");
      const runId = String(handoff?.runId || "").trim();
      if (runId) setSelectedRunId(runId);
    } catch {
      // ignore
    }
  }, []);

  const tabs: { id: ActiveTab; label: string; icon: string }[] = [
    { id: "create", label: "Create", icon: "sparkles" },
    { id: "calendar", label: "Calendar", icon: "calendar" },
    { id: "list", label: "List", icon: "clipboard-list" },
    { id: "running", label: "Tasks", icon: "activity" },
    { id: "optimize", label: "Optimize", icon: "flask-conical" },
    { id: "approvals", label: "Active Teams", icon: "users" },
  ];

  return (
    <div className="grid gap-4">
      {/* Tab bar */}
      <div className="flex gap-1 rounded-xl border border-slate-700/50 bg-slate-900/40 p-1">
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`flex flex-1 items-center justify-center gap-1.5 rounded-lg px-3 py-2 text-sm font-medium transition-all ${
              tab === t.id
                ? "bg-amber-500/20 text-amber-300 shadow-sm"
                : "text-slate-400 hover:text-slate-200"
            }`}
          >
            <i data-lucide={t.icon}></i>
            <span>{t.label}</span>
          </button>
        ))}
      </div>

      {/* Tab content */}
      <AnimatePresence mode="wait">
        <motion.div
          key={tab}
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -6 }}
          transition={{ duration: 0.15 }}
        >
          {tab === "create" ? (
            <PageCard
              title="Create an Automation"
              subtitle="Describe what you want, pick a schedule, and Tandem handles the rest"
            >
              <div className="grid gap-4">
                <div className="rounded-xl border border-slate-700/50 bg-slate-950/50 p-4">
                  <div className="mb-2 text-xs font-medium uppercase tracking-[0.24em] text-slate-500">
                    Builder Mode
                  </div>
                  <div className="tcp-subtle text-xs">
                    Keep the simple wizard for quick automations, or switch to the advanced swarm
                    builder for orchestrated mission planning.
                  </div>
                  <div className="mt-3 flex flex-wrap gap-2">
                    <button
                      type="button"
                      className={`tcp-btn h-9 px-3 text-sm ${
                        createMode === "simple"
                          ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                          : ""
                      }`}
                      onClick={() => {
                        setCreateMode("simple");
                        setAdvancedEditAutomation(null);
                      }}
                    >
                      Simple Wizard
                    </button>
                    <button
                      type="button"
                      className={`tcp-btn h-9 px-3 text-sm ${
                        createMode === "advanced"
                          ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                          : ""
                      }`}
                      onClick={() => setCreateMode("advanced")}
                    >
                      Advanced Swarm Builder
                    </button>
                  </div>
                </div>

                {createMode === "advanced" ? (
                  <AdvancedMissionBuilderPanel
                    client={client}
                    api={api}
                    toast={toast}
                    defaultProvider={providerStatus.defaultProvider}
                    defaultModel={providerStatus.defaultModel}
                    editingAutomation={advancedEditAutomation}
                    onShowAutomations={() => {
                      setAdvancedEditAutomation(null);
                      setTab("calendar");
                    }}
                    onShowRuns={() => {
                      setAdvancedEditAutomation(null);
                      setTab("running");
                    }}
                    onClearEditing={() => setAdvancedEditAutomation(null)}
                  />
                ) : (
                  <CreateWizard
                    client={client}
                    api={api}
                    toast={toast}
                    navigate={navigate}
                    defaultProvider={providerStatus.defaultProvider}
                    defaultModel={providerStatus.defaultModel}
                  />
                )}
              </div>
            </PageCard>
          ) : tab === "calendar" ? (
            <PageCard
              title="Automation Calendar"
              subtitle="Weekly schedule view for cron automations"
            >
              <MyAutomations
                client={client}
                toast={toast}
                navigate={navigate}
                viewMode="calendar"
                selectedRunId={selectedRunId}
                onSelectRunId={setSelectedRunId}
                onOpenRunningView={() => setTab("running")}
                onOpenAdvancedEdit={(automation) => {
                  setAdvancedEditAutomation(automation);
                  setCreateMode("advanced");
                  setTab("create");
                }}
              />
            </PageCard>
          ) : tab === "list" ? (
            <PageCard title="My Automations" subtitle="Installed packs, routines and run history">
              <MyAutomations
                client={client}
                toast={toast}
                navigate={navigate}
                viewMode="list"
                selectedRunId={selectedRunId}
                onSelectRunId={setSelectedRunId}
                onOpenRunningView={() => setTab("running")}
                onOpenAdvancedEdit={(automation) => {
                  setAdvancedEditAutomation(automation);
                  setCreateMode("advanced");
                  setTab("create");
                }}
              />
            </PageCard>
          ) : tab === "running" ? (
            <PageCard
              title="Live Running Tasks"
              subtitle="Inspect active runs and open detailed event logs for each run"
            >
              <MyAutomations
                client={client}
                toast={toast}
                navigate={navigate}
                viewMode="running"
                selectedRunId={selectedRunId}
                onSelectRunId={setSelectedRunId}
                onOpenRunningView={() => setTab("running")}
                onOpenAdvancedEdit={(automation) => {
                  setAdvancedEditAutomation(automation);
                  setCreateMode("advanced");
                  setTab("create");
                }}
              />
            </PageCard>
          ) : tab === "optimize" ? (
            <PageCard
              title="Workflow Optimization"
              subtitle="Create and inspect overnight shadow-eval optimization campaigns"
            >
              <OptimizationCampaignsPanel client={client} toast={toast} />
            </PageCard>
          ) : (
            <PageCard
              title="Active Teams"
              subtitle="Running team instances and pending spawn approvals"
            >
              <SpawnApprovals client={client} toast={toast} />
            </PageCard>
          )}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
