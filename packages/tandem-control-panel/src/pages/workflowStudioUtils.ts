import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useState } from "react";
import { renderIcons } from "../app/icons.js";
import { workflowLatestStabilitySnapshot } from "../features/orchestration/workflowStability";
import { useSystemHealth } from "../features/system/queries";
import {
  STUDIO_TEMPLATE_CATALOG,
  createWorkflowDraftFromTemplate,
} from "../features/studio/catalog";
import type {
  StudioAgentDraft,
  StudioNodeDraft,
  StudioPromptSections,
  StudioRole,
  StudioWorkflowDraft,
} from "../features/studio/schema";
import {
  createEmptyAgentDraft,
  createEmptyNodeDraft,
  emptyPromptSections,
} from "../features/studio/schema";
import { EmptyState, PageCard } from "./ui";
import type { AppPageProps } from "./pageTypes";

export const ROLE_OPTIONS: StudioRole[] = [
  "worker",
  "reviewer",
  "tester",
  "watcher",
  "delegator",
  "committer",
  "orchestrator",
];

export type StudioRepairState = {
  repairedAgentIds: string[];
  repairedTemplateIds: string[];
  missingNodeAgentIds: string[];
  reason: "load" | "run_preflight" | "save";
  requiresSave: boolean;
};

export type ProviderOption = {
  id: string;
  models: string[];
};

export const AUTOMATIONS_STUDIO_HANDOFF_KEY = "tandem.automations.studioHandoff";
export const AGENT_CATALOG_HANDOFF_KEY = "tandem.studio.agentCatalogHandoff";

export type AgentCatalogHandoff = {
  agentId: string;
  displayName: string;
  categoryId: string;
  categoryTitle: string;
  summary: string;
  sourcePath: string;
  sandboxMode: string;
  role: StudioRole;
  tags: string[];
  requires: string[];
  instructions: string;
};

export function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

export function safeString(value: unknown) {
  return String(value || "").trim();
}

export function normalizeStudioRole(value: unknown): StudioRole {
  const role = safeString(value) as StudioRole;
  return ROLE_OPTIONS.includes(role) ? role : "worker";
}

export function createAgentDraftFromCatalog(entry: AgentCatalogHandoff): StudioAgentDraft {
  return createEmptyAgentDraft(entry.agentId, entry.displayName, {
    role: normalizeStudioRole(entry.role),
    prompt: emptyPromptSections({
      role: `${entry.displayName} (${entry.categoryTitle})`,
      mission: entry.instructions,
      inputs: [
        `Source path: ${entry.sourcePath}`,
        `Category: ${entry.categoryTitle}`,
        `Sandbox: ${entry.sandboxMode}`,
        entry.tags.length ? `Tags: ${entry.tags.join(", ")}` : "",
        entry.requires.length ? `Requires: ${entry.requires.join(", ")}` : "",
      ]
        .filter(Boolean)
        .join("\n"),
      outputContract:
        "Use this catalog specialization to complete the selected workflow stage clearly and concretely.",
      guardrails:
        "Keep the stage scoped to the imported catalog instructions and preserve the workflow's existing conventions.",
    }),
  });
}

export function isCodeLikeOutputPath(value: string) {
  const normalized = safeString(value).toLowerCase();
  const ext = normalized.includes(".") ? normalized.split(".").pop() || "" : "";
  return [
    "rs",
    "ts",
    "tsx",
    "js",
    "jsx",
    "py",
    "go",
    "java",
    "kt",
    "kts",
    "c",
    "cc",
    "cpp",
    "h",
    "hpp",
    "cs",
    "rb",
    "php",
    "swift",
    "scala",
    "sh",
    "bash",
    "zsh",
  ].includes(ext);
}

export function outputPathExtension(value: string) {
  const normalized = safeString(value).toLowerCase();
  return normalized.includes(".") ? normalized.split(".").pop() || "" : "";
}

export function isCodeLikeTaskKind(value: string) {
  const normalized = safeString(value).toLowerCase();
  return (
    normalized === "code_change" || normalized === "repo_fix" || normalized === "implementation"
  );
}

export function nodeHasCodeWorkflowMetadata(node: StudioNodeDraft) {
  return Boolean(
    node.projectBacklogTasks ||
    safeString(node.repoRoot) ||
    safeString(node.writeScope) ||
    safeString(node.verificationCommand)
  );
}

export function inferredArtifactOutputKind(node: StudioNodeDraft) {
  const ext = outputPathExtension(node.outputPath);
  if (ext === "md" || ext === "markdown") return "report_markdown";
  if (ext === "json") return "structured_json";
  if (ext === "txt") return "text_summary";
  return "artifact";
}

export function normalizeNodeWorkflowClassification(node: StudioNodeDraft): StudioNodeDraft {
  const staleCodeTaskKind =
    isCodeLikeTaskKind(node.taskKind || "") &&
    !isCodeLikeOutputPath(node.outputPath) &&
    !nodeHasCodeWorkflowMetadata(node);
  const staleCodeOutputKind =
    safeString(node.outputKind).toLowerCase() === "code_patch" &&
    !isCodeLikeOutputPath(node.outputPath) &&
    !nodeHasCodeWorkflowMetadata(node);
  if (!staleCodeTaskKind && !staleCodeOutputKind) {
    return node;
  }
  return {
    ...node,
    taskKind: staleCodeTaskKind ? "" : node.taskKind,
    outputKind: staleCodeOutputKind ? inferredArtifactOutputKind(node) : node.outputKind,
  };
}

export function isCodeLikeNode(node: StudioNodeDraft) {
  const normalizedNode = normalizeNodeWorkflowClassification(node);
  const taskKind = safeString(normalizedNode.taskKind).toLowerCase();
  return (
    isCodeLikeTaskKind(taskKind) ||
    safeString(normalizedNode.outputKind).toLowerCase() === "code_patch" ||
    isCodeLikeOutputPath(normalizedNode.outputPath)
  );
}

