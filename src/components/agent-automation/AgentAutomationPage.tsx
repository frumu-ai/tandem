import { useCallback, useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui";
import { ProjectSwitcher } from "@/components/sidebar";
import { AgentCommandCenter } from "@/components/orchestrate/AgentCommandCenter";
import {
  mcpConnect,
  mcpDisconnect,
  mcpListServers,
  mcpListTools,
  mcpRefresh,
  mcpSetEnabled,
  onSidecarEventV2,
  listProvidersFromSidecar,
  routinesCreate,
  routinesList,
  routinesPatch,
  routinesRunApprove,
  routinesRunDeny,
  routinesRunPause,
  routinesRunResume,
  routinesRunsAll,
  toolIds,
  type McpRemoteTool,
  type McpServerRecord,
  type ProviderInfo,
  type RoutineRunRecord,
  type RoutineSpec,
  type StreamEventEnvelopeV2,
  type UserProject,
} from "@/lib/tauri";

type AgentAutomationTab = "automated-bots" | "agent-ops";
type BotTemplateId = "daily-research" | "issue-triage" | "release-reporter";
type RunFilter = "all" | "pending" | "blocked" | "failed";
type AgentRoleId = "orchestrator" | "planner" | "worker" | "verifier" | "notifier";

interface ModelChoice {
  provider_id: string;
  model_id: string;
}

interface ModelPolicyDraft {
  default_model?: ModelChoice;
  role_models?: Partial<Record<AgentRoleId, ModelChoice>>;
}

interface ModelPreset {
  id: string;
  label: string;
  description: string;
  mode: "standalone" | "orchestrated" | "both";
  defaultModel: ModelChoice;
  roleModels?: Partial<Record<AgentRoleId, ModelChoice>>;
}

const AGENT_ROLES: AgentRoleId[] = ["orchestrator", "planner", "worker", "verifier", "notifier"];

const MODEL_PRESETS: ModelPreset[] = [
  {
    id: "openrouter-balanced",
    label: "OpenRouter Balanced",
    description: "Good default quality/cost balance for solo bots.",
    mode: "both",
    defaultModel: { provider_id: "openrouter", model_id: "openai/gpt-4.1-mini" },
  },
  {
    id: "openrouter-orchestrated",
    label: "OpenRouter Orchestrated",
    description: "Role-aware mapping for orchestrated runs.",
    mode: "orchestrated",
    defaultModel: { provider_id: "openrouter", model_id: "openai/gpt-4.1-mini" },
    roleModels: {
      orchestrator: { provider_id: "openrouter", model_id: "anthropic/claude-3.5-sonnet" },
      planner: { provider_id: "openrouter", model_id: "openai/gpt-4.1-mini" },
      worker: { provider_id: "openrouter", model_id: "openai/gpt-4.1-mini" },
      verifier: { provider_id: "openrouter", model_id: "anthropic/claude-3.5-sonnet" },
      notifier: { provider_id: "openrouter", model_id: "openai/gpt-4.1-mini" },
    },
  },
  {
    id: "opencode-zen-fast",
    label: "OpenCode Zen Fast",
    description: "Low-latency automation profile.",
    mode: "both",
    defaultModel: { provider_id: "opencode_zen", model_id: "zen/fast" },
  },
  {
    id: "opencode-zen-quality",
    label: "OpenCode Zen Quality",
    description: "Higher-quality output profile.",
    mode: "both",
    defaultModel: { provider_id: "opencode_zen", model_id: "zen/pro" },
  },
];

interface BotTemplate {
  id: BotTemplateId;
  label: string;
  description: string;
  name: string;
  intervalSeconds: number;
  entrypoint: "mission.default" | "mcp_first_tool";
  allowedTools: string[];
  requiresApproval: boolean;
  externalAllowed: boolean;
  outputTargets: string[];
  missionObjective: string;
  successCriteria: string[];
}

interface WorkshopMessage {
  id: string;
  role: "user" | "assistant";
  text: string;
}

interface MissionDraft {
  objective: string;
  successCriteria: string[];
  suggestedMode: "standalone" | "orchestrated";
}

interface RunTimelineEvent {
  eventType: string;
  phase: "plan" | "do" | "verify" | "approval" | "blocked" | "failed" | "info";
  tsMs: number;
  note?: string;
}

function buildMissionDraft(brief: string, tools: string[]): MissionDraft {
  const normalized = brief.trim();
  const lower = normalized.toLowerCase();
  const suggestedMode =
    lower.includes("verify") || lower.includes("multi") || lower.includes("workflow")
      ? "orchestrated"
      : "standalone";
  const objective =
    normalized.length > 0
      ? normalized
      : "Run a scheduled automation that gathers context, performs the task, and produces a clear artifact.";
  const successCriteria = [
    "Produces at least one output artifact at configured output targets.",
    "Uses only allowed tools and records run events.",
    suggestedMode === "orchestrated"
      ? "Verifier confirms output quality before completion."
      : "Completes without blocked policy or approval timeout.",
  ];
  if (tools.some((tool) => tool === "webfetch_document")) {
    successCriteria.push("Uses webfetch_document for web content extraction when relevant.");
  }
  return { objective, successCriteria, suggestedMode };
}

function modeFromArgs(args: Record<string, unknown> | undefined): string {
  const value = typeof args?.["mode"] === "string" ? args["mode"].trim() : "";
  return value || "standalone";
}

function parseModelChoice(value: unknown): ModelChoice | null {
  if (!isRecord(value)) return null;
  const provider_id = asString(value["provider_id"]);
  const model_id = asString(value["model_id"]);
  if (!provider_id || !model_id) return null;
  return { provider_id, model_id };
}

function parseModelPolicy(args: Record<string, unknown> | undefined): ModelPolicyDraft | null {
  if (!args) return null;
  const policyRaw = args["model_policy"];
  if (!isRecord(policyRaw)) return null;
  const policy: ModelPolicyDraft = {};
  const defaultModel = parseModelChoice(policyRaw["default_model"]);
  if (defaultModel) {
    policy.default_model = defaultModel;
  }
  const roleModelsRaw = policyRaw["role_models"];
  if (isRecord(roleModelsRaw)) {
    const roleModels: Partial<Record<AgentRoleId, ModelChoice>> = {};
    for (const role of AGENT_ROLES) {
      const parsed = parseModelChoice(roleModelsRaw[role]);
      if (parsed) {
        roleModels[role] = parsed;
      }
    }
    if (Object.keys(roleModels).length > 0) {
      policy.role_models = roleModels;
    }
  }
  return Object.keys(policy).length > 0 ? policy : null;
}

function modelChoiceLabel(choice: ModelChoice | null | undefined): string | null {
  if (!choice) return null;
  return `${choice.provider_id}/${choice.model_id}`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function asString(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0 ? value : null;
}

function extractRunId(data: unknown): string | null {
  if (!isRecord(data)) return null;
  const direct = asString(data["runID"]) ?? asString(data["run_id"]);
  if (direct) return direct;
  const run = data["run"];
  if (isRecord(run)) {
    return asString(run["run_id"]) ?? asString(run["runID"]);
  }
  return null;
}

function extractRunNote(eventType: string, data: unknown): string | undefined {
  if (!isRecord(data)) return undefined;
  if (eventType === "routine.approval_required" || eventType === "approval.required") {
    return asString(data["reason"]) ?? "Approval required";
  }
  if (eventType === "routine.blocked") {
    return asString(data["reason"]) ?? "Blocked by policy";
  }
  if (eventType === "routine.run.failed" || eventType === "run.failed") {
    return asString(data["reason"]) ?? asString(data["detail"]) ?? "Run failed";
  }
  if (eventType === "routine.run.artifact_added") {
    const artifact = data["artifact"];
    if (isRecord(artifact)) {
      return asString(artifact["uri"]) ?? "Artifact added";
    }
    return "Artifact added";
  }
  return undefined;
}

function phaseFromEventType(eventType: string, data: unknown): RunTimelineEvent["phase"] {
  if (eventType === "routine.run.created") return "plan";
  if (eventType === "routine.run.started" || eventType === "routine.fired") return "do";
  if (eventType === "routine.run.completed") return "verify";
  if (eventType === "routine.approval_required" || eventType === "approval.required") {
    return "approval";
  }
  if (eventType === "routine.blocked") return "blocked";
  if (eventType === "routine.run.failed" || eventType === "run.failed") return "failed";
  if (eventType === "run.step") {
    if (isRecord(data) && asString(data["phase"]) === "do") return "do";
    return "info";
  }
  if (eventType === "run.completed") return "verify";
  return "info";
}

function eventLabel(event: RunTimelineEvent): string {
  if (event.phase === "approval") return "Approval";
  if (event.phase === "blocked") return "Blocked";
  if (event.phase === "failed") return "Failed";
  if (event.phase === "plan") return "Plan";
  if (event.phase === "do") return "Do";
  if (event.phase === "verify") return "Verify";
  return "Info";
}

function formatTimestamp(tsMs: number | null | undefined): string {
  if (!tsMs || !Number.isFinite(tsMs)) return "n/a";
  try {
    return new Date(tsMs).toLocaleString();
  } catch {
    return "n/a";
  }
}

function runReason(run: RoutineRunRecord): string | null {
  const detail =
    typeof run.detail === "string" && run.detail.trim().length > 0 ? run.detail.trim() : null;
  if (detail) return detail;
  const approval =
    typeof run.approval_reason === "string" && run.approval_reason.trim().length > 0
      ? run.approval_reason.trim()
      : null;
  if (approval) return approval;
  const denial =
    typeof run.denial_reason === "string" && run.denial_reason.trim().length > 0
      ? run.denial_reason.trim()
      : null;
  if (denial) return denial;
  const paused =
    typeof run.paused_reason === "string" && run.paused_reason.trim().length > 0
      ? run.paused_reason.trim()
      : null;
  return paused;
}

interface AgentAutomationPageProps {
  userProjects: UserProject[];
  activeProject: UserProject | null;
  onSwitchProject: (projectId: string) => void;
  onAddProject: () => void;
  onManageProjects: () => void;
  projectSwitcherLoading?: boolean;
  onOpenMcpExtensions?: () => void;
}

export function AgentAutomationPage({
  userProjects,
  activeProject,
  onSwitchProject,
  onAddProject,
  onManageProjects,
  projectSwitcherLoading = false,
  onOpenMcpExtensions,
}: AgentAutomationPageProps) {
  const [tab, setTab] = useState<AgentAutomationTab>("automated-bots");
  const [error, setError] = useState<string | null>(null);

  const [mcpServers, setMcpServers] = useState<McpServerRecord[]>([]);
  const [mcpTools, setMcpTools] = useState<McpRemoteTool[]>([]);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [mcpLoading, setMcpLoading] = useState(false);
  const [busyConnector, setBusyConnector] = useState<string | null>(null);
  const [availableToolIds, setAvailableToolIds] = useState<string[]>([]);

  const [routines, setRoutines] = useState<RoutineSpec[]>([]);
  const [routinesLoading, setRoutinesLoading] = useState(false);
  const [createRoutineLoading, setCreateRoutineLoading] = useState(false);
  const [routineNameDraft, setRoutineNameDraft] = useState("MCP Automation");
  const [routineEntrypointDraft, setRoutineEntrypointDraft] = useState("mission.default");
  const [routineIntervalSecondsDraft, setRoutineIntervalSecondsDraft] = useState(300);
  const [routineAllowedToolsDraft, setRoutineAllowedToolsDraft] = useState<string[]>([]);
  const [routineMissionObjectiveDraft, setRoutineMissionObjectiveDraft] = useState("");
  const [routineSuccessCriteriaDraft, setRoutineSuccessCriteriaDraft] = useState("");
  const [routineModeDraft, setRoutineModeDraft] = useState<"standalone" | "orchestrated">(
    "standalone"
  );
  const [routineModelProviderDraft, setRoutineModelProviderDraft] = useState("");
  const [routineModelIdDraft, setRoutineModelIdDraft] = useState("");
  const [routineRoleModelDrafts, setRoutineRoleModelDrafts] = useState<
    Partial<Record<AgentRoleId, ModelChoice>>
  >({});
  const [routineModelPresetDraft, setRoutineModelPresetDraft] = useState(MODEL_PRESETS[0].id);
  const [routineOrchestratorOnlyToolCallsDraft, setRoutineOrchestratorOnlyToolCallsDraft] =
    useState(false);
  const [routineOutputTargetsDraft, setRoutineOutputTargetsDraft] = useState("");
  const [routineRequiresApprovalDraft, setRoutineRequiresApprovalDraft] = useState(true);
  const [routineExternalAllowedDraft, setRoutineExternalAllowedDraft] = useState(true);
  const [workshopInputDraft, setWorkshopInputDraft] = useState("");
  const [workshopMessages, setWorkshopMessages] = useState<WorkshopMessage[]>([
    {
      id: "workshop-welcome",
      role: "assistant",
      text: "Mission Workshop: describe the bot mission in plain language. I will suggest an objective, success criteria, and default mode.",
    },
  ]);

  const [routineRuns, setRoutineRuns] = useState<RoutineRunRecord[]>([]);
  const [routineRunsLoading, setRoutineRunsLoading] = useState(false);
  const [routineActionBusyRunId, setRoutineActionBusyRunId] = useState<string | null>(null);
  const [runTimeline, setRunTimeline] = useState<Record<string, RunTimelineEvent[]>>({});
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [runFilter, setRunFilter] = useState<RunFilter>("all");

  const templates: BotTemplate[] = useMemo(
    () => [
      {
        id: "daily-research",
        label: "Daily Research",
        description: "Web + MCP research with markdown extraction output.",
        name: "Daily MCP Research",
        intervalSeconds: 86400,
        entrypoint: "mission.default",
        allowedTools: ["websearch", "webfetch_document", "read", "write"],
        requiresApproval: true,
        externalAllowed: true,
        outputTargets: ["file://reports/daily-mcp-research.md"],
        missionObjective:
          "Research daily topic signals and produce a concise markdown digest with citations.",
        successCriteria: [
          "Includes top findings with source URLs.",
          "Writes an artifact to the configured report path.",
          "Highlights uncertain claims and verification notes.",
        ],
      },
      {
        id: "issue-triage",
        label: "Issue Triage",
        description: "Classify inbound issues and post suggested actions.",
        name: "Issue Triage Bot",
        intervalSeconds: 900,
        entrypoint: "mission.default",
        allowedTools: ["read", "write", "websearch", "webfetch_document"],
        requiresApproval: true,
        externalAllowed: true,
        outputTargets: ["file://reports/issue-triage.json"],
        missionObjective:
          "Review incoming issues, classify severity, and draft recommended next actions.",
        successCriteria: [
          "Classifies each issue into priority buckets.",
          "Includes suggested owner/team when inferable.",
          "Produces a machine-readable triage artifact.",
        ],
      },
      {
        id: "release-reporter",
        label: "Release Reporter",
        description: "Compile status updates into a periodic artifact report.",
        name: "Release Reporter",
        intervalSeconds: 3600,
        entrypoint: "mission.default",
        allowedTools: ["read", "websearch", "webfetch_document", "write"],
        requiresApproval: false,
        externalAllowed: true,
        outputTargets: ["file://reports/release-status.md"],
        missionObjective:
          "Compile release readiness status and generate an hourly summary report for operators.",
        successCriteria: [
          "Summarizes current release blockers and risks.",
          "Writes a markdown status report artifact.",
          "Runs unattended without policy violations.",
        ],
      },
    ],
    []
  );

  const loadMcpStatus = useCallback(async () => {
    setMcpLoading(true);
    try {
      const [servers, tools] = await Promise.all([mcpListServers(), mcpListTools()]);
      setMcpServers(servers);
      setMcpTools(tools);
    } catch {
      setMcpServers([]);
      setMcpTools([]);
    } finally {
      setMcpLoading(false);
    }
  }, []);

  const loadProviders = useCallback(async () => {
    try {
      const rows = await listProvidersFromSidecar();
      setProviders(rows.filter((row) => row.models.length > 0));
    } catch {
      setProviders([]);
    }
  }, []);

  const loadToolCatalog = useCallback(async () => {
    try {
      const ids = await toolIds();
      setAvailableToolIds(
        [...new Set(ids.map((id) => id.trim()).filter((id) => id.length > 0))].sort()
      );
    } catch {
      setAvailableToolIds([]);
    }
  }, []);

  const loadRoutines = useCallback(async () => {
    setRoutinesLoading(true);
    try {
      const rows = await routinesList();
      rows.sort((a, b) => a.routine_id.localeCompare(b.routine_id));
      setRoutines(rows);
    } catch {
      setRoutines([]);
    } finally {
      setRoutinesLoading(false);
    }
  }, []);

  const loadRoutineRuns = useCallback(async () => {
    setRoutineRunsLoading(true);
    try {
      const rows = await routinesRunsAll(undefined, 30);
      rows.sort((a, b) => b.created_at_ms - a.created_at_ms);
      setRoutineRuns(rows);
    } catch {
      setRoutineRuns([]);
    } finally {
      setRoutineRunsLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadMcpStatus();
    const timer = setInterval(() => void loadMcpStatus(), 10000);
    return () => clearInterval(timer);
  }, [loadMcpStatus]);

  useEffect(() => {
    void loadProviders();
    const timer = setInterval(() => void loadProviders(), 20000);
    return () => clearInterval(timer);
  }, [loadProviders]);

  useEffect(() => {
    void loadToolCatalog();
    const timer = setInterval(() => void loadToolCatalog(), 15000);
    return () => clearInterval(timer);
  }, [loadToolCatalog]);

  useEffect(() => {
    void loadRoutines();
    const timer = setInterval(() => void loadRoutines(), 15000);
    return () => clearInterval(timer);
  }, [loadRoutines]);

  useEffect(() => {
    void loadRoutineRuns();
    const timer = setInterval(() => void loadRoutineRuns(), 10000);
    return () => clearInterval(timer);
  }, [loadRoutineRuns]);

  useEffect(() => {
    const knownRunIds = new Set(routineRuns.map((run) => run.run_id));
    if (routineRuns.length > 0 && !selectedRunId) {
      setSelectedRunId(routineRuns[0].run_id);
    }
    if (selectedRunId && !knownRunIds.has(selectedRunId)) {
      setSelectedRunId(routineRuns.length > 0 ? routineRuns[0].run_id : null);
    }
    setRunTimeline((prev) => {
      const next: Record<string, RunTimelineEvent[]> = {};
      for (const [runId, events] of Object.entries(prev)) {
        if (knownRunIds.has(runId)) {
          next[runId] = events;
        }
      }
      return next;
    });
  }, [routineRuns, selectedRunId]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    const setup = async () => {
      unlisten = await onSidecarEventV2((envelope: StreamEventEnvelopeV2) => {
        if (envelope?.payload?.type !== "raw") {
          return;
        }
        const eventType = envelope.payload.event_type;
        if (eventType.startsWith("mcp.")) {
          void loadMcpStatus();
          void loadToolCatalog();
          return;
        }
        if (eventType.startsWith("routine.") || eventType.startsWith("run.")) {
          const data = envelope.payload.data;
          const runId = extractRunId(data);
          if (runId) {
            const nextEvent: RunTimelineEvent = {
              eventType,
              phase: phaseFromEventType(eventType, data),
              tsMs: envelope.ts_ms,
              note: extractRunNote(eventType, data),
            };
            setRunTimeline((prev) => {
              const current = prev[runId] ?? [];
              const merged = [...current, nextEvent].slice(-8);
              return { ...prev, [runId]: merged };
            });
          }
        }
        if (eventType.startsWith("routine.")) {
          void loadRoutines();
          void loadRoutineRuns();
          return;
        }
        if (eventType.startsWith("run.") || eventType.startsWith("approval.")) {
          void loadRoutineRuns();
          return;
        }
        if (eventType.startsWith("agent_team.")) {
          // Agent Ops tab handles its own refresh; this keeps page-level state simple.
        }
      });
    };
    void setup();
    return () => {
      if (unlisten) unlisten();
    };
  }, [loadMcpStatus, loadRoutineRuns, loadRoutines, loadToolCatalog]);

  const mcpToolIds = useMemo(
    () =>
      [...new Set(mcpTools.map((tool) => tool.namespaced_name))]
        .filter((tool) => tool.trim().length > 0)
        .sort(),
    [mcpTools]
  );

  const allowlistChoices = useMemo(
    () =>
      [
        ...new Set([
          ...availableToolIds,
          "read",
          "write",
          "bash",
          "websearch",
          "webfetch_document",
          ...mcpToolIds,
        ]),
      ]
        .filter((tool) => tool.trim().length > 0)
        .sort(),
    [availableToolIds, mcpToolIds]
  );

  const providerModelMap = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const provider of providers) {
      map.set(provider.id, [...provider.models]);
    }
    return map;
  }, [providers]);

  const defaultProviderModels = useMemo(
    () => providerModelMap.get(routineModelProviderDraft) ?? [],
    [providerModelMap, routineModelProviderDraft]
  );

  const providerOptions = useMemo(
    () => providers.map((provider) => provider.id).sort((a, b) => a.localeCompare(b)),
    [providers]
  );

  useEffect(() => {
    if (routineAllowedToolsDraft.length > 0) return;
    const defaults = ["read", "websearch", "webfetch_document"];
    if (mcpToolIds.length > 0) {
      defaults.push(mcpToolIds[0]);
    }
    setRoutineAllowedToolsDraft(defaults);
  }, [mcpToolIds, routineAllowedToolsDraft.length]);

  useEffect(() => {
    if (providerOptions.length === 0) return;
    if (!routineModelProviderDraft || !providerModelMap.has(routineModelProviderDraft)) {
      const firstProvider = providerOptions[0];
      const firstModel = providerModelMap.get(firstProvider)?.[0] ?? "";
      setRoutineModelProviderDraft(firstProvider);
      setRoutineModelIdDraft(firstModel);
      return;
    }
    const models = providerModelMap.get(routineModelProviderDraft) ?? [];
    if (models.length === 0) return;
    if (!models.includes(routineModelIdDraft)) {
      setRoutineModelIdDraft(models[0]);
    }
  }, [
    providerOptions,
    providerModelMap,
    routineModelIdDraft,
    routineModelProviderDraft,
    setRoutineModelIdDraft,
    setRoutineModelProviderDraft,
  ]);

  const applyTemplate = (template: BotTemplate) => {
    const firstMcpTool = mcpToolIds[0];
    const nextEntrypoint =
      template.entrypoint === "mcp_first_tool" && firstMcpTool ? firstMcpTool : "mission.default";
    const mergedTools = [...template.allowedTools];
    if (firstMcpTool && !mergedTools.includes(firstMcpTool)) {
      mergedTools.push(firstMcpTool);
    }
    setRoutineNameDraft(template.name);
    setRoutineIntervalSecondsDraft(template.intervalSeconds);
    setRoutineEntrypointDraft(nextEntrypoint);
    setRoutineAllowedToolsDraft(mergedTools);
    setRoutineRequiresApprovalDraft(template.requiresApproval);
    setRoutineExternalAllowedDraft(template.externalAllowed);
    setRoutineOutputTargetsDraft(template.outputTargets.join(", "));
    setRoutineMissionObjectiveDraft(template.missionObjective);
    setRoutineSuccessCriteriaDraft(template.successCriteria.join("\n"));
    setRoutineModeDraft("standalone");
    setRoutineRoleModelDrafts({});
    setRoutineOrchestratorOnlyToolCallsDraft(false);
  };

  useEffect(() => {
    if (routineMissionObjectiveDraft.trim().length > 0) return;
    setRoutineMissionObjectiveDraft(
      "Run a scheduled automation with a clear mission objective and artifact output."
    );
    setRoutineSuccessCriteriaDraft(
      "Produces one artifact per run.\nUses only allowed tools.\nLogs run events for observability."
    );
  }, [routineMissionObjectiveDraft]);

  const formatIntervalHint = (seconds: number): string => {
    if (!Number.isFinite(seconds) || seconds <= 0) return "";
    if (seconds % 86400 === 0) {
      const days = seconds / 86400;
      return `every ${days} day${days === 1 ? "" : "s"}`;
    }
    if (seconds % 3600 === 0) {
      const hours = seconds / 3600;
      return `every ${hours} hour${hours === 1 ? "" : "s"}`;
    }
    if (seconds % 60 === 0) {
      const mins = seconds / 60;
      return `every ${mins} minute${mins === 1 ? "" : "s"}`;
    }
    return `every ${seconds} second${seconds === 1 ? "" : "s"}`;
  };

  const toggleRoutineAllowedTool = (toolId: string) => {
    setRoutineAllowedToolsDraft((prev) => {
      if (prev.includes(toolId)) {
        return prev.filter((row) => row !== toolId);
      }
      return [...prev, toolId];
    });
  };

  const setRoleModelDraft = (role: AgentRoleId, providerId: string, modelId: string) => {
    setRoutineRoleModelDrafts((prev) => {
      const next = { ...prev };
      if (!providerId || !modelId) {
        delete next[role];
      } else {
        next[role] = { provider_id: providerId, model_id: modelId };
      }
      return next;
    });
  };

  const applyModelPreset = () => {
    const preset = MODEL_PRESETS.find((row) => row.id === routineModelPresetDraft);
    if (!preset) return;

    setRoutineModelProviderDraft(preset.defaultModel.provider_id);
    setRoutineModelIdDraft(preset.defaultModel.model_id);
    if (preset.mode === "orchestrated") {
      setRoutineModeDraft("orchestrated");
    }
    setRoutineRoleModelDrafts(preset.roleModels ?? {});
  };

  const applyMissionDraft = (draft: MissionDraft) => {
    setRoutineMissionObjectiveDraft(draft.objective);
    setRoutineSuccessCriteriaDraft(draft.successCriteria.join("\n"));
    setRoutineModeDraft(draft.suggestedMode);
  };

  const handleWorkshopSubmit = () => {
    const text = workshopInputDraft.trim();
    if (!text) return;
    const userMessage: WorkshopMessage = {
      id: `workshop-user-${Date.now()}`,
      role: "user",
      text,
    };
    const draft = buildMissionDraft(text, routineAllowedToolsDraft);
    const assistantText = [
      `Suggested objective: ${draft.objective}`,
      `Suggested mode: ${draft.suggestedMode}`,
      "Suggested success criteria:",
      ...draft.successCriteria.map((row, index) => `${index + 1}. ${row}`),
    ].join("\n");
    const assistantMessage: WorkshopMessage = {
      id: `workshop-assistant-${Date.now()}`,
      role: "assistant",
      text: assistantText,
    };
    setWorkshopMessages((prev) => [...prev, userMessage, assistantMessage]);
    setWorkshopInputDraft("");
    applyMissionDraft(draft);
  };

  const handleCreateRoutine = async () => {
    const trimmedName = routineNameDraft.trim();
    if (!trimmedName) {
      setError("Routine name is required.");
      return;
    }
    const missionObjective = routineMissionObjectiveDraft.trim();
    if (!missionObjective) {
      setError("Mission objective is required.");
      return;
    }
    const successCriteria = routineSuccessCriteriaDraft
      .split("\n")
      .map((row) => row.trim())
      .filter((row) => row.length > 0);
    const intervalSeconds = Math.max(1, Math.floor(routineIntervalSecondsDraft));
    const outputTargets = routineOutputTargetsDraft
      .split(",")
      .map((value) => value.trim())
      .filter((value) => value.length > 0);
    const modelPolicy: ModelPolicyDraft = {};
    if (routineModelProviderDraft.trim() && routineModelIdDraft.trim()) {
      modelPolicy.default_model = {
        provider_id: routineModelProviderDraft.trim(),
        model_id: routineModelIdDraft.trim(),
      };
    }
    const roleModels: Partial<Record<AgentRoleId, ModelChoice>> = {};
    for (const role of AGENT_ROLES) {
      const draft = routineRoleModelDrafts[role];
      if (draft?.provider_id?.trim() && draft.model_id?.trim()) {
        roleModels[role] = {
          provider_id: draft.provider_id.trim(),
          model_id: draft.model_id.trim(),
        };
      }
    }
    if (Object.keys(roleModels).length > 0) {
      modelPolicy.role_models = roleModels;
    }
    const hasModelPolicy = Object.keys(modelPolicy).length > 0;

    setCreateRoutineLoading(true);
    setError(null);
    try {
      await routinesCreate({
        name: trimmedName,
        schedule: { interval_seconds: { seconds: intervalSeconds } },
        entrypoint: routineEntrypointDraft.trim() || "mission.default",
        args: {
          prompt: missionObjective,
          success_criteria: successCriteria,
          mode: routineModeDraft,
          orchestrator_only_tool_calls: routineOrchestratorOnlyToolCallsDraft,
          ...(hasModelPolicy ? { model_policy: modelPolicy } : {}),
        },
        allowed_tools: routineAllowedToolsDraft,
        output_targets: outputTargets,
        requires_approval: routineRequiresApprovalDraft,
        external_integrations_allowed: routineExternalAllowedDraft,
      });
      await Promise.all([loadRoutines(), loadRoutineRuns()]);
      setRoutineNameDraft("MCP Automation");
      setRoutineIntervalSecondsDraft(300);
      setRoutineOutputTargetsDraft("");
      setRoutineMissionObjectiveDraft("");
      setRoutineSuccessCriteriaDraft("");
      setRoutineModeDraft("standalone");
      setRoutineRoleModelDrafts({});
      setRoutineOrchestratorOnlyToolCallsDraft(false);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(`Create routine failed: ${message}`);
    } finally {
      setCreateRoutineLoading(false);
    }
  };

  const handleToggleRoutineStatus = async (routine: RoutineSpec) => {
    try {
      await routinesPatch(routine.routine_id, {
        status: routine.status === "active" ? "paused" : "active",
      });
      await loadRoutines();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(`Update routine failed: ${message}`);
    }
  };

  const handleRoutineRunAction = async (
    run: RoutineRunRecord,
    action: "approve" | "deny" | "pause" | "resume"
  ) => {
    setRoutineActionBusyRunId(run.run_id);
    try {
      if (action === "approve") {
        await routinesRunApprove(run.run_id);
      } else if (action === "deny") {
        await routinesRunDeny(run.run_id);
      } else if (action === "pause") {
        await routinesRunPause(run.run_id);
      } else {
        await routinesRunResume(run.run_id);
      }
      await loadRoutineRuns();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(`Routine run action failed: ${message}`);
    } finally {
      setRoutineActionBusyRunId(null);
    }
  };

  const handleConnectorAction = async (
    serverName: string,
    action: "set-enabled" | "connect" | "disconnect" | "refresh",
    nextEnabled?: boolean
  ) => {
    setBusyConnector(`${serverName}:${action}`);
    setError(null);
    try {
      if (action === "set-enabled") {
        await mcpSetEnabled(serverName, !!nextEnabled);
      } else if (action === "connect") {
        await mcpConnect(serverName);
      } else if (action === "disconnect") {
        await mcpDisconnect(serverName);
      } else {
        await mcpRefresh(serverName);
      }
      await loadMcpStatus();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(`Connector action failed: ${message}`);
    } finally {
      setBusyConnector(null);
    }
  };

  const connectedConnectors = mcpServers.filter((row) => row.connected).length;
  const activeRoutines = routines.filter((routine) => routine.status === "active").length;
  const pendingApprovals = routineRuns.filter((run) => run.status === "pending_approval").length;
  const blockedRuns = routineRuns.filter((run) => run.status === "blocked_policy").length;
  const failedRuns = routineRuns.filter((run) => run.status === "failed").length;
  const artifactCount = routineRuns.reduce((sum, run) => sum + run.artifacts.length, 0);
  const visibleRuns = routineRuns.filter((run) => {
    if (runFilter === "pending") return run.status === "pending_approval";
    if (runFilter === "blocked") return run.status === "blocked_policy";
    if (runFilter === "failed") return run.status === "failed";
    return true;
  });
  const selectedRun = selectedRunId
    ? (routineRuns.find((run) => run.run_id === selectedRunId) ?? null)
    : null;
  const selectedRunEvents = selectedRun ? (runTimeline[selectedRun.run_id] ?? []) : [];
  const selectedRunLatestEvent =
    selectedRunEvents.length > 0 ? selectedRunEvents[selectedRunEvents.length - 1] : null;

  return (
    <div className="h-full overflow-y-auto p-4">
      <div className="mx-auto max-w-[1600px] space-y-4">
        <div className="rounded-lg border border-border bg-surface p-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h2 className="text-lg font-semibold text-text">Agent Automation</h2>
              <p className="text-xs text-text-muted">
                Scheduled bots, MCP connector operations, approvals, and runtime visibility.
              </p>
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant={tab === "automated-bots" ? "primary" : "secondary"}
                size="sm"
                onClick={() => setTab("automated-bots")}
              >
                Automated Bots
              </Button>
              <Button
                variant={tab === "agent-ops" ? "primary" : "secondary"}
                size="sm"
                onClick={() => setTab("agent-ops")}
              >
                Agent Ops
              </Button>
            </div>
          </div>
        </div>

        <div className="rounded-lg border border-border bg-surface p-4">
          <ProjectSwitcher
            projects={userProjects}
            activeProject={activeProject}
            onSwitchProject={onSwitchProject}
            onAddProject={onAddProject}
            onManageProjects={onManageProjects}
            isLoading={projectSwitcherLoading}
          />
        </div>

        {error ? (
          <div className="rounded border border-red-500/30 bg-red-500/10 p-2 text-xs text-red-200">
            {error}
          </div>
        ) : null}

        {tab === "automated-bots" ? (
          <>
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-5">
              <div className="rounded-md border border-border bg-surface p-3">
                <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                  Active Routines
                </div>
                <div className="text-lg font-semibold text-text">{activeRoutines}</div>
              </div>
              <div className="rounded-md border border-border bg-surface p-3">
                <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                  Needs Approval
                </div>
                <div className="text-lg font-semibold text-text">{pendingApprovals}</div>
              </div>
              <div className="rounded-md border border-border bg-surface p-3">
                <div className="text-[10px] uppercase tracking-wide text-text-subtle">Blocked</div>
                <div className="text-lg font-semibold text-text">{blockedRuns}</div>
              </div>
              <div className="rounded-md border border-border bg-surface p-3">
                <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                  Connected MCP
                </div>
                <div className="text-lg font-semibold text-text">
                  {connectedConnectors}/{mcpServers.length}
                </div>
              </div>
              <div className="rounded-md border border-border bg-surface p-3">
                <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                  Artifacts
                </div>
                <div className="text-lg font-semibold text-text">{artifactCount}</div>
              </div>
            </div>

            <div className="rounded-lg border border-border bg-surface p-4">
              <div className="flex items-center justify-between gap-2">
                <div className="text-xs uppercase tracking-wide text-text-subtle">
                  Automation Wiring
                </div>
                <div className="text-xs text-text-muted">
                  {routinesLoading ? "Refreshing..." : `${routines.length} configured`}
                </div>
              </div>
              <div className="mt-2 grid grid-cols-1 gap-3 lg:grid-cols-2">
                <div className="rounded-md border border-border bg-surface-elevated/40 p-3">
                  <div className="text-xs font-semibold text-text">Create Scheduled Bot</div>
                  <div className="mt-2">
                    <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                      Ready Templates
                    </div>
                    <div className="mt-1 grid grid-cols-1 gap-1 sm:grid-cols-3">
                      {templates.map((template) => (
                        <button
                          key={template.id}
                          type="button"
                          className="rounded border border-border bg-surface px-2 py-1 text-left hover:border-primary/40 hover:bg-surface-elevated"
                          onClick={() => applyTemplate(template)}
                          title={template.description}
                        >
                          <div className="text-[11px] font-semibold text-text">
                            {template.label}
                          </div>
                          <div className="truncate text-[10px] text-text-muted">
                            {template.description}
                          </div>
                        </button>
                      ))}
                    </div>
                  </div>
                  <div className="mt-3 rounded border border-border bg-surface p-2">
                    <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                      Mission Workshop
                    </div>
                    <div className="mt-1 max-h-28 space-y-1 overflow-y-auto rounded border border-border/60 bg-surface-elevated/30 p-2">
                      {workshopMessages.slice(-6).map((message) => (
                        <div key={message.id} className="text-[11px] text-text">
                          <span className="font-semibold text-text-subtle">
                            {message.role === "assistant" ? "Workshop" : "You"}:
                          </span>{" "}
                          <span className="whitespace-pre-wrap">{message.text}</span>
                        </div>
                      ))}
                    </div>
                    <div className="mt-2 flex gap-2">
                      <input
                        value={workshopInputDraft}
                        onChange={(event) => setWorkshopInputDraft(event.target.value)}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") {
                            event.preventDefault();
                            handleWorkshopSubmit();
                          }
                        }}
                        className="w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                        placeholder="Describe the mission in plain language..."
                      />
                      <Button size="sm" variant="secondary" onClick={handleWorkshopSubmit}>
                        Suggest
                      </Button>
                    </div>
                  </div>
                  <div className="mt-2 space-y-2">
                    <input
                      value={routineNameDraft}
                      onChange={(event) => setRoutineNameDraft(event.target.value)}
                      className="w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                      placeholder="Routine name"
                    />
                    <div className="grid grid-cols-2 gap-2">
                      <div className="space-y-1">
                        <input
                          type="number"
                          min={1}
                          value={routineIntervalSecondsDraft}
                          onChange={(event) =>
                            setRoutineIntervalSecondsDraft(
                              Number.parseInt(event.target.value || "300", 10)
                            )
                          }
                          className="w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                          placeholder="Interval (seconds)"
                        />
                        <div className="text-[10px] text-text-subtle">
                          Unit: seconds ({formatIntervalHint(routineIntervalSecondsDraft)})
                        </div>
                      </div>
                      <select
                        value={routineEntrypointDraft}
                        onChange={(event) => setRoutineEntrypointDraft(event.target.value)}
                        className="w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                      >
                        <option value="mission.default">mission.default</option>
                        {mcpToolIds.map((toolId) => (
                          <option key={toolId} value={toolId}>
                            {toolId}
                          </option>
                        ))}
                      </select>
                    </div>
                    <textarea
                      value={routineMissionObjectiveDraft}
                      onChange={(event) => setRoutineMissionObjectiveDraft(event.target.value)}
                      className="min-h-[72px] w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                      placeholder="Mission objective (required)"
                    />
                    <textarea
                      value={routineSuccessCriteriaDraft}
                      onChange={(event) => setRoutineSuccessCriteriaDraft(event.target.value)}
                      className="min-h-[64px] w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                      placeholder="Success criteria (one per line)"
                    />
                    <div className="grid grid-cols-2 gap-2">
                      <label className="text-[11px] text-text-subtle">
                        Mode
                        <select
                          value={routineModeDraft}
                          onChange={(event) =>
                            setRoutineModeDraft(
                              event.target.value === "orchestrated" ? "orchestrated" : "standalone"
                            )
                          }
                          className="mt-1 w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                        >
                          <option value="standalone">standalone</option>
                          <option value="orchestrated">orchestrated</option>
                        </select>
                      </label>
                      <label className="inline-flex items-center gap-2 self-end text-[11px] text-text-subtle">
                        <input
                          type="checkbox"
                          checked={routineOrchestratorOnlyToolCallsDraft}
                          onChange={(event) =>
                            setRoutineOrchestratorOnlyToolCallsDraft(event.target.checked)
                          }
                        />
                        Orchestrator-only tool calls
                      </label>
                    </div>
                    <div className="rounded border border-border bg-surface p-2">
                      <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                        Model Routing
                      </div>
                      <div className="mt-1 grid grid-cols-1 gap-2 sm:grid-cols-[1fr_auto]">
                        <select
                          value={routineModelPresetDraft}
                          onChange={(event) => setRoutineModelPresetDraft(event.target.value)}
                          className="w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                        >
                          {MODEL_PRESETS.filter(
                            (preset) => preset.mode === "both" || preset.mode === routineModeDraft
                          ).map((preset) => (
                            <option key={preset.id} value={preset.id}>
                              {preset.label}
                            </option>
                          ))}
                        </select>
                        <Button size="sm" variant="secondary" onClick={applyModelPreset}>
                          Apply preset
                        </Button>
                      </div>
                      <div className="mt-1 text-[10px] text-text-subtle">
                        {
                          MODEL_PRESETS.find((preset) => preset.id === routineModelPresetDraft)
                            ?.description
                        }
                      </div>
                      <div className="mt-2 grid grid-cols-2 gap-2">
                        <label className="text-[11px] text-text-subtle">
                          Default provider
                          <select
                            value={routineModelProviderDraft}
                            onChange={(event) => {
                              const nextProvider = event.target.value;
                              const nextModel = providerModelMap.get(nextProvider)?.[0] ?? "";
                              setRoutineModelProviderDraft(nextProvider);
                              setRoutineModelIdDraft(nextModel);
                            }}
                            className="mt-1 w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                          >
                            {providerOptions.map((providerId) => (
                              <option key={`provider-${providerId}`} value={providerId}>
                                {providerId}
                              </option>
                            ))}
                          </select>
                        </label>
                        <label className="text-[11px] text-text-subtle">
                          Default model
                          <select
                            value={routineModelIdDraft}
                            onChange={(event) => setRoutineModelIdDraft(event.target.value)}
                            className="mt-1 w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                          >
                            {defaultProviderModels.map((modelId) => (
                              <option key={`model-${modelId}`} value={modelId}>
                                {modelId}
                              </option>
                            ))}
                          </select>
                        </label>
                      </div>
                      {providerOptions.length === 0 ? (
                        <div className="mt-1 text-[10px] text-text-muted">
                          No provider catalog available yet. Start/connect your model providers.
                        </div>
                      ) : null}
                      {routineModeDraft === "orchestrated" ? (
                        <div className="mt-2">
                          <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                            Role Models (optional)
                          </div>
                          <div className="mt-1 space-y-1">
                            {AGENT_ROLES.map((role) => {
                              const selected = routineRoleModelDrafts[role];
                              const providerId = selected?.provider_id ?? "";
                              const roleModels = providerId
                                ? (providerModelMap.get(providerId) ?? [])
                                : [];
                              return (
                                <div key={`role-model-${role}`} className="grid grid-cols-3 gap-1">
                                  <div className="truncate pt-1 text-[10px] uppercase text-text-subtle">
                                    {role}
                                  </div>
                                  <select
                                    value={providerId}
                                    onChange={(event) => {
                                      const nextProvider = event.target.value;
                                      const nextModel =
                                        providerModelMap.get(nextProvider)?.[0] ?? "";
                                      setRoleModelDraft(role, nextProvider, nextModel);
                                    }}
                                    className="rounded border border-border bg-surface px-1 py-1 text-[11px] text-text outline-none focus:border-primary/60"
                                  >
                                    <option value="">inherit default</option>
                                    {providerOptions.map((id) => (
                                      <option key={`role-${role}-provider-${id}`} value={id}>
                                        {id}
                                      </option>
                                    ))}
                                  </select>
                                  <select
                                    value={selected?.model_id ?? ""}
                                    onChange={(event) =>
                                      setRoleModelDraft(role, providerId, event.target.value)
                                    }
                                    disabled={!providerId}
                                    className="rounded border border-border bg-surface px-1 py-1 text-[11px] text-text outline-none focus:border-primary/60 disabled:opacity-50"
                                  >
                                    <option value="">
                                      {providerId ? "inherit default" : "select provider first"}
                                    </option>
                                    {roleModels.map((modelId) => (
                                      <option key={`role-${role}-model-${modelId}`} value={modelId}>
                                        {modelId}
                                      </option>
                                    ))}
                                  </select>
                                </div>
                              );
                            })}
                          </div>
                        </div>
                      ) : null}
                    </div>
                    <div className="grid grid-cols-2 gap-2 text-[11px] text-text-subtle">
                      <label className="inline-flex items-center gap-1">
                        <input
                          type="checkbox"
                          checked={routineRequiresApprovalDraft}
                          onChange={(event) =>
                            setRoutineRequiresApprovalDraft(event.target.checked)
                          }
                        />
                        Requires approval
                      </label>
                      <label className="inline-flex items-center gap-1">
                        <input
                          type="checkbox"
                          checked={routineExternalAllowedDraft}
                          onChange={(event) => setRoutineExternalAllowedDraft(event.target.checked)}
                        />
                        External allowed
                      </label>
                    </div>
                    <div className="rounded border border-border bg-surface p-2">
                      <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                        Allowed Tools
                      </div>
                      <div className="mt-1 max-h-32 space-y-1 overflow-y-auto pr-1">
                        {allowlistChoices.map((toolId) => (
                          <label
                            key={`allowlist-${toolId}`}
                            className="flex items-center gap-2 text-[11px] text-text"
                          >
                            <input
                              type="checkbox"
                              checked={routineAllowedToolsDraft.includes(toolId)}
                              onChange={() => toggleRoutineAllowedTool(toolId)}
                            />
                            <span className="truncate font-mono text-[10px]">{toolId}</span>
                          </label>
                        ))}
                        {allowlistChoices.length === 0 ? (
                          <div className="text-[11px] text-text-muted">
                            No tools available yet. Connect MCP servers to populate options.
                          </div>
                        ) : null}
                      </div>
                    </div>
                    <input
                      value={routineOutputTargetsDraft}
                      onChange={(event) => setRoutineOutputTargetsDraft(event.target.value)}
                      className="w-full rounded border border-border bg-surface px-2 py-1 text-xs text-text outline-none focus:border-primary/60"
                      placeholder="Output targets (comma-separated URIs)"
                    />
                    <Button
                      size="sm"
                      variant="primary"
                      disabled={createRoutineLoading}
                      onClick={() => void handleCreateRoutine()}
                    >
                      {createRoutineLoading ? "Creating..." : "Create routine"}
                    </Button>
                  </div>
                </div>
                <div className="rounded-md border border-border bg-surface-elevated/40 p-3">
                  <div className="text-xs font-semibold text-text">Configured Routines</div>
                  <div className="mt-2 space-y-2">
                    {routines.slice(0, 8).map((routine) => (
                      <div
                        key={routine.routine_id}
                        className="rounded border border-border bg-surface px-2 py-2"
                      >
                        <div className="flex items-center justify-between gap-2">
                          <div className="min-w-0">
                            <div className="truncate text-xs font-semibold text-text">
                              {routine.name}
                            </div>
                            <div className="truncate text-[11px] text-text-muted">
                              {routine.routine_id}
                            </div>
                            <div className="truncate text-[11px] text-text-subtle">
                              {routine.entrypoint} | {routine.status} | mode:{" "}
                              {modeFromArgs(routine.args)}
                            </div>
                            {routine.args?.["orchestrator_only_tool_calls"] ? (
                              <div className="truncate text-[11px] text-text-subtle">
                                policy: orchestrator-only tool calls
                              </div>
                            ) : null}
                            {typeof routine.args?.["prompt"] === "string" &&
                            routine.args["prompt"].trim().length > 0 ? (
                              <div className="line-clamp-2 text-[11px] text-text-subtle">
                                mission: {routine.args["prompt"]}
                              </div>
                            ) : null}
                            {(() => {
                              const policy = parseModelPolicy(routine.args);
                              const orchestrator =
                                modeFromArgs(routine.args) === "orchestrated"
                                  ? modelChoiceLabel(policy?.role_models?.orchestrator)
                                  : null;
                              const fallback = modelChoiceLabel(policy?.default_model);
                              const modelLabel = orchestrator ?? fallback;
                              if (!modelLabel) return null;
                              return (
                                <div className="truncate text-[11px] text-text-subtle">
                                  model: {modelLabel}
                                </div>
                              );
                            })()}
                            {routine.output_targets.length > 0 ? (
                              <div className="truncate text-[11px] text-text-subtle">
                                outputs: {routine.output_targets.length}
                              </div>
                            ) : null}
                          </div>
                          <Button
                            size="sm"
                            variant="secondary"
                            onClick={() => void handleToggleRoutineStatus(routine)}
                          >
                            {routine.status === "active" ? "Pause" : "Resume"}
                          </Button>
                        </div>
                      </div>
                    ))}
                    {!routinesLoading && routines.length === 0 ? (
                      <div className="rounded border border-border bg-surface px-2 py-2 text-xs text-text-muted">
                        No routines configured.
                      </div>
                    ) : null}
                  </div>
                </div>
              </div>
            </div>

            <div className="rounded-lg border border-border bg-surface p-4">
              <div className="flex items-center justify-between gap-2">
                <div className="text-xs uppercase tracking-wide text-text-subtle">
                  Scheduled Bots
                </div>
                <div className="text-xs text-text-muted">
                  {routineRunsLoading ? "Refreshing..." : `${visibleRuns.length} shown`}
                </div>
              </div>
              <div className="mt-2 flex flex-wrap gap-2">
                <Button
                  size="sm"
                  variant={runFilter === "all" ? "primary" : "secondary"}
                  onClick={() => setRunFilter("all")}
                >
                  All ({routineRuns.length})
                </Button>
                <Button
                  size="sm"
                  variant={runFilter === "pending" ? "primary" : "secondary"}
                  onClick={() => setRunFilter("pending")}
                >
                  Pending ({pendingApprovals})
                </Button>
                <Button
                  size="sm"
                  variant={runFilter === "blocked" ? "primary" : "secondary"}
                  onClick={() => setRunFilter("blocked")}
                >
                  Blocked ({blockedRuns})
                </Button>
                <Button
                  size="sm"
                  variant={runFilter === "failed" ? "primary" : "secondary"}
                  onClick={() => setRunFilter("failed")}
                >
                  Failed ({failedRuns})
                </Button>
              </div>
              <div className="mt-2 space-y-2">
                {visibleRuns.slice(0, 8).map((run) => {
                  const busy = routineActionBusyRunId === run.run_id;
                  return (
                    <div
                      key={run.run_id}
                      className="rounded-md border border-border bg-surface-elevated/50 px-3 py-2"
                    >
                      <div className="flex items-center justify-between gap-2">
                        <div className="min-w-0">
                          <div className="truncate text-xs font-semibold text-text">
                            {run.routine_id} | {run.status}
                          </div>
                          <div className="mt-0.5 truncate text-[11px] text-text-muted">
                            run {run.run_id} | {run.trigger_type} | mode: {modeFromArgs(run.args)}
                          </div>
                          {runTimeline[run.run_id]?.length ? (
                            <div className="mt-1 flex flex-wrap gap-1">
                              {runTimeline[run.run_id].map((event, index) => (
                                <span
                                  key={`${run.run_id}-${event.tsMs}-${index}`}
                                  title={
                                    event.note
                                      ? `${event.eventType}: ${event.note}`
                                      : `${event.eventType}`
                                  }
                                  className="rounded border border-border bg-surface px-1.5 py-0.5 text-[10px] text-text-subtle"
                                >
                                  {eventLabel(event)}
                                </span>
                              ))}
                            </div>
                          ) : null}
                          {run.args?.["orchestrator_only_tool_calls"] ? (
                            <div className="mt-0.5 text-[11px] text-text-subtle">
                              policy: orchestrator-only tool calls
                            </div>
                          ) : null}
                          {(() => {
                            const policy = parseModelPolicy(run.args);
                            const orchestrator =
                              modeFromArgs(run.args) === "orchestrated"
                                ? modelChoiceLabel(policy?.role_models?.orchestrator)
                                : null;
                            const fallback = modelChoiceLabel(policy?.default_model);
                            const modelLabel = orchestrator ?? fallback;
                            if (!modelLabel) return null;
                            return (
                              <div className="mt-0.5 text-[11px] text-text-subtle">
                                model: {modelLabel}
                              </div>
                            );
                          })()}
                          {run.allowed_tools.length > 0 ? (
                            <div className="mt-1 flex flex-wrap gap-1">
                              {run.allowed_tools.slice(0, 3).map((toolId) => (
                                <span
                                  key={`${run.run_id}-${toolId}`}
                                  className="rounded border border-border bg-surface px-1.5 py-0.5 text-[10px] text-text-subtle"
                                >
                                  {toolId}
                                </span>
                              ))}
                              {run.allowed_tools.length > 3 ? (
                                <span className="rounded border border-border bg-surface px-1.5 py-0.5 text-[10px] text-text-subtle">
                                  +{run.allowed_tools.length - 3} more
                                </span>
                              ) : null}
                            </div>
                          ) : (
                            <div className="mt-0.5 text-[11px] text-text-subtle">
                              tool scope: all
                            </div>
                          )}
                          {run.output_targets.length > 0 ? (
                            <div className="mt-0.5 text-[11px] text-text-subtle">
                              outputs: {run.output_targets.length}
                            </div>
                          ) : null}
                          {run.artifacts.length > 0 ? (
                            <div className="mt-0.5 text-[11px] text-text-subtle">
                              {run.artifacts.length} artifact{run.artifacts.length === 1 ? "" : "s"}
                            </div>
                          ) : null}
                        </div>
                        <div className="flex items-center gap-1">
                          <Button
                            size="sm"
                            variant="ghost"
                            onClick={() => setSelectedRunId(run.run_id)}
                          >
                            Details
                          </Button>
                          {run.status === "pending_approval" ? (
                            <>
                              <Button
                                size="sm"
                                variant="secondary"
                                disabled={busy}
                                onClick={() => void handleRoutineRunAction(run, "approve")}
                              >
                                Approve
                              </Button>
                              <Button
                                size="sm"
                                variant="ghost"
                                disabled={busy}
                                onClick={() => void handleRoutineRunAction(run, "deny")}
                              >
                                Deny
                              </Button>
                            </>
                          ) : null}
                          {(run.status === "queued" || run.status === "running") && (
                            <Button
                              size="sm"
                              variant="ghost"
                              disabled={busy}
                              onClick={() => void handleRoutineRunAction(run, "pause")}
                            >
                              Pause
                            </Button>
                          )}
                          {run.status === "paused" && (
                            <Button
                              size="sm"
                              variant="ghost"
                              disabled={busy}
                              onClick={() => void handleRoutineRunAction(run, "resume")}
                            >
                              Resume
                            </Button>
                          )}
                        </div>
                      </div>
                    </div>
                  );
                })}
                {!routineRunsLoading && visibleRuns.length === 0 ? (
                  <div className="rounded-md border border-border bg-surface-elevated/50 px-3 py-2 text-xs text-text-muted">
                    No runs for this filter.
                  </div>
                ) : null}
              </div>
              {selectedRun ? (
                <div className="mt-3 rounded-md border border-border bg-surface-elevated/40 p-3">
                  <div className="text-[11px] font-semibold uppercase tracking-wide text-text-subtle">
                    Run Details
                  </div>
                  <div className="mt-1 text-xs text-text">
                    {selectedRun.routine_id} | {selectedRun.run_id}
                  </div>
                  <div className="mt-1 text-[11px] text-text-subtle">
                    created: {formatTimestamp(selectedRun.created_at_ms)} | started:{" "}
                    {formatTimestamp(selectedRun.started_at_ms ?? null)} | finished:{" "}
                    {formatTimestamp(selectedRun.finished_at_ms ?? null)}
                  </div>
                  {runReason(selectedRun) ? (
                    <div className="mt-1 text-[11px] text-text-subtle">
                      reason: {runReason(selectedRun)}
                    </div>
                  ) : null}
                  {(() => {
                    const policy = parseModelPolicy(selectedRun.args);
                    const orchestrator =
                      modeFromArgs(selectedRun.args) === "orchestrated"
                        ? modelChoiceLabel(policy?.role_models?.orchestrator)
                        : null;
                    const fallback = modelChoiceLabel(policy?.default_model);
                    const modelLabel = orchestrator ?? fallback;
                    if (!modelLabel) return null;
                    return (
                      <div className="mt-1 text-[11px] text-text-subtle">model: {modelLabel}</div>
                    );
                  })()}
                  {selectedRunLatestEvent ? (
                    <div className="mt-1 text-[11px] text-text-subtle">
                      latest event: {selectedRunLatestEvent.eventType}
                      {selectedRunLatestEvent.note ? ` | ${selectedRunLatestEvent.note}` : ""}
                    </div>
                  ) : null}
                  {selectedRun.output_targets.length > 0 ? (
                    <div className="mt-2">
                      <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                        Output Targets
                      </div>
                      <div className="mt-1 space-y-1">
                        {selectedRun.output_targets.slice(0, 5).map((target) => (
                          <div
                            key={`${selectedRun.run_id}-target-${target}`}
                            className="truncate font-mono text-[10px] text-text-muted"
                          >
                            {target}
                          </div>
                        ))}
                      </div>
                    </div>
                  ) : null}
                  {selectedRun.artifacts.length > 0 ? (
                    <div className="mt-2">
                      <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                        Artifacts
                      </div>
                      <div className="mt-1 space-y-1">
                        {selectedRun.artifacts.slice(0, 5).map((artifact) => (
                          <div key={artifact.artifact_id} className="text-[11px] text-text-subtle">
                            <span className="font-mono text-[10px] text-text-muted">
                              {artifact.kind}
                            </span>{" "}
                            <span className="truncate font-mono text-[10px]">{artifact.uri}</span>
                          </div>
                        ))}
                      </div>
                    </div>
                  ) : null}
                </div>
              ) : null}
            </div>

            <div className="rounded-lg border border-border bg-surface p-4">
              <div className="flex items-center justify-between gap-2">
                <div className="text-xs uppercase tracking-wide text-text-subtle">Connectors</div>
                <div className="text-xs text-text-muted">
                  {mcpLoading
                    ? "Refreshing..."
                    : `${connectedConnectors}/${mcpServers.length} connected`}
                </div>
              </div>
              <div className="mt-2 flex items-center justify-between gap-2">
                <div className="text-xs text-text-muted">
                  Add or edit server config in Extensions, then operate connectors here.
                </div>
                {onOpenMcpExtensions ? (
                  <Button size="sm" variant="secondary" onClick={onOpenMcpExtensions}>
                    Open Extensions MCP
                  </Button>
                ) : null}
              </div>
              <div className="mt-2 grid grid-cols-1 gap-2 md:grid-cols-2">
                {mcpServers.slice(0, 8).map((server) => {
                  const count = mcpTools.filter((tool) => tool.server_name === server.name).length;
                  const busy = busyConnector?.startsWith(`${server.name}:`) ?? false;
                  return (
                    <div
                      key={server.name}
                      className="rounded-md border border-border bg-surface-elevated/50 px-3 py-2"
                    >
                      <div className="text-xs font-semibold text-text">{server.name}</div>
                      <div className="mt-0.5 text-[11px] text-text-muted">
                        {server.enabled ? "enabled" : "disabled"} ·{" "}
                        {server.connected ? "connected" : "disconnected"} · {count} tools
                      </div>
                      {server.last_error ? (
                        <div className="mt-1 text-[11px] text-red-300">{server.last_error}</div>
                      ) : null}
                      <div className="mt-2 flex flex-wrap gap-1">
                        <Button
                          size="sm"
                          variant="secondary"
                          disabled={busy}
                          onClick={() =>
                            void handleConnectorAction(server.name, "set-enabled", !server.enabled)
                          }
                        >
                          {server.enabled ? "Disable" : "Enable"}
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          disabled={busy || !server.enabled}
                          onClick={() =>
                            void handleConnectorAction(
                              server.name,
                              server.connected ? "disconnect" : "connect"
                            )
                          }
                        >
                          {server.connected ? "Disconnect" : "Connect"}
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          disabled={busy || !server.enabled}
                          onClick={() => void handleConnectorAction(server.name, "refresh")}
                        >
                          Refresh
                        </Button>
                      </div>
                    </div>
                  );
                })}
                {!mcpLoading && mcpServers.length === 0 ? (
                  <div className="rounded-md border border-border bg-surface-elevated/50 px-3 py-2 text-xs text-text-muted">
                    No MCP connectors configured.
                  </div>
                ) : null}
              </div>
            </div>
          </>
        ) : (
          <AgentCommandCenter />
        )}
      </div>
    </div>
  );
}
