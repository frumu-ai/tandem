import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatedPage, Badge, PanelCard, StatusPulse } from "../ui/index.tsx";
import { EmptyState } from "./ui";
import { useCapabilities } from "../features/system/queries.ts";
import { subscribeSse } from "../services/sse.js";
import { TaskPlanningPanel } from "./TaskPlanningPanel";
import type { AppPageProps } from "./pageTypes";

type CodingTab = "overview" | "board" | "planning" | "manual" | "integrations";
type TaskSourceType = "manual" | "kanban_board" | "github_project" | "local_backlog";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function normalizeServers(raw: any) {
  const rows = Array.isArray(raw)
    ? raw
    : Array.isArray(raw?.servers)
      ? raw.servers
      : raw && typeof raw === "object"
        ? Object.entries(raw).map(([name, row]) => ({ name, ...(row as any) }))
        : [];
  return rows
    .map((row: any) => ({
      name: String(row?.name || "").trim(),
      connected: !!row?.connected,
      enabled: row?.enabled !== false,
      transport: String(row?.transport || "").trim(),
      lastError: String(row?.last_error || row?.lastError || "").trim(),
    }))
    .filter((row: any) => row.name)
    .sort((a: any, b: any) => a.name.localeCompare(b.name));
}

function normalizeTools(raw: any) {
  const rows = Array.isArray(raw) ? raw : Array.isArray(raw?.tools) ? raw.tools : [];
  return rows
    .map((tool: any) => {
      if (typeof tool === "string") return tool;
      return String(tool?.namespaced_name || tool?.namespacedName || tool?.id || "").trim();
    })
    .filter(Boolean);
}

function normalizeProjects(raw: any) {
  const rows = Array.isArray(raw)
    ? raw
    : raw && typeof raw === "object"
      ? Object.entries(raw).map(([slug, record]) => ({
          slug,
          ...(record as any),
        }))
      : [];
  const bySignature = new Map<string, any>();
  for (const row of rows) {
    const taskSource = row?.task_source || row?.taskSource || {};
    const taskType = String(taskSource?.type || "").trim();
    const repo = row?.repo || {};
    const repoUrl = String(
      row?.repo_url || row?.repoUrl || repo?.clone_url || repo?.cloneUrl || ""
    ).trim();
    const signature =
      taskType === "github_project" &&
      String(taskSource?.owner || "").trim() &&
      String(taskSource?.repo || "").trim() &&
      String(taskSource?.project || "").trim()
        ? `github:${String(taskSource.owner).trim().toLowerCase()}/${String(taskSource.repo)
            .trim()
            .toLowerCase()}#${String(taskSource.project).trim()}`
        : repoUrl
          ? `repo:${repoUrl.toLowerCase()}`
          : `slug:${row.slug}`;
    const current = bySignature.get(signature);
    if (!current) {
      bySignature.set(signature, row);
      continue;
    }
    const currentRepo = current?.repo || {};
    const currentRepoUrl = String(
      current?.repo_url || current?.repoUrl || currentRepo?.clone_url || currentRepo?.cloneUrl || ""
    ).trim();
    const currentScore = (current.implicit ? 0 : 10) + (currentRepoUrl ? 1 : 0);
    const nextScore = (row.implicit ? 0 : 10) + (repoUrl ? 1 : 0);
    if (nextScore > currentScore) {
      bySignature.set(signature, row);
    }
  }

  return Array.from(bySignature.values())
    .map((row: any) => {
      const repo = row?.repo || {};
      const taskSource = row?.task_source || row?.taskSource || {};
      return {
        slug: String(row?.slug || "").trim(),
        name: String(row?.name || row?.display_name || row?.displayName || row?.slug || "").trim(),
        repoUrl: String(
          row?.repo_url || row?.repoUrl || repo?.clone_url || repo?.cloneUrl || ""
        ).trim(),
        repoPath: String(repo?.path || row?.repo_path || row?.repoPath || "").trim(),
        worktreeRoot: String(
          repo?.worktree_root || row?.worktree_root || row?.worktreeRoot || ""
        ).trim(),
        defaultBranch: String(
          repo?.default_branch || row?.default_branch || row?.defaultBranch || "main"
        ).trim(),
        remoteName: String(
          repo?.remote_name || row?.remote_name || row?.remoteName || "origin"
        ).trim(),
        credentialFile: String(
          repo?.credential_file || row?.credential_file || row?.credentialFile || ""
        ).trim(),
        taskSource,
        implicit: row?.implicit === true,
      };
    })
    .filter((row: any) => row.slug)
    .sort((a: any, b: any) => {
      if (a.implicit !== b.implicit) return a.implicit ? 1 : -1;
      return a.slug.localeCompare(b.slug);
    });
}

function normalizeGithubBoard(raw: any) {
  const columns = Array.isArray(raw?.columns)
    ? raw.columns
        .map((column: any, index: number) => ({
          id: String(column?.id || column?.key || `column-${index}`).trim(),
          key: String(column?.key || column?.name || `column-${index}`).trim(),
          name: String(column?.name || column?.key || `Column ${index + 1}`).trim(),
          itemCount: Number(column?.item_count || column?.itemCount || 0),
        }))
        .filter((column: any) => column.key)
    : [];
  const items = Array.isArray(raw?.items)
    ? raw.items
        .map((item: any, index: number) => ({
          id: String(item?.id || item?.project_item_id || `item-${index}`).trim(),
          projectItemId: String(item?.project_item_id || item?.projectItemId || "").trim(),
          title: String(item?.title || "Untitled item").trim(),
          statusKey: String(item?.status_key || item?.statusKey || "unknown").trim(),
          statusName: String(item?.status_name || item?.statusName || "Unknown").trim(),
          issueNumber: item?.issue_number || item?.issueNumber || null,
          issueUrl: String(item?.issue_url || item?.issueUrl || "").trim(),
          repoName: String(item?.repo_name || item?.repoName || "").trim(),
          actionable: item?.actionable === true,
          selector: String(
            item?.project_item_id ||
              item?.projectItemId ||
              item?.issue_number ||
              item?.issueNumber ||
              item?.id ||
              ""
          ).trim(),
        }))
        .filter((item: any) => item.id)
    : [];
  return {
    columns,
    items,
    source: String(raw?.source || "").trim(),
    warning: String(raw?.warning || "").trim(),
    isStale: raw?.is_stale === true,
    lastSyncedAtMs: Number(raw?.last_synced_at_ms || raw?.lastSyncedAtMs || 0),
  };
}

