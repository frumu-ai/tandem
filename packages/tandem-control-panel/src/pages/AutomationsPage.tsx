import { AutomationsPageTabs } from "../features/automations/AutomationsPageTabs";
import {
  buildRunBlockers,
  collectPathStrings,
  compactIdentifier,
  deriveRunDebugHints,
  detectWorkflowActiveTaskId,
  detectWorkflowActiveTaskIds,
  explainRunFailure,
  formatRunDateTime,
  formatTimestampLabel,
  isActiveRunStatus,
  normalizeTimestamp,
  runDisplayTitle,
  runObjectiveText,
  runTimeLabel,
  sessionLabel,
  sessionMessageCreatedAt,
  sessionMessageId,
  sessionMessageParts,
  sessionMessageText,
  sessionMessageVariant,
  shortText,
  timestampOrNull,
  uniqueStrings,
  workflowDescendantTaskIds,
  workflowQueueReason,
  workflowStatusDisplay,
  workflowStatusSubtleDetail,
} from "../features/automations/AutomationsRunHelpers";
import { MyAutomationsContainer } from "../features/automations/MyAutomationsContainer";
import { useAutomationsPageState } from "../features/automations/useAutomationsPageState";
import {
  AUTOMATION_WIZARD_CONFIG,
  CreateWizard as CreateWizardExternal,
} from "../features/automations/create/CreateWizard";
import { AutomationComposerPanel } from "../features/automations/create/AutomationComposerPanel";
import { describeScheduleValue } from "../features/automations/scheduleBuilder";
import { AdvancedMissionBuilderPanel } from "./AdvancedMissionBuilderPanel";
import { PageCard, formatJson } from "./ui";
import type { AppPageProps } from "./pageTypes";
import { splitMcpAllowedTools } from "../features/mcp/mcpTools";

// ─── Types ─────────────────────────────────────────────────────────────────

type ExecutionMode = "single" | "team" | "swarm";
type WorkflowToolAccessMode = "all" | "custom";
type WorkflowTaskToolAccessMode = "inherit" | "custom";
type WorkflowApprovalOverride = "default" | "auto" | "skip";

interface McpServerOption {
  name: string;
  connected: boolean;
  enabled: boolean;
  toolCache: string[];
  allowedTools: string[] | null;
}

interface WorkflowEditDraft {
  automationId: string;
  name: string;
  description: string;
  recoveryPrompt: string;
  sourceAutomation: any | null;
  scheduleKind: "manual" | "cron" | "interval";
  cronExpression: string;
  intervalSeconds: string;
  timezone: string;
  workspaceRoot: string;
  executionMode: ExecutionMode;
  maxParallelAgents: string;
  /** Saved execution profile ("strict" | "guided" | "yolo"). Empty string = inherit system default (Guided). */
  executionProfile: "" | "strict" | "guided" | "yolo";
  modelProvider: string;
  modelId: string;
  plannerModelProvider: string;
  plannerModelId: string;
  toolAccessMode: WorkflowToolAccessMode;
  customToolsText: string;
  selectedMcpServers: string[];
  selectedMcpTools: string[] | null;
  mcpOtherAllowedTools: string[];
  nodes: WorkflowNodeEditDraft[];
  scopeSnapshot: any | null;
  planPackageBundle: any | null;
  planPackageReplay: any | null;
  scopeValidation: any | null;
  runtimeContext: any | null;
  approvedPlanMaterialization: any | null;
  connectorBindingsJson: string;
  sharedContextPackIdsText: string;
  // Connected-agent handoff fields
  handoffConfig: any | null;
  watchConditions: any[];
  scopePolicy: any | null;
}

