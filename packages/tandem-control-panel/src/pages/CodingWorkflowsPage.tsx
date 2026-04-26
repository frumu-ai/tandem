import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatedPage, Badge, PanelCard, StatusPulse } from "../ui/index.tsx";
import { EmptyState } from "./ui";
import { useCapabilities } from "../features/system/queries.ts";
import { subscribeSse } from "../services/sse.js";
import { CodingWorkflowsOverviewTab } from "./CodingWorkflowsOverviewTab";
import { TaskPlanningPanel } from "./TaskPlanningPanel";
import { ProviderModelSelector } from "../components/ProviderModelSelector";
import {
  buildPlannerProviderOptions,
  type PlannerProviderOption,
} from "../features/planner/plannerShared";
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
    items,
    source: String(raw?.source || "").trim(),
    warning: String(raw?.warning || "").trim(),
    isStale: raw?.is_stale === true,
    lastSyncedAtMs: Number(raw?.last_synced_at_ms || raw?.lastSyncedAtMs || 0),
  };
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
  // Empty run overrides are intentional: ACA should inherit its configured base provider/model.
  const [overrideProvider, setOverrideProvider] = useState("");
  const [overrideModel, setOverrideModel] = useState("");
  const [registering, setRegistering] = useState(false);
  const [triggering, setTriggering] = useState(false);
  const [lastGlobalEvent, setLastGlobalEvent] = useState("");
  const [lastRunEvent, setLastRunEvent] = useState("");
  const [taskPreviewRefreshAt, setTaskPreviewRefreshAt] = useState<number | null>(null);
  const [githubBoardRefreshAt, setGithubBoardRefreshAt] = useState<number | null>(null);
  const [runDetailOpen, setRunDetailOpen] = useState(false);
  const [liveLogsOpen, setLiveLogsOpen] = useState(false);
  const [selectedGithubItemIds, setSelectedGithubItemIds] = useState<string[]>([]);
  const [launchingGithubItemIds, setLaunchingGithubItemIds] = useState<Record<string, number>>({});
  const [batchTriggering, setBatchTriggering] = useState(false);

  const caps = useCapabilities();
  const acaAvailable = caps.data?.aca_integration === true;
  const engineAvailable = caps.data?.engine_healthy === true;
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
  const githubBoard = useMemo(
    () => normalizeGithubBoard(projectBoardQuery.data),
    [projectBoardQuery.data]
  );
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
  const inheritedAcaModelLabel =
    providerStatus.defaultProvider && providerStatus.defaultModel
      ? `ACA default (${providerStatus.defaultProvider} / ${providerStatus.defaultModel})`
      : "ACA default";
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
        Leave blank to inherit the base ACA provider and model for this run.
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
    { id: "board", label: "Intake", icon: "list-checks" },
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
      toast("ok", "Refreshed GitHub Project items through Tandem MCP.");
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
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
        <CodingWorkflowsOverviewTab
          projects={projects}
          selectedProjectSlug={selectedProjectSlug}
          setSelectedProjectSlug={setSelectedProjectSlug}
          selectedProject={selectedProject}
          acaOverview={acaOverview}
          projectTasksQuery={projectTasksQuery}
          refreshTaskPreview={refreshTaskPreview}
          taskPreviewRefreshAt={taskPreviewRefreshAt}
          visibleRunsCount={filteredRuns.length}
          activeRunsCount={activeRuns.length}
          connectedMcpServersCount={mcpServers.length}
          registeredToolsCount={mcpTools.length}
        />
      ) : null}

      {tab === "board" ? (
        <div className="grid gap-4">
          <div className="grid gap-4">
            <PanelCard title="GitHub Project intake" subtitle="GitHub items ACA can launch">
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
                  <div className="tcp-subtle text-sm">Loading GitHub Project items...</div>
                ) : projectBoardQuery.isError ? (
                  <div className="rounded-2xl border border-red-500/20 bg-red-500/10 p-4 text-sm text-red-200">
                    {projectBoardQuery.error instanceof Error
                      ? projectBoardQuery.error.message
                      : "Could not load the GitHub Project items."}
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
                      <div className="min-w-[320px] flex-1">
                        {renderAcaModelSelector(batchTriggering)}
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
                    {githubBoard.items.length ? (
                      <div className="grid gap-3">
                        {githubBoard.items.map((item: any) => {
                          const itemCanRun = githubBoardItemCanRun(item);
                          const itemId = String(item.id || "");
                          const itemIsRunning =
                            itemCanRun &&
                            activeGithubItemIdentities.has(githubBoardItemIdentity(item));
                          const itemIsLaunching = launchingGithubItemIdSet.has(itemId);
                          const itemIsLaunchLocked =
                            itemIsRunning || itemIsLaunching || batchTriggering;
                          return (
                            <div
                              key={itemId}
                              className={`rounded-2xl border px-4 py-3 transition ${
                                selectedGithubItemIds.includes(itemId)
                                  ? "border-cyan-400/60 bg-cyan-500/10"
                                  : "border-white/10 bg-black/20"
                              }`}
                            >
                              <div className="flex flex-wrap items-start justify-between gap-3">
                                <div className="flex min-w-0 flex-1 items-start gap-3">
                                  <input
                                    type="checkbox"
                                    className="mt-1 h-4 w-4 shrink-0"
                                    checked={selectedGithubItemIds.includes(itemId)}
                                    disabled={!itemCanRun || itemIsLaunchLocked}
                                    onChange={() => toggleGithubItemSelection(itemId)}
                                  />
                                  <div className="min-w-0">
                                    <div className="break-words text-sm font-semibold leading-5">
                                      {String(item.title || "Untitled item")}
                                    </div>
                                    <div className="tcp-subtle mt-1 break-words text-xs leading-5">
                                      {item.repoName ? String(item.repoName) : selectedProjectSlug}
                                      {item.issueNumber ? ` #${String(item.issueNumber)}` : ""}
                                    </div>
                                  </div>
                                </div>
                                <div className="flex shrink-0 flex-wrap items-center gap-2">
                                  {item.actionable ? <Badge tone="ok">Actionable</Badge> : null}
                                  <Badge tone="ghost">
                                    {formatStatus(
                                      String(item.statusName || item.statusKey || "Unknown")
                                    )}
                                  </Badge>
                                  {itemIsRunning ? <Badge tone="info">Run active</Badge> : null}
                                  {itemIsLaunching && !itemIsRunning ? (
                                    <Badge tone="warn">Starting</Badge>
                                  ) : null}
                                </div>
                              </div>
                              <div className="mt-3 flex flex-wrap gap-2 pl-7">
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
                        })}
                      </div>
                    ) : (
                      <EmptyState text="No GitHub Project items returned for this project." />
                    )}
                  </div>
                )
              ) : (
                <EmptyState text="The selected project is not backed by a GitHub Project task source." />
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
