import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatedPage, Badge, LoadingState, PanelCard, StatusPulse } from "../ui/index.tsx";
import { EmptyState } from "./ui";
import { useCapabilities } from "../features/system/queries.ts";
import { subscribeSse } from "../services/sse.js";
import { CodingWorkflowsAgentCockpit } from "./CodingWorkflowsAgentCockpit";
import { CodingWorkflowsOverviewTab } from "./CodingWorkflowsOverviewTab";
import { optimisticallyMoveBoardItems } from "./CodingWorkflowsOptimisticBoard";
import { CodingWorkflowsRegisterProjectPanel } from "./CodingWorkflowsRegisterProjectPanel";
import { CodingWorkflowsLinearTaskStateSelect } from "./CodingWorkflowsLinearTaskStateSelect";
import { TaskPlanningPanel } from "./TaskPlanningPanel";
import { ProviderModelSelector } from "../components/ProviderModelSelector";
import { buildPlannerProviderOptions } from "../features/planner/plannerShared";
import type { AppPageProps } from "./pageTypes";
import { LazyJson } from "../features/automations/LazyJson";
import { CodingWorkflowsConnectingState, CodingWorkflowsDisconnectedState } from "./CodingWorkflowsDisconnectedState";
import {
  CodingWorkflowsRunDiagnosticsPanel,
  runEventDiagnosticBadges,
} from "./CodingWorkflowsRunDiagnosticsPanel";
import {
  type CodingTab,
  type GithubRepoRef,
  type PlannerProviderOption,
  type TaskSourceType,
  ACTIVE_RUN_STALE_AFTER_MS,
  GITHUB_ITEM_LAUNCH_LOCK_MS,
  buildTaskSourcePayload,
  dedupeRuns,
  formatStatus,
  findLinearCatalogEntry,
  githubBoardItemCanRun,
  githubBoardItemIdentity,
  githubBoardItemLaunchLabel,
  isSafeManagedPath,
  normalizeGithubBoard,
  normalizeProjects,
  normalizeServers,
  normalizeTools,
  parseGithubRepoRef,
  parseSseEnvelope,
  runHasLiveSession,
  runId,
  runIsActive,
  runPhase,
  runStatus,
  runTaskIdentity,
  runTitle,
  runUpdatedAt,
  toArray,
} from "./CodingWorkflowsHelpers";
import { Icon } from "../ui/Icon";

const LINEAR_CATALOG_TIMEOUT_MS = 8_000;