export function isBacklogProjectingNode(node: StudioNodeDraft) {
  return !!node.projectBacklogTasks;
}

export function normalizeWorkspaceToolAllowlist(values: string[]) {
  const normalized = values.map((entry) => safeString(entry)).filter(Boolean);
  if (normalized.includes("*")) return ["*"];
  return Array.from(new Set(normalized));
}

export function normalizeNodeAwareToolAllowlist(values: string[], nodes: StudioNodeDraft[]) {
  const normalized = normalizeWorkspaceToolAllowlist(values);
  if (normalized.includes("*")) return ["*"];
  const hasCodeNode = nodes.some((node) => isCodeLikeNode(node));
  const required = hasCodeNode ? ["read", "glob", "edit", "apply_patch", "write", "bash"] : [];
  return Array.from(new Set([...normalized, ...required]));
}

export function shortId(value: unknown) {
  const text = safeString(value);
  if (!text) return "";
  if (text.length <= 18) return text;
  return `${text.slice(0, 8)}...${text.slice(-6)}`;
}

export function timestampLabel(value: unknown) {
  const timestamp = Number(value || 0);
  if (!timestamp) return "";
  try {
    return new Intl.DateTimeFormat(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    }).format(new Date(timestamp));
  } catch {
    return "";
  }
}

export function splitCsv(value: string) {
  return String(value || "")
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean);
}

export function joinCsv(values: string[]) {
  return Array.isArray(values) ? values.join(", ") : "";
}

const STUDIO_OUTPUT_TOKEN_GUIDE = [
  "{current_date}",
  "{current_time}",
  "{current_timestamp}",
  "{current_timestamp_filename}",
];

export function studioRuntimePreviewValues(now = new Date()) {
  const pad = (value: number) => String(value).padStart(2, "0");
  const year = now.getFullYear();
  const month = pad(now.getMonth() + 1);
  const day = pad(now.getDate());
  const hour = pad(now.getHours());
  const minute = pad(now.getMinutes());
  const second = pad(now.getSeconds());
  return {
    current_date: `${year}-${month}-${day}`,
    current_time: `${hour}${minute}`,
    current_timestamp: `${year}-${month}-${day} ${hour}:${minute}`,
    current_timestamp_filename: `${year}-${month}-${day}_${hour}-${minute}-${second}`,
  };
}

export function canonicalizeStudioOutputPathTemplate(value: string) {
  const trimmed = safeString(value);
  if (!trimmed) return "";
  let next = trimmed;
  [
    ["{{current_timestamp_filename}}", "{current_timestamp_filename}"],
    ["{{current_date}}", "{current_date}"],
    ["{{current_time}}", "{current_time}"],
    ["{{current_timestamp}}", "{current_timestamp}"],
    ["{{date}}", "{current_date}"],
    ["{date}", "{current_date}"],
    ["YYYY-MM-DD_HH-MM-SS", "{current_timestamp_filename}"],
    ["YYYY-MM-DD-HH-MM-SS", "{current_timestamp_filename}"],
    ["YYYY-MM-DD_HHMMSS", "{current_timestamp_filename}"],
    ["YYYY-MM-DD-HHMMSS", "{current_timestamp_filename}"],
    ["YYYY-MM-DD_HHMM", "{current_date}_{current_time}"],
    ["YYYY-MM-DD-HHMM", "{current_date}-{current_time}"],
    ["YYYY-MM-DD", "{current_date}"],
  ].forEach(([needle, replacement]) => {
    next = next.split(needle).join(replacement);
  });
  if (!next.includes("HHMMSS")) {
    next = next.split("HHMM").join("{current_time}");
  }
  return next;
}

export function resolveStudioOutputPathTemplate(value: string, now = new Date()) {
  const canonical = canonicalizeStudioOutputPathTemplate(value);
  const runtime = studioRuntimePreviewValues(now);
  return canonical
    .split("{current_timestamp_filename}")
    .join(runtime.current_timestamp_filename)
    .split("{current_timestamp}")
    .join(runtime.current_timestamp)
    .split("{current_date}")
    .join(runtime.current_date)
    .split("{current_time}")
    .join(runtime.current_time);
}

export function studioOutputPathWarning(value: string) {
  const canonical = canonicalizeStudioOutputPathTemplate(value);
  if (!canonical) return "";
  const unknownTokens = Array.from(
    new Set(
      (canonical.match(/\{[^}]+\}/g) || []).filter(
        (token) => !STUDIO_OUTPUT_TOKEN_GUIDE.includes(token)
      )
    )
  );
  if (unknownTokens.length) {
    return `Unknown output token${unknownTokens.length === 1 ? "" : "s"}: ${unknownTokens.join(", ")}`;
  }
  if (/(YYYY|YYYYMMDD|HHMMSS|HH-MM-SS|HH-MM|HH:MM|\{\{date\}\}|\{date\})/.test(canonical)) {
    return "This path still contains legacy timestamp text Tandem cannot safely canonicalize on save.";
  }
  return "";
}

export function canonicalizeStudioNodeOutputTemplates(node: StudioNodeDraft): StudioNodeDraft {
  return {
    ...node,
    outputPath: canonicalizeStudioOutputPathTemplate(node.outputPath),
    outputFiles: normalizeStringList(node.outputFiles).map(canonicalizeStudioOutputPathTemplate),
  };
}

export function canonicalizeStudioDraftOutputTemplates(
  draft: StudioWorkflowDraft
): StudioWorkflowDraft {
  return {
    ...draft,
    outputTargets: normalizeStringList(draft.outputTargets).map(
      canonicalizeStudioOutputPathTemplate
    ),
    nodes: draft.nodes.map(canonicalizeStudioNodeOutputTemplates),
  };
}