interface WorkflowNodeEditDraft {
  nodeId: string;
  title: string;
  objective: string;
  agentId: string;
  modelProvider: string;
  modelId: string;
  toolAccessMode: WorkflowTaskToolAccessMode;
  toolAllowlist: string[];
  toolDenylist: string[];
  mcpAllowedServers: string[];
  mcpAllowedTools: string[] | null;
  mcpOtherAllowedTools: string[];
  mcpAllowedConnections: Array<Record<string, any>>;
  approvalOverride: WorkflowApprovalOverride;
  approvalCondition: string;
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
          toolCache: Array.isArray(row?.tool_cache || row?.toolCache)
            ? (row.tool_cache || row.toolCache)
                .map((tool: any) =>
                  String(
                    tool?.namespaced_name ||
                      tool?.namespacedName ||
                      tool?.tool_name ||
                      tool?.name ||
                      ""
                  ).trim()
                )
                .filter(Boolean)
            : [],
          allowedTools: Array.isArray(row?.allowed_tools || row?.allowedTools)
            ? (row.allowed_tools || row.allowedTools)
                .map((tool: any) => String(tool || "").trim())
                .filter(Boolean)
            : null,
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
          toolCache: Array.isArray((cfg as any).tool_cache || (cfg as any).toolCache)
            ? (((cfg as any).tool_cache || (cfg as any).toolCache) as any[])
                .map((tool: any) =>
                  String(
                    tool?.namespaced_name ||
                      tool?.namespacedName ||
                      tool?.tool_name ||
                      tool?.name ||
                      ""
                  ).trim()
                )
                .filter(Boolean)
            : [],
          allowedTools: Array.isArray((cfg as any).allowed_tools || (cfg as any).allowedTools)
            ? (((cfg as any).allowed_tools || (cfg as any).allowedTools) as any[])
                .map((tool: any) => String(tool || "").trim())
                .filter(Boolean)
            : null,
        };
      })
      .filter((row): row is McpServerOption => !!row)
      .sort((a, b) => a.name.localeCompare(b.name));
  }
  return [];
}