export function CodingWorkflowsPage({
  api,
  client,
  toast,
  providerStatus,
  navigate,
}: AppPageProps) {
  const queryClient = useQueryClient();
  const [tab, setTab] = useState<CodingTab>("overview");
  const [selectedProjectSlug, setSelectedProjectSlug] = useState("");
  const [selectedRunId, setSelectedRunId] = useState("");
  const [selectedLogName, setSelectedLogName] = useState("");
  const [newProjectSlug, setNewProjectSlug] = useState("");
  const [newProjectName, setNewProjectName] = useState("");
  const [newRepoUrl, setNewRepoUrl] = useState("");
  const [newRepoPath, setNewRepoPath] = useState("");
  const [newWorktreeRoot, setNewWorktreeRoot] = useState("");
  const [newDefaultBranch, setNewDefaultBranch] = useState("main");
  const [newRemoteName, setNewRemoteName] = useState("origin");
  const [newCredentialFile, setNewCredentialFile] = useState("");
  const [taskSourceType, setTaskSourceType] = useState<TaskSourceType>("github_project");
  const [taskSourcePrompt, setTaskSourcePrompt] = useState("");
  const [taskSourcePath, setTaskSourcePath] = useState("");
  const [taskSourceProject, setTaskSourceProject] = useState("");
  const [taskSourceLinearTeam, setTaskSourceLinearTeam] = useState("");
  const [taskSourceLinearProject, setTaskSourceLinearProject] = useState("");
  const [taskSourceLinearStatuses, setTaskSourceLinearStatuses] = useState("Backlog,Todo,Triage,Ready");
  const [taskSourceLinearLabels, setTaskSourceLinearLabels] = useState("");
  const [taskSourceLinearQuery, setTaskSourceLinearQuery] = useState("");
  const [runItem, setRunItem] = useState("");
  // Empty run overrides are intentional: ACA should inherit its configured base provider/model.
  const [overrideProvider, setOverrideProvider] = useState("");
  const [overrideModel, setOverrideModel] = useState("");
  const [registering, setRegistering] = useState(false);
  const [triggering, setTriggering] = useState(false);
  const [lastGlobalEvent, setLastGlobalEvent] = useState("");
  const [lastRunEvent, setLastRunEvent] = useState("");
  const [taskPreviewRefreshAt, setTaskPreviewRefreshAt] = useState<number | null>(null);
  const [githubBoardRefreshAt, setGithubBoardRefreshAt] = useState<number | null>(null);
  const [repoSyncing, setRepoSyncing] = useState(false);
  const [repoSyncResult, setRepoSyncResult] = useState<any>(null);
  const [runDetailOpen, setRunDetailOpen] = useState(false);
  const [liveLogsOpen, setLiveLogsOpen] = useState(false);
  const [selectedGithubItemIds, setSelectedGithubItemIds] = useState<string[]>([]);
  const [launchingGithubItemIds, setLaunchingGithubItemIds] = useState<Record<string, number>>({});
  const [batchTriggering, setBatchTriggering] = useState(false);
  const [movingTaskStates, setMovingTaskStates] = useState<Record<string, string>>({});
  const caps = useCapabilities();
  const acaAvailable = caps.data?.aca_integration === true;
  const engineAvailable = caps.data?.engine_healthy === true;
  const acaReason = String(caps.data?.aca_reason || "").trim();
  const acaStatusText =
    acaReason === "aca_not_configured"
      ? "ACA_BASE_URL is not configured for this control panel service."
      : acaReason === "aca_endpoint_not_found"
        ? "The configured ACA service did not expose the expected health endpoint."
        : acaReason === "aca_probe_timeout"
          ? "The ACA health probe timed out."
          : acaReason === "aca_probe_error"
            ? "The ACA health probe failed before it could read a response."
            : acaReason.match(/^aca_health_failed_\d+$/)
              ? `The ACA health probe returned ${acaReason.replace("aca_health_failed_", "HTTP ")}.`
              : "ACA has not been detected by the control panel yet.";
  const controlPanelConfigMissing = Array.isArray(caps.data?.control_panel_config_missing)
    ? caps.data.control_panel_config_missing
    : [];
  const hostedManaged = caps.data?.hosted_managed === true;
  const integrationsEnabled = acaAvailable || engineAvailable;
  const health = useQuery({
    queryKey: ["coding-workflows", "health"],
    queryFn: () => api("/api/system/health"),
    refetchInterval: 15000,
  });
  const acaHealth = useQuery({
    queryKey: ["coding-workflows", "aca-health"],
    queryFn: () => api("/api/aca/health"),
    enabled: acaAvailable,
  });
  const acaOverview = useQuery({
    queryKey: ["coding-workflows", "aca-overview"],
    queryFn: () => api("/api/aca/overview"),
    enabled: acaAvailable,
    refetchInterval: acaAvailable ? 30000 : false,
  });
  const projectsQuery = useQuery({
    queryKey: ["coding-workflows", "aca-projects"],
    queryFn: () => api("/api/aca/projects"),
    enabled: acaAvailable,
  });
  const workspaceGuideQuery = useQuery({
    queryKey: ["coding-workflows", "aca-workspace-guide"],
    queryFn: () => api("/api/aca/workspace/guide"),
    enabled: acaAvailable,
  });
  const linearCatalogQuery = useQuery({
    queryKey: ["coding-workflows", "linear-catalog", taskSourceLinearTeam],
    queryFn: async ({ signal }) => {
      const params = new URLSearchParams();
      if (taskSourceLinearTeam.trim()) params.set("team", taskSourceLinearTeam.trim());
      const path = `/api/aca/linear/catalog${params.toString() ? `?${params.toString()}` : ""}`;
      let timeoutId: ReturnType<typeof setTimeout> | null = null;
      try {
        return await Promise.race([
          api(path, { signal }),
          new Promise((_, reject) => {
            timeoutId = setTimeout(() => {
              reject(
                new Error("Linear catalog timed out. Manual team/project entry is still available.")
              );
            }, LINEAR_CATALOG_TIMEOUT_MS);
          }),
        ]);
      } finally {
        if (timeoutId) clearTimeout(timeoutId);
      }
    },
    enabled: acaAvailable && taskSourceType === "linear",
    retry: false,
    staleTime: 60_000,
  });
  const runsQuery = useQuery({
    queryKey: ["coding-workflows", "aca-runs"],
    queryFn: () => api("/api/aca/runs"),
    enabled: acaAvailable,
  });
  const coderRunsQuery = useQuery({
    queryKey: ["coding-workflows", "coder-runs"],
    queryFn: () => api("/api/aca/operator/coder-runs"),
    enabled: acaAvailable,
    refetchInterval: acaAvailable ? 15000 : false,
  });
  const projectTasksQuery = useQuery({
    queryKey: ["coding-workflows", "aca-project-tasks", selectedProjectSlug],
    queryFn: () => api(`/api/aca/projects/${encodeURIComponent(selectedProjectSlug)}/tasks`),
    enabled: acaAvailable && !!selectedProjectSlug,
  });
  const projectBoardQuery = useQuery({
    queryKey: ["coding-workflows", "aca-project-board", selectedProjectSlug],
    queryFn: () =>
      api(`/api/aca/projects/${encodeURIComponent(selectedProjectSlug)}/board?refresh=true`),
    enabled:
      acaAvailable &&
      !!selectedProjectSlug &&
      normalizeProjects(projectsQuery.data).some(
        (project: any) =>
          project.slug === selectedProjectSlug &&
          ["github_project", "linear"].includes(String(project?.taskSource?.type || "").trim())
      ),
    refetchInterval:
      acaAvailable &&
      !!selectedProjectSlug &&
      normalizeProjects(projectsQuery.data).some(
        (project: any) =>
          project.slug === selectedProjectSlug &&
          ["github_project", "linear"].includes(String(project?.taskSource?.type || "").trim())
      )
        ? 5000
        : false,
  });
  const runDetailQuery = useQuery({
    queryKey: ["coding-workflows", "aca-run-detail", selectedRunId],
    queryFn: () => api(`/api/aca/runs/${encodeURIComponent(selectedRunId)}`),
    enabled: acaAvailable && !!selectedRunId,
  });
  const runLogsQuery = useQuery({
    queryKey: ["coding-workflows", "aca-run-logs", selectedRunId],
    queryFn: () => api(`/api/aca/runs/${encodeURIComponent(selectedRunId)}/logs`),
    enabled: acaAvailable && !!selectedRunId,
  });
  const logTailQuery = useQuery({
    queryKey: ["coding-workflows", "aca-run-log-tail", selectedRunId, selectedLogName],
    queryFn: () =>
      api(
        `/api/aca/runs/${encodeURIComponent(selectedRunId)}/logs/${encodeURIComponent(selectedLogName)}?tail=120`
      ),
    enabled: acaAvailable && !!selectedRunId && !!selectedLogName,
  });
  const mcpServersQuery = useQuery({
    queryKey: ["coding-workflows", "mcp-servers"],
    queryFn: () => client.mcp.list().catch(() => ({})),
    refetchInterval: integrationsEnabled ? 10000 : false,
    enabled: integrationsEnabled,
  });
  const mcpToolsQuery = useQuery({
    queryKey: ["coding-workflows", "mcp-tools"],
    queryFn: () => client.mcp.listTools().catch(() => []),
    refetchInterval: integrationsEnabled ? 15000 : false,
    enabled: integrationsEnabled,
  });
  const providersCatalogQuery = useQuery({
    queryKey: ["coding-workflows", "providers", "catalog"],
    queryFn: () => client.providers.catalog().catch(() => ({ all: [] })),
    refetchInterval: integrationsEnabled ? 30000 : false,
    enabled: integrationsEnabled,
  });
  const providersConfigQuery = useQuery({
    queryKey: ["coding-workflows", "providers", "config"],
    queryFn: () => client.providers.config().catch(() => ({})),
    refetchInterval: integrationsEnabled ? 30000 : false,
    enabled: integrationsEnabled,
  });
  const mcpServers = useMemo(() => normalizeServers(mcpServersQuery.data), [mcpServersQuery.data]);
  const mcpTools = useMemo(() => normalizeTools(mcpToolsQuery.data), [mcpToolsQuery.data]);
  const projects = useMemo(() => normalizeProjects(projectsQuery.data), [projectsQuery.data]);
  const runs = useMemo(() => toArray(runsQuery.data, "runs"), [runsQuery.data]);
  const coderRuns = useMemo(
    () => toArray(coderRunsQuery.data, "coder_runs"),
    [coderRunsQuery.data]
  );
  const githubBoard = useMemo(
    () => normalizeGithubBoard(projectBoardQuery.data),
    [projectBoardQuery.data]
  );
  const githubBoardLoading =
    projectBoardQuery.isLoading || (projectBoardQuery.isFetching && !projectBoardQuery.data);
  const providerOptions = useMemo<PlannerProviderOption[]>(() => {
    return buildPlannerProviderOptions({
      providerCatalog: providersCatalogQuery.data,
      providerConfig: providersConfigQuery.data,
      defaultProvider: providerStatus.defaultProvider,
      defaultModel: providerStatus.defaultModel,
    });
  }, [
    providerStatus.defaultModel,
    providerStatus.defaultProvider,
    providersCatalogQuery.data,
    providersConfigQuery.data,
  ]);
  const newRepoRef = useMemo(() => parseGithubRepoRef(newRepoUrl), [newRepoUrl]);
  useEffect(() => {
    if (!projects.length) return;
    if (
      !selectedProjectSlug ||
      !projects.some((project: any) => project.slug === selectedProjectSlug)
    ) {
      setSelectedProjectSlug(projects[0].slug);
    }
  }, [projects, selectedProjectSlug]);
  const filteredRuns = useMemo(() => {
    if (!selectedProjectSlug) return runs;
    return runs.filter(
      (run: any) => String(run?.project_slug || "").trim() === selectedProjectSlug
    );
  }, [runs, selectedProjectSlug]);
  const visibleRuns = useMemo(() => dedupeRuns(filteredRuns), [filteredRuns]);
  const activeRuns = useMemo(() => visibleRuns.filter(runIsActive), [visibleRuns]);
  useEffect(() => {
    if (!visibleRuns.length) {
      setSelectedRunId("");
      return;
    }
    const activeRunIds = activeRuns.map((run: any, index: number) => runId(run, index));
    const selectedStillVisible = visibleRuns.some(
      (run: any, index: number) => runId(run, index) === selectedRunId
    );
    const selectedIsActive = activeRunIds.includes(selectedRunId);
    if (activeRunIds.length && (!selectedRunId || !selectedIsActive)) {
      setSelectedRunId(activeRunIds[0]);
      return;
    }
    if (!selectedRunId || !selectedStillVisible) {
      setSelectedRunId(runId(visibleRuns[0], 0));
    }
  }, [activeRuns, selectedRunId, visibleRuns]);
  const logRows = useMemo(() => toArray(runLogsQuery.data, "logs"), [runLogsQuery.data]);
  useEffect(() => {
    if (!logRows.length) {
      setSelectedLogName("");
      return;
    }
    if (
      !selectedLogName ||
      !logRows.some((log: any) => String(log?.name || "") === selectedLogName)
    ) {
      setSelectedLogName(String(logRows[0]?.name || ""));
    }
  }, [logRows, selectedLogName]);
  const healthy = !!(health.data?.engine?.ready || health.data?.engine?.healthy);
  const githubConnected = mcpServers.some(
    (server) => server.connected && server.name.toLowerCase().includes("github")
  );
  const linearConnected = mcpServers.some(
    (server) => server.connected && server.name.toLowerCase().includes("linear")
  );
  const selectedProject =
    projects.find((project: any) => project.slug === selectedProjectSlug) || null;
  const selectedProjectRepo = selectedProject?.repo || {};
  const selectedProjectTaskSourceType = String(selectedProject?.taskSource?.type || "").trim();
  const selectedTaskSourceLabel =
    selectedProjectTaskSourceType === "linear" ? "Linear" : "GitHub Project";
  const selectedProjectHasLiveBoard = ["github_project", "linear"].includes(
    selectedProjectTaskSourceType
  );
  const planningWorkspaceRootSeed = String(
    (health.data as any)?.workspaceRoot ||
      (health.data as any)?.workspace_root ||
      (selectedProjectTaskSourceType === "kanban_board" ||
      selectedProjectTaskSourceType === "local_backlog"
        ? selectedProject?.taskSource?.path || ""
        : "") ||
      ""
  ).trim();
  const connectedMcpServers = mcpServers
    .filter((server) => server.connected)
    .map((server) => server.name);
  const selectedGithubItems = useMemo(
    () =>
      githubBoard.items.filter(
        (item: any) =>
          selectedGithubItemIds.includes(String(item.id || "")) && githubBoardItemCanRun(item)
      ),
    [githubBoard.items, selectedGithubItemIds]
  );
  const actionableGithubItems = useMemo(
    () => githubBoard.items.filter((item: any) => githubBoardItemCanRun(item)),
    [githubBoard.items]
  );
  const launchingGithubItemIdSet = useMemo(
    () => new Set(Object.keys(launchingGithubItemIds)),
    [launchingGithubItemIds]
  );
  const controlPanelDefaultProvider = String(providerStatus.defaultProvider || "").trim();
  const controlPanelDefaultModel = String(providerStatus.defaultModel || "").trim();
  const inheritedAcaModelLabel =
    controlPanelDefaultProvider && controlPanelDefaultModel
      ? `Control panel default (${controlPanelDefaultProvider} / ${controlPanelDefaultModel})`
      : "Control panel default";
  const buildAcaProviderOverrides = () => {
    const provider = String(overrideProvider || controlPanelDefaultProvider).trim();
    const model = String(overrideModel || controlPanelDefaultModel).trim();
    const overrides: Record<string, string> = {};
    if (provider) overrides.ACA_PROVIDER = provider;
    if (model) overrides.ACA_MODEL = model;
    return overrides;
  };
  const renderAcaModelSelector = (disabled = false) => (
    <div className="grid gap-2">
      {/* Keep run launch overrides on the shared selector used by planner/settings screens. */}
      <ProviderModelSelector
        providerLabel="ACA provider"
        modelLabel="ACA model"
        draft={{ provider: overrideProvider, model: overrideModel }}
        providers={providerOptions}
        onChange={({ provider, model }) => {
          setOverrideProvider(provider);
          setOverrideModel(model);
        }}
        inheritLabel={inheritedAcaModelLabel}
        disabled={disabled}
      />
      <div className="tcp-subtle text-xs">
        Leave blank to inherit the control panel provider and model for this run.
      </div>
    </div>
  );
  const activeGithubItemIdentities = useMemo(
    () =>
      new Set(
        activeRuns
          .map((run: any, index: number) => runTaskIdentity(run, index))
          .map((value) => String(value || "").trim())
          .filter(Boolean)
      ),
    [activeRuns]
  );
  const activeGithubRunByIdentity = useMemo(() => {
    const rows = new Map<string, any>();
    activeRuns.forEach((run: any, index: number) => {
      const identity = runTaskIdentity(run, index);
      if (identity) rows.set(identity, { run, id: runId(run, index) });
    });
    return rows;
  }, [activeRuns]);
  const launchableGithubItems = useMemo(
    () =>
      actionableGithubItems.filter(
        (item: any) =>
          !activeGithubItemIdentities.has(githubBoardItemIdentity(item)) &&
          !launchingGithubItemIdSet.has(String(item.id || ""))
      ),
    [actionableGithubItems, activeGithubItemIdentities, launchingGithubItemIdSet]
  );
  const githubScheduler = githubBoard.scheduler || {};
  const githubSchedulerActivePhase =
    githubScheduler?.active_phase ?? githubScheduler?.activePhase ?? null;
  const githubSchedulerNextIssues = Array.isArray(githubScheduler?.next_issue_numbers)
    ? githubScheduler.next_issue_numbers
    : Array.isArray(githubScheduler?.nextIssueNumbers)
      ? githubScheduler.nextIssueNumbers
      : [];
  const githubSchedulerPolicy = String(githubScheduler?.policy || "").trim();
  const githubBoardColumns = useMemo(() => {
    const normalizeStatus = (value: unknown) =>
      String(value || "")
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "_")
        .replace(/^_+|_+$/g, "");
    const columnForItem = (item: any) => {
      const statusKey =
        normalizeStatus(item?.statusKey) && normalizeStatus(item?.statusKey) !== "unknown"
          ? normalizeStatus(item?.statusKey)
          : normalizeStatus(item?.statusName);
      if (["in_progress", "started", "active", "working"].includes(statusKey)) {
        return "in_progress";
      }
      if (["blocked", "stalled", "on_hold", "failed"].includes(statusKey)) return "blocked";
      if (["review", "in_review", "ready_for_review"].includes(statusKey)) return "review";
      if (["done", "complete", "completed", "closed", "canceled", "cancelled"].includes(statusKey)) return "done";
      if (["todo", "todos", "ready", "backlog", "open", "triage", "unstarted"].includes(statusKey)) return "todos";
      return "unknown";
    };
    const rankItem = (item: any) => {
      const title = String(item?.title || "").toLowerCase();
      if (title.includes("[aca slice parent]") || title.includes("slice parent")) return 0;
      if (title.includes("[tenant isolation]")) return 1;
      return 2;
    };
    const columns = [
      { key: "todos", label: "TODOS", hint: "Not started", items: [] as any[] },
      { key: "in_progress", label: "In Progress", hint: "Actively running", items: [] as any[] },
      { key: "blocked", label: "Blocked", hint: "Needs attention", items: [] as any[] },
      { key: "review", label: "Review", hint: "Ready to inspect", items: [] as any[] },
      { key: "done", label: "Done", hint: "Completed", items: [] as any[] },
      { key: "unknown", label: "Unknown", hint: "Missing status", items: [] as any[] },
    ];
    const byKey = new Map(columns.map((column) => [column.key, column]));
    githubBoard.items.forEach((item: any) => {
      const column = byKey.get(columnForItem(item)) || byKey.get("unknown");
      column?.items.push(item);
    });
    columns.forEach((column) => {
      column.items.sort((a, b) => {
        const rankDelta = rankItem(a) - rankItem(b);
        if (rankDelta) return rankDelta;
        const actionableDelta = Number(b?.actionable === true) - Number(a?.actionable === true);
        if (actionableDelta) return actionableDelta;
        return String(a?.title || "").localeCompare(String(b?.title || ""));
      });
    });
    return columns.filter((column) => column.key !== "unknown" || column.items.length);
  }, [githubBoard.items]);
  const selectedRun =
    visibleRuns.find((run: any, index: number) => runId(run, index) === selectedRunId) || null;
  const runSummary = String(runDetailQuery.data?.summary || "").trim();
  const blackboard = runDetailQuery.data?.blackboard || null;
  const runEvents = useMemo(
    () => toArray(runDetailQuery.data, "events").slice(-12),
    [runDetailQuery.data]
  );
  const runWorkers = useMemo(() => toArray(blackboard, "workers"), [blackboard]);
  const runSubtasks = useMemo(() => toArray(blackboard, "subtasks"), [blackboard]);
  const runChangedFiles = useMemo(() => {
    const paths = new Set<string>();
    toArray(runDetailQuery.data?.diff, "changed_files").forEach((path: any) => {
      const text = String(path || "").trim();
      if (text) paths.add(text);
    });
    runWorkers.forEach((worker: any) => {
      toArray(worker, "changed_files").forEach((path: any) => {
        const text = String(path || "").trim();
        if (text) paths.add(text);
      });
    });
    return Array.from(paths).sort();
  }, [runDetailQuery.data?.diff, runWorkers]);
  const runDiffStat = String(
    runDetailQuery.data?.diff?.after || runDetailQuery.data?.snapshot?.diff?.after || ""
  ).trim();
  useEffect(() => {
    setLastRunEvent("");
  }, [selectedRunId]);
  useEffect(() => {
    if (!acaAvailable) return;
    const unsubscribe = subscribeSse("/api/aca/events", (event: MessageEvent) => {
      const envelope = parseSseEnvelope(String(event?.data || ""));
      if (!envelope || envelope.event_type === "ping") return;
      const eventType = String(envelope.event_type || "event").trim();
      setLastGlobalEvent(eventType);
      void queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-runs"] });
      void queryClient.invalidateQueries({ queryKey: ["coding-workflows", "coder-runs"] });
      void queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-overview"] });
      void queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-projects"] });
      if (selectedProjectSlug) {
        void queryClient.invalidateQueries({
          queryKey: ["coding-workflows", "aca-project-board", selectedProjectSlug],
        });
        void queryClient.invalidateQueries({
          queryKey: ["coding-workflows", "aca-project-tasks", selectedProjectSlug],
        });
      }
      const runIdFromPayload = String(
        envelope?.payload?.run_id || envelope?.payload?.event?.run_id || ""
      ).trim();
      if (selectedRunId && runIdFromPayload && runIdFromPayload === selectedRunId) {
        void queryClient.invalidateQueries({
          queryKey: ["coding-workflows", "aca-run-detail", selectedRunId],
        });
      }
    });
    return () => unsubscribe();
  }, [acaAvailable, queryClient, selectedProjectSlug, selectedRunId]);
  useEffect(() => {
    if (!acaAvailable || !selectedRunId || !selectedRun?.is_running) return;
    const url = `/api/aca/runs/${encodeURIComponent(selectedRunId)}/events`;
    const unsubscribe = subscribeSse(url, (event: MessageEvent) => {
      const envelope = parseSseEnvelope(String(event?.data || ""));
      if (!envelope || envelope.event_type === "ping") return;
      const eventType = String(envelope.event_type || "event").trim();
      setLastRunEvent(eventType);
      void queryClient.invalidateQueries({
        queryKey: ["coding-workflows", "aca-runs"],
      });
      void queryClient.invalidateQueries({ queryKey: ["coding-workflows", "coder-runs"] });
      void queryClient.invalidateQueries({
        queryKey: ["coding-workflows", "aca-run-detail", selectedRunId],
      });
      void queryClient.invalidateQueries({
        queryKey: ["coding-workflows", "aca-run-logs", selectedRunId],
      });
      if (selectedLogName) {
        void queryClient.invalidateQueries({
          queryKey: ["coding-workflows", "aca-run-log-tail", selectedRunId, selectedLogName],
        });
      }
    });
    return () => unsubscribe();
  }, [acaAvailable, queryClient, selectedLogName, selectedRun?.is_running, selectedRunId]);
  useEffect(() => {
    if (projectTasksQuery.data) {
      setTaskPreviewRefreshAt(Date.now());
    }
  }, [projectTasksQuery.data]);
  useEffect(() => {
    if (projectBoardQuery.data) {
      setGithubBoardRefreshAt(Number(projectBoardQuery.data?.last_synced_at_ms || Date.now()));
    }
  }, [projectBoardQuery.data]);
  useEffect(() => {
    setSelectedGithubItemIds([]);
  }, [selectedProjectSlug]);
  useEffect(() => {
    setLaunchingGithubItemIds({});
  }, [selectedProjectSlug]);
  useEffect(() => {
    const pendingEntries = Object.entries(launchingGithubItemIds);
    if (!pendingEntries.length) return;
    const timers = pendingEntries.map(([itemId, launchedAt]) => {
      const elapsedMs = Date.now() - Number(launchedAt || 0);
      const delayMs = Math.max(0, GITHUB_ITEM_LAUNCH_LOCK_MS - elapsedMs);
      return window.setTimeout(() => {
        setLaunchingGithubItemIds((current) => {
          if (!current[itemId]) return current;
          const next = { ...current };
          delete next[itemId];
          return next;
        });
      }, delayMs);
    });
    return () => {
      timers.forEach((timer) => window.clearTimeout(timer));
    };
  }, [launchingGithubItemIds]);
  useEffect(() => {
    if (!Object.keys(launchingGithubItemIds).length || !githubBoard.items.length) return;
    setLaunchingGithubItemIds((current) => {
      let changed = false;
      const next = { ...current };
      githubBoard.items.forEach((item: any) => {
        const itemId = String(item?.id || "").trim();
        if (!itemId || next[itemId] === undefined) return;
        if (activeGithubItemIdentities.has(githubBoardItemIdentity(item))) {
          delete next[itemId];
          changed = true;
        }
      });
      return changed ? next : current;
    });
  }, [activeGithubItemIdentities, githubBoard.items, launchingGithubItemIds]);
  useEffect(() => {
    const validIds = new Set(githubBoard.items.map((item: any) => String(item.id || "")));
    setSelectedGithubItemIds((current) => current.filter((id) => validIds.has(id)));
  }, [githubBoard.items]);
  const tabs: Array<{ id: CodingTab; label: string; icon: string }> = [
    { id: "overview", label: "Overview", icon: "layout-dashboard" },
    { id: "manual", label: "Launch", icon: "rocket" },
    { id: "board", label: "Intake", icon: "list-checks" },
    { id: "cockpit", label: "Cockpit", icon: "messages-square" },
    { id: "planning", label: "Planning", icon: "clipboard-list" },
    { id: "integrations", label: "Integrations", icon: "plug-zap" },
  ];
  async function reconcileCoderRun(runId: string) {
    const id = String(runId || "").trim();
    if (!id) return;
    try {
      await api(`/api/aca/operator/coder-runs/${encodeURIComponent(id)}/reconcile`, {
        method: "POST",
      });
      await queryClient.invalidateQueries({ queryKey: ["coding-workflows", "coder-runs"] });
      await queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-runs"] });
      toast("ok", `Reconciled coder run ${id}.`);
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  }
  async function cancelCoderRun(runId: string) {
    const id = String(runId || "").trim();
    if (!id) return;
    if (!window.confirm(`Cancel coder run ${id}?`)) return;
    try {
      await api(`/api/aca/operator/coder-runs/${encodeURIComponent(id)}/cancel`, {
        method: "POST",
        body: JSON.stringify({ reason: "cancelled from Tandem Control Panel Coder view" }),
      });
      await queryClient.invalidateQueries({ queryKey: ["coding-workflows", "coder-runs"] });
      await queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-runs"] });
      toast("ok", `Cancelled coder run ${id}.`);
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  }
  async function registerProject() {
    const repoRef = parseGithubRepoRef(newRepoUrl);
    const linearSlugSeed = taskSourceType === "linear"
      ? `linear-${taskSourceLinearTeam || "team"}-${taskSourceLinearProject || "issues"}`
      : "";
    const safeLinearSlug = linearSlugSeed
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9._/-]+/g, "-")
      .replace(/^-+|-+$/g, "");
    const slug =
      newProjectSlug.trim() ||
      (taskSourceType === "github_project" ? repoRef?.slug || "" : "") ||
      (taskSourceType === "linear" ? safeLinearSlug : "");
    const name = newProjectName.trim();
    const repoUrl = newRepoUrl.trim();
    const repoPath = newRepoPath.trim();
    const worktreeRoot = newWorktreeRoot.trim();
    const defaultBranch = newDefaultBranch.trim();
    const remoteName = newRemoteName.trim();
    const credentialFile = newCredentialFile.trim();
    const selectedLinearTeam = findLinearCatalogEntry(
      linearCatalogQuery.data?.teams,
      taskSourceLinearTeam
    );
    const selectedLinearProject = findLinearCatalogEntry(
      linearCatalogQuery.data?.projects,
      taskSourceLinearProject
    );
    if (taskSourceType === "github_project" && !repoRef) {
      toast("warn", "Paste a GitHub repository URL like https://github.com/owner/repo.");
      return;
    }
    if (!slug) {
      toast("warn", "Project slug is required.");
      return;
    }
    if (
      (repoPath && !isSafeManagedPath(repoPath)) ||
      (worktreeRoot && !isSafeManagedPath(worktreeRoot))
    ) {
      toast("warn", "Repo paths must stay within the managed workspace root.");
      return;
    }
    const taskSource = buildTaskSourcePayload(taskSourceType, {
      prompt: taskSourcePrompt,
      path: taskSourcePath,
      repoRef,
      projectNumber: taskSourceProject,
      linearTeam: taskSourceLinearTeam,
      linearProject: taskSourceLinearProject,
      linearTeamName: selectedLinearTeam?.name || "",
      linearProjectName: selectedLinearProject?.name || "",
      linearStatuses: taskSourceLinearStatuses,
      linearLabels: taskSourceLinearLabels,
      linearQuery: taskSourceLinearQuery,
    });
    if (taskSource.type === "manual" && !taskSource.prompt) {
      toast("warn", "Manual task source requires a prompt.");
      return;
    }
    if (["kanban_board", "local_backlog"].includes(taskSource.type) && !taskSource.path) {
      toast("warn", "This task source requires a path.");
      return;
    }
    if (
      taskSource.type === "github_project" &&
      (!taskSource.owner || !taskSource.repo || !taskSource.project)
    ) {
      toast("warn", "GitHub Project task sources require a repo URL and project number.");
      return;
    }
    if (taskSource.type === "linear" && !taskSource.team) {
      toast("warn", "Linear task sources require a team key or name.");
      return;
    }
    setRegistering(true);
    try {
      const params = new URLSearchParams({ slug });
      if (repoUrl) params.set("repo_url", repoUrl);
      if (name) params.set("name", name);
      if (repoPath) params.set("repo_path", repoPath);
      if (worktreeRoot) params.set("worktree_root", worktreeRoot);
      if (defaultBranch) params.set("default_branch", defaultBranch);
      if (remoteName) params.set("remote_name", remoteName);
      if (credentialFile) params.set("credential_file", credentialFile);
      await api(`/api/aca/projects?${params.toString()}`, {
        method: "POST",
        body: JSON.stringify(taskSource),
      });
      await queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-projects"] });
      await queryClient.invalidateQueries({
        queryKey: ["coding-workflows", "aca-workspace-guide"],
      });
      setSelectedProjectSlug(slug);
      setNewProjectSlug("");
      setNewProjectName("");
      setNewRepoUrl("");
      setNewRepoPath("");
      setNewWorktreeRoot("");
      setNewDefaultBranch("main");
      setNewRemoteName("origin");
      setNewCredentialFile("");
      setTaskSourceLinearTeam("");
      setTaskSourceLinearProject("");
      setTaskSourceLinearStatuses("Backlog,Todo,Triage,Ready");
      setTaskSourceLinearLabels("");
      setTaskSourceLinearQuery("");
      toast("ok", `Registered ACA project ${slug}.`);
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    } finally {
      setRegistering(false);
    }
  }
  async function triggerRun() {
    if (!selectedProjectSlug) {
      toast("warn", "Select a project before triggering a run.");
      return;
    }
    const overrides = buildAcaProviderOverrides();
    setTriggering(true);
    try {
      const params = new URLSearchParams({ project_slug: selectedProjectSlug });
      if (runItem.trim()) params.set("item", runItem.trim());
      const result = await api(`/api/aca/runs/trigger?${params.toString()}`, {
        method: "POST",
        body: JSON.stringify(overrides),
      });
      await queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-runs"] });
      const nextRunId = String(result?.run_id || "").trim();
      if (nextRunId) {
        setSelectedRunId(nextRunId);
        setTab("cockpit");
        setRunDetailOpen(true);
        setLiveLogsOpen(true);
      }
      toast("ok", `ACA run started${nextRunId ? `: ${nextRunId}` : "."}`);
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    } finally {
      setTriggering(false);
    }
  }
  function toggleGithubItemSelection(itemId: string) {
    setSelectedGithubItemIds((current) =>
      current.includes(itemId) ? current.filter((value) => value !== itemId) : [...current, itemId]
    );
  }
  function selectAllActionableGithubItems() {
    setSelectedGithubItemIds(launchableGithubItems.map((item: any) => String(item.id || "")));
  }
  function clearGithubSelection() {
    setSelectedGithubItemIds([]);
  }
  async function moveLinearTaskState(item: any, state: string) {
    if (!selectedProjectSlug) {
      toast("warn", "Select a project before moving a task.");
      return;
    }
    if (selectedProjectTaskSourceType !== "linear") return;
    const itemId = String(item?.id || "").trim();
    const itemRef = String(
      item?.identifier || item?.issueNumber || item?.issue_number || item?.selector || itemId || ""
    ).trim();
    const targetState = String(state || "").trim();
    if (!itemRef || !targetState) return;
    const movingKey = `${itemId || itemRef}:${targetState}`;
    setMovingTaskStates((current) => ({ ...current, [movingKey]: targetState }));
    queryClient.setQueryData(["coding-workflows", "aca-project-board", selectedProjectSlug], (current: any) => optimisticallyMoveBoardItems(current, [item], targetState));
    try {
      const path = `/api/aca/projects/${encodeURIComponent(selectedProjectSlug)}/tasks/${encodeURIComponent(itemRef)}/state`;
      await api(path, { method: "POST", body: JSON.stringify({ state: targetState }) });
      void queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-project-board", selectedProjectSlug] });
      void queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-project-tasks", selectedProjectSlug] });
      toast("ok", `Moved ${itemRef} to ${formatStatus(targetState)}.`);
    } catch (error) {
      void queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-project-board", selectedProjectSlug] });
      toast("err", error instanceof Error ? error.message : String(error));
    } finally {
      setMovingTaskStates((current) => {
        const next = { ...current };
        delete next[movingKey];
        return next;
      });
    }
  }
  async function triggerGithubItems(items: any[]) {
    if (!selectedProjectSlug) {
      toast("warn", "Select a project before starting ACA runs.");
      return;
    }
    const launchableItems = items.filter(
      (item: any) =>
        githubBoardItemCanRun(item) &&
        !activeGithubItemIdentities.has(githubBoardItemIdentity(item)) &&
        !launchingGithubItemIdSet.has(String(item.id || ""))
    );
    const selectors = launchableItems
      .map((item: any) => String(item?.selector || "").trim())
      .filter(Boolean);
    if (!selectors.length) {
      toast(
        "warn",
        "Those intake items are already running or are not launchable from ACA intake."
      );
      return;
    }
    const overrides = buildAcaProviderOverrides();
    setLaunchingGithubItemIds((current) => {
      const next = { ...current };
      const launchedAt = Date.now();
      launchableItems.forEach((item: any) => {
        const itemId = String(item?.id || "").trim();
        if (itemId) {
          next[itemId] = launchedAt;
        }
      });
      return next;
    });
    queryClient.setQueryData(["coding-workflows", "aca-project-board", selectedProjectSlug], (current: any) => optimisticallyMoveBoardItems(current, launchableItems, "in_progress"));
    setBatchTriggering(true);
    try {
      const result = await api("/api/aca/runs/trigger-batch", {
        method: "POST",
        body: JSON.stringify({
          project_slug: selectedProjectSlug,
          items: selectors,
          overrides,
        }),
      });
      void queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-runs"] });
      void queryClient.invalidateQueries({ queryKey: ["coding-workflows", "coder-runs"] });
      void queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-overview"] });
      void queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-project-board", selectedProjectSlug] });
      void queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-project-tasks", selectedProjectSlug] });
      const runs = toArray(result, "runs");
      const nextRunId = String(runs?.[0]?.run_id || "").trim();
      if (nextRunId) {
        setSelectedRunId(nextRunId);
        setTab("cockpit");
        setRunDetailOpen(true);
        setLiveLogsOpen(true);
      }
      toast("ok", `Started ${selectors.length} ACA run${selectors.length === 1 ? "" : "s"}.`);
    } catch (error) {
      setLaunchingGithubItemIds((current) => {
        const next = { ...current };
        launchableItems.forEach((item: any) => {
          const itemId = String(item?.id || "").trim();
          if (itemId) {
            delete next[itemId];
          }
        });
        return next;
      });
      void queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-project-board", selectedProjectSlug] });
      toast("err", error instanceof Error ? error.message : String(error));
    } finally {
      setBatchTriggering(false);
    }
  }
  function inspectAcaRun(runRef: { run: any; id: string } | null | undefined) {
    const id = String(runRef?.id || "").trim();
    if (!id) return;
    setSelectedRunId(id);
    setTab("cockpit");
    setRunDetailOpen(true);
    setLiveLogsOpen(true);
    window.setTimeout(() => {
      document.getElementById("aca-run-inspector")?.scrollIntoView({
        behavior: "smooth",
        block: "start",
      });
    }, 0);
  }
  async function refreshTaskPreview() {
    if (!selectedProjectSlug) {
      toast("warn", "Select a project before refreshing intake.");
      return;
    }
    try {
      await projectTasksQuery.refetch();
      setTaskPreviewRefreshAt(Date.now());
      toast("ok", `Refreshed task intake from ${selectedTaskSourceLabel} MCP.`);
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  }
  async function refreshGithubBoard() {
    if (!selectedProjectSlug) {
      toast("warn", "Select a project with a live task-source board before refreshing.");
      return;
    }
    try {
      const data = await api(
        `/api/aca/projects/${encodeURIComponent(selectedProjectSlug)}/board?refresh=true`
      );
      queryClient.setQueryData(
        ["coding-workflows", "aca-project-board", selectedProjectSlug],
        data
      );
      setGithubBoardRefreshAt(Number(data?.last_synced_at_ms || Date.now()));
      toast("ok", `Refreshed ${selectedTaskSourceLabel} items through Tandem MCP.`);
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  }
  async function syncSelectedRepo() {
    if (!selectedProjectSlug) {
      toast("warn", "Select a project before syncing its repository.");
      return;
    }
    setRepoSyncing(true);
    setRepoSyncResult(null);
    try {
      const data = await api(
        `/api/aca/projects/${encodeURIComponent(selectedProjectSlug)}/repo/sync`,
        { method: "POST" }
      );
      setRepoSyncResult(data);
      await queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-projects"] });
      await queryClient.invalidateQueries({
        queryKey: ["coding-workflows", "aca-workspace-guide"],
      });
      await queryClient.invalidateQueries({
        queryKey: ["coding-workflows", "aca-project-tasks", selectedProjectSlug],
      });
      toast("ok", `Repository ready at ${String(data?.repo?.path || "managed checkout")}.`);
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    } finally {
      setRepoSyncing(false);
    }
  }
  async function refreshAcaConnection() {
    try {
      const data = await api("/api/capabilities?refresh=1");
      queryClient.setQueryData(["system", "capabilities"], data);
      toast(
        data?.aca_integration ? "ok" : "warn",
        data?.aca_integration ? "ACA connection detected." : "ACA is still disconnected."
      );
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  }
  if (caps.isLoading && !caps.data) return <CodingWorkflowsConnectingState />;
  if (!acaAvailable) {
    return (
      <CodingWorkflowsDisconnectedState
        acaReason={acaReason}
        acaStatusText={acaStatusText}
        configPath={String(caps.data?.control_panel_config_path || "")}
        missingFields={controlPanelConfigMissing}
        navigateSettings={() => navigate("settings")}
        refreshAcaConnection={refreshAcaConnection}
      />
    );
  }
  return (
    <AnimatedPage className="grid gap-4">
      <PanelCard className="overflow-hidden">
        <div className="grid gap-5 xl:grid-cols-[minmax(0,1.3fr)_minmax(320px,0.9fr)] xl:items-start">
          <div className="min-w-0">
            <div className="tcp-page-eyebrow">Coder</div>
            <h1 className="tcp-page-title">Coder project intake and run dashboard</h1>
            <p className="tcp-subtle mt-2 max-w-3xl">
              This view talks to the ACA control plane for project registration, task preview,
              durable coder runs, live logs, and final handoff artifacts.
            </p>
            <div className="mt-3 flex flex-wrap gap-2">
              <Badge tone={acaHealth.data?.status === "healthy" ? "ok" : "warn"}>
                {acaHealth.data?.status === "healthy" ? "ACA healthy" : "ACA checking"}
              </Badge>
              {selectedProjectTaskSourceType === "linear" ? (
                <Badge tone={linearConnected ? "ok" : "warn"}>
                  {linearConnected ? "Linear MCP connected" : "Linear MCP pending"}
                </Badge>
              ) : (
                <Badge tone={githubConnected ? "ok" : "warn"}>
                  {githubConnected ? "GitHub MCP connected" : "GitHub MCP pending"}
                </Badge>
              )}
              <StatusPulse
                tone={activeRuns.length ? "live" : "info"}
                text={`${activeRuns.length} active runs`}
              />
              {lastGlobalEvent ? (
                <Badge tone="ghost">Live {formatStatus(lastGlobalEvent)}</Badge>
              ) : null}
            </div>
          </div>
          <div className="grid gap-3 rounded-2xl border border-white/10 bg-black/20 p-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="min-w-0">
                <div className="text-sm font-semibold text-slate-100">Repository</div>
                <div className="tcp-subtle mt-1 truncate text-xs">
                  {selectedProjectSlug
                    ? String(
                        selectedProjectRepo?.path ||
                          selectedProjectRepo?.clone_url ||
                          selectedProject?.repo_url ||
                          selectedProjectSlug
                      )
                    : "Select a project to sync its checkout."}
                </div>
              </div>
              <button
                type="button"
                className="tcp-btn h-8 px-3 text-xs"
                onClick={syncSelectedRepo}
                disabled={!selectedProjectSlug || repoSyncing}
              >
                <Icon name={repoSyncing ? "loader-circle" : "refresh-cw"} />
                {repoSyncing ? "Syncing" : "Sync repo"}
              </button>
            </div>
            <div className="grid gap-2 text-xs">
              <div className="flex flex-wrap gap-2">
                <Badge tone={selectedProjectRepo?.clone_url ? "ok" : "ghost"}>
                  {selectedProjectRepo?.clone_url ? "remote git" : "local git"}
                </Badge>
                <Badge tone="ghost">{String(selectedProjectRepo?.default_branch || "main")}</Badge>
                {repoSyncResult?.repo?.dirty ? <Badge tone="warn">dirty</Badge> : null}
              </div>
              {repoSyncResult?.repo?.commit ? (
                <div className="grid gap-2">
                  <div className="tcp-subtle truncate">
                    Ready at {String(repoSyncResult.repo.path)} ·{" "}
                    {String(repoSyncResult.repo.commit).slice(0, 7)}
                  </div>
                  <div className="rounded-xl border border-sky-500/20 bg-sky-500/10 p-2 text-sky-100">
                    Incident Monitor should use this checkout as its local directory when reporting
                    issues for this repo.
                  </div>
                </div>
              ) : null}
            </div>
          </div>
        </div>
      </PanelCard>
      <div className="tcp-settings-tabs">
        {tabs.map((item) => (
          <button
            key={item.id}
            type="button"
            className={`tcp-settings-tab tcp-settings-tab-underline ${tab === item.id ? "active" : ""}`}
            onClick={() => setTab(item.id)}
          >
            <Icon name={item.icon} />
            {item.label}
          </button>
        ))}
      </div>
      {tab === "overview" ? (
        <CodingWorkflowsOverviewTab
          projects={projects}
          selectedProjectSlug={selectedProjectSlug}
          setSelectedProjectSlug={setSelectedProjectSlug}
          selectedProject={selectedProject}
          acaOverview={acaOverview}
          projectTasksQuery={projectTasksQuery}
          refreshTaskPreview={refreshTaskPreview}
          taskPreviewRefreshAt={taskPreviewRefreshAt}
          coderRuns={coderRuns}
          coderRunsQuery={coderRunsQuery}
          reconcileCoderRun={reconcileCoderRun}
          cancelCoderRun={cancelCoderRun}
          visibleRunsCount={filteredRuns.length}
          activeRunsCount={activeRuns.length}
          connectedMcpServersCount={mcpServers.length}
          registeredToolsCount={mcpTools.length}
        />
      ) : null}
      {tab === "cockpit" ? (
        <CodingWorkflowsAgentCockpit
          selectedRunId={selectedRunId}
          selectedRun={selectedRun}
          selectedProject={selectedProject}
          runDetailQuery={runDetailQuery}
          coderRuns={coderRuns}
          reconcileCoderRun={reconcileCoderRun}
          cancelCoderRun={cancelCoderRun}
          lastRunEvent={lastRunEvent || lastGlobalEvent}
        />
      ) : null}
      {tab === "board" ? (
        <div className="grid gap-4">
          <div className="grid gap-4">
            <PanelCard title="Task-source intake" subtitle="Issues ACA can launch">
              <div className="mb-4">
                <select
                  className="tcp-input"
                  value={selectedProjectSlug}
                  onChange={(event) =>
                    setSelectedProjectSlug((event.target as HTMLSelectElement).value)
                  }
                >
                  {!projects.length ? <option value="">No ACA projects found</option> : null}
                  {projects.map((project: any) => (
                    <option key={project.slug} value={project.slug}>
                      {project.name ? `${project.name} · ${project.slug}` : project.slug}
                    </option>
                  ))}
                </select>
              </div>
              {selectedProjectHasLiveBoard ? (
                githubBoardLoading ? (
                  <LoadingState
                    title={`Loading ${selectedTaskSourceLabel} items`}
                    text={`Tandem is syncing the intake board through the ${selectedTaskSourceLabel} connection.`}
                    className="min-h-[10rem]"
                  />
                ) : projectBoardQuery.isError ? (
                  <div className="rounded-2xl border border-red-500/20 bg-red-500/10 p-4 text-sm text-red-200">
                    {projectBoardQuery.error instanceof Error
                      ? projectBoardQuery.error.message
                      : `Could not load the ${selectedTaskSourceLabel} items.`}
                  </div>
                ) : (
                  <div className="grid gap-3">
                    <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-white/10 bg-black/20 p-3">
                      <div className="tcp-subtle text-xs">
                        Items are listed by ACA launchability with direct run controls.
                        {githubBoardRefreshAt
                          ? ` Last synced ${new Date(githubBoardRefreshAt).toLocaleTimeString()}.`
                          : ""}
                      </div>
                      <div className="flex flex-wrap items-center gap-2">
                        <Badge tone={githubBoard.isStale ? "warn" : "ok"}>
                          {githubBoard.isStale
                            ? "Cached snapshot"
                            : formatStatus(githubBoard.source || "live")}
                        </Badge>
                        {projectBoardQuery.isFetching ? (
                          <StatusPulse tone="live" text="syncing" />
                        ) : null}
                        <button
                          type="button"
                          className="tcp-btn tcp-btn-secondary"
                          onClick={refreshGithubBoard}
                          disabled={projectBoardQuery.isFetching}
                        >
                          {projectBoardQuery.isFetching ? "Refreshing..." : `Refresh from ${selectedTaskSourceLabel}`}
                        </button>
                      </div>
                    </div>
                    <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-cyan-500/20 bg-cyan-500/10 p-3">
                      <div className="tcp-subtle text-xs">
                        ACA runs only scheduler-approved issues. GitHub Projects can apply phase
                        ordering, while Linear queues the next issue by status and priority.
                      </div>
                      <div className="flex flex-wrap gap-2 text-xs">
                        <Badge tone="info">
                          Current phase{" "}
                          {githubSchedulerActivePhase === null ||
                          githubSchedulerActivePhase === undefined
                            ? "unknown"
                            : Number(githubSchedulerActivePhase) === 99
                              ? "Gate"
                              : `Phase ${String(githubSchedulerActivePhase)}`}
                        </Badge>
                        <Badge tone={launchableGithubItems.length ? "ok" : "ghost"}>
                          Runnable now {launchableGithubItems.length}
                        </Badge>
                        <Badge tone={githubSchedulerNextIssues.length ? "ok" : "ghost"}>
                          Next{" "}
                          {githubSchedulerNextIssues.length
                            ? githubSchedulerNextIssues
                                .slice(0, 4)
                                .map((value: any) =>
                                  selectedProjectTaskSourceType === "linear"
                                    ? String(value)
                                    : `#${String(value)}`
                                )
                                .join(", ")
                            : "none"}
                        </Badge>
                        {githubSchedulerPolicy ? (
                          <Badge tone="ghost">{formatStatus(githubSchedulerPolicy)}</Badge>
                        ) : null}
                      </div>
                      <div className="min-w-[320px] flex-1">
                        {renderAcaModelSelector(batchTriggering)}
                      </div>
                      <div className="flex flex-wrap gap-2">
                        <button
                          type="button"
                          className="tcp-btn-primary"
                          onClick={() => triggerGithubItems(launchableGithubItems)}
                          disabled={!launchableGithubItems.length || batchTriggering}
                        >
                          {batchTriggering
                            ? "Starting..."
                            : `Run scheduler next${launchableGithubItems.length ? ` (${launchableGithubItems.length})` : ""}`}
                        </button>
                        <button
                          type="button"
                          className="tcp-btn tcp-btn-secondary"
                          onClick={selectAllActionableGithubItems}
                          disabled={!launchableGithubItems.length || batchTriggering}
                        >
                          Select scheduler next
                          {launchableGithubItems.length ? ` (${launchableGithubItems.length})` : ""}
                        </button>
                        <button
                          type="button"
                          className="tcp-btn tcp-btn-secondary"
                          onClick={clearGithubSelection}
                          disabled={!selectedGithubItemIds.length || batchTriggering}
                        >
                          Clear selection
                        </button>
                        <button
                          type="button"
                          className="tcp-btn-primary"
                          onClick={() => triggerGithubItems(selectedGithubItems)}
                          disabled={!selectedGithubItems.length || batchTriggering}
                        >
                          {batchTriggering
                            ? "Starting..."
                            : `Run selected${selectedGithubItems.length ? ` (${selectedGithubItems.length})` : ""}`}
                        </button>
                      </div>
                    </div>
                    {githubBoard.warning ? (
                      <div className="rounded-2xl border border-yellow-500/20 bg-yellow-500/10 p-4 text-sm text-yellow-100">
                        {githubBoard.warning}
                      </div>
                    ) : null}
                    {githubBoard.items.length ? (
                      <div className="grid gap-3">
                        <div className="grid min-h-[28rem] gap-3 overflow-x-auto pb-2 lg:grid-cols-5">
                          {githubBoardColumns.map((column) => (
                            <div
                              key={column.key}
                              className="min-w-[18rem] border border-white/10 bg-black/20"
                            >
                              <div className="flex items-start justify-between gap-2 border-b border-white/10 px-3 py-3">
                                <div>
                                  <div className="text-sm font-semibold">{column.label}</div>
                                  <div className="tcp-subtle text-xs">{column.hint}</div>
                                </div>
                                <Badge tone={column.items.length ? "info" : "ghost"}>
                                  {column.items.length}
                                </Badge>
                              </div>
                              <div className="grid max-h-[36rem] content-start gap-2 overflow-y-auto p-2">
                                {column.items.length ? (
                                  column.items.map((item: any) => {
                                    const itemCanRun = githubBoardItemCanRun(item);
                                    const launchLabel = githubBoardItemLaunchLabel(item);
                                    const itemId = String(item.id || "");
                                    const title = String(item.title || "Untitled item");
                                    const lowerTitle = title.toLowerCase();
                                    const activeGithubRun = activeGithubRunByIdentity.get(
                                      githubBoardItemIdentity(item)
                                    );
                                    const itemIsRunning =
                                      !!activeGithubRun ||
                                      activeGithubItemIdentities.has(
                                        githubBoardItemIdentity(item)
                                      ) ||
                                      !!String(item.activeRunId || item.active_run_id || "").trim();
                                    const itemIsLaunching = launchingGithubItemIdSet.has(itemId);
                                    const itemIsLaunchLocked =
                                      itemIsRunning || itemIsLaunching || batchTriggering;
                                    const selected = selectedGithubItemIds.includes(itemId);
                                    const isParent =
                                      item?.isParent === true ||
                                      lowerTitle.includes("[aca slice parent]") ||
                                      lowerTitle.includes("slice parent");
                                    const isDraft = !item.issueNumber && !item.issueUrl;
                                    const issueDisplay =
                                      selectedProjectTaskSourceType === "linear"
                                        ? String(item.identifier || item.issueNumber || "").trim()
                                        : item.issueNumber
                                          ? `#${String(item.issueNumber)}`
                                          : "";
                                    return (
                                      <div
                                        key={itemId}
                                        className={`border p-3 transition ${
                                          selected
                                            ? "border-cyan-400/70 bg-cyan-500/10"
                                            : "border-white/10 bg-slate-950/40"
                                        }`}
                                      >
                                        <div className="flex items-start gap-2">
                                          <input
                                            type="checkbox"
                                            className="mt-1 h-4 w-4 shrink-0"
                                            checked={selected}
                                            disabled={!itemCanRun || itemIsLaunchLocked}
                                            onChange={() => toggleGithubItemSelection(itemId)}
                                            aria-label={`Select ${title}`}
                                          />
                                          <div className="min-w-0 flex-1">
                                            <div className="break-words text-sm font-semibold leading-5">
                                              {title}
                                            </div>
                                            <div className="tcp-subtle mt-1 break-words text-xs leading-5">
                                              {item.repoName
                                                ? String(item.repoName)
                                                : selectedProjectSlug}
                                              {issueDisplay ? ` ${issueDisplay}` : ""}
                                            </div>
                                          </div>
                                        </div>
                                        <div className="mt-3 flex flex-wrap gap-2">
                                          {isParent ? <Badge tone="info">Parent</Badge> : null}
                                          {item.phase !== null && item.phase !== undefined ? (
                                            <Badge tone="ghost">
                                              {Number(item.phase) === 99
                                                ? "Gate"
                                                : `Phase ${String(item.phase)}`}
                                            </Badge>
                                          ) : null}
                                          {item.launchState ? (
                                            <Badge tone={item.actionable ? "ok" : "ghost"}>
                                              {formatStatus(String(item.launchState))}
                                            </Badge>
                                          ) : null}
                                          {isDraft ? <Badge tone="warn">Draft</Badge> : null}
                                          {item.actionable ? (
                                            <Badge tone="ok">Actionable</Badge>
                                          ) : null}
                                          {itemIsRunning ? (
                                            <Badge tone="info">Run active</Badge>
                                          ) : null}
                                          {item.runState ? (
                                            <Badge tone="info">
                                              {formatStatus(String(item.runState))}
                                            </Badge>
                                          ) : null}
                                          {activeGithubRun ? (
                                            <Badge tone="info">{String(activeGithubRun.id)}</Badge>
                                          ) : null}
                                          {item.handoffUrl || item.handoff_url ? (
                                            <Badge tone="ok">PR handoff</Badge>
                                          ) : null}
                                          {itemIsLaunching && !itemIsRunning ? (
                                            <Badge tone="warn">Starting</Badge>
                                          ) : null}
                                        </div>
                                        {item.blockedBy?.length ? (
                                          <div className="tcp-subtle mt-2 text-xs">
                                            Waiting on {item.blockedBy.join(", ")}
                                          </div>
                                        ) : null}
                                        <div className="mt-3 flex flex-wrap gap-2">
                                          {item.issueUrl ? (
                                            <a
                                              className="tcp-btn h-8 px-3 text-xs"
                                              href={item.issueUrl}
                                              target="_blank"
                                              rel="noreferrer"
                                            >
                                              Open in {selectedTaskSourceLabel}
                                            </a>
                                          ) : null}
                                          <button
                                            type="button"
                                            className="tcp-btn h-8 px-3 text-xs"
                                            onClick={() => triggerGithubItems([item])}
                                            disabled={!itemCanRun || itemIsLaunchLocked}
                                          >
                                            {!itemCanRun
                                              ? launchLabel
                                              : itemIsRunning
                                                ? "Already running"
                                                : itemIsLaunching
                                                  ? "Starting..."
                                                  : "Run task"}
                                          </button>
                                          {selectedProjectTaskSourceType === "linear" ? (
                                            <CodingWorkflowsLinearTaskStateSelect
                                              item={item}
                                              itemId={itemId}
                                              movingTaskStates={movingTaskStates}
                                              onMove={moveLinearTaskState}
                                            />
                                          ) : null}
                                          {activeGithubRun ? (
                                            <button
                                              type="button"
                                              className="tcp-btn tcp-btn-secondary h-8 px-3 text-xs"
                                              onClick={() => inspectAcaRun(activeGithubRun)}
                                            >
                                              <Icon name="terminal" />
                                              View live run
                                            </button>
                                          ) : null}
                                          {item.handoffUrl || item.handoff_url ? (
                                            <a
                                              className="tcp-btn tcp-btn-secondary h-8 px-3 text-xs"
                                              href={String(item.handoffUrl || item.handoff_url)}
                                              target="_blank"
                                              rel="noreferrer"
                                            >
                                              PR
                                            </a>
                                          ) : null}
                                        </div>
                                      </div>
                                    );
                                  })
                                ) : (
                                  <div className="tcp-subtle px-2 py-4 text-xs">No items</div>
                                )}
                              </div>
                            </div>
                          ))}
                        </div>
                      </div>
                    ) : (
                      <EmptyState text={`No ${selectedTaskSourceLabel} items returned for this project.`} />
                    )}
                  </div>
                )
              ) : (
                <EmptyState text="The selected project is not backed by a live issue task source." />
              )}
            </PanelCard>
            <PanelCard
              title="ACA execution history"
              subtitle="Recent runs for the selected project"
            >
              {visibleRuns.length ? (
                <div className="grid gap-2">
                  {visibleRuns.map((run: any, index: number) => {
                    const id = runId(run, index);
                    const isSelected = id === selectedRunId;
                    return (
                      <button
                        key={id}
                        type="button"
                        className={`rounded-2xl border px-3 py-3 text-left transition ${
                          isSelected
                            ? "border-cyan-400/60 bg-cyan-500/10"
                            : "border-white/10 bg-black/20 hover:border-white/20"
                        }`}
                        onClick={() => setSelectedRunId(id)}
                      >
                        <div className="flex flex-wrap items-start justify-between gap-3">
                          <div className="min-w-0">
                            <div className="truncate text-sm font-semibold">{runTitle(run)}</div>
                            <div className="tcp-subtle mt-1 text-xs">
                              {String(run?.project_slug || "unknown")}
                              {run?.branch ? ` · ${String(run.branch)}` : ""}
                            </div>
                          </div>
                          <div className="flex flex-wrap gap-2">
                            <Badge tone={runIsActive(run) ? "info" : "ok"}>
                              {formatStatus(runStatus(run))}
                            </Badge>
                            {runPhase(run) ? (
                              <Badge tone="ghost">{formatStatus(runPhase(run))}</Badge>
                            ) : null}
                          </div>
                        </div>
                        <div className="tcp-subtle mt-2 text-xs">{id}</div>
                      </button>
                    );
                  })}
                </div>
              ) : (
                <EmptyState text="No ACA runs for this project yet." />
              )}
            </PanelCard>
            <div id="aca-run-inspector">
              <PanelCard
                title="Run detail"
                subtitle={selectedRunId ? `ACA detail for ${selectedRunId}` : "Select a run"}
                actions={
                  <div className="flex flex-wrap gap-2">
                    <button
                      type="button"
                      className="tcp-btn h-8 px-3 text-xs"
                      onClick={() => {
                        setRunDetailOpen(true);
                        setLiveLogsOpen(true);
                      }}
                      disabled={!selectedRunId}
                    >
                      <Icon name="terminal" />
                      Open console view
                    </button>
                    <button
                      type="button"
                      className="tcp-btn h-8 px-3 text-xs"
                      onClick={() => setRunDetailOpen((prev) => !prev)}
                    >
                      <Icon name={runDetailOpen ? "chevron-down" : "chevron-right"} />
                      {runDetailOpen ? "Collapse" : "Expand"}
                    </button>
                  </div>
                }
              >
                {runDetailOpen ? (
                  selectedRunId ? (
                    runDetailQuery.isLoading ? (
                      <div className="tcp-subtle text-sm">Loading run detail...</div>
                    ) : runDetailQuery.isError ? (
                      <div className="rounded-2xl border border-red-500/20 bg-red-500/10 p-4 text-sm text-red-200">
                        {runDetailQuery.error instanceof Error
                          ? runDetailQuery.error.message
                          : "Could not load run detail."}
                      </div>
                    ) : (
                      <div className="grid gap-3">
                        <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                          <div className="flex items-start justify-between gap-3">
                            <div className="min-w-0">
                              <div className="text-sm font-semibold">
                                {String(
                                  runDetailQuery.data?.status?.task?.title ||
                                    selectedRun?.title ||
                                    selectedRunId
                                )}
                              </div>
                              <div className="tcp-subtle mt-1 text-xs">
                                {String(
                                  runDetailQuery.data?.project_slug ||
                                    selectedRun?.project_slug ||
                                    "unknown"
                                )}
                              </div>
                            </div>
                            <Badge tone={runDetailQuery.data?.is_running ? "info" : "ok"}>
                              {formatStatus(
                                String(
                                  runDetailQuery.data?.status?.run?.status ||
                                    selectedRun?.status ||
                                    "unknown"
                                )
                              )}
                            </Badge>
                          </div>
                          <div className="mt-3 flex flex-wrap gap-2">
                            {runDetailQuery.data?.status?.phase?.name ? (
                              <Badge tone="info">
                                Phase {formatStatus(String(runDetailQuery.data.status.phase.name))}
                              </Badge>
                            ) : null}
                            {lastRunEvent ? (
                              <Badge tone="ghost">Latest {formatStatus(lastRunEvent)}</Badge>
                            ) : null}
                            {runDetailQuery.data?.snapshot?.summary_available ? (
                              <Badge tone="ok">Summary ready</Badge>
                            ) : null}
                            {runDetailQuery.data?.error ? (
                              <Badge tone="warn">Has error</Badge>
                            ) : null}
                          </div>
                        </div>
                        <div className="grid gap-3 lg:grid-cols-2">
                          <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                            <div className="mb-3 flex items-center justify-between gap-3">
                              <div className="text-sm font-semibold">Progress</div>
                              <Badge tone={runDetailQuery.data?.is_running ? "info" : "ghost"}>
                                {runDetailQuery.data?.is_running ? "Live" : "Snapshot"}
                              </Badge>
                            </div>
                            <div className="grid gap-2 text-xs leading-5">
                              <div className="flex justify-between gap-3">
                                <span className="tcp-subtle">Phase</span>
                                <span className="text-right font-semibold text-slate-100">
                                  {formatStatus(
                                    String(runDetailQuery.data?.status?.phase?.name || "unknown")
                                  )}
                                </span>
                              </div>
                              {runDetailQuery.data?.status?.phase?.detail ? (
                                <div className="flex justify-between gap-3">
                                  <span className="tcp-subtle">Detail</span>
                                  <span className="max-w-[70%] text-right text-slate-200">
                                    {String(runDetailQuery.data.status.phase.detail)}
                                  </span>
                                </div>
                              ) : null}
                              <div className="flex justify-between gap-3">
                                <span className="tcp-subtle">Workers</span>
                                <span className="text-right text-slate-200">
                                  {runWorkers.length
                                    ? `${runWorkers.filter((worker: any) => String(worker?.status || "") === "completed").length}/${runWorkers.length} completed`
                                    : runSubtasks.length
                                      ? `${runSubtasks.length} planned`
                                      : "not planned yet"}
                                </span>
                              </div>
                              {blackboard?.coder_supervision ? (
                                <div className="flex justify-between gap-3">
                                  <span className="tcp-subtle">Coder</span>
                                  <span className="text-right text-slate-200">
                                    {formatStatus(
                                      String(
                                        blackboard.coder_supervision?.tandem_status ||
                                          blackboard.coder_supervision?.status ||
                                          "watching"
                                      )
                                    )}
                                  </span>
                                </div>
                              ) : null}
                            </div>
                          </div>
                          <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                            <div className="mb-3 text-sm font-semibold">Changed files</div>
                            {runChangedFiles.length ? (
                              <div className="grid gap-2">
                                {runChangedFiles.slice(0, 12).map((path) => (
                                  <code
                                    key={path}
                                    className="block truncate rounded-lg border border-white/10 bg-black/30 px-2 py-1 text-xs text-slate-200"
                                  >
                                    {path}
                                  </code>
                                ))}
                                {runChangedFiles.length > 12 ? (
                                  <div className="tcp-subtle text-xs">
                                    +{runChangedFiles.length - 12} more
                                  </div>
                                ) : null}
                              </div>
                            ) : (
                              <div className="tcp-subtle text-sm">
                                No file changes reported yet.
                              </div>
                            )}
                          </div>
                        </div>
                        <CodingWorkflowsRunDiagnosticsPanel runDetail={runDetailQuery.data} />
                        {runEvents.length ? (
                          <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                            <div className="mb-3 text-sm font-semibold">Recent activity</div>
                            <div className="grid gap-2">
                              {runEvents.map((event: any) => (
                                <div
                                  key={`${String(event?.seq || "")}-${String(event?.type || "")}`}
                                  className="grid gap-1 rounded-lg border border-white/10 bg-black/20 px-3 py-2 text-xs"
                                >
                                  <div className="flex flex-wrap items-center justify-between gap-2">
                                    <span className="font-semibold text-slate-100">
                                      {formatStatus(String(event?.type || "event"))}
                                    </span>
                                    <span className="tcp-subtle">
                                      {event?.timestamp
                                        ? new Date(String(event.timestamp)).toLocaleTimeString()
                                        : ""}
                                    </span>
                                  </div>
                                  {event?.payload ? (
                                    <div className="tcp-subtle truncate">
                                      {String(
                                        event.payload?.summary ||
                                          event.payload?.detail ||
                                          event.payload?.reason ||
                                          event.payload?.worker_id ||
                                          event.payload?.status ||
                                          ""
                                      )}
                                    </div>
                                  ) : null}
                                  {runEventDiagnosticBadges(event).length ? (
                                    <div className="flex flex-wrap gap-1">
                                      {runEventDiagnosticBadges(event).map((badge, index) => (
                                        <Badge key={`${badge.label}-${index}`} tone={badge.tone}>
                                          {badge.label}
                                        </Badge>
                                      ))}
                                    </div>
                                  ) : null}
                                </div>
                              ))}
                            </div>
                          </div>
                        ) : null}
                        {runDiffStat ? (
                          <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                            <div className="mb-2 text-sm font-semibold">Diff stat</div>
                            <pre className="max-h-48 overflow-auto whitespace-pre-wrap text-xs leading-6 text-slate-200">
                              {runDiffStat}
                            </pre>
                          </div>
                        ) : null}
                        {runSummary ? (
                          <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                            <div className="mb-2 text-sm font-semibold">Summary</div>
                            <pre className="max-h-56 overflow-auto whitespace-pre-wrap text-xs leading-6 text-slate-200">
                              {runSummary}
                            </pre>
                          </div>
                        ) : null}
                        <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                          <div className="mb-2 text-sm font-semibold">Blackboard</div>
                          <LazyJson
                            value={blackboard || {}}
                            label="Show blackboard"
                            preClassName="max-h-72 overflow-auto whitespace-pre-wrap text-xs leading-6 text-slate-200"
                          />
                        </div>
                      </div>
                    )
                  ) : (
                    <EmptyState text="Select a run from the board to inspect its status, summary, and blackboard." />
                  )
                ) : null}
              </PanelCard>
            </div>
            <PanelCard
              title="Live logs"
              subtitle="Tail ACA worker and manager logs"
              actions={
                <button
                  type="button"
                  className="tcp-btn h-8 px-3 text-xs"
                  onClick={() => setLiveLogsOpen((prev) => !prev)}
                >
                  <Icon name={liveLogsOpen ? "chevron-down" : "chevron-right"} />
                  {liveLogsOpen ? "Collapse" : "Expand"}
                </button>
              }
            >
              {liveLogsOpen ? (
                selectedRunId ? (
                  <div className="grid gap-3">
                    {logRows.length ? (
                      <select
                        className="tcp-input"
                        value={selectedLogName}
                        onChange={(event) =>
                          setSelectedLogName((event.target as HTMLSelectElement).value)
                        }
                      >
                        {logRows.map((log: any) => (
                          <option key={String(log?.name || "")} value={String(log?.name || "")}>
                            {String(log?.name || "")}
                          </option>
                        ))}
                      </select>
                    ) : (
                      <EmptyState text="No logs available yet." />
                    )}
                    {selectedLogName && logTailQuery.data?.lines ? (
                      <pre className="max-h-80 overflow-auto rounded-2xl border border-white/10 bg-black/30 p-4 text-xs leading-6 text-slate-200">
                        {toArray(logTailQuery.data, "lines").join("\n")}
                      </pre>
                    ) : null}
                  </div>
                ) : (
                  <EmptyState text="Choose a run to inspect log output." />
                )
              ) : null}
            </PanelCard>
          </div>
        </div>
      ) : null}
      {tab === "manual" ? (
        <div className="grid gap-4">
          <div className="grid gap-4 xl:grid-cols-2">
            <CodingWorkflowsRegisterProjectPanel
              hostedManaged={hostedManaged}
              linearCatalog={linearCatalogQuery.data || null}
              linearCatalogError={
                linearCatalogQuery.error instanceof Error ? linearCatalogQuery.error.message : ""
              }
              linearCatalogLoading={
                linearCatalogQuery.isLoading ||
                (linearCatalogQuery.isFetching && !linearCatalogQuery.data)
              }
              newCredentialFile={newCredentialFile}
              newDefaultBranch={newDefaultBranch}
              newProjectName={newProjectName}
              newProjectSlug={newProjectSlug}
              newRemoteName={newRemoteName}
              newRepoPath={newRepoPath}
              newRepoRef={newRepoRef}
              newRepoUrl={newRepoUrl}
              newWorktreeRoot={newWorktreeRoot}
              registering={registering}
              registerProject={registerProject}
              refreshLinearCatalog={() => linearCatalogQuery.refetch()}
              setNewCredentialFile={setNewCredentialFile}
              setNewDefaultBranch={setNewDefaultBranch}
              setNewProjectName={setNewProjectName}
              setNewProjectSlug={setNewProjectSlug}
              setNewRemoteName={setNewRemoteName}
              setNewRepoPath={setNewRepoPath}
              setNewRepoUrl={setNewRepoUrl}
              setNewWorktreeRoot={setNewWorktreeRoot}
              setTaskSourceLinearLabels={setTaskSourceLinearLabels}
              setTaskSourceLinearProject={setTaskSourceLinearProject}
              setTaskSourceLinearQuery={setTaskSourceLinearQuery}
              setTaskSourceLinearStatuses={setTaskSourceLinearStatuses}
              setTaskSourceLinearTeam={setTaskSourceLinearTeam}
              setTaskSourcePath={setTaskSourcePath}
              setTaskSourceProject={setTaskSourceProject}
              setTaskSourcePrompt={setTaskSourcePrompt}
              setTaskSourceType={setTaskSourceType}
              taskSourceLinearLabels={taskSourceLinearLabels}
              taskSourceLinearProject={taskSourceLinearProject}
              taskSourceLinearQuery={taskSourceLinearQuery}
              taskSourceLinearStatuses={taskSourceLinearStatuses}
              taskSourceLinearTeam={taskSourceLinearTeam}
              taskSourcePath={taskSourcePath}
              taskSourceProject={taskSourceProject}
              taskSourcePrompt={taskSourcePrompt}
              taskSourceType={taskSourceType}
            />
            <PanelCard
              title="Trigger run"
              subtitle="Launch an ACA coding session for the selected project"
            >
              <div className="grid gap-3">
                <select
                  className="tcp-input"
                  value={selectedProjectSlug}
                  onChange={(event) =>
                    setSelectedProjectSlug((event.target as HTMLSelectElement).value)
                  }
                >
                  {!projects.length ? <option value="">No ACA projects found</option> : null}
                  {projects.map((project: any) => (
                    <option key={project.slug} value={project.slug}>
                      {project.slug}
                    </option>
                  ))}
                </select>
                <input
                  className="tcp-input"
                  placeholder="Specific item or card id (optional)"
                  value={runItem}
                  onInput={(event) => setRunItem((event.target as HTMLInputElement).value)}
                />
                {renderAcaModelSelector(triggering)}
                <button
                  type="button"
                  className="tcp-btn-primary"
                  disabled={triggering}
                  onClick={triggerRun}
                >
                  {triggering ? "Starting..." : "Trigger ACA Run"}
                </button>
              </div>
            </PanelCard>
          </div>
        </div>
      ) : null}
      {tab === "planning" ? (
        <TaskPlanningPanel
          client={client}
          api={api}
          toast={toast}
          selectedProjectSlug={selectedProjectSlug}
          selectedProject={selectedProject}
          githubProjectBoardSnapshot={projectBoardQuery.data || null}
          taskSourceType={selectedProjectTaskSourceType}
          workspaceRootSeed={planningWorkspaceRootSeed}
          connectedMcpServers={connectedMcpServers}
          engineHealthy={healthy}
          providerStatus={providerStatus}
        />
      ) : null}
      {tab === "integrations" ? (
        <div className="grid gap-4 xl:grid-cols-2">
          <PanelCard title="ACA connection" subtitle="Control-plane endpoint the coding page uses">
            <div className="grid gap-3">
              <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                <div className="text-sm font-semibold">Health</div>
                <div className="tcp-subtle mt-1 text-xs">
                  {acaHealth.data?.status || "Unavailable"}
                  {acaHealth.data?.version ? ` · ${String(acaHealth.data.version)}` : ""}
                </div>
              </div>
              <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                <div className="text-sm font-semibold">Projects</div>
                <div className="tcp-subtle mt-1 text-xs">{projects.length} registered</div>
              </div>
              <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                <div className="text-sm font-semibold">Runs</div>
                <div className="tcp-subtle mt-1 text-xs">{runs.length} visible through ACA</div>
              </div>
            </div>
          </PanelCard>
          <PanelCard title="Workspace guide" subtitle="What the agent should inspect first">
            {workspaceGuideQuery.data ? (
              <div className="grid gap-3">
                <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                  <div className="text-sm font-semibold">Active project</div>
                  <div className="tcp-subtle mt-1 text-xs">
                    {String(workspaceGuideQuery.data?.active_project?.name || "None").trim()}
                    {workspaceGuideQuery.data?.active_project?.repo?.path
                      ? ` · ${String(workspaceGuideQuery.data.active_project.repo.path)}`
                      : ""}
                  </div>
                </div>
                <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                  <div className="text-sm font-semibold">Layout</div>
                  <div className="tcp-subtle mt-1 text-xs">
                    {String(workspaceGuideQuery.data?.layout?.worktree_root || "managed root")}
                  </div>
                </div>
                <ul className="grid gap-2 text-xs text-slate-200">
                  {(Array.isArray(workspaceGuideQuery.data?.instructions)
                    ? workspaceGuideQuery.data.instructions
                    : []
                  ).map((line: string, index: number) => (
                    <li
                      key={`${index}-${line}`}
                      className="rounded-xl border border-white/10 bg-black/10 px-3 py-2"
                    >
                      {line}
                    </li>
                  ))}
                </ul>
              </div>
            ) : (
              <EmptyState text="Workspace guide unavailable yet." />
            )}
          </PanelCard>
          <PanelCard
            title="Connected MCP servers"
            subtitle="Engine integrations still available alongside ACA"
          >
            {mcpServers.length ? (
              <div className="grid gap-2">
                {mcpServers.map((server) => (
                  <div
                    key={server.name}
                    className="flex items-center justify-between gap-3 rounded-2xl border border-white/10 bg-black/20 px-3 py-2"
                  >
                    <div className="min-w-0">
                      <div className="truncate text-sm font-semibold">{server.name}</div>
                      <div className="tcp-subtle text-xs">
                        {server.transport || "transport pending"}
                        {server.lastError ? ` · ${server.lastError}` : ""}
                      </div>
                    </div>
                    <Badge tone={server.connected ? "ok" : server.enabled ? "warn" : "ghost"}>
                      {server.connected ? "Connected" : server.enabled ? "Configured" : "Disabled"}
                    </Badge>
                  </div>
                ))}
              </div>
            ) : (
              <EmptyState text="No MCP servers detected yet." />
            )}
          </PanelCard>
        </div>
      ) : null}
    </AnimatedPage>
  );
}