function githubBoardViewStorageKey(projectSlug: string) {
  return `tcp.coding.github-board.view.v1:${projectSlug}`;
}

function loadHiddenGithubColumns(projectSlug: string) {
  if (!projectSlug || typeof localStorage === "undefined") return [];
  try {
    const parsed = JSON.parse(localStorage.getItem(githubBoardViewStorageKey(projectSlug)) || "{}");
    return Array.isArray(parsed?.hiddenColumns)
      ? parsed.hiddenColumns.map((value: any) => String(value || "").trim()).filter(Boolean)
      : [];
  } catch {
    return [];
  }
}

function saveHiddenGithubColumns(projectSlug: string, hiddenColumns: string[]) {
  if (!projectSlug || typeof localStorage === "undefined") return;
  localStorage.setItem(
    githubBoardViewStorageKey(projectSlug),
    JSON.stringify({
      hiddenColumns: hiddenColumns.map((value) => String(value || "").trim()).filter(Boolean),
    })
  );
}

function runId(run: any, index: number) {
  return String(run?.run_id || run?.runId || run?.id || `run-${index}`).trim();
}

function runTitle(run: any) {
  return String(run?.title || run?.summary || run?.run_id || run?.runId || "Untitled run").trim();
}

function runUpdatedAt(run: any) {
  const value = Number(
    run?.updated_at_ms ||
      run?.created_at_ms ||
      run?.snapshot?.updated_at_ms ||
      run?.snapshot?.created_at_ms ||
      0
  );
  return Number.isFinite(value) ? value : 0;
}

function runStatus(run: any) {
  return String(run?.status || run?.snapshot?.status || run?.status?.run?.status || "unknown")
    .trim()
    .toLowerCase();
}

function runPhase(run: any) {
  return String(run?.phase?.name || run?.snapshot?.phase?.name || run?.status?.phase?.name || "")
    .trim()
    .toLowerCase();
}

function formatStatus(status: string) {
  return String(status || "unknown")
    .replace(/_/g, " ")
    .replace(/\b\w/g, (match) => match.toUpperCase());
}

const ACTIVE_RUN_STALE_AFTER_MS = 30 * 60 * 1000;
const GITHUB_ITEM_LAUNCH_LOCK_MS = 15 * 1000;

function runHasLiveSession(run: any) {
  return run?.is_running === true || run?.snapshot?.is_running === true;
}

function runIsActive(run: any) {
  const status = runStatus(run);
  if (["completed", "done", "failed", "cancelled", "blocked", "archived"].includes(status)) {
    return false;
  }
  if (runHasLiveSession(run)) {
    return true;
  }
  const updatedAt = runUpdatedAt(run);
  if (!updatedAt) {
    return false;
  }
  return Date.now() - updatedAt < ACTIVE_RUN_STALE_AFTER_MS;
}

function runTaskIdentity(run: any, index: number) {
  const task = run?.blackboard?.task || run?.snapshot?.blackboard?.task || {};
  const source = task?.source || {};
  const repo = task?.repo || {};
  return String(
    source?.issue_url ||
      source?.url ||
      source?.item_url ||
      source?.project_item_id ||
      source?.card_id ||
      task?.task_id ||
      `${repo?.slug || run?.project_slug || "project"}:${runTitle(run)}:${index}`
  ).trim();
}

function githubBoardItemIdentity(item: any) {
  return String(item?.issueUrl || item?.projectItemId || item?.id || "").trim();
}

function githubBoardItemCanRun(item: any) {
  const statusKey = String(item?.statusKey || "")
    .trim()
    .toLowerCase();
  if (!item?.selector) return false;
  if (statusKey === "in_review" || statusKey === "done") return false;
  return item?.actionable === true || ["ready", "backlog", "todo"].includes(statusKey);
}

function dedupeRuns(runs: any[]) {
  const latestByIdentity = new Map<string, any>();
  runs.forEach((run, index) => {
    const identity = runTaskIdentity(run, index);
    const existing = latestByIdentity.get(identity);
    if (!existing || runUpdatedAt(run) >= runUpdatedAt(existing)) {
      latestByIdentity.set(identity, run);
    }
  });
  return Array.from(latestByIdentity.values()).sort((a, b) => runUpdatedAt(b) - runUpdatedAt(a));
}

function groupRuns(runs: any[]) {
  const lanes: Array<{
    id: string;
    label: string;
    hint: string;
    statuses: string[];
    items: any[];
  }> = [
    {
      id: "queue",
      label: "Queue",
      hint: "Created or waiting for work to begin",
      statuses: ["created", "queued", "pending", "idle", "starting"],
      items: [],
    },
    {
      id: "planning",
      label: "Planning",
      hint: "Resolving task source and planning work",
      statuses: ["bootstrap", "engine_check", "task_resolution", "planning", "triage"],
      items: [],
    },
    {
      id: "active",
      label: "Active",
      hint: "Executing, reviewing, or testing",
      statuses: ["running", "worker_execution", "review", "test", "handoff", "active"],
      items: [],
    },
    {
      id: "waiting",
      label: "Waiting",
      hint: "Blocked or waiting on intervention",
      statuses: ["blocked", "paused", "waiting", "needs_info"],
      items: [],
    },
    {
      id: "done",
      label: "Done",
      hint: "Completed, failed, or archived",
      statuses: ["completed", "done", "failed", "cancelled", "archived"],
      items: [],
    },
    { id: "other", label: "Other", hint: "Unclassified run states", statuses: [], items: [] },
  ];

  runs.forEach((run) => {
    const status = runStatus(run);
    const phase = runPhase(run);
    const statusBucket = lanes.find((lane) => lane.statuses.includes(status));
    const phaseBucket = lanes.find((lane) => lane.statuses.includes(phase));
    const bucket = statusBucket || phaseBucket || lanes[lanes.length - 1];
    bucket.items.push(run);
  });

  return lanes;
}