export function collectStudioOutputPathWarnings(draft: StudioWorkflowDraft) {
  const warnings: string[] = [];
  draft.outputTargets.forEach((target) => {
    const warning = studioOutputPathWarning(target);
    if (warning) warnings.push(`Workflow target ${safeString(target)}: ${warning}`);
  });
  draft.nodes.forEach((node) => {
    const outputPath = safeString(node.outputPath);
    if (!outputPath) return;
    const warning = studioOutputPathWarning(outputPath);
    if (warning) warnings.push(`${safeString(node.title) || safeString(node.nodeId)}: ${warning}`);
  });
  return warnings;
}

export function normalizeStringList(values: unknown) {
  if (!Array.isArray(values)) return [];
  return values
    .map((entry) => safeString(entry))
    .filter(Boolean)
    .filter((value, index, all) => all.indexOf(value) === index);
}

export function effectiveNodeOutputFiles(node: StudioNodeDraft) {
  const explicit = normalizeStringList(node.outputFiles);
  if (explicit.length) return explicit;
  const outputPath = safeString(node.outputPath);
  return outputPath ? [outputPath] : [];
}

export function effectiveNodeInputFiles(node: StudioNodeDraft, nodes: StudioNodeDraft[]) {
  const explicit = normalizeStringList(node.inputFiles);
  if (explicit.length) return explicit;
  const nodesById = new Map(nodes.map((entry) => [safeString(entry.nodeId), entry]));
  return normalizeStringList(
    syncInputRefs(node.dependsOn, node.inputRefs).flatMap((ref) => {
      const upstream = nodesById.get(safeString(ref.fromStepId));
      return upstream ? effectiveNodeOutputFiles(upstream) : [];
    })
  );
}

