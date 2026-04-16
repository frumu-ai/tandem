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

const ROLE_OPTIONS: StudioRole[] = [
  "worker",
  "reviewer",
  "tester",
  "watcher",
  "delegator",
  "committer",
  "orchestrator",
];

type StudioRepairState = {
  repairedAgentIds: string[];
  repairedTemplateIds: string[];
  missingNodeAgentIds: string[];
  reason: "load" | "run_preflight" | "save";
  requiresSave: boolean;
};

type ProviderOption = {
  id: string;
  models: string[];
};

const AUTOMATIONS_STUDIO_HANDOFF_KEY = "tandem.automations.studioHandoff";
const AGENT_CATALOG_HANDOFF_KEY = "tandem.studio.agentCatalogHandoff";

type AgentCatalogHandoff = {
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

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function safeString(value: unknown) {
  return String(value || "").trim();
}

function normalizeStudioRole(value: unknown): StudioRole {
  const role = safeString(value) as StudioRole;
  return ROLE_OPTIONS.includes(role) ? role : "worker";
}

function createAgentDraftFromCatalog(entry: AgentCatalogHandoff): StudioAgentDraft {
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

function isCodeLikeOutputPath(value: string) {
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

function outputPathExtension(value: string) {
  const normalized = safeString(value).toLowerCase();
  return normalized.includes(".") ? normalized.split(".").pop() || "" : "";
}

function isCodeLikeTaskKind(value: string) {
  const normalized = safeString(value).toLowerCase();
  return (
    normalized === "code_change" || normalized === "repo_fix" || normalized === "implementation"
  );
}

function nodeHasCodeWorkflowMetadata(node: StudioNodeDraft) {
  return Boolean(
    node.projectBacklogTasks ||
    safeString(node.repoRoot) ||
    safeString(node.writeScope) ||
    safeString(node.verificationCommand)
  );
}

function inferredArtifactOutputKind(node: StudioNodeDraft) {
  const ext = outputPathExtension(node.outputPath);
  if (ext === "md" || ext === "markdown") return "report_markdown";
  if (ext === "json") return "structured_json";
  if (ext === "txt") return "text_summary";
  return "artifact";
}

function normalizeNodeWorkflowClassification(node: StudioNodeDraft): StudioNodeDraft {
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

function isCodeLikeNode(node: StudioNodeDraft) {
  const normalizedNode = normalizeNodeWorkflowClassification(node);
  const taskKind = safeString(normalizedNode.taskKind).toLowerCase();
  return (
    isCodeLikeTaskKind(taskKind) ||
    safeString(normalizedNode.outputKind).toLowerCase() === "code_patch" ||
    isCodeLikeOutputPath(normalizedNode.outputPath)
  );
}

function isBacklogProjectingNode(node: StudioNodeDraft) {
  return !!node.projectBacklogTasks;
}

function normalizeWorkspaceToolAllowlist(values: string[]) {
  const normalized = values.map((entry) => safeString(entry)).filter(Boolean);
  if (normalized.includes("*")) return ["*"];
  return Array.from(new Set(normalized));
}

function normalizeNodeAwareToolAllowlist(values: string[], nodes: StudioNodeDraft[]) {
  const normalized = normalizeWorkspaceToolAllowlist(values);
  if (normalized.includes("*")) return ["*"];
  const hasCodeNode = nodes.some((node) => isCodeLikeNode(node));
  const required = hasCodeNode ? ["read", "glob", "edit", "apply_patch", "write", "bash"] : [];
  return Array.from(new Set([...normalized, ...required]));
}

function shortId(value: unknown) {
  const text = safeString(value);
  if (!text) return "";
  if (text.length <= 18) return text;
  return `${text.slice(0, 8)}...${text.slice(-6)}`;
}

function timestampLabel(value: unknown) {
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

function splitCsv(value: string) {
  return String(value || "")
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function joinCsv(values: string[]) {
  return Array.isArray(values) ? values.join(", ") : "";
}

const STUDIO_OUTPUT_TOKEN_GUIDE = [
  "{current_date}",
  "{current_time}",
  "{current_timestamp}",
  "{current_timestamp_filename}",
];

function studioRuntimePreviewValues(now = new Date()) {
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

function canonicalizeStudioOutputPathTemplate(value: string) {
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

function resolveStudioOutputPathTemplate(value: string, now = new Date()) {
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

function studioOutputPathWarning(value: string) {
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

function canonicalizeStudioNodeOutputTemplates(node: StudioNodeDraft): StudioNodeDraft {
  return {
    ...node,
    outputPath: canonicalizeStudioOutputPathTemplate(node.outputPath),
    outputFiles: normalizeStringList(node.outputFiles).map(canonicalizeStudioOutputPathTemplate),
  };
}

function canonicalizeStudioDraftOutputTemplates(draft: StudioWorkflowDraft): StudioWorkflowDraft {
  return {
    ...draft,
    outputTargets: normalizeStringList(draft.outputTargets).map(
      canonicalizeStudioOutputPathTemplate
    ),
    nodes: draft.nodes.map(canonicalizeStudioNodeOutputTemplates),
  };
}

function collectStudioOutputPathWarnings(draft: StudioWorkflowDraft) {
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

function normalizeStringList(values: unknown) {
  if (!Array.isArray(values)) return [];
  return values
    .map((entry) => safeString(entry))
    .filter(Boolean)
    .filter((value, index, all) => all.indexOf(value) === index);
}

function effectiveNodeOutputFiles(node: StudioNodeDraft) {
  const explicit = normalizeStringList(node.outputFiles);
  if (explicit.length) return explicit;
  const outputPath = safeString(node.outputPath);
  return outputPath ? [outputPath] : [];
}

function effectiveNodeInputFiles(node: StudioNodeDraft, nodes: StudioNodeDraft[]) {
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

function slugify(value: string) {
  return String(value || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
}

function titleToId(value: string, fallback: string) {
  return slugify(value) || fallback;
}

function validateWorkspaceRootInput(raw: string) {
  const value = safeString(raw);
  if (!value) return "Workspace root is required.";
  if (!value.startsWith("/")) return "Workspace root must be an absolute path.";
  return "";
}

function seedAutomationsStudioHandoff(payload: {
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

function composePromptSections(prompt: StudioPromptSections) {
  const sections = [
    ["Role", prompt.role],
    ["Mission", prompt.mission],
    ["Inputs", prompt.inputs],
    ["Output Contract", prompt.outputContract],
    ["Guardrails", prompt.guardrails],
  ].filter(([, text]) => safeString(text));
  return sections.map(([label, text]) => `${label}:\n${String(text).trim()}`).join("\n\n");
}

function buildModelPolicy(agent: StudioAgentDraft) {
  if (!safeString(agent.modelProvider) || !safeString(agent.modelId)) return undefined;
  return {
    default_model: {
      provider_id: safeString(agent.modelProvider),
      model_id: safeString(agent.modelId),
    },
  };
}

function resolveDefaultModel(
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

function modelsForProvider(providerOptions: ProviderOption[], providerId: string) {
  return providerOptions.find((entry) => entry.id === providerId)?.models || [];
}

function applyDefaultModelToAgents(
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

function inferSharedModelFromAgents(agents: StudioAgentDraft[]) {
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

function applySharedModelToAgents(agents: StudioAgentDraft[], provider: string, model: string) {
  const normalizedProvider = safeString(provider);
  const normalizedModel = safeString(model);
  if (!normalizedProvider || !normalizedModel) return agents;
  return agents.map((agent) => ({
    ...agent,
    modelProvider: normalizedProvider,
    modelId: normalizedModel,
  }));
}

function buildSchedulePayload(draft: StudioWorkflowDraft) {
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

function extractMcpServers(raw: any) {
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

function normalizeTemplateRow(row: any) {
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

function promptSectionsFromFreeform(text: string): StudioPromptSections {
  return {
    role: "",
    mission: safeString(text),
    inputs: "",
    outputContract: "",
    guardrails: "",
  };
}

function syncInputRefs(dependsOn: string[], refs: Array<{ fromStepId: string; alias: string }>) {
  const nextRefs = dependsOn.map((depId) => {
    const existing = refs.find((ref) => ref.fromStepId === depId);
    return existing || { fromStepId: depId, alias: depId.replace(/-/g, "_") };
  });
  return nextRefs;
}

function computeNodeDepths(nodes: StudioNodeDraft[]) {
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

function normalizeAgentDraft(row: any): StudioAgentDraft {
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

function normalizeNodeDraft(row: any): StudioNodeDraft {
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

function composeNodeExecutionPrompt(node: StudioNodeDraft, agent: StudioAgentDraft) {
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

function defaultAgentFromStarter(
  starterTemplateId: string,
  agentId: string,
  workspaceRoot: string
): StudioAgentDraft | null {
  const template = STUDIO_TEMPLATE_CATALOG.find((entry) => entry.id === starterTemplateId);
  if (!template) return null;
  const starterDraft = createWorkflowDraftFromTemplate(template, workspaceRoot || "");
  return starterDraft.agents.find((agent) => agent.agentId === agentId) || null;
}

function repairDraftTemplateLinks(
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

function analyzeAutomationTemplateHealth(
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

function preflightDraft(
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

function draftFromAutomation(
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

function buildTemplatePayload(agent: StudioAgentDraft, templateId: string) {
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

function buildStudioMetadata(
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

function normalizeNodesForSave(nodes: StudioNodeDraft[]) {
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

async function confirmAutomationDeleted(
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

export function WorkflowStudioPage({ client, api, toast, navigate }: AppPageProps) {
  const queryClient = useQueryClient();
  const healthQuery = useSystemHealth(true);
  const automationsQuery = useQuery({
    queryKey: ["studio", "automations"],
    queryFn: () => client.automationsV2.list().catch(() => ({ automations: [] })),
    refetchInterval: 15000,
  });
  const templatesQuery = useQuery({
    queryKey: ["studio", "templates"],
    queryFn: () => client.agentTeams.listTemplates().catch(() => ({ templates: [] })),
    refetchInterval: 10000,
  });
  const providersCatalogQuery = useQuery({
    queryKey: ["studio", "providers", "catalog"],
    queryFn: () => client.providers.catalog().catch(() => ({ all: [] })),
    refetchInterval: 30000,
  });
  const providersConfigQuery = useQuery({
    queryKey: ["studio", "providers", "config"],
    queryFn: () => client.providers.config().catch(() => ({})),
    refetchInterval: 30000,
  });
  const mcpQuery = useQuery({
    queryKey: ["studio", "mcp"],
    queryFn: () => client.mcp.list().catch(() => ({})),
    refetchInterval: 15000,
  });
  const defaultWorkspaceRoot = safeString(
    (healthQuery.data as any)?.workspaceRoot || (healthQuery.data as any)?.workspace_root
  );
  const templateRows = useMemo(
    () =>
      toArray(templatesQuery.data, "templates")
        .map(normalizeTemplateRow)
        .filter((row) => row.templateId),
    [templatesQuery.data]
  );
  const templateMap = useMemo(
    () => new Map(templateRows.map((row) => [row.templateId, row])),
    [templateRows]
  );
  const providerOptions = useMemo<ProviderOption[]>(() => {
    const rows = Array.isArray((providersCatalogQuery.data as any)?.all)
      ? (providersCatalogQuery.data as any).all
      : [];
    return rows
      .map((provider: any) => ({
        id: safeString(provider?.id),
        models: Object.keys(provider?.models || {}),
      }))
      .filter((provider: ProviderOption) => !!provider.id)
      .sort((a, b) => a.id.localeCompare(b.id));
  }, [providersCatalogQuery.data]);
  const studioDefaultModel = useMemo(
    () => resolveDefaultModel(providerOptions, providersConfigQuery.data),
    [providerOptions, providersConfigQuery.data]
  );
  const studioAutomations = useMemo(
    () =>
      toArray(automationsQuery.data, "automations").filter((row: any) => {
        const studio = row?.metadata?.studio;
        return (
          !!studio &&
          (Number(studio?.version || 0) > 0 || safeString(studio?.created_from) === "studio")
        );
      }),
    [automationsQuery.data]
  );
  const studioWorkflowRunIds = useMemo(
    () =>
      studioAutomations
        .map((automation: any) =>
          safeString(automation?.automation_id || automation?.automationId || automation?.id)
        )
        .filter(Boolean),
    [studioAutomations]
  );
  const studioWorkflowRunsQuery = useQuery({
    queryKey: ["studio", "workflow-runs", studioWorkflowRunIds],
    enabled: !!client?.automationsV2?.listRuns && studioWorkflowRunIds.length > 0,
    queryFn: async () => {
      const results = await Promise.all(
        studioWorkflowRunIds.map(async (automationId) => {
          const response = await client.automationsV2
            .listRuns(automationId, 6)
            .catch(() => ({ runs: [] }));
          return Array.isArray((response as any)?.runs) ? (response as any).runs : [];
        })
      );
      return { runs: results.flat() };
    },
    refetchInterval: 12000,
  });
  const studioWorkflowLatestRuns = useMemo(() => {
    const rows = toArray(studioWorkflowRunsQuery.data, "runs");
    const map = new Map<string, any>();
    rows.forEach((run: any) => {
      const automationId = safeString(run?.automation_id || run?.automationId);
      if (!automationId) return;
      const current = map.get(automationId);
      const currentTime = Number(
        current?.updated_at_ms ||
          current?.updatedAtMs ||
          current?.created_at_ms ||
          current?.createdAtMs ||
          0
      );
      const nextTime = Number(
        run?.updated_at_ms || run?.updatedAtMs || run?.created_at_ms || run?.createdAtMs || 0
      );
      if (!current || nextTime >= currentTime) {
        map.set(automationId, run);
      }
    });
    return map;
  }, [studioWorkflowRunsQuery.data]);
  const mcpServers = useMemo(() => extractMcpServers(mcpQuery.data), [mcpQuery.data]);

  const [draft, setDraft] = useState<StudioWorkflowDraft>(() =>
    createWorkflowDraftFromTemplate(STUDIO_TEMPLATE_CATALOG[0], "")
  );
  const [selectedNodeId, setSelectedNodeId] = useState(
    () => STUDIO_TEMPLATE_CATALOG[0]?.nodes?.[0]?.nodeId || ""
  );
  const [selectedAgentId, setSelectedAgentId] = useState(
    () => STUDIO_TEMPLATE_CATALOG[0]?.agents?.[0]?.agentId || ""
  );
  const [selectedTemplateLoadId, setSelectedTemplateLoadId] = useState("");
  const [saveReusableTemplates, setSaveReusableTemplates] = useState(false);
  const [runAfterSave, setRunAfterSave] = useState(false);
  const [templatesOpen, setTemplatesOpen] = useState(true);
  const [savedWorkflowsOpen, setSavedWorkflowsOpen] = useState(true);
  const [workspaceBrowserOpen, setWorkspaceBrowserOpen] = useState(false);
  const [workspaceBrowserDir, setWorkspaceBrowserDir] = useState("");
  const [workspaceBrowserSearch, setWorkspaceBrowserSearch] = useState("");
  const [deleteConfirm, setDeleteConfirm] = useState<{
    automationId: string;
    title: string;
  } | null>(null);
  const [repairState, setRepairState] = useState<StudioRepairState | null>(null);
  const [catalogAgentHandoff, setCatalogAgentHandoff] = useState<AgentCatalogHandoff | null>(null);

  useEffect(() => {
    if (!defaultWorkspaceRoot) return;
    setDraft((current) =>
      safeString(current.workspaceRoot)
        ? current
        : { ...current, workspaceRoot: defaultWorkspaceRoot }
    );
  }, [defaultWorkspaceRoot]);

  useEffect(() => {
    const raw = sessionStorage.getItem(AGENT_CATALOG_HANDOFF_KEY);
    if (!raw) return;
    sessionStorage.removeItem(AGENT_CATALOG_HANDOFF_KEY);
    try {
      const parsed = JSON.parse(raw) as Partial<AgentCatalogHandoff>;
      const agentId = safeString(parsed.agentId);
      if (!agentId) return;
      setCatalogAgentHandoff({
        agentId,
        displayName: safeString(parsed.displayName) || agentId,
        categoryId: safeString(parsed.categoryId),
        categoryTitle: safeString(parsed.categoryTitle),
        summary: safeString(parsed.summary),
        sourcePath: safeString(parsed.sourcePath),
        sandboxMode: safeString(parsed.sandboxMode),
        role: normalizeStudioRole(parsed.role),
        tags: Array.isArray(parsed.tags)
          ? parsed.tags.map((tag) => safeString(tag)).filter(Boolean)
          : [],
        requires: Array.isArray(parsed.requires)
          ? parsed.requires.map((req) => safeString(req)).filter(Boolean)
          : [],
        instructions: safeString(parsed.instructions),
      });
    } catch (error) {
      console.warn("Failed to parse catalog agent handoff:", error);
    }
  }, []);

  useEffect(() => {
    if (!studioDefaultModel.provider || !studioDefaultModel.model) return;
    setDraft((current) => {
      const nextAgents = applyDefaultModelToAgents(current.agents, studioDefaultModel);
      const changed = nextAgents.some(
        (agent, index) =>
          agent.modelProvider !== current.agents[index]?.modelProvider ||
          agent.modelId !== current.agents[index]?.modelId
      );
      return changed ? { ...current, agents: nextAgents } : current;
    });
  }, [studioDefaultModel]);

  useEffect(() => {
    if (!draft.useSharedModel) return;
    if (!safeString(draft.sharedModelProvider) || !safeString(draft.sharedModelId)) return;
    setDraft((current) => {
      if (!current.useSharedModel) return current;
      const nextAgents = applySharedModelToAgents(
        current.agents,
        current.sharedModelProvider,
        current.sharedModelId
      );
      const changed = nextAgents.some(
        (agent, index) =>
          agent.modelProvider !== current.agents[index]?.modelProvider ||
          agent.modelId !== current.agents[index]?.modelId
      );
      return changed ? { ...current, agents: nextAgents } : current;
    });
  }, [draft.useSharedModel, draft.sharedModelProvider, draft.sharedModelId]);

  useEffect(() => {
    const nodeIds = new Set(draft.nodes.map((node) => node.nodeId));
    if (!nodeIds.has(selectedNodeId)) {
      setSelectedNodeId(draft.nodes[0]?.nodeId || "");
    }
  }, [draft.nodes, selectedNodeId]);

  useEffect(() => {
    const agentIds = new Set(draft.agents.map((agent) => agent.agentId));
    if (!agentIds.has(selectedAgentId)) {
      setSelectedAgentId(draft.agents[0]?.agentId || "");
    }
  }, [draft.agents, selectedAgentId]);

  useEffect(() => {
    if (!catalogAgentHandoff) return;
    const agentId = safeString(catalogAgentHandoff.agentId);
    if (!agentId) {
      setCatalogAgentHandoff(null);
      return;
    }

    const targetNodeId =
      draft.nodes.find((node) => node.nodeId === selectedNodeId)?.nodeId ||
      draft.nodes.find((node) => node.agentId === agentId)?.nodeId ||
      draft.nodes[0]?.nodeId ||
      "";

    setDraft((current) => {
      const existingAgent = current.agents.find((agent) => agent.agentId === agentId);
      const nextAgents = existingAgent
        ? current.agents.map((agent) =>
            agent.agentId === agentId
              ? { ...agent, role: normalizeStudioRole(catalogAgentHandoff.role) }
              : agent
          )
        : [...current.agents, createAgentDraftFromCatalog(catalogAgentHandoff)];

      const nextNodes = targetNodeId
        ? current.nodes.map((node) => (node.nodeId === targetNodeId ? { ...node, agentId } : node))
        : current.nodes;

      return {
        ...current,
        agents: nextAgents,
        nodes: nextNodes,
      };
    });
    if (targetNodeId) setSelectedNodeId(targetNodeId);
    setSelectedAgentId(agentId);
    setRepairState(null);
    toast(
      "ok",
      `Seeded ${catalogAgentHandoff.displayName} into Studio and bound it to a workflow stage.`
    );
    setCatalogAgentHandoff(null);
  }, [catalogAgentHandoff, draft.nodes, selectedNodeId, toast]);

  useEffect(() => {
    try {
      renderIcons();
    } catch {}
  }, [
    draft,
    selectedNodeId,
    selectedTemplateLoadId,
    templatesOpen,
    savedWorkflowsOpen,
    studioAutomations.length,
    templateRows.length,
    workspaceBrowserOpen,
    workspaceBrowserSearch,
  ]);

  const workspaceBrowserQuery = useQuery({
    queryKey: ["studio", "workspace-browser", workspaceBrowserDir],
    enabled: workspaceBrowserOpen && !!workspaceBrowserDir,
    queryFn: () =>
      api(`/api/orchestrator/workspaces/list?dir=${encodeURIComponent(workspaceBrowserDir)}`, {
        method: "GET",
      }),
    refetchInterval: workspaceBrowserOpen ? 15000 : false,
  });

  const selectedNode =
    draft.nodes.find((node) => node.nodeId === selectedNodeId) || draft.nodes[0] || null;
  const selectedNodeInputFiles = selectedNode
    ? effectiveNodeInputFiles(selectedNode, draft.nodes)
    : [];
  const selectedNodeOutputFiles = selectedNode ? effectiveNodeOutputFiles(selectedNode) : [];
  const canonicalDraftOutputPreview = useMemo(
    () => canonicalizeStudioDraftOutputTemplates(draft),
    [draft]
  );
  const outputPathWarnings = useMemo(() => collectStudioOutputPathWarnings(draft), [draft]);
  const outputPathPreviewRows = useMemo(() => {
    const now = new Date();
    const rows: Array<{
      id: string;
      label: string;
      raw: string;
      canonical: string;
      resolved: string;
      warning: string;
    }> = [];
    canonicalDraftOutputPreview.outputTargets.forEach((target, index) => {
      const raw = safeString(draft.outputTargets[index] || target);
      rows.push({
        id: `target-${index}`,
        label: `Workflow target ${index + 1}`,
        raw,
        canonical: target,
        resolved: resolveStudioOutputPathTemplate(target, now),
        warning: studioOutputPathWarning(raw),
      });
    });
    canonicalDraftOutputPreview.nodes.forEach((node, index) => {
      const raw = safeString(draft.nodes[index]?.outputPath || node.outputPath);
      if (!safeString(node.outputPath) && !raw) return;
      rows.push({
        id: `node-${node.nodeId}`,
        label: `${safeString(node.title) || safeString(node.nodeId) || `Stage ${index + 1}`}`,
        raw: raw || safeString(node.outputPath),
        canonical: safeString(node.outputPath),
        resolved: resolveStudioOutputPathTemplate(safeString(node.outputPath), now),
        warning: studioOutputPathWarning(raw || safeString(node.outputPath)),
      });
    });
    return rows.filter((row) => row.raw || row.canonical);
  }, [canonicalDraftOutputPreview, draft]);
  const selectedNodeOutputPathPreview = useMemo(() => {
    if (!selectedNode) return null;
    const raw = safeString(selectedNode.outputPath);
    const canonical = canonicalizeStudioOutputPathTemplate(raw);
    if (!raw && !canonical) return null;
    return {
      raw,
      canonical,
      resolved: resolveStudioOutputPathTemplate(canonical || raw),
      warning: studioOutputPathWarning(raw),
    };
  }, [selectedNode]);
  const selectedAgent =
    draft.agents.find((agent) => agent.agentId === (selectedAgentId || selectedNode?.agentId)) ||
    draft.agents.find((agent) => agent.agentId === selectedNode?.agentId) ||
    draft.agents[0] ||
    null;
  const workspaceRootError = validateWorkspaceRootInput(draft.workspaceRoot);
  const workspaceDirectories = Array.isArray((workspaceBrowserQuery.data as any)?.directories)
    ? (workspaceBrowserQuery.data as any).directories
    : [];
  const workspaceParentDir = safeString((workspaceBrowserQuery.data as any)?.parent);
  const workspaceCurrentBrowseDir = safeString(
    (workspaceBrowserQuery.data as any)?.dir || workspaceBrowserDir || ""
  );
  const filteredWorkspaceDirectories = useMemo(() => {
    const search = safeString(workspaceBrowserSearch).toLowerCase();
    if (!search) return workspaceDirectories;
    return workspaceDirectories.filter((entry: any) =>
      safeString(entry?.name || entry?.path)
        .toLowerCase()
        .includes(search)
    );
  }, [workspaceBrowserSearch, workspaceDirectories]);
  const nodeDepths = useMemo(() => computeNodeDepths(draft.nodes), [draft.nodes]);
  const graphColumns = useMemo(() => {
    const columns = new Map<number, StudioNodeDraft[]>();
    for (const node of draft.nodes) {
      const depth = Number(nodeDepths.get(node.nodeId) || 0);
      if (!columns.has(depth)) columns.set(depth, []);
      columns.get(depth)?.push(node);
    }
    return Array.from(columns.entries()).sort((a, b) => a[0] - b[0]);
  }, [draft.nodes, nodeDepths]);

  const applyTemplate = (templateId: string) => {
    const template =
      STUDIO_TEMPLATE_CATALOG.find((entry) => entry.id === templateId) ||
      STUDIO_TEMPLATE_CATALOG[0];
    const nextDraft = createWorkflowDraftFromTemplate(
      template,
      draft.workspaceRoot || defaultWorkspaceRoot
    );
    setDraft({
      ...nextDraft,
      agents: applyDefaultModelToAgents(nextDraft.agents, studioDefaultModel),
    });
    setSelectedNodeId(nextDraft.nodes[0]?.nodeId || "");
    setSelectedAgentId(nextDraft.agents[0]?.agentId || "");
    setSaveReusableTemplates(false);
    setRepairState(null);
  };

  const updateDraft = (patch: Partial<StudioWorkflowDraft>) =>
    setDraft((current) => ({ ...current, ...patch }));

  const updateNode = (nodeId: string, patch: Partial<StudioNodeDraft>) =>
    setDraft((current) => ({
      ...current,
      nodes: current.nodes.map((node) =>
        node.nodeId === nodeId
          ? {
              ...node,
              ...patch,
              inputRefs:
                patch.dependsOn || patch.inputRefs
                  ? syncInputRefs(
                      patch.dependsOn ? [...patch.dependsOn] : node.dependsOn,
                      patch.inputRefs ? [...patch.inputRefs] : node.inputRefs
                    )
                  : node.inputRefs,
            }
          : node
      ),
    }));

  const updateAgent = (agentId: string, patch: Partial<StudioAgentDraft>) =>
    setDraft((current) => ({
      ...current,
      agents: current.agents.map((agent) =>
        agent.agentId === agentId ? { ...agent, ...patch } : agent
      ),
    }));

  const addAgent = () => {
    const nextId = `agent-${draft.agents.length + 1}`;
    const uniqueId = draft.agents.some((agent) => agent.agentId === nextId)
      ? `agent-${Date.now().toString().slice(-5)}`
      : nextId;
    setDraft((current) => ({
      ...current,
      agents: [
        ...current.agents,
        applyDefaultModelToAgents(
          [createEmptyAgentDraft(uniqueId, `Agent ${current.agents.length + 1}`)],
          studioDefaultModel
        )[0],
      ],
    }));
    setSelectedAgentId(uniqueId);
  };

  const addNode = () => {
    const nextIndex = draft.nodes.length + 1;
    const fallbackId = `stage-${nextIndex}`;
    const agentId = selectedAgent?.agentId || draft.agents[0]?.agentId || "";
    const nextNode: StudioNodeDraft = createEmptyNodeDraft(
      fallbackId,
      `Stage ${nextIndex}`,
      agentId,
      selectedNode ? [selectedNode.nodeId] : [],
      selectedNode
        ? [{ fromStepId: selectedNode.nodeId, alias: selectedNode.nodeId.replace(/-/g, "_") }]
        : [],
      {
        objective: "Describe what this stage should produce.",
      }
    );
    setDraft((current) => ({ ...current, nodes: [...current.nodes, nextNode] }));
    setSelectedNodeId(fallbackId);
    if (agentId) setSelectedAgentId(agentId);
  };

  const removeSelectedNode = () => {
    if (!selectedNode) return;
    setDraft((current) => ({
      ...current,
      nodes: current.nodes
        .filter((node) => node.nodeId !== selectedNode.nodeId)
        .map((node) => {
          const dependsOn = node.dependsOn.filter((dep) => dep !== selectedNode.nodeId);
          return {
            ...node,
            dependsOn,
            inputRefs: syncInputRefs(
              dependsOn,
              node.inputRefs.filter((ref) => ref.fromStepId !== selectedNode.nodeId)
            ),
          };
        }),
    }));
  };

  const removeSelectedAgent = () => {
    if (!selectedAgent) return;
    const isUsed = draft.nodes.some((node) => node.agentId === selectedAgent.agentId);
    if (isUsed) {
      toast("warn", "Reassign or remove the stages using this agent first.");
      return;
    }
    setDraft((current) => ({
      ...current,
      agents: current.agents.filter((agent) => agent.agentId !== selectedAgent.agentId),
    }));
    setSelectedAgentId("");
  };

  const loadTemplateIntoSelectedAgent = () => {
    if (!selectedAgent || !selectedTemplateLoadId) return;
    const linked = templateMap.get(selectedTemplateLoadId);
    if (!linked) return;
    updateAgent(selectedAgent.agentId, {
      displayName: linked.displayName || selectedAgent.displayName,
      avatarUrl: linked.avatarUrl || selectedAgent.avatarUrl,
      role: linked.role || selectedAgent.role,
      linkedTemplateId: linked.templateId,
      templateId: linked.templateId,
      prompt: linked.systemPrompt
        ? promptSectionsFromFreeform(linked.systemPrompt)
        : selectedAgent.prompt,
      modelProvider: linked.modelProvider || selectedAgent.modelProvider,
      modelId: linked.modelId || selectedAgent.modelId,
      skills: linked.skills.length ? linked.skills : selectedAgent.skills,
    });
    setRepairState((current) =>
      current
        ? {
            ...current,
            repairedAgentIds: current.repairedAgentIds.filter(
              (agentId) => agentId !== selectedAgent.agentId
            ),
            repairedTemplateIds: current.repairedTemplateIds.filter(
              (templateId) => templateId !== linked.templateId
            ),
          }
        : current
    );
    toast("ok", `Loaded template ${linked.templateId}.`);
  };

  const applyRepairToCurrentDraft = (reason: StudioRepairState["reason"] = "run_preflight") => {
    const repaired = repairDraftTemplateLinks(draft, templateMap);
    if (!repaired.repairState) return { repaired: false, draft: repaired.draft };
    const nextRepairState = { ...repaired.repairState, reason, requiresSave: true };
    setDraft(repaired.draft);
    setRepairState(nextRepairState);
    setSelectedNodeId(repaired.draft.nodes[0]?.nodeId || "");
    setSelectedAgentId(repaired.draft.agents[0]?.agentId || "");
    return { repaired: true, draft: repaired.draft, repairState: nextRepairState };
  };

  const openAutomationInStudio = (automation: any) => {
    const loaded = draftFromAutomation(automation, defaultWorkspaceRoot, templateMap);
    const hydratedDraft = {
      ...loaded.draft,
      agents: applyDefaultModelToAgents(loaded.draft.agents, studioDefaultModel),
    };
    setDraft(hydratedDraft);
    setSelectedNodeId(hydratedDraft.nodes[0]?.nodeId || "");
    setSelectedAgentId(hydratedDraft.agents[0]?.agentId || "");
    setSaveReusableTemplates(
      loaded.draft.agents.some((agent) => !!safeString(agent.linkedTemplateId || agent.templateId))
    );
    setRepairState(
      loaded.repairState
        ? {
            ...loaded.repairState,
            reason: "load",
            requiresSave: true,
          }
        : null
    );
    if (loaded.repairState?.repairedAgentIds.length) {
      toast(
        "warn",
        `Repaired missing template links for: ${loaded.repairState.repairedAgentIds.join(", ")}. Save the workflow before running it.`
      );
    }
    navigate("studio");
  };

  const runSavedAutomation = async (automation: any) => {
    const automationId = safeString(
      automation?.automation_id || automation?.automationId || automation?.id
    );
    const health = analyzeAutomationTemplateHealth(automation, templateMap);
    if (health.isBroken) {
      openAutomationInStudio(automation);
      toast(
        "warn",
        "This workflow references missing agent templates. It was repaired in Studio. Save it before running."
      );
      return;
    }
    const result = await client.automationsV2.runNow(automationId);
    const runId = safeString((result as any)?.run?.run_id || (result as any)?.run?.runId);
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["studio", "automations"] }),
      queryClient.invalidateQueries({ queryKey: ["automations"] }),
      queryClient.invalidateQueries({ queryKey: ["automations", "runs"] }),
      queryClient.invalidateQueries({ queryKey: ["automations", "v2", "runs"] }),
    ]);
    toast(
      "ok",
      runId
        ? `Workflow run started: ${runId}. Opening Automations.`
        : "Workflow run started. Opening Automations."
    );
    seedAutomationsStudioHandoff({
      tab: "running",
      runId,
      automationId,
      openTaskInspector: true,
    });
    navigate("automations");
  };

  const saveMutation = useMutation({
    mutationFn: async () => {
      const repairedDraft = !saveReusableTemplates
        ? repairDraftTemplateLinks(draft, templateMap).draft
        : draft;
      let workingDraft = {
        ...repairedDraft,
        agents: applyDefaultModelToAgents(repairedDraft.agents, studioDefaultModel),
      };
      if (
        workingDraft.useSharedModel &&
        safeString(workingDraft.sharedModelProvider) &&
        safeString(workingDraft.sharedModelId)
      ) {
        workingDraft = {
          ...workingDraft,
          agents: applySharedModelToAgents(
            workingDraft.agents,
            workingDraft.sharedModelProvider,
            workingDraft.sharedModelId
          ),
        };
      }
      const workspaceError = validateWorkspaceRootInput(workingDraft.workspaceRoot);
      if (workspaceError) throw new Error(workspaceError);
      if (!safeString(workingDraft.name)) throw new Error("Workflow name is required.");
      if (!workingDraft.agents.length) throw new Error("Add at least one agent.");
      if (!workingDraft.nodes.length) throw new Error("Add at least one stage.");
      const preflight = preflightDraft(workingDraft, templateMap);
      if (preflight.errors.length) throw new Error(preflight.errors[0]);
      const modelMissingAgents = workingDraft.agents
        .filter((agent) => !safeString(agent.modelProvider) || !safeString(agent.modelId))
        .map((agent) => agent.displayName || agent.agentId);
      if (modelMissingAgents.length) {
        throw new Error(
          `Model selection is required. Set a default provider/model in Settings or choose one for: ${modelMissingAgents.join(", ")}.`
        );
      }
      const unresolvedOutputWarnings = collectStudioOutputPathWarnings(workingDraft);
      if (unresolvedOutputWarnings.length) {
        toast(
          "warn",
          `${unresolvedOutputWarnings[0]} Use the output preview to confirm the saved path before you run this workflow.`
        );
      }
      workingDraft = canonicalizeStudioDraftOutputTemplates(workingDraft);
      const workflowSlug = slugify(workingDraft.name) || "studio-workflow";
      const linkedTemplateIds = new Map<string, string>();
      if (saveReusableTemplates) {
        for (const agent of workingDraft.agents) {
          const desiredTemplateId =
            safeString(agent.linkedTemplateId) ||
            safeString(agent.templateId) ||
            `${workflowSlug}--${slugify(agent.agentId) || "agent"}`;
          linkedTemplateIds.set(agent.agentId, desiredTemplateId);
          if (templateMap.has(desiredTemplateId)) {
            await client.agentTeams.updateTemplate(desiredTemplateId, {
              display_name: safeString(agent.displayName) || desiredTemplateId,
              avatar_url: safeString(agent.avatarUrl) || undefined,
              role: safeString(agent.role) || "worker",
              system_prompt: composePromptSections(agent.prompt) || undefined,
              default_model:
                safeString(agent.modelProvider) && safeString(agent.modelId)
                  ? {
                      provider_id: safeString(agent.modelProvider),
                      model_id: safeString(agent.modelId),
                    }
                  : undefined,
              skills: agent.skills.map((skill) => ({ id: skill, skill_id: skill, name: skill })),
            } as any);
          } else {
            await client.agentTeams.createTemplate({
              template: buildTemplatePayload(agent, desiredTemplateId),
            } as any);
          }
        }
      }
      const normalizedNodes = normalizeNodesForSave(workingDraft.nodes);
      const automationPayload = {
        name: safeString(workingDraft.name),
        description: safeString(workingDraft.description) || undefined,
        status: workingDraft.status,
        schedule: buildSchedulePayload(workingDraft),
        workspace_root: safeString(workingDraft.workspaceRoot),
        execution: {
          max_parallel_agents: Math.max(
            1,
            Number.parseInt(String(workingDraft.maxParallelAgents || "1"), 10) || 1
          ),
        },
        output_targets: workingDraft.outputTargets
          .map((entry) => safeString(entry))
          .filter(Boolean),
        agents: workingDraft.agents.map((agent) => ({
          agent_id: safeString(agent.agentId),
          template_id: saveReusableTemplates
            ? linkedTemplateIds.get(agent.agentId) ||
              safeString(agent.linkedTemplateId) ||
              undefined
            : undefined,
          display_name: safeString(agent.displayName) || safeString(agent.agentId),
          avatar_url: safeString(agent.avatarUrl) || undefined,
          model_policy: buildModelPolicy(agent),
          skills: agent.skills.map((skill) => safeString(skill)).filter(Boolean),
          tool_policy: {
            allowlist: normalizeNodeAwareToolAllowlist(
              agent.toolAllowlist,
              normalizedNodes.filter((node) => node.agentId === agent.agentId)
            ),
            denylist: agent.toolDenylist.map((entry) => safeString(entry)).filter(Boolean),
          },
          mcp_policy: {
            allowed_servers: agent.mcpAllowedServers
              .map((entry) => safeString(entry))
              .filter(Boolean),
            allowed_tools: [],
          },
        })),
        flow: {
          nodes: normalizedNodes.map((node) => {
            const agent = workingDraft.agents.find((entry) => entry.agentId === node.agentId);
            const outputPath = safeString(node.outputPath);
            const inputFiles = effectiveNodeInputFiles(node, normalizedNodes);
            const outputFiles = effectiveNodeOutputFiles(node);
            const codeLike = isCodeLikeNode(node);
            const researchStage = safeString(node.stageKind);
            const researchFinalize = researchStage === "research_finalize";
            const stagedHandoff = !!researchStage && !outputPath;
            const hasExternalResearchInput = node.inputRefs.some(
              (ref) => safeString(ref.alias) === "external_research"
            );
            const toolAllowlist = normalizeNodeAwareToolAllowlist(
              agent?.toolAllowlist || [],
              normalizedNodes.filter((entry) => entry.agentId === node.agentId)
            );
            const expectsWebResearch =
              !!agent?.toolAllowlist?.includes("websearch") && !researchFinalize;
            const isResearchBrief = safeString(node.outputKind) === "brief";
            const stageRequiresWorkspaceInspection =
              stagedHandoff && researchStage === "research_discover";
            const stageRequiresConcreteReads =
              stagedHandoff &&
              (researchStage === "research_discover" || researchStage === "research_local_sources");
            const stageRequiresWebsearch =
              stagedHandoff && researchStage === "research_external_sources" && expectsWebResearch;
            const requiredTools = outputPath
              ? [
                  toolAllowlist.includes("read") && !researchFinalize ? "read" : null,
                  toolAllowlist.includes("websearch") && !researchFinalize ? "websearch" : null,
                ]
                  .filter((value): value is string => Boolean(value))
                  .filter((value, index, all) => all.indexOf(value) === index)
              : stagedHandoff
                ? [
                    stageRequiresConcreteReads ? "read" : null,
                    stageRequiresWebsearch ? "websearch" : null,
                  ].filter((value): value is string => Boolean(value))
                : [];
            const requiredEvidence = outputPath
              ? [
                  !researchFinalize && (requiredTools.includes("read") || isResearchBrief)
                    ? "local_source_reads"
                    : null,
                  !researchFinalize && expectsWebResearch ? "external_sources" : null,
                ].filter((value): value is string => Boolean(value))
              : stagedHandoff
                ? [
                    stageRequiresConcreteReads ? "local_source_reads" : null,
                    stageRequiresWebsearch ? "external_sources" : null,
                  ].filter((value): value is string => Boolean(value))
                : [];
            const requiredSections = isResearchBrief
              ? [
                  "files_reviewed",
                  "files_not_reviewed",
                  "citations",
                  expectsWebResearch || hasExternalResearchInput ? "web_sources_reviewed" : null,
                ].filter((value): value is string => Boolean(value))
              : [];
            const prewriteGates = outputPath
              ? [
                  !researchFinalize ? "workspace_inspection" : null,
                  !researchFinalize && (requiredTools.includes("read") || isResearchBrief)
                    ? "concrete_reads"
                    : null,
                  !researchFinalize && expectsWebResearch ? "successful_web_research" : null,
                ].filter((value): value is string => Boolean(value))
              : stagedHandoff
                ? [
                    stageRequiresWorkspaceInspection ? "workspace_inspection" : null,
                    stageRequiresConcreteReads ? "concrete_reads" : null,
                    stageRequiresWebsearch ? "successful_web_research" : null,
                  ].filter((value): value is string => Boolean(value))
                : [];
            const retryOnMissing = [...requiredEvidence, ...requiredSections, ...prewriteGates];
            return {
              node_id: safeString(node.nodeId),
              agent_id: safeString(node.agentId),
              objective: safeString(node.objective) || safeString(node.title),
              depends_on: node.dependsOn.map((dep) => safeString(dep)).filter(Boolean),
              input_refs: syncInputRefs(node.dependsOn, node.inputRefs).map((ref) => ({
                from_step_id: safeString(ref.fromStepId),
                alias: safeString(ref.alias) || safeString(ref.fromStepId).replace(/-/g, "_"),
              })),
              stage_kind: researchStage ? "workstream" : undefined,
              output_contract: {
                kind: safeString(node.outputKind) || "artifact",
                enforcement: outputPath
                  ? {
                      required_tools: requiredTools,
                      required_evidence: requiredEvidence,
                      required_sections: requiredSections,
                      prewrite_gates: prewriteGates,
                      retry_on_missing: retryOnMissing,
                      terminal_on: retryOnMissing.length
                        ? ["tool_unavailable", "repair_budget_exhausted"]
                        : [],
                      repair_budget: retryOnMissing.length ? 5 : undefined,
                      session_text_recovery:
                        retryOnMissing.length || isResearchBrief
                          ? "require_prewrite_satisfied"
                          : "allow",
                    }
                  : undefined,
                summary_guidance: outputPath
                  ? codeLike
                    ? `Apply the scoped repository changes, update \`${outputPath}\` in the workspace, and use patch/edit/write tools before completing this stage.`
                    : `Create or update \`${outputPath}\` in the workspace and use the write tool before completing this stage.`
                  : stagedHandoff
                    ? "Return a structured handoff in the final response instead of writing workspace files."
                    : undefined,
              },
              metadata: {
                studio: {
                  input_files: inputFiles.length ? inputFiles : undefined,
                  output_path: outputPath || undefined,
                  output_files: outputFiles.length ? outputFiles : undefined,
                  research_stage: researchStage || undefined,
                },
                builder: {
                  title: safeString(node.title) || safeString(node.nodeId),
                  role: safeString(agent?.role) || "worker",
                  input_files: inputFiles.length ? inputFiles : undefined,
                  output_path: outputPath || undefined,
                  output_files: outputFiles.length ? outputFiles : undefined,
                  research_stage: researchStage || undefined,
                  write_required: !!outputPath,
                  required_tools: requiredTools,
                  web_research_expected: expectsWebResearch,
                  task_kind: safeString(node.taskKind) || undefined,
                  project_backlog_tasks: isBacklogProjectingNode(node) || undefined,
                  task_id: safeString(node.backlogTaskId) || undefined,
                  repo_root: safeString(node.repoRoot) || undefined,
                  write_scope: safeString(node.writeScope) || undefined,
                  acceptance_criteria: safeString(node.acceptanceCriteria) || undefined,
                  task_dependencies: safeString(node.taskDependencies) || undefined,
                  verification_state: safeString(node.verificationState) || undefined,
                  task_owner: safeString(node.taskOwner) || undefined,
                  verification_command: safeString(node.verificationCommand) || undefined,
                  prompt: composeNodeExecutionPrompt(
                    node,
                    agent ||
                      createEmptyAgentDraft(safeString(node.agentId), safeString(node.agentId))
                  ),
                },
              },
            };
          }),
        },
        metadata: {
          workspace_root: safeString(draft.workspaceRoot),
          studio: buildStudioMetadata(
            {
              ...workingDraft,
              agents: workingDraft.agents.map((agent) => ({
                ...agent,
                templateId: saveReusableTemplates
                  ? linkedTemplateIds.get(agent.agentId) ||
                    safeString(agent.linkedTemplateId) ||
                    safeString(agent.templateId)
                  : "",
                linkedTemplateId: saveReusableTemplates
                  ? linkedTemplateIds.get(agent.agentId) ||
                    safeString(agent.linkedTemplateId) ||
                    safeString(agent.templateId)
                  : "",
              })),
            },
            normalizedNodes,
            saveReusableTemplates
              ? null
              : repairState || repairDraftTemplateLinks(workingDraft, templateMap).repairState
          ),
        },
      };
      const response = draft.automationId
        ? await client.automationsV2.update(draft.automationId, automationPayload as any)
        : await client.automationsV2.create(automationPayload as any);
      const automationId = safeString(
        (response as any)?.automation?.automation_id || (response as any)?.automation?.automationId
      );
      let startedRunId = "";
      if (runAfterSave && automationId) {
        const runResponse = await client.automationsV2.runNow(automationId);
        startedRunId = safeString(
          (runResponse as any)?.run?.run_id || (runResponse as any)?.run?.runId
        );
      }
      return { response, automationId, linkedTemplateIds, workingDraft, startedRunId };
    },
    onSuccess: async ({
      response,
      automationId,
      linkedTemplateIds,
      workingDraft,
      startedRunId,
    }) => {
      toast(
        "ok",
        runAfterSave ? "Studio workflow saved and run started." : "Studio workflow saved."
      );
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["studio", "automations"] }),
        queryClient.invalidateQueries({ queryKey: ["automations"] }),
        queryClient.invalidateQueries({ queryKey: ["studio", "templates"] }),
        queryClient.invalidateQueries({ queryKey: ["teams"] }),
      ]);
      setDraft((current) => ({
        ...current,
        ...workingDraft,
        automationId:
          automationId ||
          safeString(
            (response as any)?.automation?.automation_id ||
              (response as any)?.automation?.automationId
          ),
        agents: workingDraft.agents.map((agent) => ({
          ...agent,
          templateId: saveReusableTemplates
            ? linkedTemplateIds.get(agent.agentId) || agent.templateId
            : "",
          linkedTemplateId: saveReusableTemplates
            ? linkedTemplateIds.get(agent.agentId) || agent.linkedTemplateId
            : "",
        })),
      }));
      setRepairState((current) =>
        current
          ? {
              ...current,
              requiresSave: false,
              reason: "save",
            }
          : null
      );
      if (runAfterSave) {
        seedAutomationsStudioHandoff({
          tab: "running",
          runId: startedRunId,
          automationId:
            automationId ||
            safeString(
              (response as any)?.automation?.automation_id ||
                (response as any)?.automation?.automationId
            ),
          openTaskInspector: true,
        });
        navigate("automations");
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const deleteAutomationMutation = useMutation({
    mutationFn: async (automationId: string) => {
      await client.automationsV2.delete(automationId);
      await confirmAutomationDeleted(client, automationId);
      return automationId;
    },
    onSuccess: async (automationId) => {
      if (draft.automationId === automationId) {
        const fallback = createWorkflowDraftFromTemplate(
          STUDIO_TEMPLATE_CATALOG[0],
          defaultWorkspaceRoot || ""
        );
        setDraft(fallback);
        setSelectedNodeId(fallback.nodes[0]?.nodeId || "");
        setSelectedAgentId(fallback.agents[0]?.agentId || "");
      }
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["studio", "automations"] }),
        queryClient.invalidateQueries({ queryKey: ["automations"] }),
      ]);
      toast("ok", "Studio workflow deleted.");
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  return (
    <div className="grid gap-4">
      <PageCard
        title="Studio"
        subtitle="Template-first multi-agent workflow builder with reusable role prompts."
        actions={
          <div className="flex flex-wrap items-center gap-2">
            <button
              className="tcp-btn inline-flex items-center gap-2"
              onClick={() =>
                applyTemplate(draft.starterTemplateId || STUDIO_TEMPLATE_CATALOG[0].id)
              }
            >
              <i data-lucide="rotate-ccw"></i>
              Reset From Template
            </button>
            <button className="tcp-btn inline-flex items-center gap-2" onClick={addAgent}>
              <i data-lucide="user-plus"></i>
              Add Agent
            </button>
            <button className="tcp-btn inline-flex items-center gap-2" onClick={addNode}>
              <i data-lucide="plus"></i>
              Add Stage
            </button>
            <button
              className="tcp-btn-primary inline-flex items-center gap-2"
              disabled={saveMutation.isPending}
              onClick={() => saveMutation.mutate()}
            >
              <i data-lucide={saveMutation.isPending ? "loader-circle" : "save"}></i>
              {saveMutation.isPending ? "Saving..." : "Save Workflow"}
            </button>
          </div>
        }
      >
        <div className="grid gap-4 xl:grid-cols-[320px_minmax(0,1fr)_360px] xl:items-start">
          <div className="grid auto-rows-max content-start self-start gap-4">
            <PageCard title="Studio Library" subtitle="Templates and saved workflows in one place.">
              <div className="grid gap-4">
                <section className="grid gap-2">
                  <button
                    type="button"
                    className="flex w-full items-center gap-2 text-left"
                    aria-expanded={templatesOpen}
                    onClick={() => setTemplatesOpen((open) => !open)}
                  >
                    <i
                      data-lucide={templatesOpen ? "chevron-down" : "chevron-right"}
                      className="text-slate-400"
                    ></i>
                    <div className="min-w-0">
                      <div className="text-sm font-semibold text-slate-100">Starter Templates</div>
                      <div className="text-xs text-slate-400">
                        Begin with a proven workflow shape.
                      </div>
                    </div>
                  </button>
                  <AnimatePresence initial={false}>
                    {templatesOpen ? (
                      <motion.div
                        initial={{ opacity: 0, height: 0 }}
                        animate={{ opacity: 1, height: "auto" }}
                        exit={{ opacity: 0, height: 0 }}
                        transition={{ duration: 0.16, ease: "easeOut" }}
                        className="grid gap-2 overflow-hidden pl-5"
                      >
                        {STUDIO_TEMPLATE_CATALOG.map((template) => (
                          <button
                            key={template.id}
                            className={`tcp-list-item text-left ${draft.starterTemplateId === template.id ? "border-emerald-400/60 bg-emerald-500/10" : ""}`}
                            onClick={() => applyTemplate(template.id)}
                          >
                            <div className="flex items-center justify-between gap-2">
                              <strong>{template.name}</strong>
                              <span className="tcp-badge-info">{template.icon}</span>
                            </div>
                            <div className="mt-1 text-sm text-slate-300">{template.summary}</div>
                          </button>
                        ))}
                      </motion.div>
                    ) : null}
                  </AnimatePresence>
                </section>

                <section className="grid gap-2 border-t border-slate-800/80 pt-3">
                  <button
                    type="button"
                    className="flex w-full items-center gap-2 text-left"
                    aria-expanded={savedWorkflowsOpen}
                    onClick={() => setSavedWorkflowsOpen((open) => !open)}
                  >
                    <i
                      data-lucide={savedWorkflowsOpen ? "chevron-down" : "chevron-right"}
                      className="text-slate-400"
                    ></i>
                    <div className="min-w-0">
                      <div className="text-sm font-semibold text-slate-100">
                        Saved Studio Workflows
                      </div>
                      <div className="text-xs text-slate-400">
                        Reopen workflows created from Studio metadata.
                      </div>
                    </div>
                  </button>
                  <AnimatePresence initial={false}>
                    {savedWorkflowsOpen ? (
                      <motion.div
                        initial={{ opacity: 0, height: 0 }}
                        animate={{ opacity: 1, height: "auto" }}
                        exit={{ opacity: 0, height: 0 }}
                        transition={{ duration: 0.16, ease: "easeOut" }}
                        className="grid gap-2 overflow-hidden pl-5"
                      >
                        {studioAutomations.length ? (
                          [...studioAutomations]
                            .sort((a: any, b: any) => {
                              const aTime = Number(
                                a?.updated_at_ms ||
                                  a?.updatedAtMs ||
                                  a?.created_at_ms ||
                                  a?.createdAtMs ||
                                  0
                              );
                              const bTime = Number(
                                b?.updated_at_ms ||
                                  b?.updatedAtMs ||
                                  b?.created_at_ms ||
                                  b?.createdAtMs ||
                                  0
                              );
                              return bTime - aTime;
                            })
                            .slice(0, 12)
                            .map((automation: any) => {
                              const automationId = safeString(
                                automation?.automation_id ||
                                  automation?.automationId ||
                                  automation?.id
                              );
                              const latestRun = studioWorkflowLatestRuns.get(automationId) || null;
                              const latestStability = workflowLatestStabilitySnapshot(latestRun);
                              const latestRunStatus = safeString(latestStability.status);
                              const latestFailureKind = safeString(latestStability.failureKind);
                              const latestPhase = safeString(latestStability.phase);
                              const latestRunLabel = timestampLabel(
                                latestRun?.updated_at_ms ||
                                  latestRun?.updatedAtMs ||
                                  latestRun?.created_at_ms ||
                                  latestRun?.createdAtMs
                              );
                              const studio = automation?.metadata?.studio || {};
                              const health = analyzeAutomationTemplateHealth(
                                automation,
                                templateMap
                              );
                              const templateId = safeString(
                                studio?.template_id ||
                                  studio?.templateId ||
                                  studio?.starter_template_id ||
                                  studio?.starterTemplateId
                              );
                              const updatedLabel = timestampLabel(
                                automation?.updated_at_ms ||
                                  automation?.updatedAtMs ||
                                  automation?.created_at_ms ||
                                  automation?.createdAtMs
                              );
                              const isDeleting =
                                deleteAutomationMutation.isPending &&
                                deleteAutomationMutation.variables === automationId;
                              return (
                                <div key={automationId} className="tcp-list-item">
                                  <div className="flex items-center justify-between gap-2">
                                    <strong>{safeString(automation?.name) || automationId}</strong>
                                    <div className="flex flex-wrap items-center justify-end gap-2">
                                      {health.isBroken ? (
                                        <span className="tcp-badge-warn">broken links</span>
                                      ) : null}
                                      <span className="tcp-badge-info">
                                        {safeString(automation?.status) || "draft"}
                                      </span>
                                    </div>
                                  </div>
                                  <div className="mt-1 text-xs text-slate-400">
                                    {safeString(studio?.summary) || "Studio workflow"}
                                  </div>
                                  <div className="mt-2 flex flex-wrap gap-2 text-[11px] text-slate-500">
                                    {templateId ? (
                                      <span className="tcp-badge-info">template: {templateId}</span>
                                    ) : null}
                                    <span className="tcp-badge-muted">
                                      id: {shortId(automationId)}
                                    </span>
                                    {updatedLabel ? (
                                      <span className="tcp-badge-muted">
                                        updated: {updatedLabel}
                                      </span>
                                    ) : null}
                                  </div>
                                  {latestRun ? (
                                    <div className="mt-2 rounded-lg border border-slate-700/50 bg-slate-950/20 p-2">
                                      <div className="text-[11px] uppercase tracking-wide text-slate-500">
                                        Latest Run Stability
                                      </div>
                                      <div className="mt-2 flex flex-wrap gap-2 text-[11px]">
                                        <span className="tcp-badge-info">
                                          status: {latestRunStatus}
                                        </span>
                                        {latestPhase ? (
                                          <span className="tcp-badge-muted">
                                            phase: {latestPhase}
                                          </span>
                                        ) : null}
                                        {latestFailureKind ? (
                                          <span className="tcp-badge-warn">
                                            failure: {latestFailureKind}
                                          </span>
                                        ) : null}
                                        {latestRunLabel ? (
                                          <span className="tcp-badge-muted">
                                            run: {latestRunLabel}
                                          </span>
                                        ) : null}
                                      </div>
                                      {safeString(latestStability.reason) ? (
                                        <div className="mt-2 text-xs text-slate-300">
                                          {safeString(latestStability.reason)}
                                        </div>
                                      ) : null}
                                    </div>
                                  ) : null}
                                  <div className="mt-2 flex flex-wrap gap-2">
                                    <button
                                      className="tcp-btn inline-flex h-7 items-center gap-2 px-2 text-xs"
                                      onClick={() => {
                                        openAutomationInStudio(automation);
                                      }}
                                    >
                                      <i data-lucide="folder-open"></i>
                                      Open
                                    </button>
                                    <button
                                      className="tcp-btn inline-flex h-7 items-center gap-2 px-2 text-xs"
                                      onClick={async () => {
                                        try {
                                          await runSavedAutomation(automation);
                                        } catch (error) {
                                          toast(
                                            "err",
                                            error instanceof Error ? error.message : String(error)
                                          );
                                        }
                                      }}
                                    >
                                      <i data-lucide="play"></i>
                                      {health.isBroken ? "Repair & Open" : "Run Now"}
                                    </button>
                                    <button
                                      className="tcp-btn inline-flex h-7 items-center gap-2 px-2 text-xs text-rose-200"
                                      disabled={isDeleting}
                                      onClick={() => {
                                        setDeleteConfirm({
                                          automationId,
                                          title: safeString(automation?.name) || automationId,
                                        });
                                      }}
                                    >
                                      <i data-lucide="trash-2"></i>
                                      {isDeleting ? "Deleting..." : "Delete"}
                                    </button>
                                  </div>
                                </div>
                              );
                            })
                        ) : (
                          <EmptyState text="No Studio-created workflows yet." />
                        )}
                      </motion.div>
                    ) : null}
                  </AnimatePresence>
                </section>
              </div>
            </PageCard>
          </div>

          <div className="grid auto-rows-max content-start self-start gap-4">
            {repairState?.requiresSave &&
            (repairState?.repairedAgentIds.length || repairState?.missingNodeAgentIds.length) ? (
              <PageCard
                title="Repair Applied"
                subtitle="Studio repaired runtime dependencies so this workflow can be saved and run locally."
                actions={
                  <button
                    className="tcp-btn-primary inline-flex items-center gap-2"
                    onClick={() => saveMutation.mutate()}
                    disabled={saveMutation.isPending}
                  >
                    <i data-lucide={saveMutation.isPending ? "loader-circle" : "save"}></i>
                    {saveMutation.isPending ? "Saving..." : "Save Repaired Workflow"}
                  </button>
                }
              >
                <div className="grid gap-2 text-sm text-slate-300">
                  {repairState.repairedAgentIds.length ? (
                    <div>
                      Repaired missing template links for: {repairState.repairedAgentIds.join(", ")}
                    </div>
                  ) : null}
                  {repairState.missingNodeAgentIds.length ? (
                    <div>
                      Stages still reference missing agents:{" "}
                      {repairState.missingNodeAgentIds.join(", ")}
                    </div>
                  ) : null}
                  <div className="text-xs text-slate-400">
                    Save this workflow to persist the repaired local-first configuration.
                  </div>
                </div>
              </PageCard>
            ) : null}

            <PageCard
              title="Workflow Settings"
              subtitle="Name, schedule, workspace, and save behavior."
            >
              <div className="grid gap-4 xl:grid-cols-[minmax(0,1.3fr)_minmax(18rem,0.95fr)]">
                <div className="grid content-start gap-3">
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Name</span>
                    <input
                      className="tcp-input text-sm"
                      value={draft.name}
                      onInput={(event) =>
                        updateDraft({ name: (event.target as HTMLInputElement).value })
                      }
                    />
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Description</span>
                    <textarea
                      className="tcp-input min-h-[88px] text-sm"
                      value={draft.description}
                      onInput={(event) =>
                        updateDraft({ description: (event.target as HTMLTextAreaElement).value })
                      }
                    />
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Workspace Root</span>
                    <div className="grid gap-2 md:grid-cols-[auto_1fr_auto]">
                      <button
                        className="tcp-btn h-10 px-3"
                        type="button"
                        onClick={() => {
                          const seed = safeString(
                            draft.workspaceRoot || defaultWorkspaceRoot || "/"
                          );
                          setWorkspaceBrowserDir(seed || "/");
                          setWorkspaceBrowserSearch("");
                          setWorkspaceBrowserOpen(true);
                        }}
                      >
                        <i data-lucide="folder-open"></i>
                        Browse
                      </button>
                      <input
                        className={`tcp-input text-sm ${workspaceRootError ? "border-red-500/60 text-red-100" : ""}`}
                        value={draft.workspaceRoot}
                        readOnly
                        placeholder="No local directory selected. Use Browse."
                      />
                      <button
                        className="tcp-btn h-10 px-3"
                        type="button"
                        onClick={() => updateDraft({ workspaceRoot: "" })}
                        disabled={!draft.workspaceRoot}
                      >
                        <i data-lucide="x"></i>
                        Clear
                      </button>
                    </div>
                    {workspaceRootError ? (
                      <span className="text-xs text-red-300">{workspaceRootError}</span>
                    ) : null}
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Output Targets</span>
                    <input
                      className="tcp-input text-sm"
                      value={joinCsv(draft.outputTargets)}
                      onInput={(event) =>
                        updateDraft({
                          outputTargets: splitCsv((event.target as HTMLInputElement).value),
                        })
                      }
                      placeholder="content-brief.md, approved-post.md"
                    />
                    <span className="text-[11px] text-slate-500">
                      Supported runtime tokens: {STUDIO_OUTPUT_TOKEN_GUIDE.join(", ")}. Legacy
                      patterns like <code>YYYY-MM-DD_HH-MM-SS</code> are normalized on save.
                    </span>
                    {outputPathWarnings.length ? (
                      <div className="rounded-lg border border-amber-500/30 bg-amber-500/8 px-3 py-2 text-[11px] text-amber-100">
                        <div className="font-medium uppercase tracking-wide text-amber-200/90">
                          Output path warnings
                        </div>
                        {outputPathWarnings.slice(0, 3).map((warning) => (
                          <div key={warning} className="mt-1">
                            {warning}
                          </div>
                        ))}
                      </div>
                    ) : null}
                    {outputPathPreviewRows.length ? (
                      <div className="rounded-lg border border-slate-700/60 bg-slate-950/30 px-3 py-2 text-[11px] text-slate-300">
                        <div className="font-medium uppercase tracking-wide text-slate-500">
                          Output path preview
                        </div>
                        {outputPathPreviewRows.slice(0, 8).map((row) => (
                          <div
                            key={row.id}
                            className="mt-2 grid gap-1 border-t border-slate-800/70 pt-2 first:mt-1 first:border-t-0 first:pt-0"
                          >
                            <div className="text-slate-400">{row.label}</div>
                            <div>
                              Draft: <code>{row.raw || row.canonical}</code>
                            </div>
                            <div>
                              Saved: <code>{row.canonical}</code>
                            </div>
                            <div>
                              Next run preview: <code>{row.resolved}</code>
                            </div>
                            {row.warning ? (
                              <div className="text-amber-200">{row.warning}</div>
                            ) : null}
                          </div>
                        ))}
                      </div>
                    ) : null}
                  </label>
                </div>

                <div className="grid content-start gap-3">
                  <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-1 2xl:grid-cols-2">
                    <label className="grid gap-1">
                      <span className="text-xs text-slate-400">Status</span>
                      <select
                        className="tcp-input text-sm"
                        value={draft.status}
                        onInput={(event) =>
                          updateDraft({
                            status: (event.target as HTMLSelectElement).value as
                              | "draft"
                              | "active"
                              | "paused",
                          })
                        }
                      >
                        <option value="draft">draft</option>
                        <option value="active">active</option>
                        <option value="paused">paused</option>
                      </select>
                    </label>
                    <label className="grid gap-1">
                      <span className="text-xs text-slate-400">Max Parallel Agents</span>
                      <input
                        className="tcp-input text-sm"
                        value={draft.maxParallelAgents}
                        onInput={(event) =>
                          updateDraft({
                            maxParallelAgents: (event.target as HTMLInputElement).value,
                          })
                        }
                      />
                    </label>
                  </div>

                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Schedule</span>
                    <select
                      className="tcp-input text-sm"
                      value={draft.scheduleType}
                      onInput={(event) =>
                        updateDraft({
                          scheduleType: (event.target as HTMLSelectElement).value as
                            | "manual"
                            | "cron"
                            | "interval",
                        })
                      }
                    >
                      <option value="manual">manual</option>
                      <option value="cron">cron</option>
                      <option value="interval">interval</option>
                    </select>
                  </label>
                  {draft.scheduleType === "cron" ? (
                    <label className="grid gap-1">
                      <span className="text-xs text-slate-400">Cron Expression</span>
                      <input
                        className="tcp-input text-sm"
                        value={draft.cronExpression}
                        onInput={(event) =>
                          updateDraft({ cronExpression: (event.target as HTMLInputElement).value })
                        }
                        placeholder="0 9 * * 1"
                      />
                    </label>
                  ) : null}
                  {draft.scheduleType === "interval" ? (
                    <label className="grid gap-1">
                      <span className="text-xs text-slate-400">Interval Seconds</span>
                      <input
                        className="tcp-input text-sm"
                        value={draft.intervalSeconds}
                        onInput={(event) =>
                          updateDraft({
                            intervalSeconds: (event.target as HTMLInputElement).value,
                          })
                        }
                      />
                    </label>
                  ) : null}

                  <label className="flex items-center gap-2 text-sm text-slate-300">
                    <input
                      type="checkbox"
                      checked={saveReusableTemplates}
                      onInput={(event) =>
                        setSaveReusableTemplates((event.target as HTMLInputElement).checked)
                      }
                    />
                    Save agent prompts as reusable templates
                  </label>
                  <div className="text-xs text-slate-400">
                    Off: this workflow runs from Studio-local prompts only. On: Studio also creates
                    shared Agent Team templates and links the workflow to them at runtime.
                  </div>
                  <div className="text-xs text-slate-500">
                    Default model fallback:{" "}
                    {studioDefaultModel.provider && studioDefaultModel.model
                      ? `${studioDefaultModel.provider}/${studioDefaultModel.model}`
                      : "No provider default configured in Settings."}
                  </div>

                  <label className="flex items-center gap-2 text-sm text-slate-300">
                    <input
                      type="checkbox"
                      checked={draft.useSharedModel}
                      onInput={(event) => {
                        const checked = (event.target as HTMLInputElement).checked;
                        const inferred = inferSharedModelFromAgents(draft.agents);
                        updateDraft({
                          useSharedModel: checked,
                          sharedModelProvider: checked
                            ? safeString(draft.sharedModelProvider) ||
                              inferred.provider ||
                              studioDefaultModel.provider
                            : draft.sharedModelProvider,
                          sharedModelId: checked
                            ? safeString(draft.sharedModelId) ||
                              inferred.model ||
                              studioDefaultModel.model
                            : draft.sharedModelId,
                        });
                      }}
                    />
                    Use one model for all agents in this workflow
                  </label>
                  {draft.useSharedModel ? (
                    <>
                      <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-1 2xl:grid-cols-2">
                        <label className="grid gap-1">
                          <span className="text-xs text-slate-400">Shared Model Provider</span>
                          <select
                            className="tcp-input text-sm"
                            value={draft.sharedModelProvider}
                            onInput={(event) => {
                              const provider = (event.target as HTMLSelectElement).value;
                              const models = modelsForProvider(providerOptions, provider);
                              updateDraft({
                                sharedModelProvider: provider,
                                sharedModelId: models.includes(draft.sharedModelId)
                                  ? draft.sharedModelId
                                  : models[0] || draft.sharedModelId,
                              });
                            }}
                          >
                            <option value="">Select provider...</option>
                            {providerOptions.map((provider) => (
                              <option key={provider.id} value={provider.id}>
                                {provider.id}
                              </option>
                            ))}
                          </select>
                        </label>
                        <label className="grid gap-1">
                          <span className="text-xs text-slate-400">Shared Model</span>
                          {modelsForProvider(providerOptions, draft.sharedModelProvider).length ? (
                            <select
                              className="tcp-input text-sm"
                              value={draft.sharedModelId}
                              onInput={(event) =>
                                updateDraft({
                                  sharedModelId: (event.target as HTMLSelectElement).value,
                                })
                              }
                            >
                              {modelsForProvider(providerOptions, draft.sharedModelProvider).map(
                                (model) => (
                                  <option key={model} value={model}>
                                    {model}
                                  </option>
                                )
                              )}
                            </select>
                          ) : (
                            <input
                              className="tcp-input text-sm"
                              value={draft.sharedModelId}
                              onInput={(event) =>
                                updateDraft({
                                  sharedModelId: (event.target as HTMLInputElement).value,
                                })
                              }
                              placeholder="provider-specific model id"
                            />
                          )}
                        </label>
                      </div>
                      <div className="rounded-lg border border-amber-500/20 bg-amber-500/8 px-3 py-2 text-xs text-amber-100">
                        Shared model mode applies the same provider/model to every agent on save and
                        while editing.
                      </div>
                    </>
                  ) : null}

                  <label className="flex items-center gap-2 text-sm text-slate-300">
                    <input
                      type="checkbox"
                      checked={runAfterSave}
                      onInput={(event) =>
                        setRunAfterSave((event.target as HTMLInputElement).checked)
                      }
                    />
                    Run workflow immediately after save
                  </label>
                  {repairState &&
                  !repairState.requiresSave &&
                  repairState.repairedAgentIds.length ? (
                    <div className="rounded-lg border border-emerald-500/30 bg-emerald-500/8 px-3 py-2 text-xs text-emerald-100">
                      Repaired template links were saved successfully. This workflow is now using
                      local Studio prompts and can run normally.
                    </div>
                  ) : null}
                </div>
              </div>
            </PageCard>

            {workspaceBrowserOpen ? (
              <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
                <button
                  type="button"
                  className="tcp-confirm-backdrop"
                  aria-label="Close workspace directory dialog"
                  onClick={() => {
                    setWorkspaceBrowserOpen(false);
                    setWorkspaceBrowserSearch("");
                  }}
                />
                <div className="tcp-confirm-dialog max-w-2xl">
                  <h3 className="tcp-confirm-title">Select Workspace Folder</h3>
                  <p className="tcp-confirm-message">
                    Current: {workspaceCurrentBrowseDir || workspaceBrowserDir || "n/a"}
                  </p>
                  <div className="mb-2 flex flex-wrap gap-2">
                    <button
                      className="tcp-btn"
                      type="button"
                      onClick={() => {
                        if (!workspaceParentDir) return;
                        setWorkspaceBrowserDir(workspaceParentDir);
                      }}
                      disabled={!workspaceParentDir}
                    >
                      <i data-lucide="arrow-left-to-line"></i>
                      Up
                    </button>
                    <button
                      className="tcp-btn-primary"
                      type="button"
                      onClick={() => {
                        if (!workspaceCurrentBrowseDir) return;
                        updateDraft({ workspaceRoot: workspaceCurrentBrowseDir });
                        setWorkspaceBrowserOpen(false);
                        setWorkspaceBrowserSearch("");
                        toast("ok", `Workspace selected: ${workspaceCurrentBrowseDir}`);
                      }}
                      disabled={!workspaceCurrentBrowseDir}
                    >
                      <i data-lucide="badge-check"></i>
                      Select This Folder
                    </button>
                    <button
                      className="tcp-btn"
                      type="button"
                      onClick={() => {
                        setWorkspaceBrowserOpen(false);
                        setWorkspaceBrowserSearch("");
                      }}
                    >
                      <i data-lucide="x"></i>
                      Close
                    </button>
                  </div>
                  <div className="mb-2">
                    <input
                      className="tcp-input"
                      placeholder="Type to filter folders..."
                      value={workspaceBrowserSearch}
                      onInput={(event) =>
                        setWorkspaceBrowserSearch((event.target as HTMLInputElement).value)
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
                          onClick={() => setWorkspaceBrowserDir(String(entry?.path || ""))}
                        >
                          <span className="inline-flex items-center gap-2">
                            <i data-lucide="folder-open"></i>
                            <span>{String(entry?.name || entry?.path || "")}</span>
                          </span>
                        </button>
                      ))
                    ) : (
                      <EmptyState
                        text={
                          safeString(workspaceBrowserSearch)
                            ? "No folders match your search."
                            : "No subdirectories in this folder."
                        }
                      />
                    )}
                  </div>
                </div>
              </div>
            ) : null}

            {deleteConfirm ? (
              <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
                <button
                  type="button"
                  className="tcp-confirm-backdrop"
                  aria-label="Close delete workflow dialog"
                  onClick={() => setDeleteConfirm(null)}
                />
                <div className="tcp-confirm-dialog w-[min(34rem,96vw)]">
                  <h3 className="tcp-confirm-title">Delete Studio workflow</h3>
                  <p className="tcp-confirm-message">
                    This will permanently remove <strong>{deleteConfirm.title}</strong>.
                  </p>
                  <div className="tcp-confirm-actions mt-3">
                    <button
                      className="tcp-btn inline-flex items-center gap-2"
                      onClick={() => setDeleteConfirm(null)}
                    >
                      <i data-lucide="x"></i>
                      Cancel
                    </button>
                    <button
                      className="tcp-btn-danger inline-flex items-center gap-2"
                      disabled={deleteAutomationMutation.isPending}
                      onClick={() =>
                        deleteAutomationMutation.mutate(deleteConfirm.automationId, {
                          onSettled: () => setDeleteConfirm(null),
                        })
                      }
                    >
                      <i data-lucide="trash-2"></i>
                      {deleteAutomationMutation.isPending ? "Deleting..." : "Delete workflow"}
                    </button>
                  </div>
                </div>
              </div>
            ) : null}

            <PageCard
              title="Workflow Map"
              subtitle="Select a stage to edit its objective, dependencies, and bound agent."
            >
              {graphColumns.length ? (
                <div className="grid gap-3 xl:grid-cols-4 xl:items-start">
                  {graphColumns.map(([depth, nodes]) => (
                    <div key={depth} className="grid content-start gap-2">
                      <div className="text-xs uppercase tracking-wide text-slate-500">
                        Column {depth + 1}
                      </div>
                      {nodes.map((node) => {
                        const agent = draft.agents.find((entry) => entry.agentId === node.agentId);
                        const active = node.nodeId === selectedNodeId;
                        return (
                          <button
                            key={node.nodeId}
                            className={`tcp-list-item flex flex-col text-left ${active ? "border-emerald-400/60 bg-emerald-500/10" : ""}`}
                            onClick={() => {
                              setSelectedNodeId(node.nodeId);
                              setSelectedAgentId(node.agentId);
                            }}
                          >
                            <div className="flex items-center justify-between gap-2">
                              <strong>{node.title}</strong>
                              <span className="tcp-badge-info">
                                {node.outputKind || "artifact"}
                              </span>
                            </div>
                            <div className="mt-1 text-xs text-slate-400">
                              {agent?.displayName || node.agentId || "Unassigned agent"}
                            </div>
                            <div className="mt-2 text-sm text-slate-300">{node.objective}</div>
                            {node.outputPath ? (
                              <div className="mt-2 text-xs text-emerald-200">
                                output: {node.outputPath}
                              </div>
                            ) : null}
                            <div className="mt-auto flex flex-wrap gap-1 pt-3">
                              {node.dependsOn.length ? (
                                node.dependsOn.map((dep) => (
                                  <span key={`${node.nodeId}-${dep}`} className="tcp-badge-warn">
                                    {"<-"} {dep}
                                  </span>
                                ))
                              ) : (
                                <span className="tcp-badge-info">start</span>
                              )}
                            </div>
                          </button>
                        );
                      })}
                    </div>
                  ))}
                </div>
              ) : (
                <EmptyState text="No stages yet. Add one to start shaping the workflow." />
              )}
            </PageCard>

            <PageCard
              title="Agent Directory"
              subtitle="All agents currently participating in this workflow."
            >
              <div className="grid gap-2 md:grid-cols-2">
                {draft.agents.map((agent) => {
                  const selected = selectedAgent?.agentId === agent.agentId;
                  return (
                    <button
                      key={agent.agentId}
                      className={`tcp-list-item text-left ${selected ? "border-emerald-400/60 bg-emerald-500/10" : ""}`}
                      onClick={() => {
                        setSelectedAgentId(agent.agentId);
                        const node = draft.nodes.find((entry) => entry.agentId === agent.agentId);
                        if (node) setSelectedNodeId(node.nodeId);
                      }}
                    >
                      <div className="flex items-center justify-between gap-2">
                        <strong>{agent.displayName || agent.agentId}</strong>
                        <div className="flex flex-wrap items-center justify-end gap-2">
                          <span className="tcp-badge-info">{agent.role}</span>
                          {repairState?.repairedAgentIds.includes(agent.agentId) ? (
                            <span className="tcp-badge-warn">missing/repaired</span>
                          ) : !safeString(agent.linkedTemplateId || agent.templateId) ? (
                            <span className="tcp-badge-muted">local</span>
                          ) : templateMap.has(
                              safeString(agent.linkedTemplateId || agent.templateId)
                            ) ? (
                            <span className="tcp-badge-info">linked</span>
                          ) : (
                            <span className="tcp-badge-warn">missing/repaired</span>
                          )}
                        </div>
                      </div>
                      <div className="mt-1 text-xs text-slate-400">{agent.agentId}</div>
                      {agent.linkedTemplateId ? (
                        <div className="mt-2 text-xs text-emerald-200">
                          linked template: {agent.linkedTemplateId}
                        </div>
                      ) : null}
                    </button>
                  );
                })}
              </div>
            </PageCard>
          </div>

          <div className="grid auto-rows-max content-start self-start gap-4">
            <PageCard
              title={selectedNode ? `Stage: ${selectedNode.title}` : "Stage"}
              subtitle="Edit stage behavior, dependencies, and handoff aliases."
              actions={
                <button
                  className="tcp-btn inline-flex h-7 items-center gap-2 px-2 text-xs"
                  onClick={removeSelectedNode}
                  disabled={!selectedNode}
                >
                  <i data-lucide="trash-2"></i>
                  Remove Stage
                </button>
              }
            >
              {selectedNode ? (
                <div className="grid gap-3">
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Title</span>
                    <input
                      className="tcp-input text-sm"
                      value={selectedNode.title}
                      onInput={(event) => {
                        updateNode(selectedNode.nodeId, {
                          title: (event.target as HTMLInputElement).value,
                        });
                      }}
                    />
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Bound Agent</span>
                    <select
                      className="tcp-input text-sm"
                      value={selectedNode.agentId}
                      onInput={(event) => {
                        const agentId = (event.target as HTMLSelectElement).value;
                        updateNode(selectedNode.nodeId, { agentId });
                        setSelectedAgentId(agentId);
                      }}
                    >
                      {draft.agents.map((agent) => (
                        <option key={agent.agentId} value={agent.agentId}>
                          {agent.displayName || agent.agentId}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Objective</span>
                    <textarea
                      className="tcp-input min-h-[110px] text-sm"
                      value={selectedNode.objective}
                      onInput={(event) =>
                        updateNode(selectedNode.nodeId, {
                          objective: (event.target as HTMLTextAreaElement).value,
                        })
                      }
                    />
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Output Kind</span>
                    <input
                      className="tcp-input text-sm"
                      value={selectedNode.outputKind}
                      onInput={(event) =>
                        updateNode(selectedNode.nodeId, {
                          outputKind: (event.target as HTMLInputElement).value,
                        })
                      }
                    />
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Required Output File</span>
                    <input
                      className="tcp-input text-sm"
                      placeholder="marketing-brief.md"
                      value={selectedNode.outputPath}
                      onInput={(event) =>
                        updateNode(selectedNode.nodeId, {
                          outputPath: (event.target as HTMLInputElement).value,
                        })
                      }
                    />
                    {selectedNodeOutputPathPreview ? (
                      <div className="rounded-lg border border-slate-700/60 bg-slate-950/30 px-3 py-2 text-[11px] text-slate-300">
                        <div>
                          Saved as:{" "}
                          <code>
                            {selectedNodeOutputPathPreview.canonical ||
                              selectedNodeOutputPathPreview.raw}
                          </code>
                        </div>
                        <div>
                          Next run preview: <code>{selectedNodeOutputPathPreview.resolved}</code>
                        </div>
                        {selectedNodeOutputPathPreview.warning ? (
                          <div className="text-amber-200">
                            {selectedNodeOutputPathPreview.warning}
                          </div>
                        ) : null}
                      </div>
                    ) : (
                      <span className="text-[11px] text-slate-500">
                        Use the same runtime tokens here as the workflow output targets.
                      </span>
                    )}
                  </label>
                  <label className="grid gap-1 sm:col-span-2">
                    <span className="text-xs text-slate-400">Input Files Contract</span>
                    <textarea
                      className="tcp-input min-h-[72px] text-sm"
                      placeholder="Comma-separated relative paths this stage should read"
                      value={joinCsv(selectedNode.inputFiles)}
                      onInput={(event) =>
                        updateNode(selectedNode.nodeId, {
                          inputFiles: splitCsv((event.target as HTMLTextAreaElement).value),
                        })
                      }
                    />
                    <span className="text-[11px] text-slate-500">
                      Effective contract:{" "}
                      {selectedNodeInputFiles.length
                        ? joinCsv(selectedNodeInputFiles)
                        : "No file inputs inferred from upstream stages."}
                    </span>
                  </label>
                  <label className="grid gap-1 sm:col-span-2">
                    <span className="text-xs text-slate-400">Output Files Contract</span>
                    <textarea
                      className="tcp-input min-h-[72px] text-sm"
                      placeholder="Comma-separated relative paths this stage must create"
                      value={joinCsv(selectedNode.outputFiles)}
                      onInput={(event) =>
                        updateNode(selectedNode.nodeId, {
                          outputFiles: splitCsv((event.target as HTMLTextAreaElement).value),
                        })
                      }
                    />
                    <span className="text-[11px] text-slate-500">
                      Effective contract:{" "}
                      {selectedNodeOutputFiles.length
                        ? joinCsv(selectedNodeOutputFiles)
                        : "No file outputs declared for this stage."}
                    </span>
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Task Kind</span>
                    <input
                      className="tcp-input text-sm"
                      placeholder="code_change"
                      value={selectedNode.taskKind || ""}
                      onInput={(event) =>
                        updateNode(selectedNode.nodeId, {
                          taskKind: (event.target as HTMLInputElement).value,
                        })
                      }
                    />
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Project Backlog Tasks</span>
                    <input
                      type="checkbox"
                      checked={Boolean(selectedNode.projectBacklogTasks)}
                      onChange={(event) =>
                        updateNode(selectedNode.nodeId, {
                          projectBacklogTasks: (event.target as HTMLInputElement).checked,
                        })
                      }
                    />
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Backlog Task ID</span>
                    <input
                      className="tcp-input text-sm"
                      placeholder="BACKLOG-123"
                      value={selectedNode.backlogTaskId || ""}
                      onInput={(event) =>
                        updateNode(selectedNode.nodeId, {
                          backlogTaskId: (event.target as HTMLInputElement).value,
                        })
                      }
                    />
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Repo Root</span>
                    <input
                      className="tcp-input text-sm"
                      placeholder="."
                      value={selectedNode.repoRoot || ""}
                      onInput={(event) =>
                        updateNode(selectedNode.nodeId, {
                          repoRoot: (event.target as HTMLInputElement).value,
                        })
                      }
                    />
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Write Scope</span>
                    <input
                      className="tcp-input text-sm"
                      placeholder="src/api, tests/api, Cargo.toml"
                      value={selectedNode.writeScope || ""}
                      onInput={(event) =>
                        updateNode(selectedNode.nodeId, {
                          writeScope: (event.target as HTMLInputElement).value,
                        })
                      }
                    />
                  </label>
                  <label className="grid gap-1 sm:col-span-2">
                    <span className="text-xs text-slate-400">Acceptance Criteria</span>
                    <input
                      className="tcp-input text-sm"
                      placeholder="Describe what must be true for this coding task to count as done."
                      value={selectedNode.acceptanceCriteria || ""}
                      onInput={(event) =>
                        updateNode(selectedNode.nodeId, {
                          acceptanceCriteria: (event.target as HTMLInputElement).value,
                        })
                      }
                    />
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Backlog Dependencies</span>
                    <input
                      className="tcp-input text-sm"
                      placeholder="BACKLOG-101, BACKLOG-102"
                      value={selectedNode.taskDependencies || ""}
                      onInput={(event) =>
                        updateNode(selectedNode.nodeId, {
                          taskDependencies: (event.target as HTMLInputElement).value,
                        })
                      }
                    />
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Verification State</span>
                    <input
                      className="tcp-input text-sm"
                      placeholder="pending"
                      value={selectedNode.verificationState || ""}
                      onInput={(event) =>
                        updateNode(selectedNode.nodeId, {
                          verificationState: (event.target as HTMLInputElement).value,
                        })
                      }
                    />
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Task Owner / Claimer</span>
                    <input
                      className="tcp-input text-sm"
                      placeholder="implementer"
                      value={selectedNode.taskOwner || ""}
                      onInput={(event) =>
                        updateNode(selectedNode.nodeId, {
                          taskOwner: (event.target as HTMLInputElement).value,
                        })
                      }
                    />
                  </label>
                  <label className="grid gap-1">
                    <span className="text-xs text-slate-400">Verification Command</span>
                    <input
                      className="tcp-input text-sm"
                      placeholder="cargo test -p tandem-server"
                      value={selectedNode.verificationCommand || ""}
                      onInput={(event) =>
                        updateNode(selectedNode.nodeId, {
                          verificationCommand: (event.target as HTMLInputElement).value,
                        })
                      }
                    />
                  </label>
                  <div className="grid gap-2">
                    <div className="text-xs text-slate-400">Dependencies</div>
                    <div className="flex flex-wrap gap-2">
                      {draft.nodes
                        .filter((node) => node.nodeId !== selectedNode.nodeId)
                        .map((node) => {
                          const enabled = selectedNode.dependsOn.includes(node.nodeId);
                          return (
                            <button
                              key={`${selectedNode.nodeId}-${node.nodeId}`}
                              className={
                                enabled
                                  ? "tcp-btn-primary inline-flex h-7 items-center gap-2 px-2 text-xs"
                                  : "tcp-btn inline-flex h-7 items-center gap-2 px-2 text-xs"
                              }
                              onClick={() => {
                                const dependsOn = enabled
                                  ? selectedNode.dependsOn.filter((dep) => dep !== node.nodeId)
                                  : [...selectedNode.dependsOn, node.nodeId];
                                updateNode(selectedNode.nodeId, { dependsOn });
                              }}
                            >
                              <i data-lucide={enabled ? "check" : "plus"}></i>
                              {node.title}
                            </button>
                          );
                        })}
                    </div>
                  </div>
                  {selectedNode.inputRefs.length ? (
                    <div className="grid gap-2">
                      <div className="text-xs text-slate-400">Input Aliases</div>
                      {selectedNode.inputRefs.map((ref) => (
                        <label
                          key={`${selectedNode.nodeId}-${ref.fromStepId}`}
                          className="grid gap-1"
                        >
                          <span className="text-xs text-slate-500">{ref.fromStepId}</span>
                          <input
                            className="tcp-input text-sm"
                            value={ref.alias}
                            onInput={(event) =>
                              updateNode(selectedNode.nodeId, {
                                inputRefs: selectedNode.inputRefs.map((entry) =>
                                  entry.fromStepId === ref.fromStepId
                                    ? { ...entry, alias: (event.target as HTMLInputElement).value }
                                    : entry
                                ),
                              })
                            }
                          />
                        </label>
                      ))}
                    </div>
                  ) : null}
                </div>
              ) : (
                <EmptyState text="Select a stage to edit it." />
              )}
            </PageCard>

            <PageCard
              title={
                selectedAgent
                  ? `Agent: ${selectedAgent.displayName || selectedAgent.agentId}`
                  : "Agent"
              }
              subtitle="Role prompt, policies, reusable template link, and model settings."
              actions={
                <button
                  className="tcp-btn inline-flex h-7 items-center gap-2 px-2 text-xs"
                  onClick={removeSelectedAgent}
                  disabled={!selectedAgent}
                >
                  <i data-lucide="trash-2"></i>
                  Remove Agent
                </button>
              }
            >
              {selectedAgent ? (
                <div className="grid gap-3">
                  <div className="grid gap-3 md:grid-cols-2">
                    <label className="grid gap-1">
                      <span className="text-xs text-slate-400">Display Name</span>
                      <input
                        className="tcp-input text-sm"
                        value={selectedAgent.displayName}
                        onInput={(event) =>
                          updateAgent(selectedAgent.agentId, {
                            displayName: (event.target as HTMLInputElement).value,
                          })
                        }
                      />
                    </label>
                    <label className="grid gap-1">
                      <span className="text-xs text-slate-400">Role</span>
                      <select
                        className="tcp-input text-sm"
                        value={selectedAgent.role}
                        onInput={(event) =>
                          updateAgent(selectedAgent.agentId, {
                            role: (event.target as HTMLSelectElement).value as StudioRole,
                          })
                        }
                      >
                        {ROLE_OPTIONS.map((role) => (
                          <option key={role} value={role}>
                            {role}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="grid gap-1 md:col-span-2">
                      <span className="text-xs text-slate-400">Skills</span>
                      <input
                        className="tcp-input text-sm"
                        value={joinCsv(selectedAgent.skills)}
                        onInput={(event) =>
                          updateAgent(selectedAgent.agentId, {
                            skills: splitCsv((event.target as HTMLInputElement).value),
                          })
                        }
                        placeholder="copywriting, websearch, qa"
                      />
                    </label>
                  </div>

                  <div className="rounded-xl border border-slate-700/60 bg-slate-950/30 p-3">
                    <div className="mb-2 flex items-center justify-between gap-2">
                      <div className="text-xs uppercase tracking-wide text-slate-500">
                        Template Link
                      </div>
                      {selectedAgent.linkedTemplateId ? (
                        <button
                          className="tcp-btn inline-flex h-7 items-center gap-2 px-2 text-xs"
                          onClick={() =>
                            updateAgent(selectedAgent.agentId, {
                              linkedTemplateId: "",
                              templateId: "",
                            })
                          }
                        >
                          <i data-lucide="unlink"></i>
                          Detach
                        </button>
                      ) : null}
                    </div>
                    <div className="grid gap-2 md:grid-cols-[minmax(0,1fr)_auto]">
                      <select
                        className="tcp-input text-sm"
                        value={selectedTemplateLoadId}
                        onInput={(event) =>
                          setSelectedTemplateLoadId((event.target as HTMLSelectElement).value)
                        }
                      >
                        <option value="">Select an existing agent template...</option>
                        {templateRows.map((template) => (
                          <option key={template.templateId} value={template.templateId}>
                            {template.displayName || template.templateId}
                          </option>
                        ))}
                      </select>
                      <button
                        className="tcp-btn inline-flex h-10 items-center gap-2 px-3 text-sm"
                        disabled={!selectedTemplateLoadId}
                        onClick={loadTemplateIntoSelectedAgent}
                      >
                        <i data-lucide="download"></i>
                        Load Template
                      </button>
                    </div>
                    <div className="mt-2 text-xs text-slate-400">
                      {repairState?.repairedAgentIds.includes(selectedAgent.agentId)
                        ? "This agent had a missing shared template link. Studio repaired it into a workflow-local prompt."
                        : selectedAgent.linkedTemplateId
                          ? templateMap.has(selectedAgent.linkedTemplateId)
                            ? `Linked template: ${selectedAgent.linkedTemplateId}`
                            : `Missing template link repaired locally: ${selectedAgent.linkedTemplateId}`
                          : "This agent is currently workflow-local unless you save reusable templates."}
                    </div>
                    <div className="mt-1 text-xs text-slate-500">
                      Local means Studio stores the prompt in workflow metadata. Linked means
                      runtime depends on a shared Agent Team template.
                    </div>
                  </div>

                  <div className="grid gap-3 md:grid-cols-2">
                    <label className="grid gap-1">
                      <span className="text-xs text-slate-400">Model Provider</span>
                      <select
                        className="tcp-input text-sm"
                        value={selectedAgent.modelProvider}
                        disabled={draft.useSharedModel}
                        onInput={(event) =>
                          updateAgent(
                            selectedAgent.agentId,
                            (() => {
                              const provider = (event.target as HTMLSelectElement).value;
                              const models = modelsForProvider(providerOptions, provider);
                              return {
                                modelProvider: provider,
                                modelId: models.includes(selectedAgent.modelId)
                                  ? selectedAgent.modelId
                                  : models[0] || selectedAgent.modelId,
                              };
                            })()
                          )
                        }
                      >
                        <option value="">Select provider...</option>
                        {providerOptions.map((provider) => (
                          <option key={provider.id} value={provider.id}>
                            {provider.id}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="grid gap-1">
                      <span className="text-xs text-slate-400">Model ID</span>
                      {modelsForProvider(providerOptions, selectedAgent.modelProvider).length ? (
                        <select
                          className="tcp-input text-sm"
                          value={selectedAgent.modelId}
                          disabled={draft.useSharedModel}
                          onInput={(event) =>
                            updateAgent(selectedAgent.agentId, {
                              modelId: (event.target as HTMLSelectElement).value,
                            })
                          }
                        >
                          {modelsForProvider(providerOptions, selectedAgent.modelProvider).map(
                            (model) => (
                              <option key={model} value={model}>
                                {model}
                              </option>
                            )
                          )}
                        </select>
                      ) : (
                        <input
                          className="tcp-input text-sm"
                          value={selectedAgent.modelId}
                          disabled={draft.useSharedModel}
                          onInput={(event) =>
                            updateAgent(selectedAgent.agentId, {
                              modelId: (event.target as HTMLInputElement).value,
                            })
                          }
                          placeholder="provider-specific model id"
                        />
                      )}
                    </label>
                    {draft.useSharedModel ? (
                      <div className="text-xs text-amber-200 md:col-span-2">
                        Per-agent model controls are locked because this workflow is using one
                        shared model for all agents.
                      </div>
                    ) : null}
                    <label className="grid gap-1 md:col-span-2">
                      <span className="text-xs text-slate-400">Tool Allowlist</span>
                      <input
                        className="tcp-input text-sm"
                        value={joinCsv(selectedAgent.toolAllowlist)}
                        onInput={(event) =>
                          updateAgent(selectedAgent.agentId, {
                            toolAllowlist: splitCsv((event.target as HTMLInputElement).value),
                          })
                        }
                      />
                    </label>
                    <label className="grid gap-1 md:col-span-2">
                      <span className="text-xs text-slate-400">Tool Denylist</span>
                      <input
                        className="tcp-input text-sm"
                        value={joinCsv(selectedAgent.toolDenylist)}
                        onInput={(event) =>
                          updateAgent(selectedAgent.agentId, {
                            toolDenylist: splitCsv((event.target as HTMLInputElement).value),
                          })
                        }
                      />
                    </label>
                    <label className="grid gap-1 md:col-span-2">
                      <span className="text-xs text-slate-400">Allowed MCP Servers</span>
                      <input
                        className="tcp-input text-sm"
                        value={joinCsv(selectedAgent.mcpAllowedServers)}
                        onInput={(event) =>
                          updateAgent(selectedAgent.agentId, {
                            mcpAllowedServers: splitCsv((event.target as HTMLInputElement).value),
                          })
                        }
                        placeholder={joinCsv(mcpServers) || "No MCP servers detected"}
                      />
                    </label>
                  </div>

                  <div className="grid gap-3">
                    <div className="text-xs uppercase tracking-wide text-slate-500">
                      Role Prompt
                    </div>
                    <label className="grid gap-1">
                      <span className="text-xs text-slate-400">Role</span>
                      <textarea
                        className="tcp-input min-h-[72px] text-sm"
                        value={selectedAgent.prompt.role}
                        onInput={(event) =>
                          updateAgent(selectedAgent.agentId, {
                            prompt: {
                              ...selectedAgent.prompt,
                              role: (event.target as HTMLTextAreaElement).value,
                            },
                          })
                        }
                      />
                    </label>
                    <label className="grid gap-1">
                      <span className="text-xs text-slate-400">Mission</span>
                      <textarea
                        className="tcp-input min-h-[92px] text-sm"
                        value={selectedAgent.prompt.mission}
                        onInput={(event) =>
                          updateAgent(selectedAgent.agentId, {
                            prompt: {
                              ...selectedAgent.prompt,
                              mission: (event.target as HTMLTextAreaElement).value,
                            },
                          })
                        }
                      />
                    </label>
                    <label className="grid gap-1">
                      <span className="text-xs text-slate-400">Inputs</span>
                      <textarea
                        className="tcp-input min-h-[72px] text-sm"
                        value={selectedAgent.prompt.inputs}
                        onInput={(event) =>
                          updateAgent(selectedAgent.agentId, {
                            prompt: {
                              ...selectedAgent.prompt,
                              inputs: (event.target as HTMLTextAreaElement).value,
                            },
                          })
                        }
                      />
                    </label>
                    <label className="grid gap-1">
                      <span className="text-xs text-slate-400">Output Contract</span>
                      <textarea
                        className="tcp-input min-h-[72px] text-sm"
                        value={selectedAgent.prompt.outputContract}
                        onInput={(event) =>
                          updateAgent(selectedAgent.agentId, {
                            prompt: {
                              ...selectedAgent.prompt,
                              outputContract: (event.target as HTMLTextAreaElement).value,
                            },
                          })
                        }
                      />
                    </label>
                    <label className="grid gap-1">
                      <span className="text-xs text-slate-400">Guardrails</span>
                      <textarea
                        className="tcp-input min-h-[72px] text-sm"
                        value={selectedAgent.prompt.guardrails}
                        onInput={(event) =>
                          updateAgent(selectedAgent.agentId, {
                            prompt: {
                              ...selectedAgent.prompt,
                              guardrails: (event.target as HTMLTextAreaElement).value,
                            },
                          })
                        }
                      />
                    </label>
                  </div>

                  <div className="rounded-xl border border-slate-700/60 bg-slate-950/40 p-3">
                    <div className="mb-2 text-xs uppercase tracking-wide text-slate-500">
                      Composed System Prompt
                    </div>
                    <pre className="whitespace-pre-wrap break-words text-xs text-slate-200">
                      {composePromptSections(selectedAgent.prompt) ||
                        "Prompt preview will appear here."}
                    </pre>
                  </div>
                </div>
              ) : (
                <EmptyState text="Select or add an agent to edit it." />
              )}
            </PageCard>
          </div>
        </div>
      </PageCard>
    </div>
  );
}
