import { Suspense, lazy, useEffect, useMemo, useState } from "react";
import { Button, Input } from "@/components/ui";
import { ProjectSwitcher } from "@/components/sidebar";
import {
  blockedNodeIds,
  completedNodeIds,
  extractRunBlockers,
  extractRunNodeOutputs,
  extractSessionIdsFromRun,
  failureChainLifecycleEntries,
  nodeOutputSessionId,
  nodeOutputSummary,
  nodeOutputText,
  pendingNodeIds,
  promotionLifecycleEntries,
  recoveryLifecycleEntries,
  repairLifecycleEntries,
  runDisplayTitle,
  runGateHistory,
  runLifecycleEntries,
  runLastFailure,
  runNodeAttempts,
  runNodeOutputMap,
  runStatusLabel,
  runStopReason,
  runSummary,
  runAwaitingGate,
  runUsageMetrics,
} from "@/components/coder/shared/coderRunUtils";
import { AdvancedMissionBuilder } from "@/components/agent-automation/AdvancedMissionBuilder";
import { ScheduleBuilder } from "@/components/agent-automation/GuidedScheduleBuilder";
import { TimezoneField } from "@/components/agent-automation/TimezoneField";
import {
  buildWorkflowCalendarOccurrences,
  formatAutomationV2ScheduleLabel,
  rewriteCronForDroppedStart,
  type CalendarRange,
} from "@/components/agent-automation/automationCalendarUtils";
import {
  type ScheduleKind,
  type ScheduleValue,
} from "@/components/agent-automation/scheduleBuilder";
import { detectBrowserTimezone, isValidTimezone } from "@/components/agent-automation/timezone";
import {
  automationsV2Delete,
  automationsV2Get,
  automationsV2List,
  automationsV2Pause,
  automationsV2Resume,
  automationsV2RunCancel,
  automationsV2RunGateDecide,
  automationsV2RunGet,
  automationsV2RunNow,
  automationsV2RunPause,
  automationsV2RunRepair,
  automationsV2RunRecover,
  automationsV2RunResume,
  automationsV2RunsAll,
  automationsV2Update,
  getSessionMessages,
  getProvidersConfig,
  listProvidersFromSidecar,
  listModels,
  mcpListServers,
  onSidecarEventV2,
  opencodeListMcpServers,
  routinesList,
  toolIds,
  type AutomationV2RunRecord,
  type AutomationV2Spec,
  type McpServerRecord,
  type ModelInfo,
  type OpencodeMcpServerEntry,
  type ProviderInfo,
  type ProvidersConfig,
  type RoutineSpec,
  type SessionMessage,
  type UserProject,
  workflowPlansApply,
  workflowPlansChatMessage,
  workflowPlansChatReset,
  workflowPlansChatStart,
  type WorkflowPlan,
  type WorkflowPlanConversation,
} from "@/lib/tauri";

type PageTab = "create" | "calendar" | "automations" | "runs";
type CreateMode = "simple" | "advanced";
type WizardStep = 1 | 2 | 3 | 4;
type ExecutionMode = "team" | "swarm";

interface WizardState {
  prompt: string;
  workspaceRoot: string;
  timezone: string;
  scheduleKind: ScheduleKind;
  intervalSeconds: string;
  cronExpression: string;
  executionMode: ExecutionMode;
  maxParallelAgents: string;
  modelProvider: string;
  modelId: string;
  plannerModelProvider: string;
  plannerModelId: string;
  selectedMcpServers: string[];
  exportPackDraft: boolean;
}

interface WorkflowEditDraft {
  automationId: string;
  name: string;
  description: string;
  workspaceRoot: string;
  timezone: string;
  scheduleKind: ScheduleKind;
  intervalSeconds: string;
  cronExpression: string;
  executionMode: ExecutionMode;
  maxParallelAgents: string;
  modelProvider: string;
  modelId: string;
  plannerModelProvider: string;
  plannerModelId: string;
  selectedMcpServers: string[];
}

interface AgentAutomationPageProps {
  userProjects: UserProject[];
  activeProject: UserProject | null;
  onSwitchProject: (projectId: string) => void;
  onAddProject: () => void;
  onManageProjects: () => void;
  projectSwitcherLoading?: boolean;
  onOpenMcpExtensions?: () => void;
  initialRunId?: string | null;
}

const AutomationCalendar = lazy(async () => {
  const mod = await import("@/components/agent-automation/AutomationCalendar");
  return { default: mod.AutomationCalendar };
});

function buildDefaultWizard(
  activeProject: UserProject | null,
  providers: ProviderInfo[],
  current?: Partial<WizardState>
): WizardState {
  const preferredProvider =
    String(current?.modelProvider || "").trim() ||
    providers.find((provider) => provider.configured && provider.models.length > 0)?.id ||
    providers.find((provider) => provider.models.length > 0)?.id ||
    "";
  const preferredModels =
    providers.find((provider) => provider.id === preferredProvider)?.models ?? [];
  return {
    prompt: String(current?.prompt || "").trim(),
    workspaceRoot: String(current?.workspaceRoot || activeProject?.path || "").trim(),
    timezone:
      String(current?.timezone || detectBrowserTimezone()).trim() || detectBrowserTimezone(),
    scheduleKind: current?.scheduleKind ?? "interval",
    intervalSeconds: String(current?.intervalSeconds || "86400"),
    cronExpression: String(current?.cronExpression || ""),
    executionMode: current?.executionMode ?? "team",
    maxParallelAgents: String(current?.maxParallelAgents || "4"),
    modelProvider: preferredProvider,
    modelId: String(current?.modelId || preferredModels[0] || ""),
    plannerModelProvider: String(current?.plannerModelProvider || ""),
    plannerModelId: String(current?.plannerModelId || ""),
    selectedMcpServers: current?.selectedMcpServers ?? [],
    exportPackDraft: current?.exportPackDraft ?? false,
  };
}

function validateWorkspaceRootInput(value: string) {
  const trimmed = String(value || "").trim();
  if (!trimmed) return "Workspace root is required.";
  if (!isAbsolutePath(trimmed)) return "Workspace root must be an absolute path.";
  return "";
}

function isAbsolutePath(value: string) {
  const trimmed = String(value || "").trim();
  return (
    trimmed.startsWith("/") ||
    /^[a-zA-Z]:[\\/]/.test(trimmed) ||
    /^\\\\[^\\]+\\[^\\]+/.test(trimmed)
  );
}

async function withFallback<T>(operation: Promise<T>, fallback: T, timeoutMs = 8000): Promise<T> {
  return Promise.race([
    operation.catch(() => fallback),
    new Promise<T>((resolve) => {
      window.setTimeout(() => resolve(fallback), timeoutMs);
    }),
  ]);
}

function normalizeProviderId(value: string) {
  const trimmed = String(value || "").trim();
  if (!trimmed) return "";
  if (trimmed === "opencode") return "opencode_zen";
  if (trimmed === "llama.cpp") return "llama_cpp";
  return trimmed;
}

function getProviderConfigById(
  providersConfig: ProvidersConfig | null,
  providerId: string
): { enabled?: boolean; has_key?: boolean; default?: boolean } | null {
  if (!providersConfig) return null;
  switch (providerId) {
    case "openrouter":
      return providersConfig.openrouter;
    case "opencode_zen":
      return providersConfig.opencode_zen;
    case "anthropic":
      return providersConfig.anthropic;
    case "openai":
      return providersConfig.openai;
    case "llama_cpp":
      return providersConfig.llama_cpp;
    case "ollama":
      return providersConfig.ollama;
    case "poe":
      return providersConfig.poe;
    default:
      return null;
  }
}

function buildFallbackProviderCatalog(
  modelRows: ModelInfo[],
  providersConfig: ProvidersConfig | null
): ProviderInfo[] {
  const groupedModels = new Map<string, Set<string>>();
  for (const row of modelRows) {
    const providerId = normalizeProviderId(row.provider || "");
    const modelId = String(row.id || row.name || "").trim();
    if (!providerId || !modelId) continue;
    const current = groupedModels.get(providerId) ?? new Set<string>();
    current.add(modelId);
    groupedModels.set(providerId, current);
  }

  const providerIds = new Set<string>(groupedModels.keys());
  if (providersConfig?.selected_model?.provider_id) {
    providerIds.add(normalizeProviderId(providersConfig.selected_model.provider_id));
  }
  for (const providerId of [
    "openrouter",
    "opencode_zen",
    "anthropic",
    "openai",
    "llama_cpp",
    "ollama",
    "poe",
  ]) {
    const config = getProviderConfigById(providersConfig, providerId);
    if (config?.enabled || config?.has_key || config?.default) {
      providerIds.add(providerId);
    }
  }

  return Array.from(providerIds)
    .filter(Boolean)
    .map((providerId) => {
      const config = getProviderConfigById(providersConfig, providerId);
      return {
        id: providerId,
        name: providerId,
        models: Array.from(groupedModels.get(providerId) ?? []).sort((a, b) => a.localeCompare(b)),
        configured: Boolean(config?.enabled || config?.has_key || config?.default),
      };
    })
    .filter((provider) => provider.configured || provider.models.length > 0)
    .sort(
      (a, b) =>
        Number(Boolean(b.configured)) - Number(Boolean(a.configured)) || a.id.localeCompare(b.id)
    );
}

function getConfiguredMcpTransport(config: Record<string, unknown>) {
  const type = String(config?.type || "").trim();
  if (type) return type;
  if (typeof config?.url === "string") return "remote";
  if (Array.isArray((config as { command?: unknown }).command)) return "local";
  return "configured";
}

function buildConfiguredMcpServerCatalog(
  runtimeRows: McpServerRecord[],
  configuredRows: OpencodeMcpServerEntry[]
): McpServerRecord[] {
  const runtimeByName = new Map(runtimeRows.map((row) => [row.name, row] as const));
  const configuredByName = new Map<string, OpencodeMcpServerEntry>();
  for (const row of configuredRows) {
    const name = String(row.name || "").trim();
    if (!name) continue;
    configuredByName.set(name, row);
  }

  const merged: McpServerRecord[] = Array.from(configuredByName.values()).map((row) => {
    const runtime = runtimeByName.get(row.name);
    return {
      name: row.name,
      transport: getConfiguredMcpTransport(row.config),
      enabled:
        typeof row.config?.enabled === "boolean"
          ? Boolean(row.config.enabled)
          : (runtime?.enabled ?? true),
      connected: runtime?.connected ?? false,
      pid: runtime?.pid,
      last_error: runtime?.last_error,
      headers: runtime?.headers ?? {},
      tool_cache: runtime?.tool_cache ?? [],
      tools_fetched_at_ms: runtime?.tools_fetched_at_ms,
    };
  });

  for (const runtime of runtimeRows) {
    if (!configuredByName.has(runtime.name)) {
      merged.push({
        name: runtime.name,
        transport: runtime.transport,
        enabled: runtime.enabled,
        connected: runtime.connected,
        pid: runtime.pid,
        last_error: runtime.last_error,
        headers: runtime.headers,
        tool_cache: runtime.tool_cache,
        tools_fetched_at_ms: runtime.tools_fetched_at_ms,
      });
    }
  }

  return merged.sort(
    (a, b) =>
      Number(Boolean(b.connected)) - Number(Boolean(a.connected)) ||
      Number(Boolean(b.enabled)) - Number(Boolean(a.enabled)) ||
      a.name.localeCompare(b.name)
  );
}

