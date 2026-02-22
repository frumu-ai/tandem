import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui";
import { FileBrowser } from "@/components/files/FileBrowser";
import { AgentCommandCenter } from "@/components/orchestrate/AgentCommandCenter";
import { BudgetMeter } from "@/components/orchestrate/BudgetMeter";
import { ModelSelector } from "@/components/chat/ModelSelector";
import { AgentModelRoutingPanel } from "@/components/orchestrate/AgentModelRoutingPanel";
import { TaskBoard } from "@/components/orchestrate/TaskBoard";
import { LogsDrawer } from "@/components/logs";
import { ConsoleTab } from "@/components/logs/ConsoleTab";
import { ProjectSwitcher } from "@/components/sidebar";
import {
  deleteOrchestratorRun,
  getSidecarStartupHealth,
  getSidecarStatus,
  getProvidersConfig,
  setProvidersConfig,
  mcpListServers,
  mcpListTools,
  routinesRunApprove,
  routinesRunDeny,
  routinesRunPause,
  routinesRunResume,
  routinesRunsAll,
  type McpRemoteTool,
  type McpServerRecord,
  type RoutineRunRecord,
  onSidecarEventV2,
  type FileEntry,
  type SidecarStartupHealth,
  type SidecarState,
  type StreamEventEnvelopeV2,
  type UserProject,
} from "@/lib/tauri";
import {
  DEFAULT_ORCHESTRATOR_CONFIG,
  type OrchestratorConfig,
  type OrchestratorModelRouting,
  type RunSummary,
  type RunSnapshot,
  type Task,
} from "@/components/orchestrate/types";
import {
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  Loader2,
  Pause,
  RefreshCw,
  RotateCcw,
  Square,
  ScrollText,
  Sparkles,
  Trash2,
} from "lucide-react";

type QualityPreset = "speed" | "balanced" | "quality";
type SwarmStage =
  | "idle"
  | "planning"
  | "awaiting_review"
  | "executing"
  | "paused"
  | "completed"
  | "failed";
type TabId = "task-to-swarm" | "advanced";

interface CommandCenterPageProps {
  userProjects: UserProject[];
  activeProject: UserProject | null;
  onSwitchProject: (projectId: string) => void;
  onAddProject: () => void;
  onManageProjects: () => void;
  onFileOpen?: (file: FileEntry) => void;
  projectSwitcherLoading?: boolean;
  initialRunId?: string | null;
}

interface RunModelSelection {
  model?: string | null;
  provider?: string | null;
}

interface MissionLimits {
  wallTimeHours: number;
  maxTotalTokens: number;
  maxTokensPerStep: number;
  maxIterations: number;
  maxSubagentRuns: number;
  maxTaskRetries: number;
}

function truncateForFeed(value: string, max = 64): string {
  const trimmed = value.trim();
  if (!trimmed) return "";
  return trimmed.length > max ? `${trimmed.slice(0, max)}...` : trimmed;
}

function formatStreamEventForFeed(payload: StreamEventEnvelopeV2["payload"]): string | null {
  switch (payload.type) {
    case "run_started":
      return `run started ${payload.run_id}`;
    case "run_finished":
      return payload.error
        ? `run finished ${payload.status}: ${truncateForFeed(payload.error)}`
        : `run finished ${payload.status}`;
    case "session_status":
      return `session ${payload.status}`;
    case "session_error":
      return `session error ${truncateForFeed(payload.error)}`;
    case "tool_start":
      return `tool start ${payload.tool}`;
    case "tool_end":
      return payload.error
        ? `tool failed ${payload.tool}: ${truncateForFeed(payload.error)}`
        : `tool done ${payload.tool}`;
    case "permission_asked":
      return `permission asked${payload.tool ? ` for ${payload.tool}` : ""}`;
    case "question_asked":
      return `question asked (${payload.questions.length})`;
    case "file_edited":
      return `file edited ${truncateForFeed(payload.file_path, 52)}`;
    case "content": {
      const chunk = payload.delta || payload.content || "";
      if (!chunk.trim()) return null;
      return `llm streaming ${truncateForFeed(chunk, 80)}`;
    }
    case "raw":
      if (
        payload.event_type.startsWith("agent_team.") ||
        payload.event_type.startsWith("session.run.")
      ) {
        return payload.event_type;
      }
      return null;
    default:
      return null;
  }
}

function stageFromSnapshot(snapshot: RunSnapshot | null): SwarmStage {
  if (!snapshot) return "idle";
  if (snapshot.status === "planning") return "planning";
  if (snapshot.status === "awaiting_approval") return "awaiting_review";
  if (snapshot.status === "executing") return "executing";
  if (snapshot.status === "paused") return "paused";
  if (snapshot.status === "completed") return "completed";
  if (snapshot.status === "failed" || snapshot.status === "cancelled") return "failed";
  return "idle";
}

function runStatusBadgeClass(status: string): string {
  if (status === "completed") return "border-emerald-500/40 bg-emerald-500/15 text-emerald-200";
  if (status === "failed") return "border-red-500/40 bg-red-500/15 text-red-200";
  if (status === "cancelled") return "border-amber-500/40 bg-amber-500/15 text-amber-200";
  if (status === "paused") return "border-yellow-500/40 bg-yellow-500/15 text-yellow-200";
  if (status === "executing" || status === "running")
    return "border-cyan-500/40 bg-cyan-500/15 text-cyan-200";
  if (status === "awaiting_approval")
    return "border-violet-500/40 bg-violet-500/15 text-violet-200";
  return "border-border bg-surface-elevated/40 text-text-muted";
}

function runStatusLabel(status: string): string {
  return status.replace(/_/g, " ");
}