function formatScheduleLabel(schedule: any) {
  const cronExpr = String(schedule?.cron?.expression || schedule?.cron_expression || "").trim();
  const timezone = String(schedule?.timezone || "UTC").trim() || "UTC";
  if (cronExpr) {
    return describeScheduleValue(
      {
        scheduleKind: "cron",
        cronExpression: cronExpr,
        intervalSeconds: "3600",
      },
      { timezone }
    );
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
  const timezone = String(schedule?.timezone || "UTC").trim() || "UTC";
  if (type === "cron") {
    return describeScheduleValue(
      {
        scheduleKind: "cron",
        cronExpression: String(schedule?.cron_expression || schedule?.cronExpression || ""),
        intervalSeconds: "3600",
      },
      { timezone }
    );
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

function getAutomationScheduleAnchorMs(automation: any) {
  const raw =
    automation?.next_fire_at_ms ||
    automation?.nextFireAtMs ||
    automation?.created_at_ms ||
    automation?.createdAtMs ||
    0;
  const value = Number(raw);
  return Number.isFinite(value) && value > 0 ? value : 0;
}

function expandIntervalOccurrences(
  intervalSeconds: number,
  rangeStartMs: number,
  rangeEndMs: number,
  anchorMs: number
) {
  const intervalMs = Math.max(1, Math.round(Math.max(1, intervalSeconds) * 1000));
  const start = Math.max(0, Math.min(rangeStartMs, rangeEndMs));
  const end = Math.max(rangeStartMs, rangeEndMs);
  if (!Number.isFinite(intervalMs) || intervalMs <= 0 || end <= 0) return [];

  const anchor = anchorMs > 0 ? anchorMs : start;
  if (end < anchor) return [];

  let first = anchor;
  if (anchor < start) {
    const offset = start - anchor;
    const steps = Math.ceil(offset / intervalMs);
    first = anchor + steps * intervalMs;
  }

  const occurrences: number[] = [];
  for (let cursor = first; cursor < end; cursor += intervalMs) {
    if (cursor >= start) occurrences.push(cursor);
    if (occurrences.length >= 400) break;
  }
  return occurrences;
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
  customToolsText: string,
  selectedMcpTools: string[] | null,
  mcpOtherAllowedTools: string[]
) {
  if (toolAccessMode === "all") return ["*"];
  const mcpEntries =
    selectedMcpTools === null
      ? selectedMcpServers.map((server) => `mcp.${normalizeMcpServerNamespace(server)}.*`)
      : selectedMcpTools;
  return normalizeAllowedTools([
    ...parseCustomToolText(customToolsText),
    ...mcpOtherAllowedTools,
    ...mcpEntries,
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
    return {
      toolAccessMode: "all" as const,
      customToolsText: "",
      selectedMcpTools: null,
      mcpOtherAllowedTools: [],
    };
  }
  const { mcpTools, otherTools } = splitMcpAllowedTools(allowlist);
  return {
    toolAccessMode: "custom" as const,
    customToolsText: formatCustomToolText(otherTools),
    selectedMcpTools: mcpTools.length ? mcpTools : null,
    mcpOtherAllowedTools: otherTools,
  };
}

function workflowNodeToolAccessDraft(
  node: any
): Pick<
  WorkflowNodeEditDraft,
  | "toolAccessMode"
  | "toolAllowlist"
  | "toolDenylist"
  | "mcpAllowedServers"
  | "mcpAllowedTools"
  | "mcpOtherAllowedTools"
  | "mcpAllowedConnections"
> {
  const metadata = node?.metadata && typeof node.metadata === "object" ? node.metadata : {};
  const toolPolicy = node?.tool_policy || node?.toolPolicy || null;
  const mcpPolicy = node?.mcp_policy || node?.mcpPolicy || null;
  const legacyAllowlist = Array.isArray(metadata?.tool_allowlist || metadata?.toolAllowlist)
    ? metadata.tool_allowlist || metadata.toolAllowlist
    : [];
  const rawToolAllowlist = Array.isArray(toolPolicy?.allowlist)
    ? toolPolicy.allowlist
    : legacyAllowlist;
  const rawToolDenylist = Array.isArray(toolPolicy?.denylist) ? toolPolicy.denylist : [];
  const rawMcpServers = Array.isArray(mcpPolicy?.allowed_servers || mcpPolicy?.allowedServers)
    ? mcpPolicy.allowed_servers || mcpPolicy.allowedServers
    : [];
  const rawMcpTools = mcpPolicy
    ? mcpPolicy.allowed_tools === null || mcpPolicy.allowedTools === null
      ? null
      : Array.isArray(mcpPolicy.allowed_tools || mcpPolicy.allowedTools)
        ? mcpPolicy.allowed_tools || mcpPolicy.allowedTools
        : []
    : rawToolAllowlist;
  const splitTools = splitMcpAllowedTools(
    normalizeAllowedTools(rawToolAllowlist.map((value: any) => String(value || "")))
  );
  const splitMcpTools =
    rawMcpTools === null
      ? { mcpTools: [] as string[], otherTools: [] as string[] }
      : splitMcpAllowedTools(
          normalizeAllowedTools((rawMcpTools || []).map((value: any) => String(value || "")))
        );
  return {
    toolAccessMode: toolPolicy || mcpPolicy || legacyAllowlist.length ? "custom" : "inherit",
    toolAllowlist: splitTools.otherTools,
    toolDenylist: normalizeAllowedTools(rawToolDenylist.map((value: any) => String(value || ""))),
    mcpAllowedServers: normalizeAllowedTools(
      rawMcpServers.map((value: any) => String(value || ""))
    ),
    mcpAllowedTools:
      rawMcpTools === null
        ? null
        : splitMcpTools.mcpTools.length
          ? splitMcpTools.mcpTools
          : mcpPolicy
            ? []
            : null,
    mcpOtherAllowedTools: splitMcpTools.otherTools,
    mcpAllowedConnections: normalizeMcpConnectionGrants(
      mcpPolicy?.allowed_connections || mcpPolicy?.allowedConnections
    ),
  };
}

function connectorBindingsJsonFromPlanPackage(planPackage: any | null) {
  return formatJson(
    Array.isArray(planPackage?.connector_bindings) ? planPackage.connector_bindings : []
  );
}

function normalizeSharedContextPackIds(raw: unknown) {
  const rows = Array.isArray(raw) ? raw : [];
  const seen = new Set<string>();
  const values: string[] = [];
  for (const row of rows) {
    const value = String(row || "").trim();
    if (!value || seen.has(value)) continue;
    seen.add(value);
    values.push(value);
  }
  return values;
}

function sharedContextPackIdsFromAutomation(automation: any) {
  const metadata =
    automation?.metadata && typeof automation.metadata === "object" ? automation.metadata : {};
  const candidateSources = [
    metadata?.shared_context_bindings,
    metadata?.sharedContextBindings,
    metadata?.shared_context_pack_ids,
    metadata?.sharedContextPackIds,
    metadata?.plan_package?.shared_context_bindings,
    metadata?.planPackage?.shared_context_bindings,
    metadata?.plan_package?.shared_context_pack_ids,
    metadata?.planPackage?.shared_context_pack_ids,
  ];
  const rows: string[] = [];
  for (const candidate of candidateSources) {
    if (!Array.isArray(candidate)) continue;
    for (const entry of candidate) {
      if (typeof entry === "string") {
        rows.push(entry);
        continue;
      }
      if (!entry || typeof entry !== "object") continue;
      rows.push(
        String(
          entry.pack_id ||
            entry.packId ||
            entry.context_pack_id ||
            entry.contextPackId ||
            entry.id ||
            ""
        ).trim()
      );
    }
  }
  return normalizeSharedContextPackIds(rows);
}

function cloneJsonValue<T>(value: T): T {
  if (value === undefined) return value;
  return JSON.parse(JSON.stringify(value));
}

function normalizeMcpConnectionGrants(raw: any): Array<Record<string, any>> {
  const rows = Array.isArray(raw) ? raw : [];
  const seen = new Set<string>();
  const grants: Array<Record<string, any>> = [];
  for (const row of rows) {
    if (!row || typeof row !== "object") continue;
    const server = String(row.server || row.server_name || row.serverName || "").trim();
    if (!server) continue;
    const connectionId = String(row.connection_id || row.connectionId || "").trim();
    const runAs = row.run_as ?? row.runAs;
    const grant: Record<string, any> = {
      server,
      ...(connectionId ? { connection_id: connectionId } : {}),
      ...(runAs && typeof runAs === "object" ? { run_as: cloneJsonValue(runAs) } : {}),
    };
    const key = JSON.stringify(grant);
    if (seen.has(key)) continue;
    seen.add(key);
    grants.push(grant);
  }
  return grants;
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
    ? automation.flow.nodes.map((node: any, index: number) => {
        const approvalMetadata =
          node?.metadata?.approval && typeof node.metadata.approval === "object"
            ? node.metadata.approval
            : {};
        const approvalCondition = String(
          approvalMetadata?.auto_approve_when || approvalMetadata?.autoApproveWhen || ""
        ).trim();
        const approvalOverride: WorkflowApprovalOverride =
          approvalMetadata?.skip_approval === true || approvalMetadata?.skipApproval === true
            ? "skip"
            : approvalCondition
              ? "auto"
              : "default";
        return {
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
          approvalOverride,
          approvalCondition,
          ...workflowNodeToolAccessDraft(node),
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
        };
      })
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
  const sharedContextPackIdsText = sharedContextPackIdsFromAutomation(automation).join("\n");
  return {
    automationId,
    name: String(automation?.name || automationId).trim(),
    description: String(automation?.description || "").trim(),
    recoveryPrompt: extractWorkflowRecoveryPrompt(automation),
    sourceAutomation: cloneJsonValue(automation),
    scheduleKind: scheduleEditor.scheduleKind,
    cronExpression: scheduleEditor.cronExpression,
    intervalSeconds: String(scheduleEditor.intervalSeconds),
    timezone: getAutomationScheduleTimezone(automation),
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
    executionProfile: ((): "" | "strict" | "guided" | "yolo" => {
      const raw = String(
        automation?.execution?.profile || (automation?.execution as any)?.executionProfile || ""
      )
        .trim()
        .toLowerCase();
      if (raw === "strict" || raw === "guided" || raw === "yolo") return raw;
      return "";
    })(),
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
    selectedMcpTools: toolAccess.selectedMcpTools,
    mcpOtherAllowedTools: toolAccess.mcpOtherAllowedTools,
    nodes,
    scopeSnapshot,
    planPackageBundle,
    planPackageReplay,
    scopeValidation,
    runtimeContext,
    approvedPlanMaterialization,
    connectorBindingsJson,
    sharedContextPackIdsText,
    // Connected-agent fields
    handoffConfig: automation?.handoff_config || automation?.handoffConfig || null,
    watchConditions: Array.isArray(automation?.watch_conditions)
      ? automation.watch_conditions
      : Array.isArray(automation?.watchConditions)
        ? automation.watchConditions
        : [],
    scopePolicy: automation?.scope_policy || automation?.scopePolicy || null,
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
  const timezone = normalizedScheduleTimezone(draft.timezone);
  const misfirePolicy = { type: "run_once" as const };
  if (draft.scheduleKind === "manual") {
    return {
      type: "manual",
      timezone,
      misfire_policy: misfirePolicy,
    };
  }
  if (draft.scheduleKind === "cron") {
    return {
      type: "cron",
      cron_expression: String(draft.cronExpression || "").trim(),
      timezone,
      misfire_policy: misfirePolicy,
    };
  }
  return {
    type: "interval",
    interval_seconds: Math.max(
      1,
      Number.parseInt(String(draft.intervalSeconds || "3600"), 10) || 3600
    ),
    timezone,
    misfire_policy: misfirePolicy,
  };
}

const CALENDAR_DISPLAY_DURATION_MS = 30 * 60 * 1000;
const CALENDAR_SLOT_MS = 60 * 1000;
const WEEKDAY_INDEX: Record<string, number> = {
  Sun: 0,
  Mon: 1,
  Tue: 2,
  Wed: 3,
  Thu: 4,
  Fri: 5,
  Sat: 6,
};
const TIMEZONE_NUMERIC_FORMATTER_CACHE = new Map<string, Intl.DateTimeFormat>();
const TIMEZONE_WEEKDAY_FORMATTER_CACHE = new Map<string, Intl.DateTimeFormat>();

function normalizedScheduleTimezone(value: string) {
  return String(value || "").trim() || "UTC";
}

function timezoneNumericFormatter(timezone: string) {
  const normalized = normalizedScheduleTimezone(timezone);
  let formatter = TIMEZONE_NUMERIC_FORMATTER_CACHE.get(normalized);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat("en-US", {
      timeZone: normalized,
      year: "numeric",
      month: "numeric",
      day: "numeric",
      hour: "numeric",
      minute: "numeric",
      hourCycle: "h23",
    });
    TIMEZONE_NUMERIC_FORMATTER_CACHE.set(normalized, formatter);
  }
  return formatter;
}

function timezoneWeekdayFormatter(timezone: string) {
  const normalized = normalizedScheduleTimezone(timezone);
  let formatter = TIMEZONE_WEEKDAY_FORMATTER_CACHE.get(normalized);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat("en-US", {
      timeZone: normalized,
      weekday: "short",
    });
    TIMEZONE_WEEKDAY_FORMATTER_CACHE.set(normalized, formatter);
  }
  return formatter;
}

function getDatePart(parts: Intl.DateTimeFormatPart[], type: Intl.DateTimeFormatPartTypes) {
  return Number.parseInt(parts.find((part) => part.type === type)?.value || "0", 10);
}

function zonedDateParts(date: Date, timezone: string) {
  const normalized = normalizedScheduleTimezone(timezone);
  const parts = timezoneNumericFormatter(normalized).formatToParts(date);
  const weekdayLabel = timezoneWeekdayFormatter(normalized).format(date);
  return {
    year: getDatePart(parts, "year"),
    month: getDatePart(parts, "month"),
    day: getDatePart(parts, "day"),
    hour: getDatePart(parts, "hour"),
    minute: getDatePart(parts, "minute"),
    weekday: WEEKDAY_INDEX[weekdayLabel] ?? 0,
  };
}

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

function cronMatchesInTimezone(date: Date, expression: string, timezone: string) {
  const fields = String(expression || "")
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  if (fields.length !== 5) return false;
  const [minuteField, hourField, domField, monthField, dowField] = fields;
  const { minute, hour, day: dom, month, weekday: dow } = zonedDateParts(date, timezone);
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

function expandCronOccurrences(
  expression: string,
  rangeStartMs: number,
  rangeEndMs: number,
  timezone: string
) {
  const out: number[] = [];
  const start = Math.max(0, Math.min(rangeStartMs, rangeEndMs));
  const end = Math.max(rangeStartMs, rangeEndMs);
  const cursor = new Date(Math.floor(start / CALENDAR_SLOT_MS) * CALENDAR_SLOT_MS);
  while (cursor.getTime() < end) {
    if (cronMatchesInTimezone(cursor, expression, timezone)) {
      out.push(cursor.getTime());
    }
    cursor.setTime(cursor.getTime() + CALENDAR_SLOT_MS);
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
  const intervalSeconds = Number(
    schedule?.interval_seconds?.seconds ||
      schedule?.interval_seconds ||
      schedule?.intervalSeconds ||
      0
  );
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
  const starts =
    scheduleType === "cron" && cronExpression
      ? expandCronOccurrences(cronExpression, rangeStartMs, rangeEndMs, timezone)
      : scheduleType === "interval" && Number.isFinite(intervalSeconds) && intervalSeconds > 0
        ? expandIntervalOccurrences(
            intervalSeconds,
            rangeStartMs,
            rangeEndMs,
            getAutomationScheduleAnchorMs(automation)
          )
        : [];
  if (!starts.length) return [];
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
      intervalSeconds:
        Number.isFinite(intervalSeconds) && intervalSeconds > 0 ? intervalSeconds : null,
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

function workflowEditToOperatorPreferences(draft: WorkflowEditDraft) {
  const prefs: Record<string, any> = {
    execution_mode: draft.executionMode,
    max_parallel_agents:
      draft.executionMode === "single"
        ? 1
        : Math.max(
            2,
            Math.min(16, Number.parseInt(String(draft.maxParallelAgents || "4"), 10) || 4)
          ),
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

function firstNonEmptyString(...values: any[]) {
  for (const value of values) {
    const text = String(value || "").trim();
    if (text) return text;
  }
  return "";
}

function extractWorkflowRecoveryPrompt(automation: any) {
  const metadata =
    automation?.metadata && typeof automation.metadata === "object" ? automation.metadata : {};
  return firstNonEmptyString(
    metadata?.original_prompt,
    metadata?.originalPrompt,
    metadata?.normalized_prompt,
    metadata?.normalizedPrompt,
    metadata?.plan_package_bundle?.plan?.mission?.goal,
    metadata?.planPackageBundle?.plan?.mission?.goal,
    metadata?.plan_package?.plan?.mission?.goal,
    metadata?.planPackage?.plan?.mission?.goal,
    metadata?.plan_package?.scope_snapshot?.plan?.mission?.goal,
    metadata?.planPackage?.scopeSnapshot?.plan?.mission?.goal,
    metadata?.plan_package_bundle?.scope_snapshot?.mission?.goal,
    metadata?.planPackageBundle?.scopeSnapshot?.mission?.goal,
    automation?.mission?.goal,
    automation?.mission?.objective,
    automation?.objective
  );
}

// ─── Wizard Steps ───────────────────────────────────────────────────────────

// moved Step1Goal to features/automations/create

// moved Step2Schedule and Step3Mode to features/automations/create

// moved Step4Review to features/automations/create

// ─── Library (combined routines + packs) ───────────────────────────────────

export function MyAutomations({
  client,
  toast,
  navigate,
  viewMode,
  selectedRunId,
  onSelectRunId,
  onOpenRunningView,
  onOpenAdvancedEdit,
  onRecreateWorkflowAutomation,
  defaultRunningSectionsOpen,
}: {
  client: any;
  toast: any;
  navigate: (route: string) => void;
  viewMode: "calendar" | "list" | "running";
  selectedRunId: string;
  onSelectRunId: (runId: string) => void;
  onOpenRunningView: () => void;
  onOpenAdvancedEdit: (automation: any) => void;
  onRecreateWorkflowAutomation?: (payload: any) => void;
  defaultRunningSectionsOpen?: {
    active?: boolean;
    issues?: boolean;
    history?: boolean;
  };
}) {
  const statusColor = (status: string) => {
    const normalizedStatus = String(status || "").toLowerCase();
    if (
      normalizedStatus === "active" ||
      normalizedStatus === "completed" ||
      normalizedStatus === "done"
    ) {
      return "tcp-badge-ok";
    }
    if (normalizedStatus === "running" || normalizedStatus === "in_progress") {
      return "tcp-badge-warn";
    }
    if (normalizedStatus === "blocked") {
      return "border border-emerald-400/60 bg-emerald-400/10 text-emerald-200";
    }
    if (
      normalizedStatus === "failed" ||
      normalizedStatus === "error" ||
      normalizedStatus === "stalled"
    ) {
      return "tcp-badge-err";
    }
    return "tcp-badge-info";
  };

  return (
    <MyAutomationsContainer
      client={client}
      toast={toast}
      navigate={navigate}
      viewMode={viewMode}
      selectedRunId={selectedRunId}
      onSelectRunId={onSelectRunId}
      onOpenRunningView={onOpenRunningView}
      onOpenAdvancedEdit={onOpenAdvancedEdit}
      onRecreateWorkflowAutomation={onRecreateWorkflowAutomation}
      defaultRunningSectionsOpen={defaultRunningSectionsOpen}
      automationWizardConfig={AUTOMATION_WIZARD_CONFIG}
      helperFns={{
        toArray,
        normalizeMcpServers,
        validateModelInput,
        validatePlannerModelInput,
        validateWorkspaceRootInput,
        workflowEditToOperatorPreferences,
        compileWorkflowModelPolicy,
        cloneJsonValue,
        compileWorkflowToolAllowlist,
        parseConnectorBindingsJson,
        workflowNodeModelPolicyWithOverride,
        deriveConnectorBindingResolutionFromPlanPackage,
        workflowAutomationToEditDraft,
        isMissionBlueprintAutomation,
        workflowEditToSchedule,
        buildCalendarOccurrences,
        normalizeTimestamp,
        workflowQueueReason,
        detectWorkflowActiveTaskId,
        detectWorkflowActiveTaskIds,
        workflowDescendantTaskIds,
        deriveRunDebugHints,
        explainRunFailure,
        buildRunBlockers,
        isStandupAutomation,
        getAutomationCalendarFamily,
        rewriteCronForDroppedStart,
        statusColor,
        formatScheduleLabel,
        formatAutomationV2ScheduleLabel,
        workflowStatusDisplay,
        workflowStatusSubtleDetail,
        runDisplayTitle,
        formatRunDateTime,
        runObjectiveText,
        shortText,
        runTimeLabel,
        compactIdentifier,
        sessionLabel,
        formatTimestampLabel,
        isActiveRunStatus,
        scheduleToEditor,
        uniqueStrings,
        collectPathStrings,
        timestampOrNull,
        sessionMessageId,
        sessionMessageCreatedAt,
        sessionMessageVariant,
        sessionMessageText,
        sessionMessageParts,
      }}
    />
  );
}

// ─── Spawn Approvals ────────────────────────────────────────────────────────

// ─── Main Page ──────────────────────────────────────────────────────────────

export function AutomationsPage({
  client,
  api,
  toast,
  navigate,
  providerStatus,
  navigationLock,
  setNavigationLock,
}: AppPageProps) {
  const {
    tab,
    setTab,
    createMode,
    setCreateMode,
    selectedRunId,
    setSelectedRunId,
    advancedEditAutomation,
    setAdvancedEditAutomation,
    composerEnabled,
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
      navigationLocked={!!navigationLock}
      onNavigationLockChange={setNavigationLock}
      client={client}
      api={api}
      toast={toast}
      navigate={navigate}
      providerStatus={providerStatus}
      composerEnabled={composerEnabled}
      PageCardComponent={PageCard}
      CreateWizardComponent={CreateWizardExternal}
      AutomationComposerPanelComponent={AutomationComposerPanel}
      MyAutomationsComponent={MyAutomations}
      AdvancedMissionBuilderPanelComponent={AdvancedMissionBuilderPanel}
    />
  );
}