function validateModelInput(provider: string, model: string) {
  const providerValue = String(provider || "").trim();
  const modelValue = String(model || "").trim();
  if (!providerValue && !modelValue) return "";
  if (!providerValue) return "Model provider is required when a model is set.";
  if (!modelValue) return "Model is required when a provider is set.";
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

function formatDateTime(raw: unknown) {
  const value = Number(raw || 0);
  if (!Number.isFinite(value) || value <= 0) return "n/a";
  return new Date(value < 1_000_000_000_000 ? value * 1000 : value).toLocaleString();
}

function workflowEditToSchedule(
  draft: WorkflowEditDraft | WizardState
): AutomationV2Spec["schedule"] {
  const timezone = String(draft.timezone || "").trim() || detectBrowserTimezone();
  if (draft.scheduleKind === "manual") {
    return { type: "manual", timezone, misfire_policy: "run_once" };
  }
  if (draft.scheduleKind === "cron") {
    return {
      type: "cron",
      cron_expression: String(draft.cronExpression || "").trim(),
      timezone,
      misfire_policy: "run_once",
    };
  }
  return {
    type: "interval",
    interval_seconds: Math.max(
      1,
      Number.parseInt(String(draft.intervalSeconds || "3600"), 10) || 3600
    ),
    timezone,
    misfire_policy: "run_once",
  };
}

function workflowEditToOperatorPreferences(draft: WorkflowEditDraft | WizardState) {
  const prefs: Record<string, unknown> = {
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
  const plannerProvider = String(draft.plannerModelProvider || "").trim();
  const plannerModelId = String(draft.plannerModelId || "").trim();
  if (modelProvider) prefs.model_provider = modelProvider;
  if (modelId) prefs.model_id = modelId;
  if (plannerProvider && plannerModelId) {
    prefs.role_models = {
      planner: {
        provider_id: plannerProvider,
        model_id: plannerModelId,
      },
    };
  }
  return prefs;
}

function compileWorkflowModelPolicy(operatorPreferences: Record<string, unknown>) {
  const payload: Record<string, unknown> = {};
  const provider = String(operatorPreferences.model_provider || "").trim();
  const model = String(operatorPreferences.model_id || "").trim();
  const roleModels = operatorPreferences.role_models;
  if (provider && model) {
    payload.default_model = {
      provider_id: provider,
      model_id: model,
    };
  }
  if (roleModels && typeof roleModels === "object") {
    payload.role_models = roleModels;
  }
  return Object.keys(payload).length > 0 ? payload : null;
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

function compileWorkflowToolAllowlist(selectedMcpServers: string[]) {
  return Array.from(
    new Set([
      "read",
      "websearch",
      "webfetch",
      "webfetch_html",
      ...selectedMcpServers.map((server) => `mcp.${normalizeMcpServerNamespace(server)}.*`),
    ])
  );
}

function extractAutomationOperatorPreferences(automation: AutomationV2Spec) {
  const metadataPrefs =
    automation?.metadata?.operator_preferences || automation?.metadata?.operatorPreferences;
  if (metadataPrefs && typeof metadataPrefs === "object") {
    return metadataPrefs as Record<string, unknown>;
  }
  const firstAgent = Array.isArray(automation?.agents) ? automation.agents[0] : null;
  const defaultModel =
    firstAgent?.model_policy?.default_model || firstAgent?.model_policy?.defaultModel || null;
  const roleModels =
    firstAgent?.model_policy?.role_models || firstAgent?.model_policy?.roleModels || null;
  const prefs: Record<string, unknown> = {};
  if (defaultModel && typeof defaultModel === "object") {
    const provider = String(
      (defaultModel as Record<string, unknown>).provider_id ||
        (defaultModel as Record<string, unknown>).providerId ||
        ""
    ).trim();
    const model = String(
      (defaultModel as Record<string, unknown>).model_id ||
        (defaultModel as Record<string, unknown>).modelId ||
        ""
    ).trim();
    if (provider) prefs.model_provider = provider;
    if (model) prefs.model_id = model;
  }
  if (roleModels && typeof roleModels === "object") {
    prefs.role_models = roleModels;
  }
  return prefs;
}

function scheduleToEditor(schedule: AutomationV2Spec["schedule"]) {
  const type = String(schedule?.type || "")
    .trim()
    .toLowerCase();
  const cronExpression = String(schedule?.cron_expression || "").trim();
  const intervalSeconds = String(
    Math.max(1, Number.parseInt(String(schedule?.interval_seconds || 3600), 10) || 3600)
  );
  return {
    scheduleKind:
      type === "manual"
        ? ("manual" as const)
        : type === "cron"
          ? ("cron" as const)
          : ("interval" as const),
    cronExpression,
    intervalSeconds,
    timezone: String(schedule?.timezone || "UTC").trim() || "UTC",
  };
}

function workflowAutomationToEditDraft(automation: AutomationV2Spec): WorkflowEditDraft {
  const prefs = extractAutomationOperatorPreferences(automation);
  const plannerRoleModel =
    (prefs.role_models as Record<string, Record<string, unknown>> | undefined)?.planner || {};
  const schedule = scheduleToEditor(automation.schedule);
  const maxParallelRaw = Number(
    (automation.execution?.max_parallel_agents as number | undefined) || 1
  );
  const selectedMcpServers = Array.isArray(automation?.metadata?.allowed_mcp_servers)
    ? (automation.metadata.allowed_mcp_servers as string[])
    : Array.isArray(automation?.agents?.[0]?.mcp_policy?.allowed_servers)
      ? (automation.agents[0].mcp_policy?.allowed_servers as string[])
      : [];
  return {
    automationId: String(automation.automation_id || "").trim(),
    name: String(automation.name || "").trim(),
    description: String(automation.description || "").trim(),
    workspaceRoot: String(
      automation.workspace_root || automation.metadata?.workspace_root || ""
    ).trim(),
    scheduleKind: schedule.scheduleKind,
    intervalSeconds: schedule.intervalSeconds,
    cronExpression: schedule.cronExpression,
    timezone: schedule.timezone,
    executionMode: maxParallelRaw > 1 ? "swarm" : "team",
    maxParallelAgents: String(maxParallelRaw > 0 ? maxParallelRaw : 1),
    modelProvider: String(prefs.model_provider || "").trim(),
    modelId: String(prefs.model_id || "").trim(),
    plannerModelProvider: String(
      plannerRoleModel.provider_id || plannerRoleModel.providerId || ""
    ).trim(),
    plannerModelId: String(plannerRoleModel.model_id || plannerRoleModel.modelId || "").trim(),
    selectedMcpServers: selectedMcpServers.map((row) => String(row || "").trim()).filter(Boolean),
  };
}

function isMissionBlueprintAutomation(automation: AutomationV2Spec | null | undefined) {
  const metadata = (automation?.metadata as Record<string, unknown> | undefined) || {};
  const builderKind = String(metadata.builder_kind || metadata.builderKind || "").trim();
  const missionBlueprint =
    (metadata.mission_blueprint as Record<string, unknown> | undefined) ||
    (metadata.missionBlueprint as Record<string, unknown> | undefined) ||
    null;
  const mission = (metadata.mission as Record<string, unknown> | undefined) || null;
  return (
    builderKind === "mission_blueprint" &&
    ((!!missionBlueprint && typeof missionBlueprint === "object") ||
      (!!mission && typeof mission === "object"))
  );
}

function eventLooksRelevant(event: {
  source: string;
  payload: { type?: string; event_type?: string };
}) {
  const type = String(event.payload?.type || event.payload?.event_type || "").toLowerCase();
  if (event.source === "system") return false;
  return (
    type.includes("run") ||
    type.includes("automation") ||
    type.includes("workflow") ||
    type.includes("routine")
  );
}

function modelLabel(provider: string, model: string) {
  const providerValue = String(provider || "").trim();
  const modelValue = String(model || "").trim();
  if (!providerValue || !modelValue) return "Engine default";
  return `${providerValue}/${modelValue}`;
}

function shortText(raw: unknown, max = 160) {
  const text = String(raw || "")
    .replace(/\s+/g, " ")
    .trim();
  if (!text) return "";
  return text.length > max ? `${text.slice(0, max - 1).trimEnd()}...` : text;
}

function sessionMessageText(message: SessionMessage) {
  const parts = Array.isArray(message?.parts) ? message.parts : [];
  const rows = parts
    .map((part) => {
      const row = (part as Record<string, unknown>) || {};
      const type = String(row.type || "").trim();
      if (type === "text" || type === "reasoning") return String(row.text || "").trim();
      if (type === "tool") {
        const tool = String(row.tool || "tool").trim();
        const error = String(row.error || "").trim();
        const result = row.result ? JSON.stringify(row.result, null, 2) : "";
        return [`tool: ${tool}`, error ? `error: ${error}` : "", result].filter(Boolean).join("\n");
      }
      return String(row.text || "").trim();
    })
    .filter(Boolean);
  return rows.join("\n\n").trim();
}

function sessionMessageVariant(message: SessionMessage) {
  const role = String(message?.info?.role || "")
    .trim()
    .toLowerCase();
  if (role === "user") return "user";
  if (role === "assistant") return "assistant";
  const body = sessionMessageText(message).toLowerCase();
  if (body.includes("error")) return "error";
  return "system";
}

function buildBlockedNodeDiagnostics(
  run: AutomationV2RunRecord | null,
  automation: AutomationV2Spec | null
) {
  if (!run || !automation) return [];
  const completedNodes = completedNodeIds(run);
  const blockedNodes = blockedNodeIds(run);
  return blockedNodes.map((nodeId) => {
    const node = automation.flow?.nodes?.find((entry) => entry.node_id === nodeId);
    const missingDeps = (node?.depends_on || []).filter((dep) => !completedNodes.includes(dep));
    return {
      nodeId,
      missingDeps,
      reason: missingDeps.length
        ? `Waiting on: ${missingDeps.join(", ")}`
        : "Blocked without an explicit upstream dependency explanation in the current checkpoint.",
    };
  });
}

function buildStepStatusDiagnostics(
  run: AutomationV2RunRecord | null,
  automation: AutomationV2Spec | null,
  sessionMessagesBySession: Record<string, SessionMessage[]>
) {
  if (!run || !automation) return [];
  const completedNodes = completedNodeIds(run);
  const pendingNodes = pendingNodeIds(run);
  const blockedNodes = blockedNodeIds(run);
  const attempts = runNodeAttempts(run);
  const outputs = runNodeOutputMap(run);

  return (automation.flow?.nodes || []).map((node) => {
    const output = outputs[node.node_id];
    const sessionId = nodeOutputSessionId(output || {});
    const missingDeps = (node.depends_on || []).filter((dep) => !completedNodes.includes(dep));
    let status = "pending";
    if (completedNodes.includes(node.node_id)) status = "done";
    else if (blockedNodes.includes(node.node_id)) status = "blocked";
    else if (missingDeps.length > 0) status = "waiting";
    else if (pendingNodes.includes(node.node_id)) status = "runnable";
    return {
      nodeId: node.node_id,
      objective: String(node.objective || "").trim(),
      agentId: String(node.agent_id || "").trim(),
      status,
      attempts: Number(attempts[node.node_id] || 0),
      missingDeps,
      sessionId,
      messageCount: sessionId ? (sessionMessagesBySession[sessionId] || []).length : 0,
      summary: nodeOutputSummary(output || {}),
    };
  });
}

function buildStepLogDiagnostics(
  stepStatusRows: Array<{
    nodeId: string;
    objective: string;
    status: string;
    sessionId: string;
    messageCount: number;
  }>,
  lifecycleHistory: Array<{
    event: string;
    recorded_at_ms: number;
    reason?: string | null;
    metadata?: Record<string, unknown> | null;
  }>,
  sessionMessagesBySession: Record<string, SessionMessage[]>
) {
  return stepStatusRows
    .map((step) => ({
      nodeId: step.nodeId,
      objective: step.objective,
      status: step.status,
      sessionId: step.sessionId,
      messageCount: step.messageCount,
      events: lifecycleHistory
        .filter((entry) => {
          const metadata = (entry.metadata || {}) as Record<string, unknown>;
          return String(metadata.node_id || "").trim() === step.nodeId;
        })
        .map((entry, index) => {
          const metadata = (entry.metadata || {}) as Record<string, unknown>;
          return {
            id: `${step.nodeId}-${entry.event}-${entry.recorded_at_ms}-${index}`,
            event: String(entry.event || "").trim(),
            createdAt: Number(entry.recorded_at_ms || 0),
            reason: String(entry.reason || metadata.reason || "").trim(),
            attempt: Number(metadata.attempt || 0),
            sessionId: String(metadata.session_id || "").trim(),
            summary: String(metadata.summary || "").trim(),
            terminal: Boolean(metadata.terminal),
          };
        })
        .sort((a, b) => a.createdAt - b.createdAt),
      messages: (sessionMessagesBySession[step.sessionId] || []).map((message) => ({
        id: String(message.info?.id || "").trim(),
        role: String(message.info?.role || "session").trim(),
        createdAt: Number(message.info?.time?.created || 0),
        variant: sessionMessageVariant(message),
        text: sessionMessageText(message),
      })),
    }))
    .filter((step) => step.events.length > 0 || step.sessionId)
    .sort((a, b) => b.messageCount - a.messageCount || a.nodeId.localeCompare(b.nodeId));
}

type StepLifecycleEntry = {
  event: string;
  recorded_at_ms: number;
  reason?: string | null;
  stop_kind?: string | null;
  metadata?: Record<string, unknown> | null;
};

function automationMissionMetadata(automation: AutomationV2Spec | null) {
  const metadata = (automation?.metadata as Record<string, unknown> | undefined) || {};
  const mission = (metadata.mission as Record<string, unknown> | undefined) || {};
  const phases = Array.isArray(mission.phases)
    ? mission.phases.map(
        (phase) => ((phase as Record<string, unknown>) || {}) as Record<string, unknown>
      )
    : [];
  const milestones = Array.isArray(mission.milestones)
    ? mission.milestones.map(
        (row) => ((row as Record<string, unknown>) || {}) as Record<string, unknown>
      )
    : [];
  return { mission, phases, milestones };
}

function nodeBuilderField(node: AutomationV2Spec["flow"]["nodes"][number] | null, key: string) {
  const metadata = (node?.metadata as Record<string, unknown> | undefined) || {};
  const builder = (metadata.builder as Record<string, unknown> | undefined) || {};
  return String(builder[key] || "").trim();
}

function nodeBuilderObject(node: AutomationV2Spec["flow"]["nodes"][number] | null) {
  const metadata = (node?.metadata as Record<string, unknown> | undefined) || {};
  return (metadata.builder as Record<string, unknown> | undefined) || {};
}

function buildPhaseDiagnostics(
  run: AutomationV2RunRecord | null,
  automation: AutomationV2Spec | null
) {
  if (!run || !automation) return [];
  const completed = new Set(completedNodeIds(run));
  const blocked = new Set(blockedNodeIds(run));
  const pending = new Set(pendingNodeIds(run));
  const { phases } = automationMissionMetadata(automation);
  const phaseRows = phases.map((phase, index) => {
    const phaseId = String(phase.phase_id || "").trim();
    const title = String(phase.title || phaseId || `Phase ${index + 1}`).trim();
    const executionMode = String(phase.execution_mode || "soft").trim();
    const nodes = (automation.flow?.nodes || []).filter(
      (node) => nodeBuilderField(node, "phase_id") === phaseId
    );
    const nodeIds = nodes.map((node) => node.node_id);
    const completedCount = nodeIds.filter((nodeId) => completed.has(nodeId)).length;
    const blockedCount = nodeIds.filter((nodeId) => blocked.has(nodeId)).length;
    const runnableCount = nodes.filter((node) => {
      if (!pending.has(node.node_id) || blocked.has(node.node_id)) return false;
      return (node.depends_on || []).every((dep) => completed.has(dep));
    }).length;
    const totalCount = nodeIds.length;
    const status =
      totalCount > 0 && completedCount === totalCount
        ? "complete"
        : blockedCount > 0
          ? "blocked"
          : runnableCount > 0
            ? "open"
            : "waiting";
    return {
      phaseId,
      title,
      executionMode,
      totalCount,
      completedCount,
      blockedCount,
      runnableCount,
      status,
    };
  });
  const assignedPhaseIds = new Set(phaseRows.map((row) => row.phaseId).filter(Boolean));
  const unassignedNodes = (automation.flow?.nodes || []).filter(
    (node) => !assignedPhaseIds.has(nodeBuilderField(node, "phase_id"))
  );
  if (unassignedNodes.length > 0) {
    const nodeIds = unassignedNodes.map((node) => node.node_id);
    phaseRows.push({
      phaseId: "unassigned",
      title: "Unassigned",
      executionMode: "n/a",
      totalCount: nodeIds.length,
      completedCount: nodeIds.filter((nodeId) => completed.has(nodeId)).length,
      blockedCount: nodeIds.filter((nodeId) => blocked.has(nodeId)).length,
      runnableCount: unassignedNodes.filter((node) => {
        if (!pending.has(node.node_id) || blocked.has(node.node_id)) return false;
        return (node.depends_on || []).every((dep) => completed.has(dep));
      }).length,
      status: "open",
    });
  }
  return phaseRows;
}

function buildMilestoneDiagnostics(
  run: AutomationV2RunRecord | null,
  automation: AutomationV2Spec | null
) {
  if (!run || !automation) return [];
  const completed = new Set(completedNodeIds(run));
  const blocked = new Set(blockedNodeIds(run));
  const { milestones } = automationMissionMetadata(automation);
  return milestones.map((milestone) => {
    const milestoneId = String(milestone.milestone_id || "").trim();
    const title = String(milestone.title || milestoneId).trim();
    const phaseId = String(milestone.phase_id || "").trim();
    const requiredStageIds = Array.isArray(milestone.required_stage_ids)
      ? milestone.required_stage_ids.map((value) => String(value || "").trim()).filter(Boolean)
      : [];
    const completedCount = requiredStageIds.filter((stageId) => completed.has(stageId)).length;
    const blockedCount = requiredStageIds.filter((stageId) => blocked.has(stageId)).length;
    const status =
      requiredStageIds.length > 0 && completedCount === requiredStageIds.length
        ? "complete"
        : blockedCount > 0
          ? "blocked"
          : completedCount > 0
            ? "in_progress"
            : "waiting";
    return {
      milestoneId,
      title,
      phaseId,
      requiredStageIds,
      completedCount,
      blockedCount,
      status,
    };
  });
}

function buildMilestonePromotionDiagnostics(
  milestoneRows: Array<{
    milestoneId: string;
    title: string;
    phaseId: string;
    requiredStageIds: string[];
    completedCount: number;
    blockedCount: number;
    status: string;
  }>,
  stepRows: Array<{
    nodeId: string;
    status: string;
    missingDeps: string[];
  }>,
  gateHistory: Array<Record<string, unknown>>,
  recoveryHistory: Array<Record<string, unknown>>
) {
  const stepMap = new Map(stepRows.map((row) => [row.nodeId, row]));
  return milestoneRows.map((milestone) => {
    const unmetStages = milestone.requiredStageIds
      .map((stageId) => ({
        stageId,
        step: stepMap.get(stageId) || null,
      }))
      .filter(({ step }) => step?.status !== "done");
    const blockedStages = unmetStages
      .filter(({ step }) => step?.status === "blocked")
      .map(({ stageId, step }) => ({
        stageId,
        reason: step?.missingDeps?.length
          ? `waiting on ${step.missingDeps.join(", ")}`
          : "blocked without a dependency explanation in the current checkpoint",
      }));
    const waitingStages = unmetStages
      .filter(
        ({ step }) =>
          step?.status === "waiting" || step?.status === "runnable" || step?.status === "pending"
      )
      .map(({ stageId, step }) => ({
        stageId,
        reason: step?.missingDeps?.length
          ? `waiting on ${step.missingDeps.join(", ")}`
          : `current status: ${step?.status || "unknown"}`,
      }));
    const latestGate =
      gateHistory.find((entry) => {
        const nodeId = String(entry.node_id || "").trim();
        return milestone.requiredStageIds.includes(nodeId);
      }) || null;
    const latestRecovery =
      recoveryHistory.find((entry) => {
        const reason = String(entry.reason || "").toLowerCase();
        return milestone.requiredStageIds.some((stageId) => reason.includes(stageId.toLowerCase()));
      }) || null;
    return {
      ...milestone,
      unmetStages,
      blockedStages,
      waitingStages,
      latestGateDecision: latestGate
        ? {
            nodeId: String(latestGate.node_id || "").trim(),
            decision: String(latestGate.decision || "").trim(),
            decidedAtMs: Number(latestGate.decided_at_ms || 0),
          }
        : null,
      latestRecovery: latestRecovery
        ? {
            event: String(latestRecovery.event || "").trim(),
            reason: String(latestRecovery.reason || "").trim(),
            recordedAtMs: Number(latestRecovery.recorded_at_ms || 0),
          }
        : null,
    };
  });
}

function canPauseRun(run: AutomationV2RunRecord) {
  const status = String(run.status || "")
    .trim()
    .toLowerCase();
  return ["queued", "running", "pausing"].includes(status);
}

function canResumeRun(run: AutomationV2RunRecord) {
  const status = String(run.status || "")
    .trim()
    .toLowerCase();
  return status === "paused";
}

function canCancelRun(run: AutomationV2RunRecord) {
  const status = String(run.status || "")
    .trim()
    .toLowerCase();
  return ["queued", "running", "pausing", "paused"].includes(status);
}

function canRecoverRun(run: AutomationV2RunRecord) {
  const status = String(run.status || "")
    .trim()
    .toLowerCase();
  return (status === "failed" && !!runLastFailure(run)) || status === "paused";
}

function SectionCard({
  title,
  subtitle,
  children,
  actions,
}: {
  title: string;
  subtitle?: string;
  children: React.ReactNode;
  actions?: React.ReactNode;
}) {
  return (
    <section className="rounded-xl border border-border bg-surface p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold text-text">{title}</h3>
          {subtitle ? <p className="mt-1 text-xs text-text-muted">{subtitle}</p> : null}
        </div>
        {actions}
      </div>
      <div className="mt-4">{children}</div>
    </section>
  );
}

export function AgentAutomationPage({
  userProjects,
  activeProject,
  onSwitchProject,
  onAddProject,
  onManageProjects,
  projectSwitcherLoading = false,
  onOpenMcpExtensions,
  initialRunId = null,
}: AgentAutomationPageProps) {
  const [calendarRange, setCalendarRange] = useState<CalendarRange>(() => {
    const now = new Date();
    const start = Date.UTC(
      now.getUTCFullYear(),
      now.getUTCMonth(),
      now.getUTCDate() - now.getUTCDay(),
      0,
      0,
      0,
      0
    );
    return {
      startMs: start,
      endMs: start + 7 * 24 * 60 * 60 * 1000,
    };
  });
  const [tab, setTab] = useState<PageTab>("create");
  const [createMode, setCreateMode] = useState<CreateMode>("simple");
  const [wizardStep, setWizardStep] = useState<WizardStep>(1);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [mcpServers, setMcpServers] = useState<McpServerRecord[]>([]);
  const [availableToolIds, setAvailableToolIds] = useState<string[]>([]);
  const [workflowAutomations, setWorkflowAutomations] = useState<AutomationV2Spec[]>([]);
  const [legacyRoutines, setLegacyRoutines] = useState<RoutineSpec[]>([]);
  const [workflowRuns, setWorkflowRuns] = useState<AutomationV2RunRecord[]>([]);
  const [wizard, setWizard] = useState<WizardState>(() => buildDefaultWizard(activeProject, []));
  const [planPreview, setPlanPreview] = useState<WorkflowPlan | null>(null);
  const [planningConversation, setPlanningConversation] = useState<WorkflowPlanConversation | null>(
    null
  );
  const [planningChangeSummary, setPlanningChangeSummary] = useState<string[]>([]);
  const [plannerDiagnostics, setPlannerDiagnostics] = useState<Record<string, unknown> | null>(
    null
  );
  const [planningMessage, setPlanningMessage] = useState("");
  const [editDraft, setEditDraft] = useState<WorkflowEditDraft | null>(null);
  const [advancedEditAutomation, setAdvancedEditAutomation] = useState<AutomationV2Spec | null>(
    null
  );
  const [selectedRunId, setSelectedRunId] = useState("");
  const [selectedRunDetail, setSelectedRunDetail] = useState<AutomationV2RunRecord | null>(null);
  const [selectedRunMessagesBySession, setSelectedRunMessagesBySession] = useState<
    Record<string, SessionMessage[]>
  >({});
  const [repairDraft, setRepairDraft] = useState<{
    nodeId: string;
    prompt: string;
    templateId: string;
    modelProvider: string;
    modelId: string;
    reason: string;
  }>({
    nodeId: "",
    prompt: "",
    templateId: "",
    modelProvider: "",
    modelId: "",
    reason: "",
  });
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [loadingState, setLoadingState] = useState(true);
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [runsLoading, setRunsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const providerModelMap = useMemo(
    () =>
      new Map(
        providers
          .map((provider) => [provider.id, provider.models] as const)
          .sort((a, b) => a[0].localeCompare(b[0]))
      ),
    [providers]
  );

  const wizardWorkspaceError = validateWorkspaceRootInput(wizard.workspaceRoot);
  const timezoneError =
    String(wizard.timezone || "").trim().length > 0 && !isValidTimezone(wizard.timezone)
      ? "Timezone must be a valid IANA timezone like Europe/Berlin."
      : "";
  const wizardModelError = validateModelInput(wizard.modelProvider, wizard.modelId);
  const wizardPlannerModelError = validatePlannerModelInput(
    wizard.plannerModelProvider,
    wizard.plannerModelId
  );
  const editDraftTimezoneError =
    editDraft &&
    String(editDraft.timezone || "").trim().length > 0 &&
    !isValidTimezone(editDraft.timezone)
      ? "Timezone must be a valid IANA timezone like Europe/Berlin."
      : "";

  const loadCatalog = async () => {
    setCatalogLoading(true);
    try {
      const [providerRows, mcpRows, toolRows] = await Promise.all([
        (async () => {
          const runtimeProviders = await withFallback(
            listProvidersFromSidecar(),
            [] as ProviderInfo[]
          );
          if (runtimeProviders.length > 0) return runtimeProviders;
          const [modelRows, providersConfig] = await Promise.all([
            withFallback(listModels(), [] as ModelInfo[]),
            withFallback(getProvidersConfig(), null as ProvidersConfig | null),
          ]);
          return buildFallbackProviderCatalog(modelRows, providersConfig);
        })(),
        (async () => {
          const [runtimeRows, globalRows, projectRows] = await Promise.all([
            withFallback(mcpListServers(), [] as McpServerRecord[]),
            withFallback(opencodeListMcpServers("global"), [] as OpencodeMcpServerEntry[]),
            withFallback(opencodeListMcpServers("project"), [] as OpencodeMcpServerEntry[]),
          ]);
          return buildConfiguredMcpServerCatalog(runtimeRows, [...globalRows, ...projectRows]);
        })(),
        withFallback(toolIds(), [] as string[]),
      ]);
      setProviders(providerRows);
      setMcpServers(mcpRows);
      setAvailableToolIds(Array.isArray(toolRows) ? toolRows : []);
      setWizard((current) => buildDefaultWizard(activeProject, providerRows, current));
    } finally {
      setCatalogLoading(false);
    }
  };

  const loadAutomationOverview = async () => {
    const [workflowResponse, routineRows] = await Promise.all([
      automationsV2List(),
      routinesList(),
    ]);
    const automations = Array.isArray(workflowResponse?.automations)
      ? workflowResponse.automations
      : [];
    setWorkflowAutomations(automations);
    setLegacyRoutines(routineRows);
  };

  const loadWorkflowRuns = async () => {
    setRunsLoading(true);
    try {
      const response = await automationsV2RunsAll(50);
      const runs = Array.isArray(response?.runs) ? response.runs : [];
      setWorkflowRuns(
        runs.sort(
          (a, b) =>
            Number(b?.updated_at_ms || b?.created_at_ms || 0) -
            Number(a?.updated_at_ms || a?.created_at_ms || 0)
        )
      );
    } finally {
      setRunsLoading(false);
    }
  };

  const refreshWorkflowData = async () => {
    await Promise.all([loadAutomationOverview(), loadWorkflowRuns()]);
  };

  const loadSelectedRunDetail = async (runId: string) => {
    const trimmed = String(runId || "").trim();
    if (!trimmed) {
      setSelectedRunDetail(null);
      setSelectedRunMessagesBySession({});
      return;
    }
    setBusyKey(`inspect:${trimmed}`);
    try {
      const response = await automationsV2RunGet(trimmed);
      const run = response?.run || null;
      setSelectedRunDetail(run);
      const sessionIds = extractSessionIdsFromRun(run);
      if (sessionIds.length > 0) {
        const sessionRows = await Promise.all(
          sessionIds.map(async (sessionId) => ({
            sessionId,
            messages: await getSessionMessages(sessionId).catch(() => []),
          }))
        );
        setSelectedRunMessagesBySession(
          Object.fromEntries(sessionRows.map((row) => [row.sessionId, row.messages]))
        );
      } else {
        setSelectedRunMessagesBySession({});
      }
    } catch (inspectError) {
      setError(inspectError instanceof Error ? inspectError.message : String(inspectError));
    } finally {
      setBusyKey((current) => (current === `inspect:${trimmed}` ? null : current));
    }
  };

  const refreshAll = async () => {
    setLoadingState(true);
    try {
      await loadAutomationOverview();
      setError(null);
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : String(loadError));
    } finally {
      setLoadingState(false);
    }
    void loadCatalog().catch(() => undefined);
    void loadWorkflowRuns().catch(() => undefined);
  };

  useEffect(() => {
    void refreshAll();
  }, [activeProject?.id]);

  useEffect(() => {
    let refreshTimeout: ReturnType<typeof setTimeout> | null = null;
    let disposed = false;
    const start = async () => {
      const unlisten = await onSidecarEventV2((event) => {
        if (!eventLooksRelevant(event) || disposed) return;
        if (refreshTimeout) clearTimeout(refreshTimeout);
        refreshTimeout = setTimeout(() => {
          void loadAutomationOverview().catch(() => undefined);
          void loadWorkflowRuns().catch(() => undefined);
          if (selectedRunId) {
            void loadSelectedRunDetail(selectedRunId).catch(() => undefined);
          }
        }, 500);
      });
      return unlisten;
    };
    let unlistenRef: (() => void) | null = null;
    void start().then((unlisten) => {
      unlistenRef = unlisten;
    });
    return () => {
      disposed = true;
      if (refreshTimeout) clearTimeout(refreshTimeout);
      if (unlistenRef) void unlistenRef();
    };
  }, [selectedRunId]);

  useEffect(() => {
    if (!selectedRunId) {
      setSelectedRunDetail(null);
      setSelectedRunMessagesBySession({});
      return;
    }
    void loadSelectedRunDetail(selectedRunId);
  }, [selectedRunId]);

  useEffect(() => {
    if (!initialRunId) return;
    setSelectedRunId(initialRunId);
    setTab("runs");
  }, [initialRunId]);

  useEffect(() => {
    if (tab !== "runs") return;
    if (workflowRuns.length > 0 || runsLoading) return;
    void loadWorkflowRuns().catch(() => undefined);
  }, [tab, workflowRuns.length, runsLoading]);

  useEffect(() => {
    if (tab !== "create") return;
    if (catalogLoading) return;
    if (providers.length > 0 || mcpServers.length > 0 || availableToolIds.length > 0) return;
    void loadCatalog().catch(() => undefined);
  }, [tab, catalogLoading, providers.length, mcpServers.length, availableToolIds.length]);

  const updateWizard = (patch: Partial<WizardState>) => {
    setWizard((current) => ({ ...current, ...patch }));
    if (planPreview) {
      setPlanPreview(null);
      setPlanningConversation(null);
      setPlanningChangeSummary([]);
      setPlannerDiagnostics(null);
    }
  };

  const updateWizardSchedule = (value: ScheduleValue) => {
    updateWizard({
      scheduleKind: value.scheduleKind,
      cronExpression: value.cronExpression,
      intervalSeconds: value.intervalSeconds,
    });
  };

  const generatePlan = async () => {
    setBusyKey("generate-plan");
    setError(null);
    try {
      const response = await workflowPlansChatStart({
        prompt: wizard.prompt,
        schedule: workflowEditToSchedule(wizard),
        plan_source: "desktop_automation_page",
        allowed_mcp_servers: wizard.selectedMcpServers,
        workspace_root: wizard.workspaceRoot,
        operator_preferences: workflowEditToOperatorPreferences(wizard),
      });
      setPlanPreview(response.plan || null);
      setPlanningConversation(response.conversation || null);
      setPlanningChangeSummary([]);
      setPlannerDiagnostics(
        (response.planner_diagnostics as Record<string, unknown> | null) || null
      );
    } catch (planError) {
      setError(planError instanceof Error ? planError.message : String(planError));
    } finally {
      setBusyKey(null);
    }
  };

  useEffect(() => {
    if (wizardStep !== 4 || planPreview || busyKey === "generate-plan") return;
    if (
      !wizard.prompt.trim() ||
      wizardWorkspaceError ||
      wizardModelError ||
      wizardPlannerModelError
    ) {
      return;
    }
    void generatePlan();
  }, [wizardStep]);

  const sendPlanningMessage = async () => {
    const nextMessage = String(planningMessage || "").trim();
    if (!planPreview?.plan_id || !nextMessage) return;
    setBusyKey("planning-message");
    setError(null);
    try {
      const response = await workflowPlansChatMessage({
        plan_id: planPreview.plan_id,
        message: nextMessage,
      });
      setPlanPreview(response.plan || null);
      setPlanningConversation(response.conversation || null);
      setPlanningChangeSummary(
        Array.isArray(response.change_summary) ? response.change_summary : []
      );
      setPlannerDiagnostics(
        (response.planner_diagnostics as Record<string, unknown> | null) || null
      );
      setPlanningMessage("");
    } catch (messageError) {
      setError(messageError instanceof Error ? messageError.message : String(messageError));
    } finally {
      setBusyKey(null);
    }
  };

  const resetPlanningChat = async () => {
    if (!planPreview?.plan_id) return;
    setBusyKey("planning-reset");
    setError(null);
    try {
      const response = await workflowPlansChatReset({
        plan_id: planPreview.plan_id,
      });
      setPlanPreview(response.plan || null);
      setPlanningConversation(response.conversation || null);
      setPlanningChangeSummary([]);
      setPlannerDiagnostics(
        (response.planner_diagnostics as Record<string, unknown> | null) || null
      );
      setPlanningMessage("");
    } catch (resetError) {
      setError(resetError instanceof Error ? resetError.message : String(resetError));
    } finally {
      setBusyKey(null);
    }
  };

  const createAutomation = async () => {
    if (!planPreview) {
      await generatePlan();
      return;
    }
    setBusyKey("apply-plan");
    setError(null);
    try {
      await workflowPlansApply({
        plan: planPreview,
        creator_id: "desktop",
        ...(wizard.exportPackDraft
          ? { pack_builder_export: { enabled: true, auto_apply: false } }
          : {}),
      });
      const nextWizard = buildDefaultWizard(activeProject, providers);
      setWizard(nextWizard);
      setWizardStep(1);
      setPlanPreview(null);
      setPlanningConversation(null);
      setPlanningChangeSummary([]);
      setPlannerDiagnostics(null);
      setPlanningMessage("");
      setTab("automations");
      await refreshWorkflowData();
    } catch (applyError) {
      setError(applyError instanceof Error ? applyError.message : String(applyError));
    } finally {
      setBusyKey(null);
    }
  };

  const openEditDraft = async (automationId: string) => {
    setBusyKey(`edit:${automationId}`);
    setError(null);
    try {
      const response = await automationsV2Get(automationId);
      if (!response.automation) throw new Error("Automation record not found.");
      if (isMissionBlueprintAutomation(response.automation)) {
        setAdvancedEditAutomation(response.automation);
        setEditDraft(null);
        setCreateMode("advanced");
        setTab("create");
        return;
      }
      setEditDraft(workflowAutomationToEditDraft(response.automation));
    } catch (editError) {
      setError(editError instanceof Error ? editError.message : String(editError));
    } finally {
      setBusyKey(null);
    }
  };

  const saveEditDraft = async () => {
    if (!editDraft) return;
    const workspaceError = validateWorkspaceRootInput(editDraft.workspaceRoot);
    const modelError = validateModelInput(editDraft.modelProvider, editDraft.modelId);
    const plannerError = validatePlannerModelInput(
      editDraft.plannerModelProvider,
      editDraft.plannerModelId
    );
    if (!editDraft.name.trim()) {
      setError("Automation name is required.");
      return;
    }
    if (workspaceError || modelError || plannerError) {
      setError(workspaceError || modelError || plannerError);
      return;
    }
    if (editDraftTimezoneError) {
      setError(editDraftTimezoneError);
      return;
    }
    setBusyKey(`save:${editDraft.automationId}`);
    setError(null);
    try {
      const existingResponse = await automationsV2Get(editDraft.automationId);
      const existing = existingResponse.automation;
      const operatorPreferences = workflowEditToOperatorPreferences(editDraft);
      const modelPolicy = compileWorkflowModelPolicy(operatorPreferences);
      const selectedMcpServers = editDraft.selectedMcpServers
        .map((row) => String(row || "").trim())
        .filter(Boolean);
      const toolAllowlist = compileWorkflowToolAllowlist(selectedMcpServers);
      const agents = Array.isArray(existing?.agents)
        ? existing.agents.map((agent) => ({
            ...agent,
            model_policy: modelPolicy ?? agent.model_policy,
            tool_policy: {
              ...(agent.tool_policy || {}),
              allowlist: toolAllowlist,
              denylist: Array.isArray(agent.tool_policy?.denylist)
                ? agent.tool_policy.denylist
                : [],
            },
            mcp_policy: {
              ...(agent.mcp_policy || {}),
              allowed_servers: selectedMcpServers,
              allowed_tools: null,
            },
          }))
        : [];
      await automationsV2Update(editDraft.automationId, {
        name: editDraft.name.trim(),
        description: editDraft.description.trim() || null,
        workspace_root: editDraft.workspaceRoot.trim(),
        schedule: workflowEditToSchedule(editDraft),
        execution: {
          ...(existing.execution || {}),
          max_parallel_agents:
            editDraft.executionMode === "swarm"
              ? Math.max(
                  1,
                  Math.min(16, Number.parseInt(String(editDraft.maxParallelAgents || "4"), 10) || 4)
                )
              : 1,
        },
        agents,
        metadata: {
          ...(existing.metadata || {}),
          workspace_root: editDraft.workspaceRoot.trim(),
          operator_preferences: operatorPreferences,
          allowed_mcp_servers: selectedMcpServers,
        },
      });
      setEditDraft(null);
      await refreshWorkflowData();
    } catch (saveError) {
      setError(saveError instanceof Error ? saveError.message : String(saveError));
    } finally {
      setBusyKey(null);
    }
  };

  const updateEditDraftSchedule = (value: ScheduleValue) => {
    setEditDraft((current) =>
      current
        ? {
            ...current,
            scheduleKind: value.scheduleKind,
            cronExpression: value.cronExpression,
            intervalSeconds: value.intervalSeconds,
          }
        : current
    );
  };

  const updateCalendarAutomationFromEvent = async (info: {
    event?: {
      extendedProps?: {
        automation?: AutomationV2Spec;
        cronExpression?: string;
      };
      start?: Date | null;
    };
    revert?: () => void;
  }) => {
    const automation = info?.event?.extendedProps?.automation;
    const automationId = String(automation?.automation_id || "").trim();
    const cronExpression = String(info?.event?.extendedProps?.cronExpression || "").trim();
    const start = info?.event?.start ? new Date(info.event.start) : null;
    const nextCron = start ? rewriteCronForDroppedStart(cronExpression, start) : null;
    if (!automationId || !nextCron) {
      info?.revert?.();
      setError("That schedule cannot be moved from the calendar yet.");
      return;
    }
    setBusyKey(`calendar:${automationId}`);
    setError(null);
    try {
      await automationsV2Update(automationId, {
        schedule: {
          type: "cron",
          cron_expression: nextCron,
          timezone: String(automation?.schedule?.timezone || "UTC").trim() || "UTC",
          misfire_policy: "run_once",
        },
      });
      await refreshWorkflowData();
    } catch (calendarError) {
      info?.revert?.();
      setError(calendarError instanceof Error ? calendarError.message : String(calendarError));
    } finally {
      setBusyKey(null);
    }
  };

  const toggleAutomationState = async (automation: AutomationV2Spec) => {
    const automationId = String(automation.automation_id || "").trim();
    if (!automationId) return;
    setBusyKey(`toggle:${automationId}`);
    setError(null);
    try {
      if (String(automation.status || "").trim() === "paused") {
        await automationsV2Resume(automationId);
      } else {
        await automationsV2Pause(automationId, "Paused from desktop automation page");
      }
      await refreshWorkflowData();
    } catch (toggleError) {
      setError(toggleError instanceof Error ? toggleError.message : String(toggleError));
    } finally {
      setBusyKey(null);
    }
  };

  const triggerAutomationRun = async (automationId: string) => {
    setBusyKey(`run:${automationId}`);
    setError(null);
    try {
      await automationsV2RunNow(automationId);
      setTab("runs");
      await refreshWorkflowData();
    } catch (runError) {
      setError(runError instanceof Error ? runError.message : String(runError));
    } finally {
      setBusyKey(null);
    }
  };

  const deleteAutomation = async (automationId: string, name: string) => {
    if (!window.confirm(`Delete workflow automation '${name}'?`)) return;
    setBusyKey(`delete:${automationId}`);
    setError(null);
    try {
      await automationsV2Delete(automationId);
      await refreshWorkflowData();
    } catch (deleteError) {
      setError(deleteError instanceof Error ? deleteError.message : String(deleteError));
    } finally {
      setBusyKey(null);
    }
  };

  const handleRunAction = async (runId: string, action: "pause" | "resume" | "cancel") => {
    setBusyKey(`${action}:${runId}`);
    setError(null);
    try {
      if (action === "pause") {
        await automationsV2RunPause(runId, "Paused from desktop automation page");
      } else if (action === "resume") {
        await automationsV2RunResume(runId, "Resumed from desktop automation page");
      } else {
        await automationsV2RunCancel(runId, "Cancelled from desktop automation page");
      }
      await refreshWorkflowData();
    } catch (actionError) {
      setError(actionError instanceof Error ? actionError.message : String(actionError));
    } finally {
      setBusyKey(null);
    }
  };

  const handleGateDecision = async (runId: string, decision: "approve" | "rework" | "cancel") => {
    setBusyKey(`gate:${decision}:${runId}`);
    setError(null);
    try {
      await automationsV2RunGateDecide(runId, { decision });
      await refreshWorkflowData();
      await loadSelectedRunDetail(runId);
    } catch (gateError) {
      setError(gateError instanceof Error ? gateError.message : String(gateError));
    } finally {
      setBusyKey(null);
    }
  };

  const handleRunRecover = async (runId: string) => {
    setBusyKey(`recover:${runId}`);
    setError(null);
    try {
      const currentRun =
        workflowRuns.find((run) => run.run_id === runId) ||
        (selectedRunDetail?.run_id === runId ? selectedRunDetail : null);
      const status = String(currentRun?.status || "")
        .trim()
        .toLowerCase();
      await automationsV2RunRecover(
        runId,
        status === "paused"
          ? "Recovered from paused state via desktop automation page"
          : "Recovered from desktop automation page"
      );
      await refreshWorkflowData();
      await loadSelectedRunDetail(runId);
    } catch (recoverError) {
      setError(recoverError instanceof Error ? recoverError.message : String(recoverError));
    } finally {
      setBusyKey(null);
    }
  };

  const handleRunRepair = async (runId: string) => {
    if (!repairDraft.nodeId.trim()) {
      setError("Select a step to repair.");
      return;
    }
    const hasPrompt = repairDraft.prompt.trim().length > 0;
    const hasTemplate = repairDraft.templateId.trim().length > 0;
    const hasModel =
      repairDraft.modelProvider.trim().length > 0 && repairDraft.modelId.trim().length > 0;
    if (!hasPrompt && !hasTemplate && !hasModel) {
      setError("Repair requires a prompt, template, or model change.");
      return;
    }
    setBusyKey(`repair:${runId}`);
    setError(null);
    try {
      await automationsV2RunRepair(runId, {
        node_id: repairDraft.nodeId.trim(),
        prompt: hasPrompt ? repairDraft.prompt.trim() : undefined,
        template_id: hasTemplate ? repairDraft.templateId.trim() : undefined,
        model_policy: hasModel
          ? {
              default_model: {
                provider_id: repairDraft.modelProvider.trim(),
                model_id: repairDraft.modelId.trim(),
              },
            }
          : undefined,
        reason: repairDraft.reason.trim() || undefined,
      });
      await refreshWorkflowData();
      await loadSelectedRunDetail(runId);
    } catch (repairError) {
      setError(repairError instanceof Error ? repairError.message : String(repairError));
    } finally {
      setBusyKey(null);
    }
  };

  const stepCanContinue =
    wizardStep === 1
      ? wizard.prompt.trim().length >= 8
      : wizardStep === 2
        ? (wizard.scheduleKind === "manual"
            ? true
            : wizard.scheduleKind === "cron"
              ? wizard.cronExpression.trim().length > 0
              : Number.parseInt(String(wizard.intervalSeconds || "0"), 10) > 0) && !timezoneError
        : wizardStep === 3
          ? !wizardWorkspaceError && !wizardModelError && !wizardPlannerModelError
          : true;

  const configuredWorkflowCount = workflowAutomations.length;
  const activeWorkflowCount = workflowAutomations.filter(
    (automation) => String(automation.status || "").trim() === "active"
  ).length;
  const calendarEvents = useMemo(
    () =>
      workflowAutomations.flatMap((automation) =>
        buildWorkflowCalendarOccurrences(automation, calendarRange)
      ),
    [calendarRange, workflowAutomations]
  );
  const runningWorkflowCount = workflowRuns.filter((run) =>
    ["queued", "running", "pausing"].includes(String(run.status || "").trim())
  ).length;
  const failedWorkflowCount = workflowRuns.filter(
    (run) => String(run.status || "").trim() === "failed"
  ).length;
  const selectedRunBlockers = useMemo(
    () => extractRunBlockers(selectedRunDetail),
    [selectedRunDetail]
  );
  const selectedRunNodeOutputs = useMemo(
    () => extractRunNodeOutputs(selectedRunDetail),
    [selectedRunDetail]
  );
  const selectedRunSessionIds = useMemo(
    () => extractSessionIdsFromRun(selectedRunDetail),
    [selectedRunDetail]
  );
  const selectedRunAutomation = useMemo(
    () =>
      workflowAutomations.find(
        (automation) => automation.automation_id === selectedRunDetail?.automation_id
      ) ||
      selectedRunDetail?.automation_snapshot ||
      null,
    [workflowAutomations, selectedRunDetail?.automation_id, selectedRunDetail?.automation_snapshot]
  );
  const selectedAwaitingGate = useMemo(() => {
    return runAwaitingGate(selectedRunDetail);
  }, [selectedRunDetail]);
  const selectedLastFailure = useMemo(() => {
    return runLastFailure(selectedRunDetail);
  }, [selectedRunDetail]);
  const selectedBlockedNodes = useMemo(() => {
    return blockedNodeIds(selectedRunDetail);
  }, [selectedRunDetail]);
  const selectedRunUsage = useMemo(() => {
    return runUsageMetrics(selectedRunDetail);
  }, [selectedRunDetail]);
  const selectedLifecycleHistory = useMemo<StepLifecycleEntry[]>(() => {
    return runLifecycleEntries(selectedRunDetail);
  }, [selectedRunDetail]);
  const selectedGateHistory = useMemo(() => {
    return runGateHistory(selectedRunDetail).sort(
      (a, b) => Number(b.decided_at_ms || 0) - Number(a.decided_at_ms || 0)
    );
  }, [selectedRunDetail]);
  const selectedRecoveryHistory = useMemo(
    () => recoveryLifecycleEntries(selectedLifecycleHistory),
    [selectedLifecycleHistory]
  );
  const selectedBlockedNodeDiagnostics = useMemo(
    () => buildBlockedNodeDiagnostics(selectedRunDetail, selectedRunAutomation),
    [selectedRunDetail, selectedRunAutomation]
  );
  const selectedFailureChain = useMemo(() => {
    return failureChainLifecycleEntries(selectedLifecycleHistory, selectedLastFailure, 8);
  }, [selectedLifecycleHistory, selectedLastFailure]);
  const selectedPromotionHistory = useMemo(
    () => promotionLifecycleEntries(selectedLifecycleHistory),
    [selectedLifecycleHistory]
  );
  const selectedRepairHistory = useMemo(
    () => repairLifecycleEntries(selectedLifecycleHistory),
    [selectedLifecycleHistory]
  );
  const selectedTranscriptSessions = useMemo(
    () =>
      selectedRunSessionIds.map((sessionId) => ({
        sessionId,
        messages: selectedRunMessagesBySession[sessionId] || [],
      })),
    [selectedRunSessionIds, selectedRunMessagesBySession]
  );
  const selectedStepDiagnostics = useMemo(
    () =>
      selectedRunNodeOutputs.map((output) => {
        const sessionId = nodeOutputSessionId(output.value || {});
        const node =
          selectedRunAutomation?.flow?.nodes?.find((entry) => entry.node_id === output.nodeId) ||
          null;
        const sessionMessages = sessionId ? selectedRunMessagesBySession[sessionId] || [] : [];
        return {
          nodeId: output.nodeId,
          objective: String(node?.objective || "").trim(),
          contractKind: String(output.value?.contract_kind || "").trim(),
          summary: nodeOutputSummary(output.value || {}),
          sessionId,
          messageCount: sessionMessages.length,
        };
      }),
    [selectedRunNodeOutputs, selectedRunAutomation, selectedRunMessagesBySession]
  );
  const selectedStepStatusRows = useMemo(
    () =>
      buildStepStatusDiagnostics(
        selectedRunDetail,
        selectedRunAutomation,
        selectedRunMessagesBySession
      ),
    [selectedRunDetail, selectedRunAutomation, selectedRunMessagesBySession]
  );
  const selectedStepLogs = useMemo(
    () =>
      buildStepLogDiagnostics(
        selectedStepStatusRows,
        selectedLifecycleHistory,
        selectedRunMessagesBySession
      ),
    [selectedStepStatusRows, selectedLifecycleHistory, selectedRunMessagesBySession]
  );
  const selectedPhaseDiagnostics = useMemo(
    () => buildPhaseDiagnostics(selectedRunDetail, selectedRunAutomation),
    [selectedRunDetail, selectedRunAutomation]
  );
  const selectedMilestoneDiagnostics = useMemo(
    () => buildMilestoneDiagnostics(selectedRunDetail, selectedRunAutomation),
    [selectedRunDetail, selectedRunAutomation]
  );
  const selectedMilestonePromotionDiagnostics = useMemo(
    () =>
      buildMilestonePromotionDiagnostics(
        selectedMilestoneDiagnostics,
        selectedStepStatusRows,
        selectedGateHistory,
        selectedRecoveryHistory
      ),
    [
      selectedMilestoneDiagnostics,
      selectedStepStatusRows,
      selectedGateHistory,
      selectedRecoveryHistory,
    ]
  );

  useEffect(() => {
    const preferredNodeId = String(
      selectedLastFailure?.node_id || selectedAwaitingGate?.node_id || ""
    ).trim();
    if (!preferredNodeId || !selectedRunAutomation) {
      setRepairDraft({
        nodeId: "",
        prompt: "",
        templateId: "",
        modelProvider: "",
        modelId: "",
        reason: "",
      });
      return;
    }
    const node =
      selectedRunAutomation.flow?.nodes?.find((entry) => entry.node_id === preferredNodeId) || null;
    const agent =
      selectedRunAutomation.agents?.find((entry) => entry.agent_id === node?.agent_id) || null;
    const agentPolicy = (agent?.model_policy as Record<string, unknown> | undefined) || {};
    const model = ((agentPolicy.default_model as Record<string, unknown> | undefined) ||
      (agentPolicy.defaultModel as Record<string, unknown> | undefined) ||
      {}) as Record<string, unknown>;
    setRepairDraft({
      nodeId: preferredNodeId,
      prompt: String(nodeBuilderObject(node).prompt || "").trim(),
      templateId: String(agent?.template_id || "").trim(),
      modelProvider: String(model.provider_id || model.providerId || "").trim(),
      modelId: String(model.model_id || model.modelId || "").trim(),
      reason: "",
    });
  }, [selectedRunAutomation, selectedLastFailure?.node_id, selectedAwaitingGate?.node_id]);

  return (
    <div className="h-full overflow-y-auto p-4">
      <div className="mx-auto max-w-[1480px] space-y-4">
        <SectionCard
          title="Desktop Automation"
          subtitle="Workflow-first desktop automation with shared engine state."
          actions={
            <div className="flex flex-wrap gap-2">
              <Button
                size="sm"
                variant={tab === "create" ? "primary" : "secondary"}
                onClick={() => setTab("create")}
              >
                Create
              </Button>
              <Button
                size="sm"
                variant={tab === "calendar" ? "primary" : "secondary"}
                onClick={() => setTab("calendar")}
              >
                Calendar
              </Button>
              <Button
                size="sm"
                variant={tab === "automations" ? "primary" : "secondary"}
                onClick={() => setTab("automations")}
              >
                My Automations
              </Button>
              <Button
                size="sm"
                variant={tab === "runs" ? "primary" : "secondary"}
                onClick={() => setTab("runs")}
              >
                Live Tasks
              </Button>
              <Button size="sm" variant="secondary" onClick={() => void refreshAll()}>
                Refresh
              </Button>
            </div>
          }
        >
          <div className="grid gap-3 sm:grid-cols-4">
            <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
              <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                Workflow Automations
              </div>
              <div className="mt-1 text-2xl font-semibold text-text">{configuredWorkflowCount}</div>
            </div>
            <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
              <div className="text-[10px] uppercase tracking-wide text-text-subtle">Active</div>
              <div className="mt-1 text-2xl font-semibold text-text">{activeWorkflowCount}</div>
            </div>
            <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
              <div className="text-[10px] uppercase tracking-wide text-text-subtle">Running</div>
              <div className="mt-1 text-2xl font-semibold text-text">{runningWorkflowCount}</div>
            </div>
            <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
              <div className="text-[10px] uppercase tracking-wide text-text-subtle">Failed</div>
              <div className="mt-1 text-2xl font-semibold text-text">{failedWorkflowCount}</div>
            </div>
          </div>
        </SectionCard>

        <SectionCard
          title="Project Context"
          subtitle="Workspace selection is shared with desktop projects."
        >
          <ProjectSwitcher
            projects={userProjects}
            activeProject={activeProject}
            onSwitchProject={onSwitchProject}
            onAddProject={onAddProject}
            onManageProjects={onManageProjects}
            isLoading={projectSwitcherLoading}
          />
        </SectionCard>

        {error ? (
          <div className="rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-200">
            {error}
          </div>
        ) : null}

        {loadingState ? (
          <div className="rounded-lg border border-border bg-surface px-4 py-8 text-center text-sm text-text-muted">
            Loading automation state...
          </div>
        ) : null}

        {!loadingState && tab === "create" ? (
          <>
            <SectionCard
              title="Builder Mode"
              subtitle="Keep the simple wizard for fast automations or switch to the advanced mission compiler."
            >
              <div className="flex flex-wrap gap-2">
                <Button
                  size="sm"
                  variant={createMode === "simple" ? "primary" : "secondary"}
                  onClick={() => {
                    setAdvancedEditAutomation(null);
                    setCreateMode("simple");
                  }}
                >
                  Simple Wizard
                </Button>
                <Button
                  size="sm"
                  variant={createMode === "advanced" ? "primary" : "secondary"}
                  onClick={() => setCreateMode("advanced")}
                >
                  Advanced Swarm Builder
                </Button>
              </div>
            </SectionCard>

            {createMode === "simple" ? (
              <SectionCard
                title="Create Workflow Automation"
                subtitle="Desktop-native flow backed by workflow-plans and automations v2."
                actions={
                  <div className="flex items-center gap-2 text-xs text-text-muted">
                    <span>Step {wizardStep} of 4</span>
                  </div>
                }
              >
                <div className="grid gap-2 sm:grid-cols-4">
                  {[
                    { step: 1 as WizardStep, label: "What" },
                    { step: 2 as WizardStep, label: "When" },
                    { step: 3 as WizardStep, label: "How" },
                    { step: 4 as WizardStep, label: "Review" },
                  ].map((entry) => (
                    <button
                      key={entry.step}
                      type="button"
                      className={`rounded-lg border px-3 py-2 text-left text-sm ${
                        wizardStep === entry.step
                          ? "border-primary bg-primary/10 text-text"
                          : wizardStep > entry.step
                            ? "border-border bg-surface-elevated/40 text-text"
                            : "border-border bg-surface text-text-muted"
                      }`}
                      onClick={() => {
                        if (entry.step <= wizardStep) setWizardStep(entry.step);
                      }}
                    >
                      <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                        Step {entry.step}
                      </div>
                      <div className="mt-1 font-medium">{entry.label}</div>
                    </button>
                  ))}
                </div>

                {wizardStep === 1 ? (
                  <div className="mt-4 space-y-3">
                    <label className="block text-sm font-medium text-text">Objective</label>
                    <textarea
                      value={wizard.prompt}
                      onChange={(event) => updateWizard({ prompt: event.target.value })}
                      className="min-h-[160px] w-full rounded-lg border border-border bg-surface px-3 py-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                      placeholder="Describe what the automation should accomplish, what signals it should use, and what output you want."
                    />
                  </div>
                ) : null}

                {wizardStep === 2 ? (
                  <div className="mt-4 space-y-4">
                    <ScheduleBuilder
                      value={{
                        scheduleKind: wizard.scheduleKind,
                        intervalSeconds: wizard.intervalSeconds,
                        cronExpression: wizard.cronExpression,
                      }}
                      onChange={updateWizardSchedule}
                    />
                    <TimezoneField
                      value={wizard.timezone}
                      onChange={(value) => updateWizard({ timezone: value })}
                      error={timezoneError || undefined}
                      hint="Use the timezone that matches when this automation should run."
                    />
                  </div>
                ) : null}

                {wizardStep === 3 ? (
                  <div className="mt-4 grid gap-4 lg:grid-cols-2">
                    <div className="space-y-3">
                      <Input
                        label="Workspace Root"
                        value={wizard.workspaceRoot}
                        error={wizardWorkspaceError || undefined}
                        onChange={(event) => updateWizard({ workspaceRoot: event.target.value })}
                      />
                      <div className="grid gap-3 sm:grid-cols-2">
                        <label className="block text-sm font-medium text-text">
                          Execution Mode
                          <select
                            value={wizard.executionMode}
                            onChange={(event) =>
                              updateWizard({
                                executionMode: event.target.value === "swarm" ? "swarm" : "team",
                              })
                            }
                            className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                          >
                            <option value="team">Team</option>
                            <option value="swarm">Swarm</option>
                          </select>
                        </label>
                        <Input
                          label="Max Parallel Agents"
                          type="number"
                          min={1}
                          max={16}
                          value={wizard.maxParallelAgents}
                          onChange={(event) =>
                            updateWizard({ maxParallelAgents: event.target.value })
                          }
                        />
                      </div>
                      <div className="grid gap-3 sm:grid-cols-2">
                        <label className="block text-sm font-medium text-text">
                          Workflow Provider
                          <select
                            value={wizard.modelProvider}
                            onChange={(event) => {
                              const nextProvider = event.target.value;
                              updateWizard({
                                modelProvider: nextProvider,
                                modelId: providerModelMap.get(nextProvider)?.[0] ?? "",
                              });
                            }}
                            className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                          >
                            <option value="">Engine default</option>
                            {providers.map((provider) => (
                              <option key={provider.id} value={provider.id}>
                                {provider.id}
                              </option>
                            ))}
                          </select>
                        </label>
                        <label className="block text-sm font-medium text-text">
                          Workflow Model
                          <select
                            value={wizard.modelId}
                            onChange={(event) => updateWizard({ modelId: event.target.value })}
                            className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                          >
                            <option value="">Engine default</option>
                            {(providerModelMap.get(wizard.modelProvider) ?? []).map((modelId) => (
                              <option key={modelId} value={modelId}>
                                {modelId}
                              </option>
                            ))}
                          </select>
                        </label>
                      </div>
                      {wizardModelError ? (
                        <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-200">
                          {wizardModelError}
                        </div>
                      ) : null}
                      <div className="grid gap-3 sm:grid-cols-2">
                        <label className="block text-sm font-medium text-text">
                          Planner Provider
                          <select
                            value={wizard.plannerModelProvider}
                            onChange={(event) => {
                              const nextProvider = event.target.value;
                              updateWizard({
                                plannerModelProvider: nextProvider,
                                plannerModelId: providerModelMap.get(nextProvider)?.[0] ?? "",
                              });
                            }}
                            className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                          >
                            <option value="">Fallback to workflow model</option>
                            {providers.map((provider) => (
                              <option key={provider.id} value={provider.id}>
                                {provider.id}
                              </option>
                            ))}
                          </select>
                        </label>
                        <label className="block text-sm font-medium text-text">
                          Planner Model
                          <select
                            value={wizard.plannerModelId}
                            onChange={(event) =>
                              updateWizard({ plannerModelId: event.target.value })
                            }
                            className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                          >
                            <option value="">Fallback to workflow model</option>
                            {(providerModelMap.get(wizard.plannerModelProvider) ?? []).map(
                              (modelId) => (
                                <option key={modelId} value={modelId}>
                                  {modelId}
                                </option>
                              )
                            )}
                          </select>
                        </label>
                      </div>
                      {wizardPlannerModelError ? (
                        <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-200">
                          {wizardPlannerModelError}
                        </div>
                      ) : null}
                    </div>
                    <div className="space-y-3">
                      <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                        <div className="flex items-center justify-between gap-2">
                          <div>
                            <div className="text-sm font-medium text-text">Allowed MCP Servers</div>
                            <div className="text-xs text-text-muted">
                              Selected servers are written into operator preferences and agent MCP
                              policy.
                            </div>
                          </div>
                          {onOpenMcpExtensions ? (
                            <Button size="sm" variant="secondary" onClick={onOpenMcpExtensions}>
                              Manage MCP
                            </Button>
                          ) : null}
                        </div>
                        <div className="mt-3 space-y-2">
                          {mcpServers.map((server) => {
                            const checked = wizard.selectedMcpServers.includes(server.name);
                            return (
                              <label
                                key={server.name}
                                className="flex items-start gap-2 rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text"
                              >
                                <input
                                  type="checkbox"
                                  checked={checked}
                                  onChange={() =>
                                    updateWizard({
                                      selectedMcpServers: checked
                                        ? wizard.selectedMcpServers.filter(
                                            (row) => row !== server.name
                                          )
                                        : [...wizard.selectedMcpServers, server.name],
                                    })
                                  }
                                />
                                <span className="min-w-0">
                                  <span className="block font-medium">{server.name}</span>
                                  <span className="block text-xs text-text-muted">
                                    {server.connected ? "connected" : "disconnected"} |{" "}
                                    {server.enabled ? "enabled" : "disabled"}
                                  </span>
                                </span>
                              </label>
                            );
                          })}
                          {mcpServers.length === 0 ? (
                            <div className="rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text-muted">
                              No MCP servers configured.
                            </div>
                          ) : null}
                        </div>
                      </div>
                    </div>
                  </div>
                ) : null}

                {wizardStep === 4 ? (
                  <div className="mt-4 space-y-4">
                    <div className="grid gap-3 lg:grid-cols-[1.2fr_0.8fr]">
                      <div className="space-y-3 rounded-lg border border-border bg-surface-elevated/40 p-3">
                        <div className="flex items-center justify-between gap-2">
                          <div>
                            <div className="text-sm font-medium text-text">Plan Preview</div>
                            <div className="text-xs text-text-muted">
                              Review and revise before creating the automation.
                            </div>
                          </div>
                          <Button
                            size="sm"
                            variant="secondary"
                            loading={busyKey === "generate-plan"}
                            onClick={() => void generatePlan()}
                          >
                            Regenerate
                          </Button>
                        </div>
                        {planPreview ? (
                          <div className="space-y-3">
                            <div>
                              <div className="text-xs uppercase tracking-wide text-text-subtle">
                                Title
                              </div>
                              <div className="mt-1 text-sm font-medium text-text">
                                {planPreview.title || "Untitled plan"}
                              </div>
                            </div>
                            {planPreview.description ? (
                              <div className="text-sm text-text-muted">
                                {planPreview.description}
                              </div>
                            ) : null}
                            <div className="rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text-muted">
                              Workspace:{" "}
                              <span className="font-medium text-text">
                                {planPreview.workspace_root || wizard.workspaceRoot}
                              </span>
                            </div>
                            <div className="space-y-2">
                              {planPreview.steps.map((step, index) => (
                                <div
                                  key={step.step_id || `${step.kind}-${index}`}
                                  className="rounded-lg border border-border bg-surface px-3 py-2"
                                >
                                  <div className="text-xs uppercase tracking-wide text-text-subtle">
                                    {step.kind}
                                  </div>
                                  <div className="mt-1 text-sm font-medium text-text">
                                    {step.objective}
                                  </div>
                                  {step.depends_on?.length ? (
                                    <div className="mt-1 text-xs text-text-muted">
                                      Depends on: {step.depends_on.join(", ")}
                                    </div>
                                  ) : null}
                                </div>
                              ))}
                            </div>
                          </div>
                        ) : (
                          <div className="rounded-lg border border-border bg-surface px-3 py-6 text-center text-sm text-text-muted">
                            {busyKey === "generate-plan"
                              ? "Generating workflow plan..."
                              : "No plan yet."}
                          </div>
                        )}
                      </div>
                      <div className="space-y-3 rounded-lg border border-border bg-surface-elevated/40 p-3">
                        <div className="text-sm font-medium text-text">Planning Chat</div>
                        <div className="max-h-[320px] space-y-2 overflow-y-auto rounded-lg border border-border bg-surface px-3 py-3">
                          {(planningConversation?.messages || []).map((message, index) => (
                            <div key={`${message.created_at_ms || index}-${index}`}>
                              <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                                {message.role}
                              </div>
                              <div className="mt-1 whitespace-pre-wrap text-sm text-text">
                                {message.text}
                              </div>
                            </div>
                          ))}
                          {!planningConversation?.messages?.length ? (
                            <div className="text-sm text-text-muted">
                              The workflow planning conversation will appear here.
                            </div>
                          ) : null}
                        </div>
                        <textarea
                          value={planningMessage}
                          onChange={(event) => setPlanningMessage(event.target.value)}
                          className="min-h-[88px] w-full rounded-lg border border-border bg-surface px-3 py-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                          placeholder="Ask for revisions, clarify the plan, or tighten the workflow."
                        />
                        <div className="flex flex-wrap gap-2">
                          <Button
                            size="sm"
                            variant="secondary"
                            loading={busyKey === "planning-message"}
                            onClick={() => void sendPlanningMessage()}
                          >
                            Send revision
                          </Button>
                          <Button
                            size="sm"
                            variant="secondary"
                            loading={busyKey === "planning-reset"}
                            onClick={() => void resetPlanningChat()}
                          >
                            Reset chat
                          </Button>
                        </div>
                        {planningChangeSummary.length ? (
                          <div className="rounded-lg border border-border bg-surface px-3 py-2">
                            <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                              Change Summary
                            </div>
                            <ul className="mt-2 space-y-1 text-sm text-text">
                              {planningChangeSummary.map((item) => (
                                <li key={item}>- {item}</li>
                              ))}
                            </ul>
                          </div>
                        ) : null}
                        {plannerDiagnostics ? (
                          <div className="rounded-lg border border-border bg-surface px-3 py-2">
                            <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                              Planner Diagnostics
                            </div>
                            <pre className="mt-2 overflow-x-auto whitespace-pre-wrap text-xs text-text-muted">
                              {JSON.stringify(plannerDiagnostics, null, 2)}
                            </pre>
                          </div>
                        ) : null}
                        <label className="inline-flex items-center gap-2 text-sm text-text-muted">
                          <input
                            type="checkbox"
                            checked={wizard.exportPackDraft}
                            onChange={(event) =>
                              updateWizard({ exportPackDraft: event.target.checked })
                            }
                          />
                          Also export Pack Builder draft
                        </label>
                      </div>
                    </div>
                  </div>
                ) : null}

                <div className="mt-4 flex items-center justify-between gap-2">
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={wizardStep === 1}
                    onClick={() =>
                      setWizardStep((current) => Math.max(1, current - 1) as WizardStep)
                    }
                  >
                    Back
                  </Button>
                  {wizardStep < 4 ? (
                    <Button
                      size="sm"
                      variant="primary"
                      disabled={!stepCanContinue}
                      onClick={() =>
                        setWizardStep((current) => Math.min(4, current + 1) as WizardStep)
                      }
                    >
                      Next
                    </Button>
                  ) : (
                    <Button
                      size="sm"
                      variant="primary"
                      loading={busyKey === "apply-plan"}
                      onClick={() => void createAutomation()}
                    >
                      Create Automation
                    </Button>
                  )}
                </div>
              </SectionCard>
            ) : (
              <AdvancedMissionBuilder
                activeProject={activeProject}
                providers={providers}
                mcpServers={mcpServers}
                toolIds={availableToolIds}
                editingAutomation={advancedEditAutomation}
                onOpenMcpExtensions={onOpenMcpExtensions}
                onRefreshAutomations={refreshWorkflowData}
                onShowAutomations={() => setTab("automations")}
                onShowRuns={() => setTab("runs")}
                onClearEditing={() => setAdvancedEditAutomation(null)}
              />
            )}
          </>
        ) : null}

        {!loadingState && tab === "calendar" ? (
          <SectionCard
            title="Workflow Calendar"
            subtitle="Week and day views for workflow automations. Legacy routines stay in the compatibility list for now."
          >
            <Suspense
              fallback={
                <div className="flex min-h-[220px] items-center justify-center rounded-lg border border-border bg-surface/40 text-sm text-text-muted">
                  Loading workflow calendar...
                </div>
              }
            >
              <AutomationCalendar
                events={calendarEvents}
                onRangeChange={setCalendarRange}
                onOpenAutomation={openEditDraft}
                onEventDrop={(info) => void updateCalendarAutomationFromEvent(info)}
              />
            </Suspense>
          </SectionCard>
        ) : null}

        {!loadingState && tab === "automations" ? (
          <>
            <SectionCard
              title="My Automations"
              subtitle="Workflow automations backed by shared engine state."
            >
              <div className="space-y-3">
                {workflowAutomations.map((automation) => {
                  const automationId = String(automation.automation_id || "").trim();
                  const draft = extractAutomationOperatorPreferences(automation);
                  const plannerRoleModel =
                    (draft.role_models as Record<string, Record<string, unknown>> | undefined)
                      ?.planner || {};
                  const runCount = workflowRuns.filter(
                    (run) => run.automation_id === automationId
                  ).length;
                  return (
                    <div
                      key={automationId}
                      className="rounded-lg border border-border bg-surface-elevated/40 p-3"
                    >
                      <div className="flex flex-wrap items-start justify-between gap-3">
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-center gap-2">
                            <div className="text-sm font-semibold text-text">
                              {automation.name || automationId}
                            </div>
                            <span className="rounded border border-border bg-surface px-2 py-0.5 text-[10px] uppercase tracking-wide text-text-subtle">
                              {automation.status || "active"}
                            </span>
                          </div>
                          {automation.description ? (
                            <div className="mt-1 text-sm text-text-muted">
                              {automation.description}
                            </div>
                          ) : null}
                          <div className="mt-2 grid gap-2 text-xs text-text-muted sm:grid-cols-2 lg:grid-cols-4">
                            <div>Workspace: {automation.workspace_root || "n/a"}</div>
                            <div>
                              Schedule: {formatAutomationV2ScheduleLabel(automation.schedule)}
                            </div>
                            <div>
                              Model:{" "}
                              {modelLabel(
                                String(draft.model_provider || ""),
                                String(draft.model_id || "")
                              )}
                            </div>
                            <div>
                              Planner:{" "}
                              {modelLabel(
                                String(
                                  plannerRoleModel.provider_id || plannerRoleModel.providerId || ""
                                ),
                                String(plannerRoleModel.model_id || plannerRoleModel.modelId || "")
                              )}
                            </div>
                            <div>
                              MCP:{" "}
                              {Array.isArray(automation.metadata?.allowed_mcp_servers)
                                ? (automation.metadata?.allowed_mcp_servers as string[]).join(
                                    ", "
                                  ) || "none"
                                : "none"}
                            </div>
                            <div>Runs loaded: {runCount}</div>
                          </div>
                        </div>
                        <div className="flex flex-wrap gap-2">
                          <Button
                            size="sm"
                            variant="secondary"
                            loading={busyKey === `run:${automationId}`}
                            onClick={() => void triggerAutomationRun(automationId)}
                          >
                            Run now
                          </Button>
                          <Button
                            size="sm"
                            variant="secondary"
                            loading={busyKey === `edit:${automationId}`}
                            onClick={() => void openEditDraft(automationId)}
                          >
                            {isMissionBlueprintAutomation(automation) ? "Edit Mission" : "Edit"}
                          </Button>
                          <Button
                            size="sm"
                            variant="secondary"
                            disabled={
                              !workflowRuns.some(
                                (run) => String(run.automation_id || "").trim() === automationId
                              )
                            }
                            onClick={() => {
                              const latestRun = workflowRuns.find(
                                (run) => String(run.automation_id || "").trim() === automationId
                              );
                              if (!latestRun?.run_id) return;
                              setSelectedRunId(latestRun.run_id);
                              setTab("runs");
                            }}
                          >
                            Inspect latest
                          </Button>
                          <Button
                            size="sm"
                            variant="secondary"
                            loading={busyKey === `toggle:${automationId}`}
                            onClick={() => void toggleAutomationState(automation)}
                          >
                            {String(automation.status || "").trim() === "paused"
                              ? "Resume"
                              : "Pause"}
                          </Button>
                          <Button
                            size="sm"
                            variant="danger"
                            loading={busyKey === `delete:${automationId}`}
                            onClick={() => void deleteAutomation(automationId, automation.name)}
                          >
                            Delete
                          </Button>
                        </div>
                      </div>
                    </div>
                  );
                })}
                {workflowAutomations.length === 0 ? (
                  <div className="rounded-lg border border-border bg-surface px-3 py-6 text-center text-sm text-text-muted">
                    No workflow automations found yet.
                  </div>
                ) : null}
              </div>
            </SectionCard>

            <SectionCard
              title="Legacy Compatibility"
              subtitle="Existing routine/legacy automation records remain visible during migration."
            >
              <div className="space-y-2">
                {legacyRoutines.map((routine) => (
                  <div
                    key={routine.routine_id}
                    className="rounded-lg border border-border bg-surface-elevated/40 px-3 py-2"
                  >
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div>
                        <div className="text-sm font-medium text-text">{routine.name}</div>
                        <div className="text-xs text-text-muted">
                          {routine.routine_id} | {routine.status} | {routine.entrypoint}
                        </div>
                      </div>
                      <div className="text-xs text-text-subtle">
                        {routine.output_targets.length} output target(s)
                      </div>
                    </div>
                  </div>
                ))}
                {legacyRoutines.length === 0 ? (
                  <div className="rounded-lg border border-border bg-surface px-3 py-6 text-center text-sm text-text-muted">
                    No legacy routines detected.
                  </div>
                ) : null}
              </div>
            </SectionCard>
          </>
        ) : null}

        {!loadingState && tab === "runs" ? (
          <SectionCard
            title="Live Tasks"
            subtitle="Recent workflow runs with desktop pause, resume, and cancel controls."
          >
            <div className="space-y-3">
              {workflowRuns.map((run) => (
                <div
                  key={run.run_id}
                  className="rounded-lg border border-border bg-surface-elevated/40 p-3"
                >
                  <div className="flex flex-wrap items-start justify-between gap-3">
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <div className="text-sm font-semibold text-text">
                          {runDisplayTitle(run)}
                        </div>
                        <span className="rounded border border-border bg-surface px-2 py-0.5 text-[10px] uppercase tracking-wide text-text-subtle">
                          {runStatusLabel(run)}
                        </span>
                      </div>
                      <div className="mt-1 text-xs text-text-muted">
                        Run ID: {run.run_id} | Automation: {run.automation_id}
                      </div>
                      <div className="mt-1 text-xs text-text-muted">
                        Updated:{" "}
                        {formatDateTime(
                          (run as Record<string, unknown>).updated_at_ms ||
                            (run as Record<string, unknown>).created_at_ms
                        )}
                      </div>
                      {runSummary(run) ? (
                        <div className="mt-2 rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text-muted">
                          {runSummary(run)}
                        </div>
                      ) : null}
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <Button
                        size="sm"
                        variant="secondary"
                        disabled={!canPauseRun(run)}
                        loading={busyKey === `pause:${run.run_id}`}
                        onClick={() => void handleRunAction(run.run_id, "pause")}
                      >
                        Pause
                      </Button>
                      <Button
                        size="sm"
                        variant="secondary"
                        disabled={!canResumeRun(run)}
                        loading={busyKey === `resume:${run.run_id}`}
                        onClick={() => void handleRunAction(run.run_id, "resume")}
                      >
                        Resume
                      </Button>
                      <Button
                        size="sm"
                        variant="danger"
                        disabled={!canCancelRun(run)}
                        loading={busyKey === `cancel:${run.run_id}`}
                        onClick={() => void handleRunAction(run.run_id, "cancel")}
                      >
                        Cancel
                      </Button>
                      <Button
                        size="sm"
                        variant={selectedRunId === run.run_id ? "primary" : "secondary"}
                        onClick={() => setSelectedRunId(run.run_id)}
                      >
                        Inspect
                      </Button>
                    </div>
                  </div>
                </div>
              ))}
              {workflowRuns.length === 0 ? (
                <div className="rounded-lg border border-border bg-surface px-3 py-6 text-center text-sm text-text-muted">
                  {runsLoading ? "Loading workflow runs..." : "No workflow runs found yet."}
                </div>
              ) : null}
            </div>
          </SectionCard>
        ) : null}

        {!loadingState && tab === "runs" && selectedRunDetail ? (
          <SectionCard
            title={`Run Inspector: ${runDisplayTitle(selectedRunDetail)}`}
            subtitle="Reopen completed or failed workflow runs and inspect node outputs plus linked session transcripts."
            actions={
              <div className="flex gap-2">
                <Button
                  size="sm"
                  variant="secondary"
                  loading={busyKey === `inspect:${selectedRunId}`}
                  onClick={() => void loadSelectedRunDetail(selectedRunId)}
                >
                  Refresh Run
                </Button>
                <Button size="sm" variant="secondary" onClick={() => setSelectedRunId("")}>
                  Close
                </Button>
              </div>
            }
          >
            <div className="grid gap-4 lg:grid-cols-[0.9fr_1.1fr]">
              <div className="space-y-4">
                <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                  <div className="grid gap-2 text-sm text-text-muted">
                    <div>
                      <span className="font-medium text-text">Run ID:</span>{" "}
                      {selectedRunDetail.run_id}
                    </div>
                    <div>
                      <span className="font-medium text-text">Automation:</span>{" "}
                      {selectedRunDetail.automation_id}
                    </div>
                    <div>
                      <span className="font-medium text-text">Status:</span>{" "}
                      {runStatusLabel(selectedRunDetail)}
                    </div>
                    <div>
                      <span className="font-medium text-text">Stop Reason:</span>{" "}
                      {runStopReason(selectedRunDetail) || "n/a"}
                    </div>
                    <div>
                      <span className="font-medium text-text">Pause Reason:</span>{" "}
                      {selectedRunDetail.pause_reason || "n/a"}
                    </div>
                    <div>
                      <span className="font-medium text-text">Continue / Recover:</span>{" "}
                      {selectedRunDetail.resume_reason || "n/a"}
                    </div>
                    <div>
                      <span className="font-medium text-text">Created:</span>{" "}
                      {formatDateTime((selectedRunDetail as Record<string, unknown>).created_at_ms)}
                    </div>
                    <div>
                      <span className="font-medium text-text">Updated:</span>{" "}
                      {formatDateTime((selectedRunDetail as Record<string, unknown>).updated_at_ms)}
                    </div>
                    <div>
                      <span className="font-medium text-text">Linked Sessions:</span>{" "}
                      {selectedRunSessionIds.join(", ") || "none"}
                    </div>
                  </div>
                  <div className="mt-3 flex flex-wrap gap-2">
                    <Button
                      size="sm"
                      variant="secondary"
                      disabled={!canPauseRun(selectedRunDetail)}
                      loading={busyKey === `pause:${selectedRunDetail.run_id}`}
                      onClick={() => void handleRunAction(selectedRunDetail.run_id, "pause")}
                    >
                      Pause
                    </Button>
                    <Button
                      size="sm"
                      variant="secondary"
                      disabled={!canResumeRun(selectedRunDetail)}
                      loading={busyKey === `resume:${selectedRunDetail.run_id}`}
                      onClick={() => void handleRunAction(selectedRunDetail.run_id, "resume")}
                    >
                      Resume
                    </Button>
                    <Button
                      size="sm"
                      variant="danger"
                      disabled={!canCancelRun(selectedRunDetail)}
                      loading={busyKey === `cancel:${selectedRunDetail.run_id}`}
                      onClick={() => void handleRunAction(selectedRunDetail.run_id, "cancel")}
                    >
                      Emergency Stop
                    </Button>
                    <Button
                      size="sm"
                      variant="secondary"
                      disabled={!canRecoverRun(selectedRunDetail)}
                      loading={busyKey === `recover:${selectedRunDetail.run_id}`}
                      onClick={() => void handleRunRecover(selectedRunDetail.run_id)}
                    >
                      {String(selectedRunDetail.status || "")
                        .trim()
                        .toLowerCase() === "paused"
                        ? "Recover From Pause"
                        : "Recover Run"}
                    </Button>
                  </div>
                  {runSummary(selectedRunDetail) ? (
                    <div className="mt-3 rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text-muted">
                      {runSummary(selectedRunDetail)}
                    </div>
                  ) : null}
                </div>

                <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                  <div className="text-sm font-medium text-text">Operator Diagnostics</div>
                  <div className="mt-3 grid gap-3 lg:grid-cols-3">
                    <div className="rounded-lg border border-border bg-surface px-3 py-2 text-sm">
                      <div className="text-[11px] uppercase tracking-wide text-text-subtle">
                        Tokens
                      </div>
                      <div className="mt-1 text-text">
                        {selectedRunUsage.totalTokens !== null
                          ? selectedRunUsage.totalTokens.toLocaleString()
                          : "unknown"}
                      </div>
                    </div>
                    <div className="rounded-lg border border-border bg-surface px-3 py-2 text-sm">
                      <div className="text-[11px] uppercase tracking-wide text-text-subtle">
                        Estimated Cost
                      </div>
                      <div className="mt-1 text-text">
                        {selectedRunUsage.estimatedCostUsd !== null
                          ? `$${selectedRunUsage.estimatedCostUsd.toFixed(4)}`
                          : "unknown"}
                      </div>
                    </div>
                    <div className="rounded-lg border border-border bg-surface px-3 py-2 text-sm">
                      <div className="text-[11px] uppercase tracking-wide text-text-subtle">
                        Tool Calls
                      </div>
                      <div className="mt-1 text-text">
                        {selectedRunUsage.totalToolCalls !== null
                          ? selectedRunUsage.totalToolCalls.toLocaleString()
                          : "unknown"}
                      </div>
                    </div>
                  </div>
                  {selectedBlockedNodes.length ? (
                    <div className="mt-3 rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text-muted">
                      <span className="font-medium text-text">Blocked Nodes:</span>{" "}
                      {selectedBlockedNodes.join(", ")}
                    </div>
                  ) : null}
                  {selectedBlockedNodeDiagnostics.length ? (
                    <div className="mt-3 rounded-lg border border-border bg-surface px-3 py-2">
                      <div className="text-sm font-medium text-text">Blocked Node Reasons</div>
                      <div className="mt-2 space-y-2">
                        {selectedBlockedNodeDiagnostics.map((entry) => (
                          <div key={entry.nodeId} className="text-sm text-text-muted">
                            <span className="font-medium text-text">{entry.nodeId}</span>
                            {" · "}
                            {entry.reason}
                          </div>
                        ))}
                      </div>
                    </div>
                  ) : null}
                  {selectedLastFailure ? (
                    <div className="mt-3 rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2">
                      <div className="text-sm font-medium text-red-100">
                        Last Failure: {String(selectedLastFailure.node_id || "unknown node")}
                      </div>
                      <div className="mt-1 text-sm text-red-200">
                        {String(selectedLastFailure.reason || "No failure reason available.")}
                      </div>
                      {selectedLastFailure.recorded_at_ms ? (
                        <div className="mt-1 text-xs text-red-200/80">
                          Recorded: {formatDateTime(selectedLastFailure.recorded_at_ms)}
                        </div>
                      ) : null}
                    </div>
                  ) : null}
                  {selectedFailureChain.length ? (
                    <div className="mt-3 rounded-lg border border-border bg-surface px-3 py-2">
                      <div className="text-sm font-medium text-text">Failure Chain</div>
                      <div className="mt-2 space-y-2">
                        {selectedFailureChain.map((entry, index) => (
                          <div
                            key={`${entry.event || "chain"}-${index}`}
                            className="text-sm text-text-muted"
                          >
                            <span className="font-medium text-text">
                              {String(entry.event || "event").replace(/_/g, " ")}
                            </span>
                            {" · "}
                            {formatDateTime(entry.recorded_at_ms)}
                            {entry.reason ? ` · ${String(entry.reason)}` : ""}
                          </div>
                        ))}
                      </div>
                    </div>
                  ) : null}
                  {selectedLifecycleHistory.length ? (
                    <div className="mt-3 rounded-lg border border-border bg-surface px-3 py-2">
                      <div className="text-sm font-medium text-text">Lifecycle History</div>
                      <div className="mt-2 space-y-2">
                        {selectedLifecycleHistory.slice(0, 6).map((entry, index) => (
                          <div
                            key={`${entry.event || "event"}-${index}`}
                            className="text-sm text-text-muted"
                          >
                            <span className="font-medium text-text">
                              {String(entry.stop_kind || entry.event || "event").replace(/_/g, " ")}
                            </span>
                            {" · "}
                            {formatDateTime(entry.recorded_at_ms)}
                            {entry.reason ? ` · ${String(entry.reason)}` : ""}
                          </div>
                        ))}
                      </div>
                    </div>
                  ) : null}
                  {selectedRecoveryHistory.length ? (
                    <div className="mt-3 rounded-lg border border-border bg-surface px-3 py-2">
                      <div className="text-sm font-medium text-text">Recovery History</div>
                      <div className="mt-2 space-y-2">
                        {selectedRecoveryHistory.map((entry, index) => (
                          <div
                            key={`${entry.event || "recovery"}-${index}`}
                            className="text-sm text-text-muted"
                          >
                            <span className="font-medium text-text">
                              {String(entry.event || "recovery").replace(/_/g, " ")}
                            </span>
                            {" · "}
                            {formatDateTime(entry.recorded_at_ms)}
                            {entry.reason ? ` · ${String(entry.reason)}` : ""}
                          </div>
                        ))}
                      </div>
                    </div>
                  ) : null}
                  {selectedLastFailure ||
                  selectedAwaitingGate ||
                  String(selectedRunDetail.status || "")
                    .trim()
                    .toLowerCase() === "paused" ? (
                    <div className="mt-3 rounded-lg border border-border bg-surface px-3 py-3">
                      <div className="text-sm font-medium text-text">Step Repair</div>
                      <div className="mt-1 text-xs text-text-muted">
                        Patch one step, reset only its affected subtree, and requeue the run.
                      </div>
                      <div className="mt-3 grid gap-3 lg:grid-cols-2">
                        <label className="block text-sm font-medium text-text">
                          Step
                          <select
                            value={repairDraft.nodeId}
                            onChange={(event) =>
                              setRepairDraft((current) => ({
                                ...current,
                                nodeId: event.target.value,
                              }))
                            }
                            className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                          >
                            <option value="">Select step</option>
                            {selectedStepStatusRows.map((step) => (
                              <option key={step.nodeId} value={step.nodeId}>
                                {step.nodeId} ({step.status})
                              </option>
                            ))}
                          </select>
                        </label>
                        <Input
                          label="Template Override"
                          value={repairDraft.templateId}
                          onChange={(event) =>
                            setRepairDraft((current) => ({
                              ...current,
                              templateId: event.target.value,
                            }))
                          }
                        />
                      </div>
                      <div className="mt-3 grid gap-3 lg:grid-cols-2">
                        <label className="block text-sm font-medium text-text">
                          Model Provider
                          <select
                            value={repairDraft.modelProvider}
                            onChange={(event) =>
                              setRepairDraft((current) => ({
                                ...current,
                                modelProvider: event.target.value,
                                modelId: providerModelMap.get(event.target.value)?.[0] || "",
                              }))
                            }
                            className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                          >
                            <option value="">Keep current</option>
                            {providers.map((provider) => (
                              <option key={provider.id} value={provider.id}>
                                {provider.id}
                              </option>
                            ))}
                          </select>
                        </label>
                        <label className="block text-sm font-medium text-text">
                          Model
                          <select
                            value={repairDraft.modelId}
                            onChange={(event) =>
                              setRepairDraft((current) => ({
                                ...current,
                                modelId: event.target.value,
                              }))
                            }
                            className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                          >
                            <option value="">Keep current</option>
                            {(providerModelMap.get(repairDraft.modelProvider) || []).map(
                              (modelId) => (
                                <option key={modelId} value={modelId}>
                                  {modelId}
                                </option>
                              )
                            )}
                          </select>
                        </label>
                      </div>
                      <label className="mt-3 block text-sm font-medium text-text">
                        Prompt Patch
                        <textarea
                          value={repairDraft.prompt}
                          onChange={(event) =>
                            setRepairDraft((current) => ({
                              ...current,
                              prompt: event.target.value,
                            }))
                          }
                          className="mt-2 min-h-[120px] w-full rounded-lg border border-border bg-surface px-3 py-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                        />
                      </label>
                      <Input
                        label="Repair Reason"
                        value={repairDraft.reason}
                        onChange={(event) =>
                          setRepairDraft((current) => ({ ...current, reason: event.target.value }))
                        }
                      />
                      <div className="mt-3 flex flex-wrap gap-2">
                        <Button
                          size="sm"
                          variant="primary"
                          loading={busyKey === `repair:${selectedRunDetail.run_id}`}
                          onClick={() => void handleRunRepair(selectedRunDetail.run_id)}
                        >
                          Repair Step And Rerun Subtree
                        </Button>
                      </div>
                    </div>
                  ) : null}
                  {selectedRepairHistory.length ? (
                    <div className="mt-3 rounded-lg border border-border bg-surface px-3 py-3">
                      <div className="text-sm font-medium text-text">Repair History</div>
                      <div className="mt-3 space-y-3">
                        {selectedRepairHistory.map((entry, index) => {
                          const metadata = ((entry.metadata as
                            | Record<string, unknown>
                            | undefined) || {}) as Record<string, unknown>;
                          const previousPrompt = String(metadata.previous_prompt || "").trim();
                          const newPrompt = String(metadata.new_prompt || "").trim();
                          const previousTemplateId = String(
                            metadata.previous_template_id || ""
                          ).trim();
                          const newTemplateId = String(metadata.new_template_id || "").trim();
                          const promptChanged = Boolean(metadata.prompt_updated);
                          const templateChanged = Boolean(metadata.template_updated);
                          const modelChanged = Boolean(metadata.model_policy_updated);
                          return (
                            <div
                              key={`${metadata.node_id || "repair"}-${index}`}
                              className="rounded-lg border border-border bg-surface-elevated/40 px-3 py-3"
                            >
                              <div className="flex flex-wrap items-center justify-between gap-2">
                                <div className="text-sm font-medium text-text">
                                  {String(metadata.node_id || "step")}
                                </div>
                                <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                                  repaired
                                </div>
                              </div>
                              <div className="mt-1 text-xs text-text-muted">
                                {formatDateTime(entry.recorded_at_ms)}
                                {entry.reason ? ` | ${String(entry.reason)}` : ""}
                              </div>
                              <div className="mt-2 text-xs text-text-muted">
                                Changes:
                                {promptChanged ? " prompt" : ""}
                                {templateChanged ? " template" : ""}
                                {modelChanged ? " model" : ""}
                              </div>
                              {templateChanged ? (
                                <div className="mt-1 text-xs text-text-muted">
                                  Template: {previousTemplateId || "none"} →{" "}
                                  {newTemplateId || "none"}
                                </div>
                              ) : null}
                              {promptChanged ? (
                                <div className="mt-2 grid gap-2 lg:grid-cols-2">
                                  <div className="rounded border border-border bg-surface px-2 py-2">
                                    <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                                      Previous Prompt
                                    </div>
                                    <pre className="mt-2 overflow-x-auto whitespace-pre-wrap text-xs text-text-muted">
                                      {previousPrompt || "(empty)"}
                                    </pre>
                                  </div>
                                  <div className="rounded border border-border bg-surface px-2 py-2">
                                    <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                                      New Prompt
                                    </div>
                                    <pre className="mt-2 overflow-x-auto whitespace-pre-wrap text-xs text-text-muted">
                                      {newPrompt || "(empty)"}
                                    </pre>
                                  </div>
                                </div>
                              ) : null}
                            </div>
                          );
                        })}
                      </div>
                    </div>
                  ) : null}
                  {selectedGateHistory.length ? (
                    <div className="mt-3 rounded-lg border border-border bg-surface px-3 py-2">
                      <div className="text-sm font-medium text-text">Gate History</div>
                      <div className="mt-2 space-y-2">
                        {selectedGateHistory.slice(0, 6).map((entry, index) => (
                          <div
                            key={`${entry.node_id || "gate"}-${index}`}
                            className="text-sm text-text-muted"
                          >
                            <span className="font-medium text-text">
                              {String(entry.node_id || "gate")}
                            </span>
                            {" · "}
                            {String(entry.decision || "decision")}
                            {" · "}
                            {formatDateTime(entry.decided_at_ms)}
                            {entry.reason ? ` · ${String(entry.reason)}` : ""}
                          </div>
                        ))}
                      </div>
                    </div>
                  ) : null}
                </div>

                {selectedAwaitingGate ? (
                  <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3">
                    <div className="text-sm font-medium text-amber-100">
                      Awaiting Approval:{" "}
                      {String(selectedAwaitingGate.title || selectedAwaitingGate.node_id || "Gate")}
                    </div>
                    {selectedAwaitingGate.instructions ? (
                      <div className="mt-2 text-sm text-amber-200">
                        {String(selectedAwaitingGate.instructions)}
                      </div>
                    ) : null}
                    <div className="mt-3 flex flex-wrap gap-2">
                      <Button
                        size="sm"
                        variant="secondary"
                        loading={busyKey === `gate:approve:${selectedRunDetail.run_id}`}
                        onClick={() => void handleGateDecision(selectedRunDetail.run_id, "approve")}
                      >
                        Approve
                      </Button>
                      <Button
                        size="sm"
                        variant="secondary"
                        loading={busyKey === `gate:rework:${selectedRunDetail.run_id}`}
                        onClick={() => void handleGateDecision(selectedRunDetail.run_id, "rework")}
                      >
                        Rework
                      </Button>
                      <Button
                        size="sm"
                        variant="danger"
                        loading={busyKey === `gate:cancel:${selectedRunDetail.run_id}`}
                        onClick={() => void handleGateDecision(selectedRunDetail.run_id, "cancel")}
                      >
                        Emergency Stop
                      </Button>
                    </div>
                  </div>
                ) : null}

                <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                  <div className="text-sm font-medium text-text">Blockers / Errors</div>
                  <div className="mt-3 space-y-2">
                    {selectedRunBlockers.map((blocker) => (
                      <div
                        key={blocker.key}
                        className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2"
                      >
                        <div className="text-sm font-medium text-red-100">{blocker.title}</div>
                        <div className="mt-1 text-sm text-red-200">{blocker.reason}</div>
                      </div>
                    ))}
                    {selectedRunBlockers.length === 0 ? (
                      <div className="rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text-muted">
                        No explicit blockers extracted from the current run payload.
                      </div>
                    ) : null}
                  </div>
                </div>

                <div className="grid gap-3 lg:grid-cols-2">
                  <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                    <div className="text-sm font-medium text-text">Phase Progress</div>
                    <div className="mt-3 space-y-2">
                      {selectedPhaseDiagnostics.map((phase) => (
                        <div
                          key={phase.phaseId}
                          className="rounded-lg border border-border bg-surface px-3 py-2"
                        >
                          <div className="flex flex-wrap items-center justify-between gap-2">
                            <div className="text-sm font-medium text-text">{phase.title}</div>
                            <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                              {phase.status}
                            </div>
                          </div>
                          <div className="mt-1 text-xs text-text-subtle">
                            {phase.phaseId} | {phase.executionMode}
                          </div>
                          <div className="mt-1 text-xs text-text-muted">
                            Completed: {phase.completedCount}/{phase.totalCount} | Runnable:{" "}
                            {phase.runnableCount} | Blocked: {phase.blockedCount}
                          </div>
                        </div>
                      ))}
                      {selectedPhaseDiagnostics.length === 0 ? (
                        <div className="rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text-muted">
                          No mission phases were found on this run.
                        </div>
                      ) : null}
                    </div>
                  </div>

                  <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                    <div className="text-sm font-medium text-text">Milestone Progress</div>
                    <div className="mt-3 space-y-2">
                      {selectedMilestoneDiagnostics.map((milestone) => (
                        <div
                          key={milestone.milestoneId}
                          className="rounded-lg border border-border bg-surface px-3 py-2"
                        >
                          <div className="flex flex-wrap items-center justify-between gap-2">
                            <div className="text-sm font-medium text-text">{milestone.title}</div>
                            <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                              {milestone.status}
                            </div>
                          </div>
                          <div className="mt-1 text-xs text-text-subtle">
                            {milestone.milestoneId}
                            {milestone.phaseId ? ` | phase ${milestone.phaseId}` : ""}
                          </div>
                          <div className="mt-1 text-xs text-text-muted">
                            Completed: {milestone.completedCount}/
                            {milestone.requiredStageIds.length} | Blocked: {milestone.blockedCount}
                          </div>
                          {milestone.requiredStageIds.length ? (
                            <div className="mt-1 text-xs text-text-muted">
                              Required stages: {milestone.requiredStageIds.join(", ")}
                            </div>
                          ) : null}
                        </div>
                      ))}
                      {selectedMilestoneDiagnostics.length === 0 ? (
                        <div className="rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text-muted">
                          No mission milestones were found on this run.
                        </div>
                      ) : null}
                    </div>
                  </div>
                </div>

                <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                  <div className="text-sm font-medium text-text">
                    Milestone Promotion Diagnostics
                  </div>
                  <div className="mt-3 space-y-3">
                    {selectedMilestonePromotionDiagnostics.map((milestone) => (
                      <div
                        key={milestone.milestoneId}
                        className="rounded-lg border border-border bg-surface px-3 py-3"
                      >
                        <div className="flex flex-wrap items-center justify-between gap-2">
                          <div className="text-sm font-medium text-text">{milestone.title}</div>
                          <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                            {milestone.status}
                          </div>
                        </div>
                        <div className="mt-1 text-xs text-text-subtle">
                          {milestone.milestoneId}
                          {milestone.phaseId ? ` | phase ${milestone.phaseId}` : ""}
                        </div>
                        {milestone.unmetStages.length ? (
                          <div className="mt-2 text-xs text-text-muted">
                            Unmet stages:{" "}
                            {milestone.unmetStages.map((row) => row.stageId).join(", ")}
                          </div>
                        ) : (
                          <div className="mt-2 text-xs text-emerald-300">
                            All required stages are complete. This milestone is promotable.
                          </div>
                        )}
                        {milestone.blockedStages.length ? (
                          <div className="mt-2 space-y-1">
                            {milestone.blockedStages.map((stage) => (
                              <div
                                key={`${milestone.milestoneId}-${stage.stageId}`}
                                className="text-xs text-red-200"
                              >
                                Blocked: {stage.stageId} ({stage.reason})
                              </div>
                            ))}
                          </div>
                        ) : null}
                        {milestone.waitingStages.length ? (
                          <div className="mt-2 space-y-1">
                            {milestone.waitingStages.map((stage) => (
                              <div
                                key={`${milestone.milestoneId}-waiting-${stage.stageId}`}
                                className="text-xs text-text-muted"
                              >
                                Waiting: {stage.stageId} ({stage.reason})
                              </div>
                            ))}
                          </div>
                        ) : null}
                        {milestone.latestGateDecision ? (
                          <div className="mt-2 text-xs text-text-muted">
                            Latest gate: {milestone.latestGateDecision.nodeId} →{" "}
                            {milestone.latestGateDecision.decision} at{" "}
                            {formatDateTime(milestone.latestGateDecision.decidedAtMs)}
                          </div>
                        ) : null}
                        {milestone.latestRecovery ? (
                          <div className="mt-1 text-xs text-text-muted">
                            Latest recovery: {milestone.latestRecovery.event} at{" "}
                            {formatDateTime(milestone.latestRecovery.recordedAtMs)}
                            {milestone.latestRecovery.reason
                              ? ` | ${shortText(milestone.latestRecovery.reason, 140)}`
                              : ""}
                          </div>
                        ) : null}
                      </div>
                    ))}
                    {selectedMilestonePromotionDiagnostics.length === 0 ? (
                      <div className="rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text-muted">
                        No milestone promotion diagnostics are available for this run.
                      </div>
                    ) : null}
                  </div>
                </div>

                <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                  <div className="text-sm font-medium text-text">Promotion History</div>
                  <div className="mt-3 space-y-2">
                    {selectedPromotionHistory.map((entry, index) => {
                      const metadata =
                        (entry.metadata as Record<string, unknown> | undefined) || {};
                      const milestoneId = String(metadata.milestone_id || "").trim();
                      const title = String(metadata.title || milestoneId || "milestone").trim();
                      const phaseId = String(metadata.phase_id || "").trim();
                      const promotedByNodeId = String(metadata.promoted_by_node_id || "").trim();
                      const requiredStageIds = Array.isArray(metadata.required_stage_ids)
                        ? metadata.required_stage_ids
                            .map((value) => String(value || "").trim())
                            .filter(Boolean)
                        : [];
                      return (
                        <div
                          key={`${milestoneId || title}-${index}`}
                          className="rounded-lg border border-border bg-surface px-3 py-2"
                        >
                          <div className="flex flex-wrap items-center justify-between gap-2">
                            <div className="text-sm font-medium text-text">{title}</div>
                            <div className="text-[10px] uppercase tracking-wide text-emerald-300">
                              promoted
                            </div>
                          </div>
                          <div className="mt-1 text-xs text-text-subtle">
                            {milestoneId}
                            {phaseId ? ` | phase ${phaseId}` : ""}
                            {promotedByNodeId ? ` | by ${promotedByNodeId}` : ""}
                          </div>
                          <div className="mt-1 text-xs text-text-muted">
                            {formatDateTime(entry.recorded_at_ms)}
                          </div>
                          {requiredStageIds.length ? (
                            <div className="mt-1 text-xs text-text-muted">
                              Required stages: {requiredStageIds.join(", ")}
                            </div>
                          ) : null}
                        </div>
                      );
                    })}
                    {selectedPromotionHistory.length === 0 ? (
                      <div className="rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text-muted">
                        No milestone promotions have been recorded for this run yet.
                      </div>
                    ) : null}
                  </div>
                </div>

                <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                  <div className="text-sm font-medium text-text">Node Outputs</div>
                  <div className="mt-3 space-y-2">
                    {selectedRunNodeOutputs.map((output) => (
                      <div
                        key={output.nodeId}
                        className="rounded-lg border border-border bg-surface px-3 py-2"
                      >
                        <div className="text-xs uppercase tracking-wide text-text-subtle">
                          {output.nodeId}
                        </div>
                        <pre className="mt-2 overflow-x-auto whitespace-pre-wrap text-xs text-text-muted">
                          {nodeOutputText(output.value) || JSON.stringify(output.value, null, 2)}
                        </pre>
                      </div>
                    ))}
                    {selectedRunNodeOutputs.length === 0 ? (
                      <div className="rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text-muted">
                        No node outputs were found on this run checkpoint.
                      </div>
                    ) : null}
                  </div>
                </div>

                <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                  <div className="text-sm font-medium text-text">Per-Step Activity</div>
                  <div className="mt-3 space-y-2">
                    {selectedStepDiagnostics.map((step) => (
                      <div
                        key={step.nodeId}
                        className="rounded-lg border border-border bg-surface px-3 py-2"
                      >
                        <div className="flex flex-wrap items-center justify-between gap-2">
                          <div className="text-sm font-medium text-text">{step.nodeId}</div>
                          <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                            {step.contractKind || "node"}
                          </div>
                        </div>
                        {step.objective ? (
                          <div className="mt-1 text-sm text-text-muted">
                            {shortText(step.objective, 200)}
                          </div>
                        ) : null}
                        {step.summary ? (
                          <div className="mt-1 text-sm text-text-muted">
                            {shortText(step.summary, 240)}
                          </div>
                        ) : null}
                        <div className="mt-2 text-xs text-text-subtle">
                          Session: {step.sessionId || "none"} | Messages: {step.messageCount}
                        </div>
                      </div>
                    ))}
                    {selectedStepDiagnostics.length === 0 ? (
                      <div className="rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text-muted">
                        No per-step activity has been captured for this run yet.
                      </div>
                    ) : null}
                  </div>
                </div>

                <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                  <div className="text-sm font-medium text-text">Per-Step Status</div>
                  <div className="mt-3 space-y-2">
                    {selectedStepStatusRows.map((step) => (
                      <div
                        key={step.nodeId}
                        className="rounded-lg border border-border bg-surface px-3 py-2"
                      >
                        <div className="flex flex-wrap items-center justify-between gap-2">
                          <div className="text-sm font-medium text-text">{step.nodeId}</div>
                          <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                            {step.status}
                          </div>
                        </div>
                        <div className="mt-1 text-xs text-text-subtle">
                          Agent: {step.agentId || "n/a"} | Attempts: {step.attempts} | Session:{" "}
                          {step.sessionId || "none"} | Messages: {step.messageCount}
                        </div>
                        {step.objective ? (
                          <div className="mt-1 text-sm text-text-muted">
                            {shortText(step.objective, 220)}
                          </div>
                        ) : null}
                        {step.missingDeps.length ? (
                          <div className="mt-1 text-xs text-text-muted">
                            Waiting on: {step.missingDeps.join(", ")}
                          </div>
                        ) : null}
                        {step.summary ? (
                          <div className="mt-1 text-xs text-text-muted">
                            Output: {shortText(step.summary, 220)}
                          </div>
                        ) : null}
                      </div>
                    ))}
                    {selectedStepStatusRows.length === 0 ? (
                      <div className="rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text-muted">
                        No step status data is available for this run.
                      </div>
                    ) : null}
                  </div>
                </div>

                <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                  <div className="flex items-center justify-between gap-2">
                    <div className="text-sm font-medium text-text">Per-Step Logs</div>
                    <div className="text-xs text-text-muted">
                      {selectedStepLogs.length} step session(s)
                    </div>
                  </div>
                  <div className="mt-3 space-y-3">
                    {selectedStepLogs.map((step) => (
                      <div
                        key={step.nodeId}
                        className="rounded-lg border border-border bg-surface px-3 py-3"
                      >
                        <div className="flex flex-wrap items-center justify-between gap-2">
                          <div>
                            <div className="text-sm font-medium text-text">{step.nodeId}</div>
                            {step.objective ? (
                              <div className="mt-1 text-sm text-text-muted">
                                {shortText(step.objective, 220)}
                              </div>
                            ) : null}
                          </div>
                          <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                            {step.status}
                          </div>
                        </div>
                        <div className="mt-2 text-xs text-text-subtle">
                          Session: {step.sessionId} | Messages: {step.messageCount}
                        </div>
                        {step.events.length ? (
                          <div className="mt-3 space-y-2 rounded-lg border border-border bg-surface-elevated/30 p-2">
                            {step.events.map((event) => (
                              <div
                                key={event.id}
                                className="rounded-lg border border-border bg-surface px-3 py-2"
                              >
                                <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                                  {event.event.replace(/_/g, " ")} |{" "}
                                  {formatDateTime(event.createdAt)}
                                </div>
                                <div className="mt-1 text-xs text-text-muted">
                                  Attempt: {event.attempt || 0}
                                  {event.sessionId ? ` | Session: ${event.sessionId}` : ""}
                                  {event.terminal ? " | terminal" : ""}
                                </div>
                                {event.reason ? (
                                  <div className="mt-1 text-xs text-text">
                                    {shortText(event.reason, 240)}
                                  </div>
                                ) : null}
                                {event.summary ? (
                                  <div className="mt-1 text-xs text-text-muted">
                                    Summary: {shortText(event.summary, 220)}
                                  </div>
                                ) : null}
                              </div>
                            ))}
                          </div>
                        ) : null}
                        <div className="mt-3 max-h-72 space-y-2 overflow-y-auto rounded-lg border border-border bg-surface-elevated/40 p-2">
                          {step.messages.map((message, index) => (
                            <div
                              key={`${step.nodeId}-${message.id || index}-${index}`}
                              className={`rounded-lg border px-3 py-2 ${
                                message.variant === "user"
                                  ? "border-primary/30 bg-primary/10"
                                  : message.variant === "assistant"
                                    ? "border-border bg-surface-elevated/60"
                                    : message.variant === "error"
                                      ? "border-red-500/30 bg-red-500/10"
                                      : "border-border bg-surface"
                              }`}
                            >
                              <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                                {message.role || "session"} | {formatDateTime(message.createdAt)}
                              </div>
                              <pre className="mt-2 overflow-x-auto whitespace-pre-wrap text-xs text-text">
                                {message.text || "(empty message)"}
                              </pre>
                            </div>
                          ))}
                          {step.messages.length === 0 ? (
                            <div className="rounded-lg border border-border bg-surface px-3 py-3 text-sm text-text-muted">
                              No transcript messages were returned for this step session.
                            </div>
                          ) : null}
                        </div>
                      </div>
                    ))}
                    {selectedStepLogs.length === 0 ? (
                      <div className="rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text-muted">
                        No step-linked session logs are available for this run yet.
                      </div>
                    ) : null}
                  </div>
                </div>
              </div>

              <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                <div className="flex items-center justify-between gap-2">
                  <div className="text-sm font-medium text-text">Linked Session Transcripts</div>
                  <div className="text-xs text-text-muted">
                    {selectedTranscriptSessions.length} session(s)
                  </div>
                </div>
                <div className="mt-3 max-h-[720px] space-y-3 overflow-y-auto rounded-lg border border-border bg-surface px-3 py-3">
                  {selectedTranscriptSessions.map(({ sessionId, messages }) => (
                    <div
                      key={sessionId}
                      className="rounded-lg border border-border bg-surface-elevated/40 p-3"
                    >
                      <div className="text-xs uppercase tracking-wide text-text-subtle">
                        Session {sessionId} · {messages.length} message(s)
                      </div>
                      <div className="mt-3 space-y-3">
                        {messages.map((message, index) => {
                          const variant = sessionMessageVariant(message);
                          return (
                            <div
                              key={`${message.info?.id || index}-${index}`}
                              className={`rounded-lg border px-3 py-2 ${
                                variant === "user"
                                  ? "border-primary/30 bg-primary/10"
                                  : variant === "assistant"
                                    ? "border-border bg-surface-elevated/60"
                                    : variant === "error"
                                      ? "border-red-500/30 bg-red-500/10"
                                      : "border-border bg-surface"
                              }`}
                            >
                              <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                                {message.info?.role || "session"} |{" "}
                                {formatDateTime(message.info?.time?.created)}
                              </div>
                              <pre className="mt-2 overflow-x-auto whitespace-pre-wrap text-xs text-text">
                                {sessionMessageText(message) || "(empty message)"}
                              </pre>
                            </div>
                          );
                        })}
                        {messages.length === 0 ? (
                          <div className="rounded-lg border border-border bg-surface px-3 py-4 text-sm text-text-muted">
                            No messages were returned for this session.
                          </div>
                        ) : null}
                      </div>
                    </div>
                  ))}
                  {selectedTranscriptSessions.length === 0 ? (
                    <div className="rounded-lg border border-border bg-surface px-3 py-4 text-sm text-text-muted">
                      No linked session messages were found for this run.
                    </div>
                  ) : null}
                </div>
              </div>
            </div>
          </SectionCard>
        ) : null}

        {editDraft ? (
          <SectionCard
            title={`Edit ${editDraft.name || editDraft.automationId}`}
            subtitle="Workflow edit parity for name, schedule, workspace, execution, models, and MCP."
            actions={
              <Button size="sm" variant="secondary" onClick={() => setEditDraft(null)}>
                Close
              </Button>
            }
          >
            <div className="grid gap-4 lg:grid-cols-2">
              <div className="space-y-3">
                <Input
                  label="Name"
                  value={editDraft.name}
                  onChange={(event) =>
                    setEditDraft((current) =>
                      current ? { ...current, name: event.target.value } : current
                    )
                  }
                />
                <label className="block text-sm font-medium text-text">
                  Description
                  <textarea
                    value={editDraft.description}
                    onChange={(event) =>
                      setEditDraft((current) =>
                        current ? { ...current, description: event.target.value } : current
                      )
                    }
                    className="mt-2 min-h-[120px] w-full rounded-lg border border-border bg-surface px-3 py-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                  />
                </label>
                <Input
                  label="Workspace Root"
                  value={editDraft.workspaceRoot}
                  onChange={(event) =>
                    setEditDraft((current) =>
                      current ? { ...current, workspaceRoot: event.target.value } : current
                    )
                  }
                />
                <ScheduleBuilder
                  value={{
                    scheduleKind: editDraft.scheduleKind,
                    intervalSeconds: editDraft.intervalSeconds,
                    cronExpression: editDraft.cronExpression,
                  }}
                  onChange={updateEditDraftSchedule}
                />
                <TimezoneField
                  value={editDraft.timezone}
                  onChange={(value) =>
                    setEditDraft((current) => (current ? { ...current, timezone: value } : current))
                  }
                  error={editDraftTimezoneError || undefined}
                  hint="Use the timezone that matches the original schedule."
                />
              </div>
              <div className="space-y-3">
                <div className="grid gap-3 sm:grid-cols-2">
                  <label className="block text-sm font-medium text-text">
                    Execution Mode
                    <select
                      value={editDraft.executionMode}
                      onChange={(event) =>
                        setEditDraft((current) =>
                          current
                            ? {
                                ...current,
                                executionMode: event.target.value === "swarm" ? "swarm" : "team",
                              }
                            : current
                        )
                      }
                      className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                    >
                      <option value="team">Team</option>
                      <option value="swarm">Swarm</option>
                    </select>
                  </label>
                  <Input
                    label="Max Parallel Agents"
                    type="number"
                    min={1}
                    max={16}
                    value={editDraft.maxParallelAgents}
                    onChange={(event) =>
                      setEditDraft((current) =>
                        current ? { ...current, maxParallelAgents: event.target.value } : current
                      )
                    }
                  />
                </div>
                <div className="grid gap-3 sm:grid-cols-2">
                  <label className="block text-sm font-medium text-text">
                    Workflow Provider
                    <select
                      value={editDraft.modelProvider}
                      onChange={(event) =>
                        setEditDraft((current) =>
                          current
                            ? {
                                ...current,
                                modelProvider: event.target.value,
                                modelId: providerModelMap.get(event.target.value)?.[0] ?? "",
                              }
                            : current
                        )
                      }
                      className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                    >
                      <option value="">Engine default</option>
                      {providers.map((provider) => (
                        <option key={provider.id} value={provider.id}>
                          {provider.id}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="block text-sm font-medium text-text">
                    Workflow Model
                    <select
                      value={editDraft.modelId}
                      onChange={(event) =>
                        setEditDraft((current) =>
                          current ? { ...current, modelId: event.target.value } : current
                        )
                      }
                      className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                    >
                      <option value="">Engine default</option>
                      {(providerModelMap.get(editDraft.modelProvider) ?? []).map((modelId) => (
                        <option key={modelId} value={modelId}>
                          {modelId}
                        </option>
                      ))}
                    </select>
                  </label>
                </div>
                <div className="grid gap-3 sm:grid-cols-2">
                  <label className="block text-sm font-medium text-text">
                    Planner Provider
                    <select
                      value={editDraft.plannerModelProvider}
                      onChange={(event) =>
                        setEditDraft((current) =>
                          current
                            ? {
                                ...current,
                                plannerModelProvider: event.target.value,
                                plannerModelId: providerModelMap.get(event.target.value)?.[0] ?? "",
                              }
                            : current
                        )
                      }
                      className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                    >
                      <option value="">Fallback to workflow model</option>
                      {providers.map((provider) => (
                        <option key={provider.id} value={provider.id}>
                          {provider.id}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="block text-sm font-medium text-text">
                    Planner Model
                    <select
                      value={editDraft.plannerModelId}
                      onChange={(event) =>
                        setEditDraft((current) =>
                          current ? { ...current, plannerModelId: event.target.value } : current
                        )
                      }
                      className="mt-2 h-10 w-full rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
                    >
                      <option value="">Fallback to workflow model</option>
                      {(providerModelMap.get(editDraft.plannerModelProvider) ?? []).map(
                        (modelId) => (
                          <option key={modelId} value={modelId}>
                            {modelId}
                          </option>
                        )
                      )}
                    </select>
                  </label>
                </div>
                <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                  <div className="text-sm font-medium text-text">Allowed MCP Servers</div>
                  <div className="mt-3 space-y-2">
                    {mcpServers.map((server) => {
                      const checked = editDraft.selectedMcpServers.includes(server.name);
                      return (
                        <label
                          key={server.name}
                          className="flex items-start gap-2 text-sm text-text"
                        >
                          <input
                            type="checkbox"
                            checked={checked}
                            onChange={() =>
                              setEditDraft((current) =>
                                current
                                  ? {
                                      ...current,
                                      selectedMcpServers: checked
                                        ? current.selectedMcpServers.filter(
                                            (row) => row !== server.name
                                          )
                                        : [...current.selectedMcpServers, server.name],
                                    }
                                  : current
                              )
                            }
                          />
                          <span>
                            <span className="block font-medium">{server.name}</span>
                            <span className="block text-xs text-text-muted">
                              {server.connected ? "connected" : "disconnected"} |{" "}
                              {server.enabled ? "enabled" : "disabled"}
                            </span>
                          </span>
                        </label>
                      );
                    })}
                  </div>
                </div>
              </div>
            </div>
            <div className="mt-4 flex justify-end gap-2">
              <Button size="sm" variant="secondary" onClick={() => setEditDraft(null)}>
                Cancel
              </Button>
              <Button
                size="sm"
                variant="primary"
                loading={busyKey === `save:${editDraft.automationId}`}
                onClick={() => void saveEditDraft()}
              >
                Save Changes
              </Button>
            </div>
          </SectionCard>
        ) : null}
      </div>
    </div>
  );
}