export function CommandCenterPage({
  userProjects,
  activeProject,
  onSwitchProject,
  onAddProject,
  onManageProjects,
  onFileOpen,
  projectSwitcherLoading = false,
  initialRunId = null,
}: CommandCenterPageProps) {
  const pageScrollRef = useRef<HTMLDivElement | null>(null);
  const taskBoardRef = useRef<HTMLDivElement | null>(null);
  const [tab, setTab] = useState<TabId>("task-to-swarm");
  const [objective, setObjective] = useState("");
  const [preset, setPreset] = useState<QualityPreset>("balanced");
  const [runId, setRunId] = useState<string | null>(null);
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [runsLoading, setRunsLoading] = useState(false);
  const [snapshot, setSnapshot] = useState<RunSnapshot | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [eventFeed, setEventFeed] = useState<string[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [isRunActionLoading, setIsRunActionLoading] = useState(false);
  const [runsCollapsed, setRunsCollapsed] = useState(false);
  const [hasExplicitRunSelection, setHasExplicitRunSelection] = useState(false);
  const [pendingCreatedRun, setPendingCreatedRun] = useState<RunSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedModel, setSelectedModel] = useState<string | undefined>(undefined);
  const [selectedProvider, setSelectedProvider] = useState<string | undefined>(undefined);
  const [sidecarStatus, setSidecarStatus] = useState<SidecarState>("stopped");
  const [sidecarStartupHealth, setSidecarStartupHealth] = useState<SidecarStartupHealth | null>(
    null
  );
  const [activeRunSessionId, setActiveRunSessionId] = useState<string | null>(null);
  const [runModelSelection, setRunModelSelection] = useState<RunModelSelection | null>(null);
  const [modelRouting, setModelRouting] = useState<OrchestratorModelRouting>({});
  const [selectedRunObjectiveExpanded, setSelectedRunObjectiveExpanded] = useState(false);
  const [mcpServers, setMcpServers] = useState<McpServerRecord[]>([]);
  const [mcpTools, setMcpTools] = useState<McpRemoteTool[]>([]);
  const [mcpLoading, setMcpLoading] = useState(false);
  const [routineRuns, setRoutineRuns] = useState<RoutineRunRecord[]>([]);
  const [routineRunsLoading, setRoutineRunsLoading] = useState(false);
  const [routineActionBusyRunId, setRoutineActionBusyRunId] = useState<string | null>(null);
  const [showLogsDrawer, setShowLogsDrawer] = useState(false);
  const [autoApproveTargetRunId, setAutoApproveTargetRunId] = useState<string | null>(null);
  const [selectedWorkspaceFile, setSelectedWorkspaceFile] = useState<FileEntry | null>(null);
  const [workspaceFilesExpanded, setWorkspaceFilesExpanded] = useState(false);
  const [missionLimits, setMissionLimits] = useState<MissionLimits>({
    wallTimeHours: 48,
    maxTotalTokens: 500_000,
    maxTokensPerStep: 30_000,
    maxIterations: 800,
    maxSubagentRuns: 3_000,
    maxTaskRetries: 5,
  });
  const lastSnapshotRef = useRef<RunSnapshot | null>(null);
  const pendingCreatedRunRef = useRef<RunSummary | null>(null);
  const autoApproveInFlightRef = useRef(false);
  const lastContentFeedMsRef = useRef(0);
  const lastSessionErrorRef = useRef<{ signature: string; atMs: number } | null>(null);
  const selectedModelRef = useRef<string | undefined>(undefined);
  const selectedProviderRef = useRef<string | undefined>(undefined);

  const stage = stageFromSnapshot(snapshot);
  const workspacePath = activeProject?.path ?? null;
  const sidecarStarting =
    sidecarStatus === "starting" || !!(sidecarStartupHealth && !sidecarStartupHealth.ready);
  const sidecarReady = sidecarStatus === "running" && !sidecarStarting;
  const isWorking = stage === "planning" || stage === "awaiting_review" || stage === "executing";
  const launchDisabled =
    isLoading ||
    isWorking ||
    !objective.trim() ||
    !selectedModel ||
    !selectedProvider ||
    !sidecarReady;
  const selectedRunSessionId = useMemo(
    () => runs.find((run) => run.run_id === runId)?.session_id ?? null,
    [runId, runs]
  );
  const selectedRunConsoleSessionIds = useMemo(() => {
    const ids = new Set<string>();
    if (selectedRunSessionId) ids.add(selectedRunSessionId);
    if (activeRunSessionId) ids.add(activeRunSessionId);
    for (const task of tasks) {
      const sid = task.session_id?.trim();
      if (sid) ids.add(sid);
    }
    return Array.from(ids);
  }, [activeRunSessionId, selectedRunSessionId, tasks]);

  const loadRuns = useCallback(async () => {
    setRunsLoading(true);
    try {
      const listed = await invoke<RunSummary[]>("orchestrator_list_runs");
      const commandCenterRuns = listed.filter((run) => run.source === "command_center");
      commandCenterRuns.sort((a, b) => Date.parse(b.updated_at) - Date.parse(a.updated_at));
      setRuns(commandCenterRuns);
      const pendingRun = pendingCreatedRunRef.current;
      if (pendingRun && commandCenterRuns.some((run) => run.run_id === pendingRun.run_id)) {
        setPendingCreatedRun(null);
      }
    } catch {
      setRuns([]);
    } finally {
      setRunsLoading(false);
    }
  }, []);

  useEffect(() => {
    pendingCreatedRunRef.current = pendingCreatedRun;
  }, [pendingCreatedRun]);

  useEffect(() => {
    let disposed = false;
    if (!runId) {
      setSnapshot(null);
      setTasks([]);
      lastSnapshotRef.current = null;
      return;
    }

    const poll = async () => {
      try {
        const [nextSnapshot, nextTasks] = await Promise.all([
          invoke<RunSnapshot>("orchestrator_get_run", { runId }),
          invoke<Task[]>("orchestrator_list_tasks", { runId }),
        ]);
        if (disposed) return;
        const prevSnapshot = lastSnapshotRef.current;
        const nextEvents: string[] = [];
        const now = new Date().toLocaleTimeString();
        if (!prevSnapshot || prevSnapshot.status !== nextSnapshot.status) {
          nextEvents.push(`${now} run.${nextSnapshot.status}`);
        }
        if (!prevSnapshot || prevSnapshot.current_task_id !== nextSnapshot.current_task_id) {
          if (nextSnapshot.current_task_id) {
            nextEvents.push(`${now} current task ${nextSnapshot.current_task_id}`);
          }
        }
        if (!prevSnapshot || prevSnapshot.task_count !== nextSnapshot.task_count) {
          nextEvents.push(`${now} planned tasks ${nextSnapshot.task_count}`);
        }
        if (
          nextSnapshot.error_message &&
          (!prevSnapshot || prevSnapshot.error_message !== nextSnapshot.error_message)
        ) {
          nextEvents.push(`${now} error ${nextSnapshot.error_message}`);
        }
        if (nextEvents.length > 0) {
          setEventFeed((prev) => [...nextEvents, ...prev].slice(0, 20));
        }
        lastSnapshotRef.current = nextSnapshot;
        setSnapshot(nextSnapshot);
        setTasks(nextTasks);
      } catch {
        if (disposed) return;
      }
    };

    void poll();
    const timer = setInterval(() => void poll(), 1250);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, [runId]);

  useEffect(() => {
    setRunId(null);
    setHasExplicitRunSelection(false);
    setSnapshot(null);
    setTasks([]);
    setEventFeed([]);
    setActiveRunSessionId(null);
    void loadRuns();
  }, [activeProject?.id, loadRuns]);

  useEffect(() => {
    if (!initialRunId) return;
    setRunId(initialRunId);
    setHasExplicitRunSelection(false);
    setTab("task-to-swarm");
  }, [initialRunId]);

  useEffect(() => {
    if (!runId) {
      setActiveRunSessionId(null);
      return;
    }
    const summary = runs.find((run) => run.run_id === runId);
    setActiveRunSessionId(summary?.session_id ?? null);
  }, [runId, runs]);

  useEffect(() => {
    setSelectedRunObjectiveExpanded(false);
  }, [runId]);

  useEffect(() => {
    void loadRuns();
    const timer = setInterval(() => void loadRuns(), 5000);
    return () => clearInterval(timer);
  }, [loadRuns]);

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

  useEffect(() => {
    void loadMcpStatus();
    const timer = setInterval(() => void loadMcpStatus(), 10000);
    return () => clearInterval(timer);
  }, [loadMcpStatus]);

  const loadRoutineRuns = useCallback(async () => {
    setRoutineRunsLoading(true);
    try {
      const merged = await routinesRunsAll(undefined, 30);
      setRoutineRuns(merged.sort((a, b) => b.created_at_ms - a.created_at_ms));
    } catch {
      setRoutineRuns([]);
    } finally {
      setRoutineRunsLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadRoutineRuns();
    const timer = setInterval(() => void loadRoutineRuns(), 10000);
    return () => clearInterval(timer);
  }, [loadRoutineRuns]);

  useEffect(() => {
    if (!autoApproveTargetRunId || !snapshot || runId !== autoApproveTargetRunId) {
      return;
    }
    if (snapshot.status === "awaiting_approval") {
      if (autoApproveInFlightRef.current) {
        return;
      }
      autoApproveInFlightRef.current = true;
      void (async () => {
        try {
          await invoke("orchestrator_approve", { runId: autoApproveTargetRunId });
          const at = new Date().toLocaleTimeString();
          setEventFeed((prev) =>
            [`${at} auto-approved plan for ${autoApproveTargetRunId}`, ...prev].slice(0, 20)
          );
        } catch (e) {
          const message = e instanceof Error ? e.message : String(e);
          setError(`Auto-approve failed: ${message}`);
        } finally {
          autoApproveInFlightRef.current = false;
          setAutoApproveTargetRunId(null);
        }
      })();
      return;
    }

    if (
      snapshot.status === "executing" ||
      snapshot.status === "completed" ||
      snapshot.status === "failed" ||
      snapshot.status === "cancelled"
    ) {
      setAutoApproveTargetRunId(null);
    }
  }, [autoApproveTargetRunId, runId, snapshot]);

  useEffect(() => {
    let disposed = false;
    const loadModelDefaults = async () => {
      try {
        const config = await getProvidersConfig();
        if (disposed) return;
        const model = config.selected_model?.model_id;
        const provider = config.selected_model?.provider_id;
        if (model) setSelectedModel(model);
        if (provider) setSelectedProvider(provider);
        selectedModelRef.current = model;
        selectedProviderRef.current = provider;
      } catch {
        // best effort only
      }
    };
    void loadModelDefaults();
    return () => {
      disposed = true;
    };
  }, []);

  useEffect(() => {
    setSelectedWorkspaceFile(null);
  }, [activeProject?.id]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    const sessionScope = new Set(selectedRunConsoleSessionIds);
    const setup = async () => {
      unlisten = await onSidecarEventV2((envelope: StreamEventEnvelopeV2) => {
        const payload = envelope?.payload;
        if (!payload) return;
        const eventRunId = ("run_id" in payload ? payload.run_id : null) ?? null;
        if (runId && eventRunId && eventRunId !== runId) {
          return;
        }
        const eventSessionId =
          ("session_id" in payload ? payload.session_id : envelope.session_id) ?? null;
        if (sessionScope.size > 0) {
          if (eventSessionId && !sessionScope.has(eventSessionId)) {
            return;
          }
        } else if (runId) {
          // Prevent global cross-run bleed before we resolve the run session scope.
          return;
        }
        const line = formatStreamEventForFeed(payload);
        if (!line) return;
        if (payload.type === "content") {
          const nowMs = Date.now();
          if (nowMs - lastContentFeedMsRef.current < 1500) {
            return;
          }
          lastContentFeedMsRef.current = nowMs;
        }
        if (payload.type === "session_error") {
          const nowMs = Date.now();
          const signature = `${eventSessionId ?? "none"}:${payload.error ?? ""}`;
          const previous = lastSessionErrorRef.current;
          // Sidecar can emit the same terminal error repeatedly; collapse duplicates.
          if (previous && previous.signature === signature && nowMs - previous.atMs < 5000) {
            return;
          }
          lastSessionErrorRef.current = { signature, atMs: nowMs };
        }
        if (payload.type === "run_started" && payload.run_id === runId) {
          setActiveRunSessionId(payload.session_id);
        }
        const at = new Date().toLocaleTimeString();
        setEventFeed((prev) => [`${at} ${line}`, ...prev].slice(0, 40));
        if (payload.type === "raw" && payload.event_type.startsWith("mcp.")) {
          void loadMcpStatus();
        }
        if (payload.type === "raw" && payload.event_type.startsWith("routine.")) {
          void loadRoutineRuns();
        }
        if (
          payload.type === "run_started" ||
          payload.type === "run_finished" ||
          payload.type === "session_status" ||
          payload.type === "session_error"
        ) {
          void loadRuns();
        }
      });
    };
    void setup();
    return () => {
      if (unlisten) unlisten();
    };
  }, [loadMcpStatus, loadRoutineRuns, loadRuns, runId, selectedRunConsoleSessionIds]);

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

  useEffect(() => {
    let disposed = false;
    const refreshSidecar = async () => {
      try {
        const status = await getSidecarStatus();
        if (disposed) return;
        setSidecarStatus(status);
        if (status === "starting" || status === "running") {
          const health = await getSidecarStartupHealth();
          if (disposed) return;
          setSidecarStartupHealth(health);
        } else {
          setSidecarStartupHealth(null);
        }
      } catch {
        if (disposed) return;
        setSidecarStartupHealth(null);
      }
    };
    void refreshSidecar();
    const timer = setInterval(() => void refreshSidecar(), 1500);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    if (!runId) {
      setRunModelSelection(null);
      return;
    }
    const loadRunModel = async () => {
      try {
        const modelSelection = await invoke<RunModelSelection>("orchestrator_get_run_model", {
          runId,
        });
        if (disposed) return;
        setRunModelSelection(modelSelection);
      } catch {
        if (disposed) return;
        setRunModelSelection(null);
      }
    };
    void loadRunModel();
    const timer = setInterval(() => void loadRunModel(), 2500);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, [runId]);

  const launchSwarm = async () => {
    if (!objective.trim()) {
      setError("Please enter an objective.");
      return;
    }
    if (!sidecarReady) {
      const detail = sidecarStartupHealth
        ? `phase=${sidecarStartupHealth.phase} elapsed_ms=${sidecarStartupHealth.startup_elapsed_ms}`
        : `state=${sidecarStatus}`;
      setError(`Engine is still starting (${detail}). Please wait a moment and retry.`);
      return;
    }
    const dispatchModel = selectedModelRef.current ?? selectedModel;
    const dispatchProvider = selectedProviderRef.current ?? selectedProvider;
    if (!dispatchModel || !dispatchProvider) {
      setError("Please select a provider/model before launching the swarm.");
      return;
    }
    setIsLoading(true);
    setError(null);
    try {
      const configByPreset = {
        speed: { max_parallel_tasks: 8, llm_parallel: 6 },
        balanced: { max_parallel_tasks: 6, llm_parallel: 4 },
        quality: { max_parallel_tasks: 3, llm_parallel: 2 },
      } as const;
      const config: OrchestratorConfig = {
        ...DEFAULT_ORCHESTRATOR_CONFIG,
        max_iterations: Math.max(1, missionLimits.maxIterations),
        max_total_tokens: Math.max(1_000, missionLimits.maxTotalTokens),
        max_tokens_per_step: Math.max(500, missionLimits.maxTokensPerStep),
        max_wall_time_secs: Math.max(300, Math.floor(missionLimits.wallTimeHours * 60 * 60)),
        max_subagent_runs: Math.max(1, missionLimits.maxSubagentRuns),
        max_task_retries: Math.max(0, missionLimits.maxTaskRetries),
        max_parallel_tasks: configByPreset[preset].max_parallel_tasks,
        llm_parallel: configByPreset[preset].llm_parallel,
        fs_write_parallel: 1,
        shell_parallel: 1,
        network_parallel: preset === "speed" ? 4 : preset === "balanced" ? 3 : 2,
      };
      const createdRunId = await invoke<string>("orchestrator_create_run", {
        objective: objective.trim(),
        config,
        model: dispatchModel,
        provider: dispatchProvider,
        agentModelRouting: modelRouting,
        source: "command_center",
      });
      const nowIso = new Date().toISOString();
      setPendingCreatedRun({
        run_id: createdRunId,
        session_id: "",
        workspace_root: workspacePath,
        source: "command_center",
        objective: objective.trim(),
        status: "planning",
        created_at: nowIso,
        updated_at: nowIso,
        started_at: nowIso,
        ended_at: null,
        last_error: null,
      });
      setRunId(createdRunId);
      setHasExplicitRunSelection(true);
      setTab("task-to-swarm");
      await loadRuns();
      await invoke("orchestrator_start", { runId: createdRunId });
      setAutoApproveTargetRunId(createdRunId);
      await loadRuns();
    } catch (e) {
      setPendingCreatedRun(null);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsLoading(false);
    }
  };

  const approvePlan = async () => {
    if (!runId) return;
    setIsLoading(true);
    setError(null);
    try {
      await invoke("orchestrator_approve", { runId });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsLoading(false);
    }
  };

  const pendingTasks = useMemo(() => tasks.filter((task) => task.state !== "done").length, [tasks]);
  const selectedRunSummary = useMemo(
    () =>
      runs.find((run) => run.run_id === runId) ??
      (pendingCreatedRun?.run_id === runId ? pendingCreatedRun : null),
    [pendingCreatedRun, runId, runs]
  );
  const effectiveWorkspaceForRun =
    selectedRunSummary?.workspace_root ?? workspacePath ?? "No workspace";
  const displayedRuns = useMemo(() => {
    if (!pendingCreatedRun) return runs;
    if (runs.some((run) => run.run_id === pendingCreatedRun.run_id)) return runs;
    return [pendingCreatedRun, ...runs];
  }, [pendingCreatedRun, runs]);
  const inferredRunCostUsd = useMemo(() => {
    if (!snapshot) return null;
    const raw = snapshot as unknown as Record<string, unknown>;
    const candidates = [raw.cost_used_usd, raw.cost_usd, raw.price_usd, raw.estimated_cost_usd];
    for (const candidate of candidates) {
      if (typeof candidate === "number" && Number.isFinite(candidate)) {
        return candidate;
      }
    }
    return null;
  }, [snapshot]);

  const loadRunIntoEngine = useCallback(async (targetRunId: string) => {
    await invoke("orchestrator_load_run", { runId: targetRunId });
  }, []);

  const handleSelectRun = async (targetRunId: string) => {
    setIsRunActionLoading(true);
    setError(null);
    try {
      await loadRunIntoEngine(targetRunId);
      setRunId(targetRunId);
      setHasExplicitRunSelection(true);
      const summary = runs.find((run) => run.run_id === targetRunId);
      if (summary?.objective) {
        setObjective(summary.objective);
      }
      setEventFeed((prev) => {
        const at = new Date().toLocaleTimeString();
        return [`${at} loaded run ${targetRunId}`, ...prev].slice(0, 40);
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsRunActionLoading(false);
    }
  };

  const handleViewTasks = () => {
    taskBoardRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  const handleContinueRun = async () => {
    if (!runId || !snapshot) return;
    setIsRunActionLoading(true);
    setError(null);
    try {
      await loadRunIntoEngine(runId);
      const canSwitchResumeModel =
        snapshot.status === "paused" ||
        snapshot.status === "failed" ||
        snapshot.status === "cancelled";
      const nextModel = selectedModelRef.current ?? selectedModel;
      const nextProvider = selectedProviderRef.current ?? selectedProvider;
      const currentModel = runModelSelection?.model ?? null;
      const currentProvider = runModelSelection?.provider ?? null;
      const shouldSwitchResumeModel =
        canSwitchResumeModel &&
        !!nextModel &&
        !!nextProvider &&
        (nextModel !== currentModel || nextProvider !== currentProvider);
      if (shouldSwitchResumeModel) {
        const selection = await invoke<RunModelSelection>("orchestrator_set_resume_model", {
          runId,
          model: nextModel,
          provider: nextProvider,
        });
        setRunModelSelection({
          model: selection.model ?? nextModel,
          provider: selection.provider ?? nextProvider,
        });
      }
      if (snapshot.status === "awaiting_approval") {
        await invoke("orchestrator_approve", { runId });
      } else if (snapshot.status === "paused") {
        await invoke("orchestrator_resume", { runId });
      } else if (snapshot.status === "failed" || snapshot.status === "cancelled") {
        await invoke("orchestrator_restart_run", { runId });
      } else if (snapshot.status === "completed") {
        await invoke("orchestrator_restart_run", { runId });
      } else if (snapshot.status === "revision_requested") {
        await invoke("orchestrator_start", { runId });
      }
      const at = new Date().toLocaleTimeString();
      setEventFeed((prev) => [`${at} continue requested for ${runId}`, ...prev].slice(0, 40));
      await loadRuns();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsRunActionLoading(false);
    }
  };

  const handlePauseRun = async () => {
    if (!runId) return;
    setIsRunActionLoading(true);
    setError(null);
    try {
      await loadRunIntoEngine(runId);
      await invoke("orchestrator_pause", { runId });
      const at = new Date().toLocaleTimeString();
      setEventFeed((prev) => [`${at} pause requested for ${runId}`, ...prev].slice(0, 40));
      await loadRuns();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsRunActionLoading(false);
    }
  };

  const handleCancelRun = async () => {
    if (!runId) return;
    setIsRunActionLoading(true);
    setError(null);
    try {
      await loadRunIntoEngine(runId);
      await invoke("orchestrator_cancel", { runId });
      const at = new Date().toLocaleTimeString();
      setEventFeed((prev) => [`${at} cancel requested for ${runId}`, ...prev].slice(0, 40));
      await loadRuns();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsRunActionLoading(false);
    }
  };

  const handleRetryTask = async (task: Task) => {
    if (!runId) return;
    setIsRunActionLoading(true);
    setError(null);
    try {
      await invoke("orchestrator_retry_task", { runId, taskId: task.id });

      const [nextSnapshot, nextTasks] = await Promise.all([
        invoke<RunSnapshot>("orchestrator_get_run", { runId }),
        invoke<Task[]>("orchestrator_list_tasks", { runId }),
      ]);
      lastSnapshotRef.current = nextSnapshot;
      setSnapshot(nextSnapshot);
      setTasks(nextTasks);

      if (nextSnapshot.status === "paused") {
        await invoke("orchestrator_resume", { runId });
      }

      const at = new Date().toLocaleTimeString();
      setEventFeed((prev) => [`${at} retry requested for ${task.id}`, ...prev].slice(0, 40));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsRunActionLoading(false);
    }
  };

  const handleDeleteRun = async (targetRunId: string) => {
    try {
      await deleteOrchestratorRun(targetRunId);
      if (runId === targetRunId) {
        setRunId(null);
        setHasExplicitRunSelection(false);
        setSnapshot(null);
        setTasks([]);
      }
      await loadRuns();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div ref={pageScrollRef} className="h-full w-full overflow-y-auto app-background p-6">
      <div className="mx-auto max-w-6xl space-y-4">
        <div className="rounded-lg border border-border bg-surface p-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="space-y-2">
              <h2 className="text-lg font-semibold text-text">Command Center (beta)</h2>
              <p className="text-sm text-text-muted">
                Launch orchestrator missions from one objective, then use operator controls for
                manual swarm intervention.
              </p>
              <div className="w-full max-w-xl">
                <ProjectSwitcher
                  projects={userProjects}
                  activeProject={activeProject}
                  onSwitchProject={onSwitchProject}
                  onAddProject={onAddProject}
                  onManageProjects={onManageProjects}
                  isLoading={projectSwitcherLoading}
                />
              </div>
              <p className="text-xs text-text-subtle">
                Workspace:{" "}
                <span className="font-mono">{workspacePath ?? "No active project selected"}</span>
              </p>
            </div>
            <div className="flex items-center gap-2">
              <ModelSelector
                currentModel={selectedModel}
                align="left"
                side="bottom"
                onModelSelect={async (modelId, providerIdRaw) => {
                  const providerId = providerIdRaw === "opencode" ? "opencode_zen" : providerIdRaw;
                  const providerIdForSidecar =
                    providerId === "opencode_zen" ? "opencode" : providerId;
                  try {
                    const config = await getProvidersConfig();
                    await setProvidersConfig({
                      ...config,
                      selected_model: {
                        provider_id: providerId,
                        model_id: modelId,
                      },
                    });
                  } catch (error) {
                    console.error("Failed to persist swarm model selection:", error);
                  }
                  setSelectedModel(modelId);
                  setSelectedProvider(providerIdForSidecar);
                  selectedModelRef.current = modelId;
                  selectedProviderRef.current = providerIdForSidecar;
                }}
              />
              <button
                type="button"
                onClick={() => setShowLogsDrawer(true)}
                className="rounded p-1 text-text-subtle hover:bg-surface-elevated hover:text-text"
                title="Open logs"
              >
                <ScrollText className="h-4 w-4" />
              </button>
              <Button
                variant={tab === "task-to-swarm" ? "primary" : "secondary"}
                size="sm"
                onClick={() => setTab("task-to-swarm")}
              >
                Task to Swarm
              </Button>
              <Button
                variant={tab === "advanced" ? "primary" : "secondary"}
                size="sm"
                onClick={() => setTab("advanced")}
              >
                Advanced Controls
              </Button>
            </div>
          </div>
        </div>

        <div className="rounded-lg border border-border bg-surface p-4">
          <div className="flex items-center justify-between gap-2">
            <div className="text-xs uppercase tracking-wide text-text-subtle">Scheduled Bots</div>
            <div className="text-xs text-text-muted">
              {routineRunsLoading ? "Refreshing..." : `${routineRuns.length} recent runs`}
            </div>
          </div>
          <div className="mt-2 grid grid-cols-1 gap-2">
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-4">
              <div className="rounded-md border border-border bg-surface-elevated/50 px-3 py-2">
                <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                  Needs Approval
                </div>
                <div className="text-sm font-semibold text-text">
                  {routineRuns.filter((run) => run.status === "pending_approval").length}
                </div>
              </div>
              <div className="rounded-md border border-border bg-surface-elevated/50 px-3 py-2">
                <div className="text-[10px] uppercase tracking-wide text-text-subtle">Blocked</div>
                <div className="text-sm font-semibold text-text">
                  {routineRuns.filter((run) => run.status === "blocked_policy").length}
                </div>
              </div>
              <div className="rounded-md border border-border bg-surface-elevated/50 px-3 py-2">
                <div className="text-[10px] uppercase tracking-wide text-text-subtle">Paused</div>
                <div className="text-sm font-semibold text-text">
                  {routineRuns.filter((run) => run.status === "paused").length}
                </div>
              </div>
              <div className="rounded-md border border-border bg-surface-elevated/50 px-3 py-2">
                <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                  Artifacts
                </div>
                <div className="text-sm font-semibold text-text">
                  {routineRuns.reduce((sum, run) => sum + run.artifacts.length, 0)}
                </div>
              </div>
            </div>
            {routineRuns.slice(0, 8).map((run) => {
              const busy = routineActionBusyRunId === run.run_id;
              return (
                <div
                  key={run.run_id}
                  className="rounded-md border border-border bg-surface-elevated/50 px-3 py-2"
                >
                  <div className="flex items-center justify-between gap-2">
                    <div className="min-w-0">
                      <div className="truncate text-xs font-semibold text-text">
                        {run.routine_id} · {run.status}
                      </div>
                      <div className="mt-0.5 truncate text-[11px] text-text-muted">
                        run {run.run_id} · {run.trigger_type}
                      </div>
                      {run.artifacts.length > 0 ? (
                        <div className="mt-0.5 text-[11px] text-text-subtle">
                          {run.artifacts.length} artifact{run.artifacts.length === 1 ? "" : "s"}
                        </div>
                      ) : null}
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
                        <div className="mt-0.5 text-[11px] text-text-subtle">tool scope: all</div>
                      )}
                    </div>
                    <div className="flex items-center gap-1">
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
                      {run.status === "queued" || run.status === "running" ? (
                        <Button
                          size="sm"
                          variant="ghost"
                          disabled={busy}
                          onClick={() => void handleRoutineRunAction(run, "pause")}
                        >
                          Pause
                        </Button>
                      ) : null}
                      {run.status === "paused" ? (
                        <Button
                          size="sm"
                          variant="ghost"
                          disabled={busy}
                          onClick={() => void handleRoutineRunAction(run, "resume")}
                        >
                          Resume
                        </Button>
                      ) : null}
                    </div>
                  </div>
                </div>
              );
            })}
            {!routineRunsLoading && routineRuns.length === 0 ? (
              <div className="rounded-md border border-border bg-surface-elevated/50 px-3 py-2 text-xs text-text-muted">
                No recent routine runs.
              </div>
            ) : null}
          </div>
        </div>

        <div className="rounded-lg border border-border bg-surface p-4">
          <div className="flex items-center justify-between gap-2">
            <div className="text-xs uppercase tracking-wide text-text-subtle">Connectors</div>
            <div className="text-xs text-text-muted">
              {mcpLoading
                ? "Refreshing..."
                : `${mcpServers.filter((row) => row.connected).length}/${mcpServers.length} connected`}
            </div>
          </div>
          <div className="mt-2 grid grid-cols-1 gap-2 md:grid-cols-2">
            {mcpServers.slice(0, 6).map((server) => {
              const count = mcpTools.filter((tool) => tool.server_name === server.name).length;
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

        <div className="rounded-lg border border-border bg-surface p-4">
          <div className="flex items-center justify-between gap-2">
            <div className="text-xs uppercase tracking-wide text-text-subtle">Workspace Files</div>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setWorkspaceFilesExpanded((prev) => !prev)}
            >
              {workspaceFilesExpanded ? (
                <>
                  <ChevronUp className="mr-1 h-3.5 w-3.5" />
                  Collapse
                </>
              ) : (
                <>
                  <ChevronDown className="mr-1 h-3.5 w-3.5" />
                  Expand
                </>
              )}
            </Button>
          </div>
          <div className="mt-2 text-xs text-text-muted">
            Click a file to open rich preview in the side pane.
          </div>
          {workspaceFilesExpanded ? (
            !workspacePath ? (
              <div className="mt-3 text-xs text-text-muted">No active project selected.</div>
            ) : (
              <div className="mt-3 h-[320px] overflow-hidden rounded border border-border bg-surface-elevated/30">
                <FileBrowser
                  rootPath={workspacePath}
                  onFileSelect={(file) => {
                    setSelectedWorkspaceFile(file);
                    if (!file.is_directory) {
                      onFileOpen?.(file);
                    }
                  }}
                  selectedPath={selectedWorkspaceFile?.path}
                />
              </div>
            )
          ) : null}
        </div>

        {tab === "task-to-swarm" ? (
          <div className="grid grid-cols-1 gap-4 xl:grid-cols-4">
            {hasExplicitRunSelection && runId ? (
              <div className="xl:col-span-4 rounded-lg border border-border bg-surface p-4">
                <div className="text-xs uppercase tracking-wide text-text-subtle">Live Status</div>
                <div className="mt-2 flex flex-col gap-3 xl:flex-row xl:items-start xl:justify-between">
                  <div className="min-w-0 flex-1 xl:min-w-[560px]">
                    <div className="flex items-center gap-2 text-xs text-text-muted">
                      {isWorking ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin text-primary" />
                      ) : null}
                      <span className="leading-relaxed">
                        {isWorking
                          ? "Swarm running. Planning/execution can take a bit on larger tasks."
                          : sidecarStarting
                            ? "Engine starting. Launch will unlock when ready."
                            : "Idle."}
                      </span>
                    </div>
                    <div className="mt-3 grid grid-cols-[repeat(auto-fit,minmax(170px,1fr))] gap-2">
                      {[
                        { label: "Engine", value: sidecarStatus },
                        { label: "Stage", value: stage.replace("_", " ") },
                        { label: "Run", value: runId || "none", mono: true },
                        {
                          label: "Effective CWD",
                          value: effectiveWorkspaceForRun,
                          mono: true,
                        },
                        {
                          label: "Model",
                          value:
                            runModelSelection?.provider && runModelSelection?.model
                              ? `${runModelSelection.provider} / ${runModelSelection.model}`
                              : "pending",
                        },
                        { label: "Tasks", value: String(tasks.length) },
                        { label: "Pending", value: String(pendingTasks) },
                        {
                          label: "Tokens",
                          value: snapshot
                            ? `${snapshot.budget.tokens_used.toLocaleString()} / ${snapshot.budget.max_tokens.toLocaleString()}`
                            : "n/a",
                        },
                        {
                          label: "Agent Turns",
                          value: snapshot
                            ? `${snapshot.budget.subagent_runs_used.toLocaleString()} / ${snapshot.budget.max_subagent_runs.toLocaleString()}`
                            : "n/a",
                        },
                        {
                          label: "Wall",
                          value: snapshot
                            ? `${snapshot.budget.wall_time_secs.toLocaleString()} / ${snapshot.budget.max_wall_time_secs.toLocaleString()}s`
                            : "n/a",
                        },
                        {
                          label: "Price",
                          value:
                            inferredRunCostUsd !== null
                              ? `$${inferredRunCostUsd.toFixed(4)}`
                              : "Unavailable",
                        },
                      ].map((item) => (
                        <div
                          key={item.label}
                          className="rounded-md border border-border bg-surface-elevated/60 px-2.5 py-2"
                        >
                          <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                            {item.label}
                          </div>
                          <div
                            className={`mt-0.5 truncate text-xs text-text ${
                              item.mono ? "font-mono" : ""
                            }`}
                            title={item.value}
                          >
                            {item.value}
                          </div>
                        </div>
                      ))}
                    </div>
                    {sidecarStartupHealth && !sidecarStartupHealth.ready ? (
                      <div className="mt-1 text-xs text-text-muted">
                        Engine phase: {sidecarStartupHealth.phase} (
                        {sidecarStartupHealth.startup_elapsed_ms}ms)
                      </div>
                    ) : null}
                  </div>
                  <div className="w-full xl:w-[420px] xl:flex-none rounded-lg border border-primary/20 bg-gradient-to-r from-primary/10 via-surface-elevated/50 to-surface-elevated/30 p-2.5">
                    <div className="mb-2 text-[10px] font-semibold uppercase tracking-wide text-text-subtle">
                      Actions
                    </div>
                    <div className="flex flex-wrap gap-2 xl:justify-end">
                      {runId ? (
                        <Button
                          size="sm"
                          variant="secondary"
                          className="border-primary/40 bg-primary/15 text-primary shadow-sm hover:-translate-y-0.5 hover:bg-primary/25 hover:shadow-md focus-visible:ring-primary/80"
                          onClick={handleViewTasks}
                          disabled={isRunActionLoading}
                        >
                          <ScrollText className="mr-1 h-4 w-4" />
                          View Tasks
                        </Button>
                      ) : null}
                      {runId &&
                      snapshot &&
                      ["planning", "awaiting_approval", "executing"].includes(snapshot.status) ? (
                        <Button
                          size="sm"
                          variant="secondary"
                          className="border-primary/35 bg-primary/10 text-primary shadow-sm hover:-translate-y-0.5 hover:bg-primary/20 hover:shadow-md focus-visible:ring-primary/80"
                          onClick={() => void handlePauseRun()}
                          disabled={isRunActionLoading}
                        >
                          {isRunActionLoading ? (
                            <Loader2 className="mr-1 h-4 w-4 animate-spin" />
                          ) : (
                            <Pause className="mr-1 h-4 w-4" />
                          )}
                          Pause Run
                        </Button>
                      ) : null}
                      {runId &&
                      snapshot &&
                      !["completed", "cancelled"].includes(snapshot.status) ? (
                        <Button
                          size="sm"
                          variant="danger"
                          className="border border-error/50 bg-error/90 text-white shadow-sm hover:-translate-y-0.5 hover:bg-error hover:shadow-md focus-visible:ring-error/80"
                          onClick={() => void handleCancelRun()}
                          disabled={isRunActionLoading}
                        >
                          {isRunActionLoading ? (
                            <Loader2 className="mr-1 h-4 w-4 animate-spin" />
                          ) : (
                            <Square className="mr-1 h-4 w-4" />
                          )}
                          Cancel Run
                        </Button>
                      ) : null}
                      {runId &&
                      snapshot &&
                      [
                        "awaiting_approval",
                        "paused",
                        "failed",
                        "cancelled",
                        "completed",
                        "revision_requested",
                      ].includes(snapshot.status) ? (
                        <Button
                          size="sm"
                          variant={
                            snapshot.status === "failed" ||
                            snapshot.status === "cancelled" ||
                            snapshot.status === "completed"
                              ? "secondary"
                              : "primary"
                          }
                          className={
                            snapshot.status === "failed" ||
                            snapshot.status === "cancelled" ||
                            snapshot.status === "completed"
                              ? "border-primary/40 bg-primary/15 text-primary shadow-sm hover:-translate-y-0.5 hover:bg-primary/25 hover:shadow-md focus-visible:ring-primary/80"
                              : "shadow-sm hover:-translate-y-0.5 hover:shadow-md focus-visible:ring-primary/80"
                          }
                          onClick={() => void handleContinueRun()}
                          disabled={isRunActionLoading}
                        >
                          {isRunActionLoading ? (
                            <Loader2 className="mr-1 h-4 w-4 animate-spin" />
                          ) : (
                            <RotateCcw className="mr-1 h-4 w-4" />
                          )}
                          {snapshot.status === "failed" ||
                          snapshot.status === "cancelled" ||
                          snapshot.status === "completed"
                            ? "Restart Run"
                            : "Continue Run"}
                        </Button>
                      ) : null}
                      {stage === "awaiting_review" ? (
                        <Button
                          size="sm"
                          className="shadow-sm hover:-translate-y-0.5 hover:shadow-md focus-visible:ring-primary/80"
                          onClick={() => void approvePlan()}
                          disabled={isLoading}
                        >
                          <CheckCircle2 className="mr-1 h-4 w-4" />
                          Approve & Execute
                        </Button>
                      ) : null}
                    </div>
                  </div>
                </div>
                {snapshot ? (
                  <div className="mt-3">{<BudgetMeter budget={snapshot.budget} />}</div>
                ) : null}
                {snapshot?.error_message ? (
                  <div className="mt-3 rounded border border-red-500/30 bg-red-500/10 p-2 text-xs text-red-200">
                    {snapshot.error_message}
                  </div>
                ) : null}
                <div className="mt-2 text-[11px] text-text-muted">
                  Default safety: plan preview first, then one-click approval to execute.
                </div>
              </div>
            ) : null}

            <div className="rounded-lg border border-border bg-surface p-4 space-y-3">
              <div className="flex items-center justify-between">
                <div className="text-xs uppercase tracking-wide text-text-subtle">Runs</div>
                <div className="flex items-center gap-2">
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => void loadRuns()}
                    disabled={runsLoading}
                  >
                    <RefreshCw
                      className={`mr-1 h-3.5 w-3.5 ${runsLoading ? "animate-spin" : ""}`}
                    />
                    Refresh
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setRunsCollapsed((prev) => !prev)}
                  >
                    {runsCollapsed ? (
                      <>
                        <ChevronDown className="mr-1 h-3.5 w-3.5" />
                        Expand
                      </>
                    ) : (
                      <>
                        <ChevronUp className="mr-1 h-3.5 w-3.5" />
                        Collapse
                      </>
                    )}
                  </Button>
                </div>
              </div>
              <button
                className={`w-full rounded border px-3 py-2 text-left text-xs ${
                  !runId
                    ? "border-primary/40 bg-primary/10 text-primary"
                    : "border-border text-text-muted hover:bg-surface-elevated"
                }`}
                onClick={() => {
                  setRunId(null);
                  setHasExplicitRunSelection(false);
                  setPendingCreatedRun(null);
                }}
              >
                New run
              </button>
              {runsCollapsed ? (
                <div className="rounded border border-border/70 bg-surface-elevated/30 p-2 text-xs text-text-muted">
                  {selectedRunSummary ? (
                    <div className="space-y-1">
                      <div className="truncate text-text">{selectedRunSummary.objective}</div>
                      <div className="flex items-center gap-2">
                        <span
                          className={`rounded border px-1.5 py-0.5 text-[10px] uppercase ${runStatusBadgeClass(selectedRunSummary.status)}`}
                        >
                          {runStatusLabel(selectedRunSummary.status)}
                        </span>
                      </div>
                    </div>
                  ) : (
                    "Collapsed. Expand to select a run."
                  )}
                </div>
              ) : null}
              {displayedRuns.length === 0 ? (
                <div className="text-xs text-text-muted">No runs yet for this project.</div>
              ) : (
                <div
                  className={`space-y-2 overflow-y-auto ${runsCollapsed ? "hidden" : "max-h-96"}`}
                >
                  {displayedRuns.map((run) => (
                    <div
                      key={run.run_id}
                      className={`rounded border p-2 ${
                        runId === run.run_id
                          ? "border-primary/40 bg-primary/10"
                          : "border-border bg-surface-elevated/30"
                      }`}
                    >
                      <button
                        className="w-full text-left"
                        onClick={() => void handleSelectRun(run.run_id)}
                        title={run.objective}
                        disabled={isRunActionLoading || isLoading}
                      >
                        <div className="truncate text-xs text-text">{run.objective}</div>
                        <div className="mt-1 flex items-center gap-2 text-[11px] text-text-muted">
                          <span
                            className={`rounded border px-1.5 py-0.5 text-[10px] uppercase ${runStatusBadgeClass(run.status)}`}
                          >
                            {runStatusLabel(run.status)}
                          </span>
                          <span>
                            started {new Date(run.started_at || run.created_at).toLocaleString()}
                          </span>
                          {run.ended_at ? (
                            <span>ended {new Date(run.ended_at).toLocaleString()}</span>
                          ) : null}
                        </div>
                        <div className="mt-1 text-[11px] text-text-muted">
                          updated {new Date(run.updated_at).toLocaleString()}
                        </div>
                        {run.last_error ? (
                          <div className="mt-1 truncate text-[11px] text-red-300">
                            {run.last_error}
                          </div>
                        ) : null}
                      </button>
                      <button
                        className="mt-2 inline-flex items-center rounded border border-red-500/30 px-2 py-1 text-[11px] text-red-300 hover:bg-red-500/10"
                        onClick={() => void handleDeleteRun(run.run_id)}
                        disabled={isLoading}
                      >
                        <Trash2 className="mr-1 h-3 w-3" />
                        Delete
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>

            {!runId ? (
              <div className="xl:col-span-2 rounded-lg border border-border bg-surface p-4 space-y-3">
                <div className="text-xs uppercase tracking-wide text-text-subtle">Objective</div>
                <textarea
                  value={objective}
                  onChange={(e) => setObjective(e.target.value)}
                  placeholder="Describe the mission. The orchestrator will plan role-assigned tasks, preview for approval, then execute."
                  className="min-h-[120px] w-full rounded-lg border border-border bg-surface-elevated p-3 text-sm text-text placeholder:text-text-muted focus:border-primary focus:outline-none"
                />
                <div className="flex flex-wrap gap-2">
                  {(["speed", "balanced", "quality"] as QualityPreset[]).map((nextPreset) => (
                    <button
                      key={nextPreset}
                      className={`rounded-full border px-3 py-1 text-xs ${
                        preset === nextPreset
                          ? "border-primary/50 bg-primary/10 text-primary"
                          : "border-border text-text-muted"
                      }`}
                      onClick={() => setPreset(nextPreset)}
                    >
                      {nextPreset}
                    </button>
                  ))}
                </div>
                <div className="flex flex-wrap gap-2">
                  <Button onClick={() => void launchSwarm()} disabled={launchDisabled}>
                    {isLoading ? (
                      <Loader2 className="mr-1 h-4 w-4 animate-spin" />
                    ) : (
                      <Sparkles className="mr-1 h-4 w-4" />
                    )}
                    Launch Swarm
                  </Button>
                  {!sidecarReady ? (
                    <div className="text-xs text-text-muted">
                      Engine is starting. Launch enables automatically when ready.
                    </div>
                  ) : null}
                </div>
                <div className="text-xs text-text-muted">
                  Limits: {missionLimits.wallTimeHours}h,{" "}
                  {missionLimits.maxTotalTokens.toLocaleString()} tokens,{" "}
                  {missionLimits.maxIterations.toLocaleString()} iterations.{" "}
                  <button
                    type="button"
                    className="text-primary underline-offset-2 hover:underline"
                    onClick={() => setTab("advanced")}
                  >
                    Edit in Advanced Controls
                  </button>
                </div>
                {error ? (
                  <div className="rounded border border-red-500/30 bg-red-500/10 p-2 text-xs text-red-200">
                    {error}
                  </div>
                ) : null}
              </div>
            ) : (
              <div className="xl:col-span-2 rounded-lg border border-border bg-surface p-4 space-y-2">
                <div className="text-xs uppercase tracking-wide text-text-subtle">Selected Run</div>
                <div
                  className={`text-sm text-text whitespace-pre-wrap break-words ${
                    selectedRunObjectiveExpanded ? "" : "line-clamp-4"
                  }`}
                >
                  {selectedRunSummary?.objective || objective || "Run selected"}
                </div>
                {(selectedRunSummary?.objective || objective)?.trim() ? (
                  <button
                    type="button"
                    className="text-xs text-primary underline-offset-2 hover:underline"
                    onClick={() => setSelectedRunObjectiveExpanded((prev) => !prev)}
                    aria-label={
                      selectedRunObjectiveExpanded
                        ? "Show less objective text"
                        : "Show more objective text"
                    }
                  >
                    {selectedRunObjectiveExpanded ? "Show less" : "Show more"}
                  </button>
                ) : null}
                {selectedRunSummary ? (
                  <div className="flex flex-wrap items-center gap-2 text-[11px] text-text-muted">
                    <span
                      className={`rounded border px-1.5 py-0.5 text-[10px] uppercase ${runStatusBadgeClass(selectedRunSummary.status)}`}
                    >
                      {runStatusLabel(selectedRunSummary.status)}
                    </span>
                    <span>
                      started{" "}
                      {new Date(
                        selectedRunSummary.started_at || selectedRunSummary.created_at
                      ).toLocaleString()}
                    </span>
                    {selectedRunSummary.ended_at ? (
                      <span>ended {new Date(selectedRunSummary.ended_at).toLocaleString()}</span>
                    ) : null}
                    {selectedRunSummary.last_error ? (
                      <span className="truncate text-red-300" title={selectedRunSummary.last_error}>
                        err: {selectedRunSummary.last_error}
                      </span>
                    ) : null}
                  </div>
                ) : null}
                <div className="text-xs text-text-muted">
                  Effective workspace/cwd:{" "}
                  <span className="font-mono">{effectiveWorkspaceForRun}</span>
                </div>
                <div className="text-xs text-text-muted">
                  Use Live Status actions to continue, pause, cancel, or restart this run.
                </div>
              </div>
            )}

            <div className="xl:col-span-1 rounded-lg border border-border bg-surface p-4">
              <div className="text-xs uppercase tracking-wide text-text-subtle mb-2">
                Activity Strip
              </div>
              {eventFeed.length === 0 ? (
                <div className="text-xs text-text-muted">
                  Waiting for orchestrator/agent-team events...
                </div>
              ) : (
                <div className="space-y-1 max-h-56 overflow-y-auto">
                  {eventFeed.map((line, idx) => (
                    <div
                      key={`${line}-${idx}`}
                      className="rounded border border-border bg-surface-elevated p-2 text-xs text-text"
                    >
                      {line}
                    </div>
                  ))}
                </div>
              )}
            </div>
            <div className="xl:col-span-4 rounded-lg border border-border bg-surface p-0 overflow-hidden min-h-[320px]">
              <div className="border-b border-border px-4 py-3 text-xs uppercase tracking-wide text-text-subtle">
                Console
              </div>
              <div className="h-[320px]">
                <ConsoleTab
                  sessionId={selectedRunSessionId ?? activeRunSessionId}
                  sessionIds={selectedRunConsoleSessionIds}
                  autoScroll
                />
              </div>
            </div>
            <div
              ref={taskBoardRef}
              className="xl:col-span-4 rounded-lg border border-border bg-surface p-4"
            >
              <div className="mb-3 text-xs uppercase tracking-wide text-text-subtle">
                Task Board
              </div>
              <TaskBoard
                tasks={tasks}
                currentTaskId={snapshot?.current_task_id}
                onRetryTask={(task) => void handleRetryTask(task)}
              />
            </div>
          </div>
        ) : (
          <div className="space-y-3">
            <div className="rounded-lg border border-border bg-surface p-4 space-y-3">
              <div className="text-xs uppercase tracking-wide text-text-subtle">Start Mission</div>
              <textarea
                value={objective}
                onChange={(e) => setObjective(e.target.value)}
                placeholder="Enter the mission objective, then launch. This triggers the orchestrator run."
                className="min-h-[90px] w-full rounded-lg border border-border bg-surface-elevated p-3 text-sm text-text placeholder:text-text-muted focus:border-primary focus:outline-none"
              />
              <div className="flex flex-wrap gap-2">
                {(["speed", "balanced", "quality"] as QualityPreset[]).map((nextPreset) => (
                  <button
                    key={nextPreset}
                    className={`rounded-full border px-3 py-1 text-xs ${
                      preset === nextPreset
                        ? "border-primary/50 bg-primary/10 text-primary"
                        : "border-border text-text-muted"
                    }`}
                    onClick={() => setPreset(nextPreset)}
                  >
                    {nextPreset}
                  </button>
                ))}
                <Button onClick={() => void launchSwarm()} disabled={launchDisabled}>
                  {isLoading ? (
                    <Loader2 className="mr-1 h-4 w-4 animate-spin" />
                  ) : (
                    <Sparkles className="mr-1 h-4 w-4" />
                  )}
                  Launch Swarm
                </Button>
              </div>
              {error ? (
                <div className="rounded border border-red-500/30 bg-red-500/10 p-2 text-xs text-red-200">
                  {error}
                </div>
              ) : null}
            </div>
            <AgentModelRoutingPanel routing={modelRouting} onChange={setModelRouting} />
            <div className="rounded-lg border border-border bg-surface p-4 space-y-3">
              <div className="text-xs uppercase tracking-wide text-text-subtle">Mission Limits</div>
              <div className="grid gap-3 md:grid-cols-2">
                <label className="space-y-1">
                  <div className="text-xs text-text-muted">Wall time (hours)</div>
                  <input
                    type="number"
                    min={1}
                    max={168}
                    value={missionLimits.wallTimeHours}
                    onChange={(e) =>
                      setMissionLimits((prev) => ({
                        ...prev,
                        wallTimeHours: Number(e.target.value) || 1,
                      }))
                    }
                    className="w-full rounded border border-border bg-surface-elevated px-2 py-1.5 text-sm text-text focus:border-primary focus:outline-none"
                  />
                </label>
                <label className="space-y-1">
                  <div className="text-xs text-text-muted">Max total tokens</div>
                  <input
                    type="number"
                    min={10000}
                    step={10000}
                    value={missionLimits.maxTotalTokens}
                    onChange={(e) =>
                      setMissionLimits((prev) => ({
                        ...prev,
                        maxTotalTokens: Number(e.target.value) || 10000,
                      }))
                    }
                    className="w-full rounded border border-border bg-surface-elevated px-2 py-1.5 text-sm text-text focus:border-primary focus:outline-none"
                  />
                </label>
                <label className="space-y-1">
                  <div className="text-xs text-text-muted">Max tokens per step</div>
                  <input
                    type="number"
                    min={1000}
                    step={1000}
                    value={missionLimits.maxTokensPerStep}
                    onChange={(e) =>
                      setMissionLimits((prev) => ({
                        ...prev,
                        maxTokensPerStep: Number(e.target.value) || 1000,
                      }))
                    }
                    className="w-full rounded border border-border bg-surface-elevated px-2 py-1.5 text-sm text-text focus:border-primary focus:outline-none"
                  />
                </label>
                <label className="space-y-1">
                  <div className="text-xs text-text-muted">Max iterations</div>
                  <input
                    type="number"
                    min={50}
                    step={50}
                    value={missionLimits.maxIterations}
                    onChange={(e) =>
                      setMissionLimits((prev) => ({
                        ...prev,
                        maxIterations: Number(e.target.value) || 50,
                      }))
                    }
                    className="w-full rounded border border-border bg-surface-elevated px-2 py-1.5 text-sm text-text focus:border-primary focus:outline-none"
                  />
                </label>
                <label className="space-y-1">
                  <div className="text-xs text-text-muted">Max subagent runs</div>
                  <input
                    type="number"
                    min={100}
                    step={100}
                    value={missionLimits.maxSubagentRuns}
                    onChange={(e) =>
                      setMissionLimits((prev) => ({
                        ...prev,
                        maxSubagentRuns: Number(e.target.value) || 100,
                      }))
                    }
                    className="w-full rounded border border-border bg-surface-elevated px-2 py-1.5 text-sm text-text focus:border-primary focus:outline-none"
                  />
                </label>
                <label className="space-y-1">
                  <div className="text-xs text-text-muted">Max task retries</div>
                  <input
                    type="number"
                    min={0}
                    max={20}
                    value={missionLimits.maxTaskRetries}
                    onChange={(e) =>
                      setMissionLimits((prev) => ({
                        ...prev,
                        maxTaskRetries: Number(e.target.value) || 0,
                      }))
                    }
                    className="w-full rounded border border-border bg-surface-elevated px-2 py-1.5 text-sm text-text focus:border-primary focus:outline-none"
                  />
                </label>
              </div>
              <div className="flex flex-wrap gap-2">
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() =>
                    setMissionLimits((prev) => ({
                      ...prev,
                      wallTimeHours: 24,
                      maxTotalTokens: 300_000,
                      maxIterations: 600,
                      maxSubagentRuns: 2_000,
                    }))
                  }
                >
                  24h profile
                </Button>
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() =>
                    setMissionLimits((prev) => ({
                      ...prev,
                      wallTimeHours: 48,
                      maxTotalTokens: 500_000,
                      maxIterations: 800,
                      maxSubagentRuns: 3_000,
                    }))
                  }
                >
                  48h profile
                </Button>
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() =>
                    setMissionLimits((prev) => ({
                      ...prev,
                      wallTimeHours: 72,
                      maxTotalTokens: 800_000,
                      maxIterations: 1200,
                      maxSubagentRuns: 5_000,
                    }))
                  }
                >
                  72h profile
                </Button>
              </div>
            </div>
            <div className="rounded-lg border border-border bg-surface-elevated/40 p-3 text-xs text-text-muted">
              Orchestrator role model routing applies to newly launched runs from this page.
            </div>
            <div className="rounded-lg border border-border bg-surface p-3 text-xs text-text-muted">
              Operator Controls use agent-team mission APIs for manual spawn, approval triage,
              mission/instance cancellation, and forensic exports.
            </div>
            <AgentCommandCenter />
          </div>
        )}
      </div>
      {showLogsDrawer && (
        <LogsDrawer
          onClose={() => setShowLogsDrawer(false)}
          sessionId={selectedRunSessionId ?? activeRunSessionId}
          sessionIds={selectedRunConsoleSessionIds}
        />
      )}
    </div>
  );
}