export function slugify(value: string) {
  return String(value || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
}

export function titleToId(value: string, fallback: string) {
  return slugify(value) || fallback;
}

export function validateWorkspaceRootInput(raw: string) {
  const value = safeString(raw);
  if (!value) return "Workspace root is required.";
  if (!value.startsWith("/")) return "Workspace root must be an absolute path.";
  return "";
}

export function seedAutomationsStudioHandoff(payload: {
  tab: "running";
  runId?: string;
  automationId?: string;
  openTaskInspector?: boolean;
}) {
  try {
    sessionStorage.setItem(AUTOMATIONS_STUDIO_HANDOFF_KEY, JSON.stringify(payload));
  } catch {
    // ignore
  }
}

export function composePromptSections(prompt: StudioPromptSections) {
  const sections = [
    ["Role", prompt.role],
    ["Mission", prompt.mission],
    ["Inputs", prompt.inputs],
    ["Output Contract", prompt.outputContract],
    ["Guardrails", prompt.guardrails],
  ].filter(([, text]) => safeString(text));
  return sections.map(([label, text]) => `${label}:\n${String(text).trim()}`).join("\n\n");
}

export function buildModelPolicy(agent: StudioAgentDraft) {
  if (!safeString(agent.modelProvider) || !safeString(agent.modelId)) return undefined;
  return {
    default_model: {
      provider_id: safeString(agent.modelProvider),
      model_id: safeString(agent.modelId),
    },
  };
}

export function resolveDefaultModel(
  providerOptions: ProviderOption[],
  providersConfig: any
): { provider: string; model: string } {
  const configuredProvider = safeString(providersConfig?.default);
  const provider = configuredProvider || providerOptions[0]?.id || "";
  if (!provider) return { provider: "", model: "" };
  const models = providerOptions.find((entry) => entry.id === provider)?.models || [];
  const model = safeString(providersConfig?.providers?.[provider]?.default_model || models[0]);
  return { provider, model };
}

export function modelsForProvider(providerOptions: ProviderOption[], providerId: string) {
  return providerOptions.find((entry) => entry.id === providerId)?.models || [];
}

export function applyDefaultModelToAgents(
  agents: StudioAgentDraft[],
  fallback: { provider: string; model: string }
) {
  if (!fallback.provider || !fallback.model) return agents;
  return agents.map((agent) =>
    safeString(agent.modelProvider) && safeString(agent.modelId)
      ? agent
      : {
          ...agent,
          modelProvider: safeString(agent.modelProvider) || fallback.provider,
          modelId: safeString(agent.modelId) || fallback.model,
        }
  );
}

export function inferSharedModelFromAgents(agents: StudioAgentDraft[]) {
  const normalized = agents
    .map((agent) => ({
      provider: safeString(agent.modelProvider),
      model: safeString(agent.modelId),
    }))
    .filter((entry) => entry.provider && entry.model);
  if (!normalized.length) {
    return { useSharedModel: false, provider: "", model: "" };
  }
  const first = normalized[0];
  const allSame = normalized.every(
    (entry) => entry.provider === first.provider && entry.model === first.model
  );
  return {
    useSharedModel: allSame,
    provider: allSame ? first.provider : "",
    model: allSame ? first.model : "",
  };
}

export function applySharedModelToAgents(
  agents: StudioAgentDraft[],
  provider: string,
  model: string
) {
  const normalizedProvider = safeString(provider);
  const normalizedModel = safeString(model);
  if (!normalizedProvider || !normalizedModel) return agents;
  return agents.map((agent) => ({
    ...agent,
    modelProvider: normalizedProvider,
    modelId: normalizedModel,
  }));
}

export function buildSchedulePayload(draft: StudioWorkflowDraft) {
  const misfirePolicy = { type: "run_once" as const };
  if (draft.scheduleType === "cron" && safeString(draft.cronExpression)) {
    return {
      type: "cron",
      cron_expression: safeString(draft.cronExpression),
      timezone: "UTC",
      misfire_policy: misfirePolicy,
    };
  }
  if (draft.scheduleType === "interval") {
    const seconds = Math.max(
      60,
      Number.parseInt(String(draft.intervalSeconds || "3600"), 10) || 3600
    );
    return {
      type: "interval",
      interval_seconds: seconds,
      timezone: "UTC",
      misfire_policy: misfirePolicy,
    };
  }
  return { type: "manual", timezone: "UTC", misfire_policy: misfirePolicy };
}

export function extractMcpServers(raw: any) {
  if (Array.isArray(raw?.servers)) {
    return raw.servers
      .map((row: any) => safeString(row?.name))
      .filter(Boolean)
      .sort((a, b) => a.localeCompare(b));
  }
  return Object.keys(raw || {})
    .map((key) => safeString(key))
    .filter(Boolean)
    .sort((a, b) => a.localeCompare(b));
}

export function normalizeTemplateRow(row: any) {
  const defaultModel = row?.default_model || row?.defaultModel || {};
  const rawSkills = Array.isArray(row?.skills) ? row.skills : [];
  const skills = rawSkills
    .map((skill: any) =>
      typeof skill === "string"
        ? safeString(skill)
        : safeString(skill?.skill_id || skill?.skillId || skill?.id || skill?.name)
    )
    .filter(Boolean);
  return {
    templateId: safeString(row?.template_id || row?.templateID || row?.id),
    displayName: safeString(row?.display_name || row?.displayName || row?.name),
    avatarUrl: safeString(row?.avatar_url || row?.avatarUrl),
    role: (safeString(row?.role || "worker") || "worker") as StudioRole,
    systemPrompt: safeString(row?.system_prompt || row?.systemPrompt),
    modelProvider: safeString(defaultModel?.provider_id || defaultModel?.providerId),
    modelId: safeString(defaultModel?.model_id || defaultModel?.modelId),
    skills,
  };
}

export function promptSectionsFromFreeform(text: string): StudioPromptSections {
  return {
    role: "",
    mission: safeString(text),
    inputs: "",
    outputContract: "",
    guardrails: "",
  };
}

export function syncInputRefs(
  dependsOn: string[],
  refs: Array<{ fromStepId: string; alias: string }>
) {
  const nextRefs = dependsOn.map((depId) => {
    const existing = refs.find((ref) => ref.fromStepId === depId);
    return existing || { fromStepId: depId, alias: depId.replace(/-/g, "_") };
  });
  return nextRefs;
}

export function computeNodeDepths(nodes: StudioNodeDraft[]) {
  const byId = new Map(nodes.map((node) => [node.nodeId, node]));
  const cache = new Map<string, number>();
  const visit = (nodeId: string, seen = new Set<string>()): number => {
    if (cache.has(nodeId)) return Number(cache.get(nodeId) || 0);
    if (seen.has(nodeId)) return 0;
    const node = byId.get(nodeId);
    if (!node || !node.dependsOn.length) {
      cache.set(nodeId, 0);
      return 0;
    }
    const nextSeen = new Set(seen);
    nextSeen.add(nodeId);
    const depth = Math.max(...node.dependsOn.map((dep) => visit(dep, nextSeen))) + 1;
    cache.set(nodeId, depth);
    return depth;
  };
  for (const node of nodes) visit(node.nodeId);
  return cache;
}

export function normalizeAgentDraft(row: any): StudioAgentDraft {
  return {
    agentId: safeString(row?.agentId || row?.agent_id || row?.id),
    displayName: safeString(row?.displayName || row?.display_name || row?.name),
    role: (safeString(row?.role || "worker") || "worker") as StudioRole,
    avatarUrl: safeString(row?.avatarUrl || row?.avatar_url),
    templateId: safeString(row?.templateId || row?.template_id),
    linkedTemplateId: safeString(
      row?.linkedTemplateId || row?.linked_template_id || row?.templateId || row?.template_id
    ),
    skills: Array.isArray(row?.skills)
      ? row.skills.map((entry: any) => safeString(entry)).filter(Boolean)
      : [],
    prompt:
      row?.prompt && typeof row.prompt === "object"
        ? {
            role: safeString(row.prompt.role),
            mission: safeString(row.prompt.mission),
            inputs: safeString(row.prompt.inputs),
            outputContract: safeString(row.prompt.outputContract || row.prompt.output_contract),
            guardrails: safeString(row.prompt.guardrails),
          }
        : promptSectionsFromFreeform(safeString(row?.systemPrompt || row?.system_prompt)),
    modelProvider: safeString(row?.modelProvider || row?.model_provider),
    modelId: safeString(row?.modelId || row?.model_id),
    toolAllowlist: normalizeWorkspaceToolAllowlist(
      Array.isArray(row?.toolAllowlist || row?.tool_allowlist)
        ? (row.toolAllowlist || row.tool_allowlist)
            .map((entry: any) => safeString(entry))
            .filter(Boolean)
        : Array.isArray(row?.tool_policy?.allowlist)
          ? row.tool_policy.allowlist.map((entry: any) => safeString(entry)).filter(Boolean)
          : []
    ),
    toolDenylist: Array.isArray(row?.toolDenylist || row?.tool_denylist)
      ? (row.toolDenylist || row.tool_denylist)
          .map((entry: any) => safeString(entry))
          .filter(Boolean)
      : Array.isArray(row?.tool_policy?.denylist)
        ? row.tool_policy.denylist.map((entry: any) => safeString(entry)).filter(Boolean)
        : [],
    mcpAllowedServers: Array.isArray(row?.mcpAllowedServers || row?.mcp_allowed_servers)
      ? (row.mcpAllowedServers || row.mcp_allowed_servers)
          .map((entry: any) => safeString(entry))
          .filter(Boolean)
      : Array.isArray(row?.mcp_policy?.allowed_servers)
        ? row.mcp_policy.allowed_servers.map((entry: any) => safeString(entry)).filter(Boolean)
        : [],
  };
}

export function normalizeNodeDraft(row: any): StudioNodeDraft {
  const dependsOn = Array.isArray(row?.dependsOn || row?.depends_on)
    ? (row.dependsOn || row.depends_on).map((entry: any) => safeString(entry)).filter(Boolean)
    : [];
  const inputRefsSource = Array.isArray(row?.inputRefs || row?.input_refs)
    ? row.inputRefs || row.input_refs
    : [];
  const metadataBuilder =
    row?.metadata?.builder && typeof row.metadata.builder === "object" ? row.metadata.builder : {};
  return normalizeNodeWorkflowClassification({
    nodeId: safeString(row?.nodeId || row?.node_id || row?.id),
    title: safeString(row?.title || row?.objective || row?.nodeId || row?.node_id),
    agentId: safeString(row?.agentId || row?.agent_id),
    objective: safeString(row?.objective),
    dependsOn,
    inputRefs: syncInputRefs(
      dependsOn,
      inputRefsSource.map((ref: any) => ({
        fromStepId: safeString(ref?.fromStepId || ref?.from_step_id),
        alias: safeString(ref?.alias),
      }))
    ),
    stageKind: safeString(
      row?.stageKind ||
        row?.stage_kind ||
        metadataBuilder?.research_stage ||
        row?.metadata?.studio?.research_stage
    ),
    inputFiles: normalizeStringList(
      row?.inputFiles ||
        row?.input_files ||
        metadataBuilder?.input_files ||
        row?.metadata?.studio?.input_files
    ),
    outputKind: safeString(
      row?.outputKind || row?.output_kind || row?.output_contract?.kind || "artifact"
    ),
    outputPath: safeString(
      row?.outputPath ||
        row?.output_path ||
        metadataBuilder?.output_path ||
        row?.metadata?.studio?.output_path
    ),
    outputFiles: normalizeStringList(
      row?.outputFiles ||
        row?.output_files ||
        metadataBuilder?.output_files ||
        row?.metadata?.studio?.output_files
    ),
    taskKind: safeString(row?.taskKind || row?.task_kind || metadataBuilder?.task_kind),
    projectBacklogTasks: Boolean(
      row?.projectBacklogTasks ||
      row?.project_backlog_tasks ||
      metadataBuilder?.project_backlog_tasks
    ),
    backlogTaskId: safeString(
      row?.backlogTaskId || row?.backlog_task_id || metadataBuilder?.task_id
    ),
    repoRoot: safeString(row?.repoRoot || row?.repo_root || metadataBuilder?.repo_root),
    writeScope: safeString(row?.writeScope || row?.write_scope || metadataBuilder?.write_scope),
    acceptanceCriteria: safeString(
      row?.acceptanceCriteria || row?.acceptance_criteria || metadataBuilder?.acceptance_criteria
    ),
    taskDependencies: safeString(
      row?.taskDependencies || row?.task_dependencies || metadataBuilder?.task_dependencies
    ),
    verificationState: safeString(
      row?.verificationState || row?.verification_state || metadataBuilder?.verification_state
    ),
    taskOwner: safeString(row?.taskOwner || row?.task_owner || metadataBuilder?.task_owner),
    verificationCommand: safeString(
      row?.verificationCommand || row?.verification_command || metadataBuilder?.verification_command
    ),
  });
}

export function composeNodeExecutionPrompt(node: StudioNodeDraft, agent: StudioAgentDraft) {
  const codeLike = isCodeLikeNode(node);
  const canGlob = agent.toolAllowlist.includes("glob");
  const canRead = agent.toolAllowlist.includes("read");
  const canEdit = agent.toolAllowlist.includes("edit") || codeLike;
  const canPatch = agent.toolAllowlist.includes("apply_patch") || codeLike;
  const canBash = agent.toolAllowlist.includes("bash");
  const lines = [
    safeString(node.objective),
    safeString(agent.prompt.mission),
    safeString(agent.prompt.inputs),
    safeString(agent.prompt.outputContract),
    safeString(agent.prompt.guardrails),
    safeString(node.taskKind) ? `Task kind: ${safeString(node.taskKind)}.` : "",
    isBacklogProjectingNode(node)
      ? "Project backlog tasks for this run. Include a fenced `json` block containing either an array of tasks or an object with `backlog_tasks`, where each task includes `task_id`, `title`, `description`, `repo_root`, `write_scope`, `acceptance_criteria`, `task_dependencies`, `verification_state`, `task_owner`, and `verification_command` when known."
      : "",
    safeString(node.backlogTaskId) ? `Backlog task id: ${safeString(node.backlogTaskId)}.` : "",
    safeString(node.repoRoot) ? `Repository root for this task: ${safeString(node.repoRoot)}.` : "",
    safeString(node.writeScope) ? `Declared write scope: ${safeString(node.writeScope)}.` : "",
    safeString(node.acceptanceCriteria)
      ? `Acceptance criteria: ${safeString(node.acceptanceCriteria)}.`
      : "",
    safeString(node.taskDependencies)
      ? `Backlog task dependencies: ${safeString(node.taskDependencies)}.`
      : "",
    safeString(node.verificationState)
      ? `Expected verification state progression: ${safeString(node.verificationState)}.`
      : "",
    safeString(node.taskOwner)
      ? `Preferred task owner or claimer: ${safeString(node.taskOwner)}.`
      : "",
    safeString(node.verificationCommand)
      ? `Expected verification command or rule: ${safeString(node.verificationCommand)}.`
      : "",
    node.inputFiles.length ? `Declared input files: ${joinCsv(node.inputFiles)}.` : "",
    node.outputFiles.length ? `Declared output files: ${joinCsv(node.outputFiles)}.` : "",
    "Execution rules:",
    canGlob || canRead
      ? `- Inspect the workspace before writing using ${[
          canGlob ? "`glob`" : "",
          canRead ? "`read`" : "",
        ]
          .filter(Boolean)
          .join(" and ")}.`
      : "",
    canGlob ? "- Use `glob` to enumerate directories or discover candidate file paths." : "",
    canRead ? "- Use `read` only for concrete file paths you have already identified." : "",
    "- Use workspace-relative paths like `README.md` or `subdir/file.md`. Do not use a `/workspace/...` prefix.",
    agent.toolAllowlist.includes("websearch")
      ? "- Use `websearch` to gather current external evidence before finalizing the file."
      : "",
    node.stageKind === "research_discover"
      ? "- After `glob` discovery, perform at least one concrete `read` on a prioritized source index or representative workspace file before completing this stage."
      : "",
    node.stageKind && !safeString(node.outputPath)
      ? "- Do not write final workspace artifacts in this stage. Return a structured handoff in your final response instead."
      : "",
    codeLike && canPatch
      ? "- Prefer `apply_patch` for multi-line source edits when a git-backed patch tool is available."
      : "",
    codeLike && canEdit
      ? "- Prefer `edit` for existing-file source changes; use `write` only for new files or when patch/edit cannot express the change."
      : safeString(node.outputPath)
        ? `- Create or update \`${safeString(node.outputPath)}\` in the workspace with the \`write\` tool.`
        : "- If this stage creates a file, use the `write` tool rather than a prose-only response.",
    codeLike && canBash
      ? "- Use `bash` for repo-appropriate build, test, or lint commands after editing, and report the exact commands run."
      : "",
    codeLike
      ? "- Do not replace an existing source file with a status note, placeholder, or preservation summary."
      : "",
    node.stageKind && !safeString(node.outputPath)
      ? "- Do not claim success unless the required structured handoff was actually returned in the final response."
      : safeString(node.outputPath)
        ? `- Do not claim success unless \`${safeString(node.outputPath)}\` was actually written.`
        : "- Do not claim success unless the required artifact was actually created.",
  ].filter(Boolean);
  return lines.join("\n");
}

export function defaultAgentFromStarter(
  starterTemplateId: string,
  agentId: string,
  workspaceRoot: string
): StudioAgentDraft | null {
  const template = STUDIO_TEMPLATE_CATALOG.find((entry) => entry.id === starterTemplateId);
  if (!template) return null;
  const starterDraft = createWorkflowDraftFromTemplate(template, workspaceRoot || "");
  return starterDraft.agents.find((agent) => agent.agentId === agentId) || null;
}

export function repairDraftTemplateLinks(
  draft: StudioWorkflowDraft,
  templateMap: Map<string, ReturnType<typeof normalizeTemplateRow>>
): { draft: StudioWorkflowDraft; repairState: StudioRepairState | null } {
  const repairedAgentIds: string[] = [];
  const repairedTemplateIds: string[] = [];
  const missingNodeAgentIds = draft.nodes
    .map((node) => node.agentId)
    .filter(
      (agentId, index, all) =>
        !!agentId &&
        all.indexOf(agentId) === index &&
        !draft.agents.some((agent) => agent.agentId === agentId)
    );

  const repairedAgents = draft.agents.map((agent) => {
    const linkedTemplateId = safeString(agent.linkedTemplateId || agent.templateId);
    if (!linkedTemplateId) return agent;
    if (templateMap.has(linkedTemplateId)) return agent;
    const fallback = defaultAgentFromStarter(
      draft.starterTemplateId,
      agent.agentId,
      draft.workspaceRoot
    );
    repairedAgentIds.push(agent.agentId);
    repairedTemplateIds.push(linkedTemplateId);
    return {
      ...agent,
      templateId: "",
      linkedTemplateId: "",
      prompt: composePromptSections(agent.prompt) ? agent.prompt : fallback?.prompt || agent.prompt,
      role: safeString(agent.role) ? agent.role : fallback?.role || "worker",
      skills: agent.skills.length ? agent.skills : fallback?.skills || [],
      toolAllowlist: normalizeWorkspaceToolAllowlist(
        agent.toolAllowlist.length
          ? agent.toolAllowlist
          : fallback?.toolAllowlist || ["read", "write", "glob"]
      ),
      toolDenylist: agent.toolDenylist.length ? agent.toolDenylist : fallback?.toolDenylist || [],
      mcpAllowedServers: agent.mcpAllowedServers.length
        ? agent.mcpAllowedServers
        : fallback?.mcpAllowedServers || [],
      modelProvider: safeString(agent.modelProvider) || fallback?.modelProvider || "",
      modelId: safeString(agent.modelId) || fallback?.modelId || "",
      avatarUrl: safeString(agent.avatarUrl) || fallback?.avatarUrl || "",
      displayName: safeString(agent.displayName) || fallback?.displayName || agent.agentId,
    };
  });

  if (!repairedAgentIds.length && !missingNodeAgentIds.length) {
    return { draft, repairState: null };
  }

  return {
    draft: {
      ...draft,
      agents: repairedAgents,
    },
    repairState: {
      repairedAgentIds,
      repairedTemplateIds,
      missingNodeAgentIds,
      reason: "load",
      requiresSave: repairedAgentIds.length > 0,
    },
  };
}

export function analyzeAutomationTemplateHealth(
  automation: any,
  templateMap: Map<string, ReturnType<typeof normalizeTemplateRow>>
) {
  const agents = Array.isArray(automation?.agents) ? automation.agents : [];
  const missingTemplateLinks = agents
    .map((agent: any) => ({
      agentId: safeString(agent?.agent_id || agent?.agentId),
      templateId: safeString(agent?.template_id || agent?.templateId),
    }))
    .filter((row) => row.templateId && !templateMap.has(row.templateId));
  return {
    missingTemplateLinks,
    isBroken: missingTemplateLinks.length > 0,
  };
}

export function preflightDraft(
  draft: StudioWorkflowDraft,
  templateMap: Map<string, ReturnType<typeof normalizeTemplateRow>>
) {
  const errors: string[] = [];
  const workspaceError = validateWorkspaceRootInput(draft.workspaceRoot);
  if (workspaceError) errors.push(workspaceError);
  if (!safeString(draft.name)) errors.push("Workflow name is required.");
  if (!draft.agents.length) errors.push("Add at least one agent.");
  if (!draft.nodes.length) errors.push("Add at least one stage.");
  const missingNodeAgentIds = draft.nodes
    .map((node) => node.agentId)
    .filter(
      (agentId, index, all) =>
        !!agentId &&
        all.indexOf(agentId) === index &&
        !draft.agents.some((agent) => agent.agentId === agentId)
    );
  if (missingNodeAgentIds.length) {
    errors.push(`Stages reference missing agents: ${missingNodeAgentIds.join(", ")}.`);
  }
  const brokenAgentLinks = draft.agents
    .map((agent) => ({
      agentId: agent.agentId,
      templateId: safeString(agent.linkedTemplateId || agent.templateId),
    }))
    .filter((row) => row.templateId && !templateMap.has(row.templateId));
  return { errors, missingNodeAgentIds, brokenAgentLinks };
}

export function draftFromAutomation(
  automation: any,
  defaultWorkspace: string,
  templateMap: Map<string, ReturnType<typeof normalizeTemplateRow>>
): { draft: StudioWorkflowDraft; repairState: StudioRepairState | null } {
  const metadata =
    automation?.metadata && typeof automation.metadata === "object" ? automation.metadata : {};
  const studio = metadata?.studio && typeof metadata.studio === "object" ? metadata.studio : {};
  const workflowMeta =
    studio?.workflow && typeof studio.workflow === "object" ? studio.workflow : {};
  const starterTemplateId = safeString(studio?.template_id || studio?.templateId);
  const studioAgents = Array.isArray(studio?.agent_drafts)
    ? studio.agent_drafts.map(normalizeAgentDraft)
    : [];
  const studioNodes = Array.isArray(studio?.node_drafts)
    ? studio.node_drafts.map(normalizeNodeDraft)
    : [];
  const automationAgents = Array.isArray(automation?.agents)
    ? automation.agents.map((agent: any) => {
        const templateId = safeString(agent?.template_id || agent?.templateId);
        const linked = templateId ? templateMap.get(templateId) : null;
        const starterFallback =
          !linked && starterTemplateId
            ? defaultAgentFromStarter(
                starterTemplateId,
                safeString(agent?.agent_id || agent?.agentId),
                safeString(
                  automation?.workspace_root ||
                    automation?.workspaceRoot ||
                    metadata?.workspace_root ||
                    defaultWorkspace
                )
              )
            : null;
        return normalizeAgentDraft({
          agentId: agent?.agent_id || agent?.agentId,
          displayName:
            agent?.display_name ||
            agent?.displayName ||
            starterFallback?.displayName ||
            agent?.agent_id ||
            agent?.agentId,
          role: linked?.role || starterFallback?.role || "worker",
          templateId,
          linkedTemplateId: templateId,
          skills: Array.isArray(agent?.skills)
            ? agent.skills
            : linked?.skills || starterFallback?.skills || [],
          prompt: linked?.systemPrompt
            ? promptSectionsFromFreeform(linked.systemPrompt)
            : starterFallback?.prompt,
          modelProvider:
            agent?.model_policy?.default_model?.provider_id ||
            agent?.modelPolicy?.defaultModel?.providerId ||
            starterFallback?.modelProvider ||
            linked?.modelProvider ||
            "",
          modelId:
            agent?.model_policy?.default_model?.model_id ||
            agent?.modelPolicy?.defaultModel?.modelId ||
            starterFallback?.modelId ||
            linked?.modelId ||
            "",
          toolAllowlist: agent?.tool_policy?.allowlist || agent?.toolPolicy?.allowlist || [],
          toolDenylist: agent?.tool_policy?.denylist || agent?.toolPolicy?.denylist || [],
          mcpAllowedServers:
            agent?.mcp_policy?.allowed_servers || agent?.mcpPolicy?.allowedServers || [],
        });
      })
    : [];
  const automationNodes = Array.isArray(automation?.flow?.nodes)
    ? automation.flow.nodes.map((node: any) =>
        normalizeNodeDraft({
          nodeId: node?.node_id || node?.nodeId,
          title:
            node?.metadata?.builder?.title ||
            node?.metadata?.studio?.title ||
            node?.objective ||
            node?.node_id ||
            node?.nodeId,
          agentId: node?.agent_id || node?.agentId,
          objective: node?.objective,
          dependsOn: node?.depends_on || node?.dependsOn || [],
          inputRefs: node?.input_refs || node?.inputRefs || [],
          outputKind: node?.output_contract?.kind || node?.outputContract?.kind || "artifact",
          metadata: node?.metadata,
        })
      )
    : [];
  const draft = {
    automationId: safeString(
      automation?.automation_id || automation?.automationId || automation?.id
    ),
    starterTemplateId,
    name: safeString(automation?.name || "Workflow"),
    description: safeString(automation?.description),
    summary: safeString(studio?.summary),
    icon: safeString(studio?.icon),
    workspaceRoot: safeString(
      automation?.workspace_root ||
        automation?.workspaceRoot ||
        metadata?.workspace_root ||
        defaultWorkspace
    ),
    status: (safeString(automation?.status || workflowMeta?.status || "draft") || "draft") as
      | "draft"
      | "active"
      | "paused",
    scheduleType: (safeString(
      automation?.schedule?.type || workflowMeta?.schedule_type || "manual"
    ) || "manual") as "manual" | "cron" | "interval",
    cronExpression: safeString(
      automation?.schedule?.cron_expression ||
        automation?.schedule?.cronExpression ||
        workflowMeta?.cron_expression
    ),
    intervalSeconds: String(
      automation?.schedule?.interval_seconds ||
        automation?.schedule?.intervalSeconds ||
        workflowMeta?.interval_seconds ||
        3600
    ),
    maxParallelAgents: String(
      automation?.execution?.max_parallel_agents ||
        automation?.execution?.maxParallelAgents ||
        workflowMeta?.max_parallel_agents ||
        1
    ),
    useSharedModel: false,
    sharedModelProvider: "",
    sharedModelId: "",
    outputTargets: Array.isArray(automation?.output_targets || automation?.outputTargets)
      ? (automation.output_targets || automation.outputTargets)
          .map((entry: any) => safeString(entry))
          .filter(Boolean)
      : Array.isArray(workflowMeta?.output_targets)
        ? workflowMeta.output_targets.map((entry: any) => safeString(entry)).filter(Boolean)
        : [],
    agents: studioAgents.length ? studioAgents : automationAgents,
    nodes: studioNodes.length ? studioNodes : automationNodes,
  };
  const sharedModel = inferSharedModelFromAgents(draft.agents);
  draft.useSharedModel = sharedModel.useSharedModel;
  draft.sharedModelProvider = sharedModel.provider;
  draft.sharedModelId = sharedModel.model;
  const repaired = repairDraftTemplateLinks(draft, templateMap);
  return repaired;
}

export function buildTemplatePayload(agent: StudioAgentDraft, templateId: string) {
  const payload: Record<string, unknown> = {
    templateID: templateId,
    display_name: safeString(agent.displayName) || templateId,
    avatar_url: safeString(agent.avatarUrl) || undefined,
    role: safeString(agent.role) || "worker",
    system_prompt: composePromptSections(agent.prompt) || undefined,
    skills: agent.skills.map((skill) => ({ id: skill, skill_id: skill, name: skill })),
    default_budget: {},
    capabilities: {},
  };
  if (safeString(agent.modelProvider) && safeString(agent.modelId)) {
    payload.default_model = {
      provider_id: safeString(agent.modelProvider),
      model_id: safeString(agent.modelId),
    };
  }
  return payload;
}

export function buildStudioMetadata(
  draft: StudioWorkflowDraft,
  nodes: StudioNodeDraft[],
  repairState?: StudioRepairState | null
) {
  const depths = computeNodeDepths(nodes);
  return {
    version: 2,
    template_id: safeString(draft.starterTemplateId),
    workflow_structure_version: 2,
    summary: safeString(draft.summary),
    icon: safeString(draft.icon),
    created_from: "studio",
    last_saved_at_ms: Date.now(),
    workflow: {
      status: safeString(draft.status) || "draft",
      schedule_type: safeString(draft.scheduleType) || "manual",
      cron_expression: safeString(draft.cronExpression),
      interval_seconds: Math.max(
        60,
        Number.parseInt(String(draft.intervalSeconds || "3600"), 10) || 3600
      ),
      output_targets: draft.outputTargets,
      max_parallel_agents: Math.max(
        1,
        Number.parseInt(String(draft.maxParallelAgents || "1"), 10) || 1
      ),
    },
    repair_state: repairState
      ? {
          status:
            repairState.repairedAgentIds.length || repairState.missingNodeAgentIds.length
              ? "repaired"
              : "clean",
          reason: repairState.reason,
          requires_save: repairState.requiresSave,
        }
      : { status: "clean", reason: "", requires_save: false },
    repaired_missing_templates: repairState?.repairedTemplateIds || [],
    repaired_agent_ids: repairState?.repairedAgentIds || [],
    agent_drafts: draft.agents,
    node_drafts: nodes,
    node_layout: Object.fromEntries(Array.from(depths.entries())),
  };
}

export function normalizeNodesForSave(nodes: StudioNodeDraft[]) {
  const idMap = new Map<string, string>();
  const used = new Set<string>();
  for (const node of nodes) {
    const base = titleToId(node.nodeId || node.title, "stage");
    let nextId = base;
    let index = 2;
    while (used.has(nextId)) {
      nextId = `${base}-${index}`;
      index += 1;
    }
    used.add(nextId);
    idMap.set(node.nodeId, nextId);
  }
  return nodes.map((node) => {
    const nextNodeId = idMap.get(node.nodeId) || titleToId(node.nodeId || node.title, "stage");
    const dependsOn = node.dependsOn
      .map((dep) => idMap.get(dep) || titleToId(dep, dep))
      .filter(Boolean);
    const inputRefs = syncInputRefs(
      dependsOn,
      node.inputRefs.map((ref) => ({
        fromStepId: idMap.get(ref.fromStepId) || titleToId(ref.fromStepId, ref.fromStepId),
        alias: ref.alias,
      }))
    );
    return canonicalizeStudioNodeOutputTemplates(
      normalizeNodeWorkflowClassification({
        ...node,
        nodeId: nextNodeId,
        dependsOn,
        inputRefs,
      })
    );
  });
}

export async function confirmAutomationDeleted(
  client: AppPageProps["client"],
  automationId: string,
  attempts = 4
) {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const response = await client.automationsV2.list().catch(() => ({ automations: [] }));
    const rows = toArray(response, "automations");
    const stillExists = rows.some((row: any) => {
      const id = safeString(row?.automation_id || row?.automationId || row?.id);
      return id === automationId;
    });
    if (!stillExists) return;
    await new Promise((resolve) => window.setTimeout(resolve, 250 * (attempt + 1)));
  }
  throw new Error(
    "Delete did not persist on the engine. The workflow is still present after verification."
  );
}