function Metric({
  label,
  value,
  helper,
  tone = "info",
}: {
  label: string;
  value: string | number;
  helper: string;
  tone?: "info" | "ok" | "warn" | "ghost";
}) {
  return (
    <div className="rounded-2xl border border-white/10 bg-black/20 p-4 shadow-[0_12px_36px_rgba(0,0,0,0.12)]">
      <div className="flex items-start justify-between gap-3">
        <div className="tcp-kpi-label text-sm">{label}</div>
        <Badge tone={tone}>{helper}</Badge>
      </div>
      <div className="mt-3 text-2xl font-semibold tracking-tight">{value}</div>
    </div>
  );
}

function buildTaskSourcePayload(
  taskSourceType: TaskSourceType,
  {
    prompt,
    path,
    owner,
    repo,
    projectNumber,
  }: {
    prompt: string;
    path: string;
    owner: string;
    repo: string;
    projectNumber: string;
  }
) {
  if (taskSourceType === "manual") {
    return { type: "manual", prompt: prompt.trim() };
  }
  if (taskSourceType === "kanban_board") {
    return { type: "kanban_board", path: path.trim() };
  }
  if (taskSourceType === "local_backlog") {
    return { type: "local_backlog", path: path.trim() };
  }
  return {
    type: "github_project",
    owner: owner.trim(),
    repo: repo.trim(),
    project: projectNumber.trim(),
  };
}

function isSafeManagedPath(raw: string) {
  const text = String(raw || "")
    .trim()
    .replace(/\\/g, "/");
  if (!text) return true;
  if (text.startsWith("/") || /^[A-Za-z]:/.test(text)) return false;
  const parts = text.split("/").filter(Boolean);
  return parts.length > 0 && !parts.some((part) => part === "." || part === "..");
}

function parseSseEnvelope(data: string) {
  try {
    const parsed = JSON.parse(String(data || "{}"));
    return parsed && typeof parsed === "object" ? parsed : null;
  } catch {
    return null;
  }
}

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
  const [taskSourceOwner, setTaskSourceOwner] = useState("");
  const [taskSourceRepo, setTaskSourceRepo] = useState("");
  const [taskSourceProject, setTaskSourceProject] = useState("");
  const [runItem, setRunItem] = useState("");
  const [overrideProvider, setOverrideProvider] = useState(providerStatus.defaultProvider || "");
  const [overrideModel, setOverrideModel] = useState(providerStatus.defaultModel || "");
  const [registering, setRegistering] = useState(false);
  const [triggering, setTriggering] = useState(false);
  const [lastGlobalEvent, setLastGlobalEvent] = useState("");
  const [lastRunEvent, setLastRunEvent] = useState("");
  const [taskPreviewRefreshAt, setTaskPreviewRefreshAt] = useState<number | null>(null);
  const [githubBoardRefreshAt, setGithubBoardRefreshAt] = useState<number | null>(null);
  const [runDetailOpen, setRunDetailOpen] = useState(false);
  const [liveLogsOpen, setLiveLogsOpen] = useState(false);
  const [hiddenGithubColumns, setHiddenGithubColumns] = useState<string[]>([]);
  const [selectedGithubItemIds, setSelectedGithubItemIds] = useState<string[]>([]);
  const [launchingGithubItemIds, setLaunchingGithubItemIds] = useState<Record<string, number>>({});
  const [batchTriggering, setBatchTriggering] = useState(false);

  const caps = useCapabilities();
  const acaAvailable = caps.data?.aca_integration === true;
  const engineAvailable = caps.data?.engine_healthy === true;
  const hostedManaged = caps.data?.hosted_managed === true;
  const integrationsEnabled = acaAvailable || engineAvailable;

  useEffect(() => {
    if (!overrideProvider && providerStatus.defaultProvider) {
      setOverrideProvider(providerStatus.defaultProvider);
    }
    if (!overrideModel && providerStatus.defaultModel) {
      setOverrideModel(providerStatus.defaultModel);
    }
  }, [
    overrideModel,
    overrideProvider,
    providerStatus.defaultModel,
    providerStatus.defaultProvider,
  ]);

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
  const runsQuery = useQuery({
    queryKey: ["coding-workflows", "aca-runs"],
    queryFn: () => api("/api/aca/runs"),
    enabled: acaAvailable,
  });
  const projectTasksQuery = useQuery({
    queryKey: ["coding-workflows", "aca-project-tasks", selectedProjectSlug],
    queryFn: () => api(`/api/aca/projects/${encodeURIComponent(selectedProjectSlug)}/tasks`),
    enabled: acaAvailable && !!selectedProjectSlug,
  });
  const projectBoardQuery = useQuery({
    queryKey: ["coding-workflows", "aca-project-board", selectedProjectSlug],
    queryFn: () => api(`/api/aca/projects/${encodeURIComponent(selectedProjectSlug)}/board`),
    enabled:
      acaAvailable &&
      !!selectedProjectSlug &&
      normalizeProjects(projectsQuery.data).some(
        (project: any) =>
          project.slug === selectedProjectSlug &&
          String(project?.taskSource?.type || "").trim() === "github_project"
      ),
    refetchInterval:
      acaAvailable &&
      !!selectedProjectSlug &&
      normalizeProjects(projectsQuery.data).some(
        (project: any) =>
          project.slug === selectedProjectSlug &&
          String(project?.taskSource?.type || "").trim() === "github_project"
      )
        ? 120000
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

  const mcpServers = useMemo(() => normalizeServers(mcpServersQuery.data), [mcpServersQuery.data]);
  const mcpTools = useMemo(() => normalizeTools(mcpToolsQuery.data), [mcpToolsQuery.data]);
  const projects = useMemo(() => normalizeProjects(projectsQuery.data), [projectsQuery.data]);
  const runs = useMemo(() => toArray(runsQuery.data, "runs"), [runsQuery.data]);
  const githubBoard = useMemo(
    () => normalizeGithubBoard(projectBoardQuery.data),
    [projectBoardQuery.data]
  );

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

  const lanes = useMemo(() => groupRuns(visibleRuns), [visibleRuns]);
  const healthy = !!(health.data?.engine?.ready || health.data?.engine?.healthy);
  const githubConnected = mcpServers.some((server) => server.name.toLowerCase().includes("github"));
  const selectedProject =
    projects.find((project: any) => project.slug === selectedProjectSlug) || null;
  const selectedProjectTaskSourceType = String(selectedProject?.taskSource?.type || "").trim();
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
  const githubBoardVisibleColumns = useMemo(
    () =>
      githubBoard.columns.filter(
        (column: any) => !hiddenGithubColumns.includes(String(column.key || ""))
      ),
    [githubBoard.columns, hiddenGithubColumns]
  );
  const selectedGithubItems = useMemo(
    () =>
      githubBoard.items.filter((item: any) =>
        selectedGithubItemIds.includes(String(item.id || ""))
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
  const selectedRun =
    visibleRuns.find((run: any, index: number) => runId(run, index) === selectedRunId) || null;
  const runSummary = String(runDetailQuery.data?.summary || "").trim();
  const blackboard = runDetailQuery.data?.blackboard || null;

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
    setHiddenGithubColumns(loadHiddenGithubColumns(selectedProjectSlug));
  }, [selectedProjectSlug]);

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
    saveHiddenGithubColumns(selectedProjectSlug, hiddenGithubColumns);
  }, [hiddenGithubColumns, selectedProjectSlug]);

  useEffect(() => {
    const validIds = new Set(githubBoard.items.map((item: any) => String(item.id || "")));
    setSelectedGithubItemIds((current) => current.filter((id) => validIds.has(id)));
  }, [githubBoard.items]);

  const tabs: Array<{ id: CodingTab; label: string; icon: string }> = [
    { id: "overview", label: "Overview", icon: "layout-dashboard" },
    { id: "board", label: "Board", icon: "kanban-square" },
    { id: "planning", label: "Planning", icon: "clipboard-list" },
    { id: "manual", label: "Manual tasks", icon: "code" },
    { id: "integrations", label: "Integrations", icon: "plug-zap" },
  ];

  async function registerProject() {
    const slug = newProjectSlug.trim();
    const name = newProjectName.trim();
    const repoUrl = newRepoUrl.trim();
    const repoPath = newRepoPath.trim();
    const worktreeRoot = newWorktreeRoot.trim();
    const defaultBranch = newDefaultBranch.trim();
    const remoteName = newRemoteName.trim();
    const credentialFile = newCredentialFile.trim();
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
      owner: taskSourceOwner,
      repo: taskSourceRepo,
      projectNumber: taskSourceProject,
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
      toast("warn", "GitHub Project task sources require owner, repo, and project number.");
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
    const overrides: Record<string, string> = {};
    if (overrideProvider.trim()) overrides.ACA_PROVIDER = overrideProvider.trim();
    if (overrideModel.trim()) overrides.ACA_MODEL = overrideModel.trim();

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
        setTab("board");
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
    setSelectedGithubItemIds(
      githubBoard.items
        .filter(
          (item: any) =>
            githubBoardItemCanRun(item) &&
            !activeGithubItemIdentities.has(githubBoardItemIdentity(item)) &&
            !launchingGithubItemIdSet.has(String(item.id || ""))
        )
        .map((item: any) => String(item.id || ""))
    );
  }

  function clearGithubSelection() {
    setSelectedGithubItemIds([]);
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
        "Those GitHub items are already running or are not launchable from ACA intake."
      );
      return;
    }
    const overrides: Record<string, string> = {};
    if (overrideProvider.trim()) overrides.ACA_PROVIDER = overrideProvider.trim();
    if (overrideModel.trim()) overrides.ACA_MODEL = overrideModel.trim();

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
      await queryClient.invalidateQueries({ queryKey: ["coding-workflows", "aca-runs"] });
      const runs = toArray(result, "runs");
      const nextRunId = String(runs?.[0]?.run_id || "").trim();
      if (nextRunId) {
        setSelectedRunId(nextRunId);
        setTab("board");
      }
      toast("ok", `Started ${selectors.length} ACA run${selectors.length === 1 ? "" : "s"}.`);
      setSelectedGithubItemIds([]);
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
      toast("err", error instanceof Error ? error.message : String(error));
    } finally {
      setBatchTriggering(false);
    }
  }

  async function refreshTaskPreview() {
    if (!selectedProjectSlug) {
      toast("warn", "Select a project before refreshing GitHub intake.");
      return;
    }
    try {
      await projectTasksQuery.refetch();
      setTaskPreviewRefreshAt(Date.now());
      toast("ok", "Refreshed task intake from GitHub MCP.");
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  }

  async function refreshGithubBoard() {
    if (!selectedProjectSlug) {
      toast("warn", "Select a GitHub-backed project before refreshing the board.");
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
      toast("ok", "Refreshed GitHub Project board through Tandem MCP.");
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  }

  function toggleGithubColumn(columnKey: string) {
    setHiddenGithubColumns((current) =>
      current.includes(columnKey)
        ? current.filter((value) => value !== columnKey)
        : [...current, columnKey]
    );
  }

  if (!acaAvailable) {
    return (
      <AnimatedPage className="grid gap-4">
        <PanelCard className="overflow-hidden">
          <div className="grid gap-5 xl:grid-cols-[minmax(0,1.3fr)_minmax(320px,0.9fr)] xl:items-start">
            <div className="min-w-0">
              <div className="tcp-page-eyebrow">Coding workflows</div>
              <h1 className="tcp-page-title">Autonomous Coding</h1>
              <p className="tcp-subtle mt-2 max-w-3xl">
                ACA integration is required for the coding dashboard. Connect the ACA control plane
                so this workspace can load registered projects, task intake, and live run details.
              </p>
              <div className="mt-3 flex flex-wrap gap-2">
                <Badge tone={engineAvailable ? "ok" : "warn"}>
                  {engineAvailable ? "Engine healthy" : "Engine unavailable"}
                </Badge>
                <Badge tone="warn">ACA disconnected</Badge>
              </div>
              <div className="mt-4 rounded-2xl border border-yellow-500/20 bg-yellow-500/10 p-4">
                <p className="text-sm text-yellow-200">
                  <strong>ACA integration required.</strong> Set `ACA_BASE_URL` and make sure the
                  control panel can authenticate to ACA with `ACA_API_TOKEN` or
                  `ACA_API_TOKEN_FILE`.
                </p>
                <button type="button" className="tcp-btn mt-3" onClick={() => navigate("settings")}>
                  Open ACA setup
                </button>
              </div>
            </div>
          </div>
        </PanelCard>
      </AnimatedPage>
    );
  }

  return (
    <AnimatedPage className="grid gap-4">
      <PanelCard className="overflow-hidden">
        <div className="grid gap-5 xl:grid-cols-[minmax(0,1.3fr)_minmax(320px,0.9fr)] xl:items-start">
          <div className="min-w-0">
            <div className="tcp-page-eyebrow">Coding workflows</div>
            <h1 className="tcp-page-title">ACA project intake and run dashboard</h1>
            <p className="tcp-subtle mt-2 max-w-3xl">
              This view now talks to the ACA FastAPI control plane for project registration, task
              preview, run launch, run detail, logs, and final handoff artifacts.
            </p>
            <div className="mt-3 flex flex-wrap gap-2">
              <Badge tone={acaHealth.data?.status === "healthy" ? "ok" : "warn"}>
                {acaHealth.data?.status === "healthy" ? "ACA healthy" : "ACA checking"}
              </Badge>
              <Badge tone={healthy ? "ok" : "warn"}>
                {healthy ? "Engine healthy" : "Engine checking"}
              </Badge>
              <Badge tone={githubConnected ? "ok" : "warn"}>
                {githubConnected ? "GitHub MCP connected" : "GitHub MCP pending"}
              </Badge>
              <StatusPulse
                tone={activeRuns.length ? "live" : "info"}
                text={`${activeRuns.length} active runs`}
              />
              {lastGlobalEvent ? (
                <Badge tone="ghost">Live {formatStatus(lastGlobalEvent)}</Badge>
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
            <i data-lucide={item.icon}></i>
            {item.label}
          </button>
        ))}
      </div>

      {tab === "overview" ? (
        <>
          <div className="grid gap-4 xl:grid-cols-2">
            <Metric
              label="Registered projects"
              value={projects.length}
              helper={
                selectedProjectSlug ? `Focused on ${selectedProjectSlug}` : "No project selected"
              }
              tone={projects.length ? "ok" : "warn"}
            />
            <Metric
              label="Visible runs"
              value={filteredRuns.length}
              helper={activeRuns.length ? `${activeRuns.length} active` : "Idle"}
              tone={activeRuns.length ? "warn" : "ok"}
            />
            <Metric
              label="Connected MCP servers"
              value={mcpServers.length}
              helper={githubConnected ? "GitHub available" : "MCP pending"}
              tone={githubConnected ? "ok" : "warn"}
            />
            <Metric
              label="Registered tools"
              value={mcpTools.length}
              helper="Engine tool surface"
              tone={mcpTools.length ? "info" : "ghost"}
            />
          </div>

          <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(320px,0.9fr)]">
            <PanelCard title="Project selector" subtitle="ACA-backed repository contexts">
              {projects.length ? (
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
                  <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                    <div className="text-sm font-semibold">
                      {selectedProject?.slug || "No project selected"}
                    </div>
                    <div className="tcp-subtle mt-1 text-xs">
                      {selectedProject?.repoUrl || "No repo URL stored"}
                    </div>
                    <div className="tcp-subtle mt-2 text-xs">
                      Task source: {String(selectedProject?.taskSource?.type || "unknown")}
                    </div>
                  </div>
                </div>
              ) : (
                <EmptyState text="Register an ACA project to start using the coding dashboard." />
              )}
            </PanelCard>

            <PanelCard title="Task intake preview" subtitle="What ACA will try to pick up next">
              {selectedProjectSlug ? (
                projectTasksQuery.isLoading ? (
                  <div className="tcp-subtle text-sm">Loading task preview...</div>
                ) : projectTasksQuery.isError ? (
                  <div className="rounded-2xl border border-red-500/20 bg-red-500/10 p-4 text-sm text-red-200">
                    {projectTasksQuery.error instanceof Error
                      ? projectTasksQuery.error.message
                      : "Could not load task preview."}
                  </div>
                ) : projectTasksQuery.data?.task ? (
                  <div className="grid gap-3">
                    <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-white/10 bg-black/20 p-3">
                      <div className="tcp-subtle text-xs">
                        GitHub Project intake is refreshed on demand through Tandem&apos;s GitHub
                        MCP. It does not auto-update here so we can keep GitHub calls down.
                        {taskPreviewRefreshAt
                          ? ` Last refreshed ${new Date(taskPreviewRefreshAt).toLocaleTimeString()}.`
                          : ""}
                      </div>
                      <button
                        type="button"
                        className="tcp-btn tcp-btn-secondary"
                        onClick={refreshTaskPreview}
                        disabled={projectTasksQuery.isFetching}
                      >
                        {projectTasksQuery.isFetching ? "Refreshing..." : "Refresh from GitHub"}
                      </button>
                    </div>
                    <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                      <div className="text-sm font-semibold">
                        {String(projectTasksQuery.data.task.title || "Untitled task")}
                      </div>
                      <div className="tcp-subtle mt-1 text-xs">
                        {String(projectTasksQuery.data.source_type || "unknown")}
                        {projectTasksQuery.data.board_path
                          ? ` · ${String(projectTasksQuery.data.board_path)}`
                          : ""}
                      </div>
                      <div className="mt-3 flex flex-wrap gap-2">
                        <Badge tone={projectTasksQuery.data.eligible ? "ok" : "warn"}>
                          {projectTasksQuery.data.eligible ? "Eligible" : "Not eligible"}
                        </Badge>
                        <Badge tone="info">
                          ACA intake lane{" "}
                          {formatStatus(String(projectTasksQuery.data.task.lane || "ready"))}
                        </Badge>
                        {projectTasksQuery.data.task?.source ? (
                          projectTasksQuery.data.task.source.status ? (
                            <Badge tone="ghost">
                              GitHub status {String(projectTasksQuery.data.task.source.status)}
                            </Badge>
                          ) : (
                            <Badge tone="ghost">GitHub status unavailable from MCP</Badge>
                          )
                        ) : null}
                      </div>
                    </div>
                    {projectTasksQuery.data?.board_summary ? (
                      <div className="flex flex-wrap gap-2">
                        {Object.entries(projectTasksQuery.data.board_summary).map(
                          ([lane, count]) => (
                            <Badge key={lane} tone="ghost">
                              {lane}: {String(count)}
                            </Badge>
                          )
                        )}
                      </div>
                    ) : null}
                    {String(projectTasksQuery.data?.warning || "").trim() ? (
                      <div className="rounded-2xl border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-100">
                        {String(projectTasksQuery.data.warning)}
                      </div>
                    ) : null}
                  </div>
                ) : (
                  <EmptyState text="No task preview available yet." />
                )
              ) : (
                <EmptyState text="Select a project to preview task intake." />
              )}
            </PanelCard>
          </div>
        </>
      ) : null}

      {tab === "board" ? (
        <div className="grid gap-4">
          <div className="grid gap-4">
            <PanelCard title="GitHub Project board" subtitle="Live project columns from GitHub MCP">
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
              {String(selectedProject?.taskSource?.type || "").trim() === "github_project" ? (
                projectBoardQuery.isLoading ? (
                  <div className="tcp-subtle text-sm">Loading GitHub Project board...</div>
                ) : projectBoardQuery.isError ? (
                  <div className="rounded-2xl border border-red-500/20 bg-red-500/10 p-4 text-sm text-red-200">
                    {projectBoardQuery.error instanceof Error
                      ? projectBoardQuery.error.message
                      : "Could not load the GitHub Project board."}
                  </div>
                ) : (
                  <div className="grid gap-3">
                    <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-white/10 bg-black/20 p-3">
                      <div className="tcp-subtle text-xs">
                        This board shows exact GitHub Project column and item names.
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
                        <button
                          type="button"
                          className="tcp-btn tcp-btn-secondary"
                          onClick={refreshGithubBoard}
                          disabled={projectBoardQuery.isFetching}
                        >
                          {projectBoardQuery.isFetching ? "Refreshing..." : "Refresh from GitHub"}
                        </button>
                      </div>
                    </div>
                    <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-cyan-500/20 bg-cyan-500/10 p-3">
                      <div className="tcp-subtle text-xs">
                        Select GitHub items here to start ACA runs directly from the board. This
                        creates one ACA run per selected item and starts them immediately.
                      </div>
                      <div className="flex flex-wrap gap-2">
                        <button
                          type="button"
                          className="tcp-btn tcp-btn-secondary"
                          onClick={selectAllActionableGithubItems}
                          disabled={!actionableGithubItems.length || batchTriggering}
                        >
                          Select actionable
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
                    {githubBoard.columns.length ? (
                      <div className="flex flex-wrap gap-2">
                        {githubBoard.columns.map((column: any) => {
                          const hidden = hiddenGithubColumns.includes(String(column.key || ""));
                          return (
                            <button
                              key={String(column.key || column.id)}
                              type="button"
                              className={`rounded-full border px-3 py-1 text-xs transition ${
                                hidden
                                  ? "border-white/10 bg-black/20 text-slate-400"
                                  : "border-cyan-400/40 bg-cyan-500/10 text-cyan-100"
                              }`}
                              onClick={() => toggleGithubColumn(String(column.key || ""))}
                            >
                              {hidden ? "Show" : "Hide"} {String(column.name || "")}
                            </button>
                          );
                        })}
                      </div>
                    ) : null}
                    {githubBoardVisibleColumns.length ? (
                      <div className="grid items-start gap-4 xl:grid-cols-2 2xl:grid-cols-3">
                        {githubBoardVisibleColumns.map((column: any) => {
                          const items = githubBoard.items.filter(
                            (item: any) => String(item.statusKey || "") === String(column.key || "")
                          );
                          return (
                            <div key={String(column.key || column.id)} className="grid gap-2">
                              <div className="flex items-center justify-between gap-3">
                                <div>
                                  <div className="text-sm font-semibold">
                                    {String(column.name || "")}
                                  </div>
                                  <div className="tcp-subtle text-xs">GitHub Project column</div>
                                </div>
                                <Badge
                                  tone={items.some((item: any) => item.actionable) ? "ok" : "ghost"}
                                >
                                  {items.length}
                                </Badge>
                              </div>
                              {items.length ? (
                                <div className="grid max-h-[38rem] content-start gap-3 overflow-y-auto pr-1">
                                  {items.map((item: any) => (
                                    <div
                                      key={String(item.id || "")}
                                      className={`rounded-2xl border px-3 py-3 text-left transition ${
                                        selectedGithubItemIds.includes(String(item.id || ""))
                                          ? "border-cyan-400/60 bg-cyan-500/10"
                                          : "border-white/10 bg-black/20 hover:border-white/20"
                                      }`}
                                    >
                                      {(() => {
                                        const itemCanRun = githubBoardItemCanRun(item);
                                        const itemId = String(item.id || "");
                                        const itemIsRunning =
                                          itemCanRun &&
                                          activeGithubItemIdentities.has(
                                            githubBoardItemIdentity(item)
                                          );
                                        const itemIsLaunching =
                                          launchingGithubItemIdSet.has(itemId);
                                        const itemIsLaunchLocked =
                                          itemIsRunning || itemIsLaunching || batchTriggering;
                                        return (
                                          <div className="grid gap-3">
                                            <div className="flex items-start gap-3">
                                              <input
                                                type="checkbox"
                                                className="mt-1 h-4 w-4 shrink-0"
                                                checked={selectedGithubItemIds.includes(itemId)}
                                                disabled={!itemCanRun || itemIsLaunchLocked}
                                                onChange={() => toggleGithubItemSelection(itemId)}
                                              />
                                              <div className="min-w-0 flex-1">
                                                <div className="break-words text-sm font-semibold leading-5">
                                                  {String(item.title || "Untitled item")}
                                                </div>
                                                <div className="tcp-subtle mt-1 break-words text-xs leading-5">
                                                  {item.repoName
                                                    ? String(item.repoName)
                                                    : selectedProjectSlug}
                                                  {item.issueNumber
                                                    ? ` #${String(item.issueNumber)}`
                                                    : ""}
                                                </div>
                                              </div>
                                            </div>
                                            <div className="flex flex-wrap items-center gap-2 pl-7">
                                              {item.actionable ? (
                                                <Badge tone="ok">Actionable</Badge>
                                              ) : null}
                                              <Badge tone="ghost">
                                                {String(item.statusName || "Unknown")}
                                              </Badge>
                                              {itemIsRunning ? (
                                                <Badge tone="info">Run active</Badge>
                                              ) : null}
                                              {itemIsLaunching && !itemIsRunning ? (
                                                <Badge tone="warn">Starting</Badge>
                                              ) : null}
                                              {!itemCanRun &&
                                              String(item.statusKey || "").trim() ===
                                                "in_review" ? (
                                                <Badge tone="warn">Review locked</Badge>
                                              ) : null}
                                              {!itemCanRun &&
                                              String(item.statusKey || "").trim() === "done" ? (
                                                <Badge tone="ghost">Done</Badge>
                                              ) : null}
                                            </div>
                                            <div className="flex flex-wrap gap-2 pl-7">
                                              {item.issueUrl ? (
                                                <a
                                                  className="tcp-btn h-8 px-3 text-xs"
                                                  href={item.issueUrl}
                                                  target="_blank"
                                                  rel="noreferrer"
                                                >
                                                  Open in GitHub
                                                </a>
                                              ) : null}
                                              <button
                                                type="button"
                                                className="tcp-btn h-8 px-3 text-xs"
                                                onClick={() => triggerGithubItems([item])}
                                                disabled={!itemCanRun || itemIsLaunchLocked}
                                              >
                                                {!itemCanRun
                                                  ? "Not launchable"
                                                  : itemIsRunning
                                                    ? "Already running"
                                                    : itemIsLaunching
                                                      ? "Starting..."
                                                      : "Run task"}
                                              </button>
                                            </div>
                                          </div>
                                        );
                                      })()}
                                    </div>
                                  ))}
                                </div>
                              ) : (
                                <EmptyState text="No items in this GitHub column." />
                              )}
                            </div>
                          );
                        })}
                      </div>
                    ) : (
                      <EmptyState text="All GitHub columns are hidden for this project view." />
                    )}
                  </div>
                )
              ) : (
                <EmptyState text="The selected project is not backed by a GitHub Project task source." />
              )}
            </PanelCard>

            <PanelCard
              title="ACA execution history"
              subtitle="Run history grouped by ACA lifecycle"
            >
              <div className="grid gap-4 xl:grid-cols-2">
                {lanes.map((lane) => (
                  <div key={lane.id} className="grid gap-2">
                    <div className="flex items-center justify-between gap-3">
                      <div>
                        <div className="text-sm font-semibold">{lane.label}</div>
                        <div className="tcp-subtle text-xs">{lane.hint}</div>
                      </div>
                      <Badge
                        tone={lane.id === "done" ? "ok" : lane.id === "waiting" ? "warn" : "info"}
                      >
                        {lane.items.length}
                      </Badge>
                    </div>
                    {lane.items.length ? (
                      lane.items.map((run: any, index: number) => {
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
                            <div className="flex items-start justify-between gap-3">
                              <div className="min-w-0">
                                <div className="truncate text-sm font-semibold">
                                  {runTitle(run)}
                                </div>
                                <div className="tcp-subtle mt-1 text-xs">
                                  {String(run?.project_slug || "unknown")}
                                  {run?.branch ? ` · ${String(run.branch)}` : ""}
                                </div>
                              </div>
                              <Badge tone={runIsActive(run) ? "info" : "ok"}>
                                {formatStatus(runStatus(run))}
                              </Badge>
                            </div>
                            <div className="mt-2 flex flex-wrap gap-2 text-xs text-slate-300">
                              <span className="rounded-full border border-white/10 px-2 py-1">
                                {id.slice(0, 12)}
                              </span>
                              {runPhase(run) ? (
                                <span className="rounded-full border border-white/10 px-2 py-1">
                                  {formatStatus(runPhase(run))}
                                </span>
                              ) : null}
                            </div>
                          </button>
                        );
                      })
                    ) : (
                      <EmptyState text="No runs in this lane yet." />
                    )}
                  </div>
                ))}
              </div>
            </PanelCard>
            <PanelCard
              title="Run detail"
              subtitle={selectedRunId ? `ACA detail for ${selectedRunId}` : "Select a run"}
              actions={
                <button
                  type="button"
                  className="tcp-btn h-8 px-3 text-xs"
                  onClick={() => setRunDetailOpen((prev) => !prev)}
                >
                  <i data-lucide={runDetailOpen ? "chevron-down" : "chevron-right"}></i>
                  {runDetailOpen ? "Collapse" : "Expand"}
                </button>
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
                          {runDetailQuery.data?.error ? <Badge tone="warn">Has error</Badge> : null}
                        </div>
                      </div>

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
                        <pre className="max-h-72 overflow-auto whitespace-pre-wrap text-xs leading-6 text-slate-200">
                          {JSON.stringify(blackboard || {}, null, 2)}
                        </pre>
                      </div>
                    </div>
                  )
                ) : (
                  <EmptyState text="Select a run from the board to inspect its status, summary, and blackboard." />
                )
              ) : null}
            </PanelCard>

            <PanelCard
              title="Live logs"
              subtitle="Tail ACA worker and manager logs"
              actions={
                <button
                  type="button"
                  className="tcp-btn h-8 px-3 text-xs"
                  onClick={() => setLiveLogsOpen((prev) => !prev)}
                >
                  <i data-lucide={liveLogsOpen ? "chevron-down" : "chevron-right"}></i>
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
                      <div className="tcp-subtle text-sm">No logs available yet.</div>
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
            <PanelCard
              title="Register project"
              subtitle="Bind a repository, managed checkout, and task source into ACA"
            >
              <div className="grid gap-3">
                <input
                  className="tcp-input"
                  placeholder="Project slug, e.g. frumu-ai/tandem"
                  value={newProjectSlug}
                  onInput={(event) => setNewProjectSlug((event.target as HTMLInputElement).value)}
                />
                <input
                  className="tcp-input"
                  placeholder="Project display name (optional)"
                  value={newProjectName}
                  onInput={(event) => setNewProjectName((event.target as HTMLInputElement).value)}
                />
                <input
                  className="tcp-input"
                  placeholder="Repo URL (optional)"
                  value={newRepoUrl}
                  onInput={(event) => setNewRepoUrl((event.target as HTMLInputElement).value)}
                />
                {hostedManaged ? (
                  <>
                    <input
                      className="tcp-input"
                      placeholder="Managed checkout path, e.g. repos/team-alpha"
                      value={newRepoPath}
                      onInput={(event) => setNewRepoPath((event.target as HTMLInputElement).value)}
                    />
                    <input
                      className="tcp-input"
                      placeholder="Worktree root (optional)"
                      value={newWorktreeRoot}
                      onInput={(event) =>
                        setNewWorktreeRoot((event.target as HTMLInputElement).value)
                      }
                    />
                    <div className="grid gap-3 md:grid-cols-2">
                      <input
                        className="tcp-input"
                        placeholder="Default branch (optional)"
                        value={newDefaultBranch}
                        onInput={(event) =>
                          setNewDefaultBranch((event.target as HTMLInputElement).value)
                        }
                      />
                      <input
                        className="tcp-input"
                        placeholder="Remote name (optional)"
                        value={newRemoteName}
                        onInput={(event) =>
                          setNewRemoteName((event.target as HTMLInputElement).value)
                        }
                      />
                    </div>
                    <input
                      className="tcp-input"
                      placeholder="Token file for private repos (optional)"
                      value={newCredentialFile}
                      onInput={(event) =>
                        setNewCredentialFile((event.target as HTMLInputElement).value)
                      }
                    />
                  </>
                ) : null}
                {hostedManaged ? (
                  <div className="rounded-2xl border border-lime-500/20 bg-lime-500/10 px-3 py-2 text-xs text-lime-100">
                    Hosted installs can use these fields to register named repos and managed
                    checkout directories without exposing an interactive shell.
                  </div>
                ) : null}
                <select
                  className="tcp-input"
                  value={taskSourceType}
                  onChange={(event) =>
                    setTaskSourceType((event.target as HTMLSelectElement).value as TaskSourceType)
                  }
                >
                  <option value="manual">Manual prompt</option>
                  <option value="kanban_board">Kanban board</option>
                  <option value="local_backlog">Local backlog</option>
                  <option value="github_project">GitHub Project</option>
                </select>
                {taskSourceType === "manual" ? (
                  <textarea
                    className="tcp-input min-h-[120px]"
                    placeholder="Manual task prompt"
                    value={taskSourcePrompt}
                    onInput={(event) =>
                      setTaskSourcePrompt((event.target as HTMLTextAreaElement).value)
                    }
                  />
                ) : null}
                {taskSourceType === "kanban_board" || taskSourceType === "local_backlog" ? (
                  <input
                    className="tcp-input"
                    placeholder="Absolute file path"
                    value={taskSourcePath}
                    onInput={(event) => setTaskSourcePath((event.target as HTMLInputElement).value)}
                  />
                ) : null}
                {taskSourceType === "github_project" ? (
                  <>
                    <input
                      className="tcp-input"
                      placeholder="GitHub owner"
                      value={taskSourceOwner}
                      onInput={(event) =>
                        setTaskSourceOwner((event.target as HTMLInputElement).value)
                      }
                    />
                    <input
                      className="tcp-input"
                      placeholder="Repository name"
                      value={taskSourceRepo}
                      onInput={(event) =>
                        setTaskSourceRepo((event.target as HTMLInputElement).value)
                      }
                    />
                    <input
                      className="tcp-input"
                      placeholder="Project number"
                      value={taskSourceProject}
                      onInput={(event) =>
                        setTaskSourceProject((event.target as HTMLInputElement).value)
                      }
                    />
                  </>
                ) : null}
                <button
                  type="button"
                  className="tcp-btn"
                  disabled={registering}
                  onClick={registerProject}
                >
                  {registering ? "Registering..." : "Register Project"}
                </button>
              </div>
            </PanelCard>

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
                <input
                  className="tcp-input"
                  placeholder="Override provider (optional)"
                  value={overrideProvider}
                  onInput={(event) => setOverrideProvider((event.target as HTMLInputElement).value)}
                />
                <input
                  className="tcp-input"
                  placeholder="Override model (optional)"
                  value={overrideModel}
                  onInput={(event) => setOverrideModel((event.target as HTMLInputElement).value)}
                />
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
