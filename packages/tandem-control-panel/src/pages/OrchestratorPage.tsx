import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { renderIcons } from "../app/icons.js";
import { normalizeMessages } from "../features/chat/messages";
import { saveStoredSessionId } from "../features/chat/session";
import { renderMarkdownSafe } from "../lib/markdown";
import { BudgetMeter } from "../features/orchestration/BudgetMeter";
import { TaskBoard } from "../features/orchestration/TaskBoard";
import type { BudgetUsage, OrchestrationTask, TaskState } from "../features/orchestration/types";
import { useRunRegistry } from "../features/orchestrator/runRegistry";
import {
  buildCursorToken,
  useOrchestratorEvents,
} from "../features/orchestrator/useOrchestratorEvents";
import { EmptyState, PageCard } from "./ui";
import type { AppPageProps } from "./pageTypes";

const DEFAULT_BUDGET: BudgetUsage = {
  max_iterations: 500,
  iterations_used: 0,
  max_tokens: 400000,
  tokens_used: 0,
  max_wall_time_secs: 3600,
  wall_time_secs: 0,
  max_subagent_runs: 2000,
  subagent_runs_used: 0,
  exceeded: false,
  exceeded_reason: "",
};

function normalizeTaskState(status: string): TaskState {
  const value = String(status || "")
    .trim()
    .toLowerCase();
  if (value === "in_progress" || value === "running") return "in_progress";
  if (value === "done" || value === "completed") return "done";
  if (value === "failed" || value === "error" || value === "cancelled" || value === "canceled")
    return "failed";
  if (value === "blocked") return "blocked";
  if (value === "runnable") return "runnable";
  return "pending";
}

function statusBadgeClass(status: string) {
  const s = String(status || "")
    .trim()
    .toLowerCase();
  if (s === "done" || s === "completed" || s === "active") return "tcp-badge-ok";
  if (s === "failed" || s === "error" || s === "cancelled" || s === "canceled")
    return "tcp-badge-err";
  if (s === "running" || s === "in_progress" || s === "runnable") return "tcp-badge-warn";
  return "tcp-badge-info";
}

function runLabelFromTimestamp(ts: unknown) {
  const ms = Number(ts || 0);
  if (!Number.isFinite(ms) || ms <= 0) return "Run";
  return `Run ${new Date(ms).toLocaleTimeString()}`;
}

function runTimestamp(run: any) {
  return Number(run?.updated_at_ms || run?.created_at_ms || 0);
}

function eventTimeLabel(ts: unknown) {
  const ms = Number(ts || 0);
  if (!Number.isFinite(ms) || ms <= 0) return "--:--:--";
  return new Date(ms).toLocaleTimeString();
}

function formatFileBytes(value: unknown) {
  const size = Number(value || 0);
  if (!Number.isFinite(size) || size <= 0) return "0 B";
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}

function fileExtension(path: string) {
  const clean = String(path || "").trim();
  const idx = clean.lastIndexOf(".");
  if (idx < 0) return "";
  return clean.slice(idx + 1).toLowerCase();
}

function normalizeWorkspacePath(path: string) {
  const value = String(path || "")
    .trim()
    .replace(/\\/g, "/");
  if (!value) return "";
  if (!value.startsWith("/")) return value;
  return value.replace(/\/{2,}/g, "/");
}

function toWorkspaceAbsolutePath(root: string, relativePath: string) {
  const workspaceRoot = normalizeWorkspacePath(root).replace(/\/+$/, "");
  const rel = normalizeWorkspacePath(relativePath).replace(/^\/+/, "");
  if (!workspaceRoot) return rel;
  if (!rel) return workspaceRoot;
  return `${workspaceRoot}/${rel}`;
}

function pathIsInside(root: string, target: string) {
  const parent = normalizeWorkspacePath(root).replace(/\/+$/, "");
  const child = normalizeWorkspacePath(target);
  if (!parent || !child) return false;
  if (child === parent) return true;
  return child.startsWith(`${parent}/`);
}

function extractWorkspacePath(input: any) {
  const candidates = [
    input?.workspace?.canonical_path,
    input?.workspace?.path,
    input?.workspace?.root,
    input?.workspace_root,
    input?.workspacePath,
    input?.cwd,
    input?.path,
  ];
  for (const value of candidates) {
    const text = normalizeWorkspacePath(String(value || ""));
    if (text.startsWith("/")) return text;
  }
  return "";
}

function buildLatestAttemptSeqByTask(events: any[]) {
  const latest: Record<string, number> = {};
  for (const evt of events) {
    const type = String(evt?.type || "")
      .trim()
      .toLowerCase();
    if (!["task_started", "step_started", "task_completed", "step_completed"].includes(type))
      continue;
    const taskId = String(evt?.step_id || "").trim();
    if (!taskId) continue;
    const seq = Number(evt?.seq || 0);
    if (!Number.isFinite(seq) || seq <= 0) continue;
    latest[taskId] = Math.max(Number(latest[taskId] || 0), seq);
  }
  return latest;
}

function normalizeTasks(payload: any): OrchestrationTask[] {
  const blackboardTasks = Array.isArray(payload?.blackboard?.tasks) ? payload.blackboard.tasks : [];
  if (blackboardTasks.length) {
    return blackboardTasks.map((task: any, index: number) => {
      const state = normalizeTaskState(String(task?.status || "pending"));
      return {
        id: String(task?.id || `task-${index}`),
        title: String(task?.payload?.title || task?.task_type || task?.id || `Task ${index + 1}`),
        description: String(task?.payload?.description || ""),
        dependencies: Array.isArray(task?.depends_on_task_ids)
          ? task.depends_on_task_ids.map((dep: unknown) => String(dep || "")).filter(Boolean)
          : [],
        state,
        retry_count: Number(task?.retry_count || 0),
        error_message:
          state === "failed" || state === "blocked" ? String(task?.last_error || "") : "",
        runtime_status: "",
        runtime_detail: "",
        assigned_role: String(task?.assigned_agent || task?.lease_owner || ""),
        workflow_id: String(task?.workflow_id || ""),
        session_id: "",
      };
    });
  }
  const steps = Array.isArray(payload?.tasks) ? payload.tasks : [];
  return steps.map((step: any, index: number) => {
    const state = normalizeTaskState(String(step?.stepStatus || step?.status || "pending"));
    return {
      id: String(step?.taskId || step?.step_id || `step-${index}`),
      title: String(step?.title || step?.step_id || `Step ${index + 1}`),
      description: String(step?.description || ""),
      dependencies: Array.isArray(step?.dependsOn)
        ? step.dependsOn.map((dep: unknown) => String(dep || "")).filter(Boolean)
        : [],
      state,
      retry_count: Number(step?.retry_count || 0),
      error_message:
        state === "failed" || state === "blocked" ? String(step?.error_message || "") : "",
      runtime_status: String(step?.runtime_status || ""),
      runtime_detail: String(step?.runtime_detail || ""),
      assigned_role: String(step?.assignedAgent || ""),
      workflow_id: String(step?.workflowId || ""),
      session_id: String(step?.sessionId || step?.session_id || ""),
    };
  });
}

export function OrchestratorPage({ api, toast, navigate }: AppPageProps) {
  const queryClient = useQueryClient();
  const reducedMotion = !!useReducedMotion();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [composeMode, setComposeMode] = useState(true);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [prompt, setPrompt] = useState("");
  const [workspaceRoot, setWorkspaceRoot] = useState("");
  const [workspaceBrowserOpen, setWorkspaceBrowserOpen] = useState(false);
  const [workspaceBrowserDir, setWorkspaceBrowserDir] = useState("");
  const [workspaceBrowserSearch, setWorkspaceBrowserSearch] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [maxTasks, setMaxTasks] = useState("4");
  const [maxAgents, setMaxAgents] = useState("3");
  const [workflowId, setWorkflowId] = useState("swarm.blackboard.default");
  const [revisionFeedback, setRevisionFeedback] = useState("");
  const [runWorkspaceDir, setRunWorkspaceDir] = useState("");
  const [selectedWorkspaceFile, setSelectedWorkspaceFile] = useState("");
  useEffect(() => {
    setComposeMode(true);
    clearSelectedRunId();
  }, []);

  const statusQuery = useQuery({
    queryKey: ["swarm", "status"],
    queryFn: () => api("/api/orchestrator/status"),
    refetchInterval: 5000,
  });

  const runsQuery = useQuery({
    queryKey: ["swarm", "runs", workspaceRoot],
    queryFn: () =>
      api(`/api/orchestrator/runs?workspace=${encodeURIComponent(workspaceRoot || "")}`),
    refetchInterval: 6000,
    enabled: !!statusQuery.data,
  });

  const runs = Array.isArray(runsQuery.data?.runs) ? runsQuery.data.runs : [];
  const runRegistry = useRunRegistry(runs, String(statusQuery.data?.runId || "").trim());
  const selectedRunId = runRegistry.selectedRunId;
  const setSelectedRunId = runRegistry.setSelectedRunId;
  const clearSelectedRunId = runRegistry.clearSelectedRunId;
  const advanceCursor = runRegistry.advanceCursor;
  const runId = composeMode ? "" : String(selectedRunId || "").trim();
  const orderedRuns = runRegistry.orderedRuns;
  const selectedRunEntry = useMemo(() => {
    if (!runId) return null;
    for (const row of orderedRuns) {
      const id = String(row?.run_id || row?.runId || "").trim();
      if (id === runId) return row;
    }
    return null;
  }, [orderedRuns, runId]);
  const cursorToken = useMemo(
    () => buildCursorToken(runRegistry.cursorsByRunId),
    [runRegistry.cursorsByRunId]
  );
  const streamWorkspace = String(workspaceRoot || statusQuery.data?.workspaceRoot || "").trim();
  const subscriptionRunIds = useMemo(() => {
    const id = String(runId || "").trim();
    return id ? [id] : [];
  }, [runId]);
  const lastInvalidateAt = useRef(0);
  const onStreamEnvelope = useCallback(
    (envelope: any) => {
      const kind = String(envelope?.kind || "")
        .trim()
        .toLowerCase();
      const eventRunId = String(envelope?.run_id || envelope?.runId || "").trim();
      if (runId && eventRunId && eventRunId !== runId) return;
      const seq = Number(envelope?.seq || 0);
      if (eventRunId && seq > 0 && (kind === "context_run_event" || kind === "blackboard_patch")) {
        advanceCursor(eventRunId, kind, seq);
      }
      const now = Date.now();
      if (now - lastInvalidateAt.current < 900) return;
      lastInvalidateAt.current = now;
      void queryClient.invalidateQueries({ queryKey: ["swarm", "runs"] });
      if (runId) void queryClient.invalidateQueries({ queryKey: ["swarm", "run", runId] });
    },
    [advanceCursor, queryClient, runId]
  );
  useOrchestratorEvents({
    workspace: streamWorkspace,
    runIds: subscriptionRunIds,
    cursorToken,
    onEnvelope: onStreamEnvelope,
  });

  const runQuery = useQuery({
    queryKey: ["swarm", "run", runId],
    queryFn: () => api(`/api/orchestrator/run/${encodeURIComponent(runId)}`),
    refetchInterval: 4000,
    enabled: !!runId,
  });
  const workspaceBrowserQuery = useQuery({
    queryKey: ["swarm", "workspace-browser", workspaceBrowserDir],
    enabled: workspaceBrowserOpen && !!workspaceBrowserDir,
    queryFn: () =>
      api(`/api/orchestrator/workspaces/list?dir=${encodeURIComponent(workspaceBrowserDir)}`),
  });

  const runStatus = String(
    runQuery.data?.runStatus || runQuery.data?.run?.status || statusQuery.data?.status || "idle"
  )
    .trim()
    .toLowerCase();
  const tasks = useMemo(() => normalizeTasks(runQuery.data), [runQuery.data]);
  const budget = useMemo(
    () => ({ ...DEFAULT_BUDGET, ...(runQuery.data?.budget || {}) }),
    [runQuery.data?.budget]
  );
  const activeWorkspaceRoot = String(
    extractWorkspacePath(runQuery.data?.run) ||
      extractWorkspacePath(selectedRunEntry) ||
      (!runId ? String(statusQuery.data?.workspaceRoot || workspaceRoot || "") : "")
  ).trim();
  useEffect(() => {
    if (!activeWorkspaceRoot) return;
    setRunWorkspaceDir((prev) => {
      if (!prev) return activeWorkspaceRoot;
      if (!pathIsInside(activeWorkspaceRoot, prev)) return activeWorkspaceRoot;
      return prev;
    });
    setSelectedWorkspaceFile((prev) => (pathIsInside(activeWorkspaceRoot, prev) ? prev : ""));
  }, [activeWorkspaceRoot]);

  const workspaceDirectories = Array.isArray(workspaceBrowserQuery.data?.directories)
    ? workspaceBrowserQuery.data.directories
    : [];
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
  const workspaceParentDir = String(workspaceBrowserQuery.data?.parent || "").trim();
  const workspaceCurrentBrowseDir = String(
    workspaceBrowserQuery.data?.dir || workspaceBrowserDir || ""
  ).trim();
  const runWorkspaceQuery = useQuery({
    queryKey: ["swarm", "workspace-files", activeWorkspaceRoot, runWorkspaceDir],
    enabled: !!activeWorkspaceRoot && !!runWorkspaceDir,
    queryFn: async () => {
      const payload = await api(
        `/api/orchestrator/workspaces/files?workspaceRoot=${encodeURIComponent(activeWorkspaceRoot)}&dir=${encodeURIComponent(runWorkspaceDir)}`
      ).catch(() => ({
        directories: [],
        files: [],
        parent: null,
        dir: runWorkspaceDir,
      }));
      const directories = Array.isArray(payload?.directories) ? payload.directories : [];
      const filesRaw = Array.isArray(payload?.files) ? payload.files : [];
      const files = filesRaw.map((file: any) => {
        const resolvedPath = String(file?.path || "").trim();
        return {
          ...file,
          name: String(file?.name || resolvedPath || "file"),
          path:
            resolvedPath || toWorkspaceAbsolutePath(activeWorkspaceRoot, String(file?.name || "")),
        };
      });
      return {
        ok: true,
        dir: String(payload?.dir || runWorkspaceDir),
        parent: payload?.parent || null,
        directories,
        files,
        fileAccessAllowed: true,
      };
    },
    refetchInterval: 10000,
  });
  const runWorkspaceReadQuery = useQuery({
    queryKey: ["swarm", "workspace-file", activeWorkspaceRoot, selectedWorkspaceFile],
    enabled: !!activeWorkspaceRoot && !!selectedWorkspaceFile,
    queryFn: async () => {
      return api(
        `/api/orchestrator/workspaces/read?workspaceRoot=${encodeURIComponent(activeWorkspaceRoot)}&path=${encodeURIComponent(selectedWorkspaceFile)}`
      );
    },
  });
  const runWorkspaceDirectories = Array.isArray(runWorkspaceQuery.data?.directories)
    ? runWorkspaceQuery.data.directories
    : [];
  const runWorkspaceFiles = Array.isArray(runWorkspaceQuery.data?.files)
    ? runWorkspaceQuery.data.files
    : [];
  const runWorkspaceParent = String(runWorkspaceQuery.data?.parent || "").trim();
  const runWorkspaceFileAccessAllowed = runWorkspaceQuery.data?.fileAccessAllowed !== false;
  const selectedWorkspaceText = String(runWorkspaceReadQuery.data?.text || "");
  const selectedWorkspaceExt = fileExtension(selectedWorkspaceFile);
  const selectedIsMarkdown = ["md", "markdown", "mdx"].includes(selectedWorkspaceExt);
  const selectedIsHtml = ["html", "htm"].includes(selectedWorkspaceExt);
  const taskRenderSignature = useMemo(
    () => tasks.map((task) => `${task.id}:${task.state}:${task.error_message || ""}`).join("|"),
    [tasks]
  );

  const latestOutput = useMemo(() => {
    const events = Array.isArray(runQuery.data?.events) ? runQuery.data.events : [];
    let latest: any = null;
    let latestTs = 0;
    for (const evt of events) {
      const type = String(evt?.type || "")
        .trim()
        .toLowerCase();
      if (!["step_completed", "task_completed"].includes(type)) continue;
      const payload = evt?.payload && typeof evt.payload === "object" ? evt.payload : {};
      const sessionId = String(payload?.session_id || "").trim();
      if (!sessionId) continue;
      const ts = Number(evt?.ts_ms || 0);
      if (!latest || ts >= latestTs) {
        latest = { sessionId, event: evt };
        latestTs = ts;
      }
    }
    return latest;
  }, [runQuery.data?.events]);
  const liveSessionId = useMemo(() => {
    const events = Array.isArray(runQuery.data?.events) ? runQuery.data.events : [];
    for (let i = events.length - 1; i >= 0; i -= 1) {
      const evt = events[i];
      const type = String(evt?.type || "")
        .trim()
        .toLowerCase();
      if (!["task_started", "step_started", "task_completed", "step_completed"].includes(type))
        continue;
      const payload = evt?.payload && typeof evt.payload === "object" ? evt.payload : {};
      const sessionId = String(payload?.session_id || "").trim();
      if (sessionId) return sessionId;
    }
    return String(latestOutput?.sessionId || "").trim();
  }, [latestOutput?.sessionId, runQuery.data?.events]);
  const activityEvents = useMemo(() => {
    const events = Array.isArray(runQuery.data?.events) ? runQuery.data.events : [];
    const latestAttemptSeqByTask = buildLatestAttemptSeqByTask(events);
    const rows: Array<{
      id: string;
      type: string;
      title: string;
      detail: string;
      at: number;
    }> = [];
    for (let i = events.length - 1; i >= 0; i -= 1) {
      const evt = events[i];
      const type = String(evt?.type || "")
        .trim()
        .toLowerCase();
      if (
        ![
          "task_started",
          "task_completed",
          "task_failed",
          "step_started",
          "step_completed",
          "step_failed",
          "run_resumed",
          "run_paused",
          "run_completed",
          "run_failed",
        ].includes(type)
      ) {
        continue;
      }
      const payload = evt?.payload && typeof evt.payload === "object" ? evt.payload : {};
      const taskId = String(evt?.step_id || "").trim();
      const seq = Number(evt?.seq || 0);
      const isFailure = type === "task_failed" || type === "step_failed";
      if (
        isFailure &&
        taskId &&
        Number.isFinite(seq) &&
        seq > 0 &&
        Number(latestAttemptSeqByTask[taskId] || 0) > seq
      ) {
        continue;
      }
      const title = String(payload?.step_title || evt?.step_id || type).trim();
      const detail = String(payload?.why_next_step || payload?.error || "").trim();
      rows.push({
        id: `${String(evt?.seq || i)}-${type}`,
        type,
        title: title || type,
        detail,
        at: Number(evt?.ts_ms || 0),
      });
      if (rows.length >= 12) break;
    }
    return rows;
  }, [runQuery.data?.events]);
  const planSource = useMemo(() => {
    const events = Array.isArray(runQuery.data?.events) ? runQuery.data.events : [];
    for (let i = events.length - 1; i >= 0; i -= 1) {
      const row = events[i];
      const type = String(row?.type || "")
        .trim()
        .toLowerCase();
      if (type === "plan_seeded_llm") return "llm";
      if (type === "plan_seeded_local") return "fallback_local";
      if (type === "plan_failed_llm_required") return "llm_failed";
    }
    return "unknown";
  }, [runQuery.data?.events]);

  const outputSessionQuery = useQuery({
    queryKey: ["swarm", "run-output-session", String(liveSessionId || "")],
    queryFn: () => api(`/api/engine/session/${encodeURIComponent(String(liveSessionId || ""))}`),
    refetchInterval: 6000,
    enabled: !!liveSessionId,
  });

  const latestAssistantOutput = useMemo(() => {
    const messages = normalizeMessages(outputSessionQuery.data, "Assistant");
    for (let i = messages.length - 1; i >= 0; i -= 1) {
      if (messages[i]?.role === "assistant" && String(messages[i]?.text || "").trim())
        return String(messages[i]?.text || "").trim();
    }
    return "";
  }, [outputSessionQuery.data]);
  const recentToolActivity = useMemo(() => {
    const source = Array.isArray(outputSessionQuery.data)
      ? outputSessionQuery.data
      : Array.isArray(outputSessionQuery.data?.messages)
        ? outputSessionQuery.data.messages
        : [];
    const rows: string[] = [];
    for (let i = source.length - 1; i >= 0; i -= 1) {
      const message = source[i];
      const parts = Array.isArray(message?.parts) ? message.parts : [];
      for (let j = parts.length - 1; j >= 0; j -= 1) {
        const part = parts[j];
        const type = String(part?.type || part?.part_type || "")
          .trim()
          .toLowerCase();
        if (!type.includes("tool")) continue;
        const tool = String(part?.tool || part?.name || "").trim();
        const state = String(part?.state || part?.status || "").trim();
        const error = String(part?.error || "").trim();
        const label = [tool || "tool", state || null, error ? `err=${error}` : null]
          .filter(Boolean)
          .join(" · ");
        rows.push(label);
        if (rows.length >= 10) return rows;
      }
    }
    return rows;
  }, [outputSessionQuery.data]);

  const verificationEvents = useMemo(() => {
    const events = Array.isArray(runQuery.data?.events) ? runQuery.data.events : [];
    const latestAttemptSeqByTask = buildLatestAttemptSeqByTask(events);
    const rows: Array<{
      id: string;
      taskId: string;
      title: string;
      type: string;
      reason: string;
      mode: string;
      at: number;
    }> = [];
    for (let i = events.length - 1; i >= 0; i -= 1) {
      const evt = events[i];
      const type = String(evt?.type || "")
        .trim()
        .toLowerCase();
      if (!["task_failed", "step_failed", "task_completed", "step_completed"].includes(type))
        continue;
      const payload = evt?.payload && typeof evt.payload === "object" ? evt.payload : {};
      const taskId = String(evt?.step_id || "").trim();
      const seq = Number(evt?.seq || 0);
      const isFailure = type === "task_failed" || type === "step_failed";
      if (
        isFailure &&
        taskId &&
        Number.isFinite(seq) &&
        seq > 0 &&
        Number(latestAttemptSeqByTask[taskId] || 0) > seq
      ) {
        continue;
      }
      const verification =
        payload?.verification && typeof payload.verification === "object"
          ? payload.verification
          : {};
      const reason = String(verification?.reason || payload?.error || "").trim();
      const mode = String(verification?.mode || "strict").trim() || "strict";
      rows.push({
        id: `${String(evt?.seq || i)}-${type}`,
        taskId,
        title: String(payload?.step_title || evt?.step_id || type),
        type,
        reason,
        mode,
        at: Number(evt?.ts_ms || 0),
      });
      if (rows.length >= 8) break;
    }
    return rows;
  }, [runQuery.data?.events]);

  const startMutation = useMutation({
    mutationFn: () => {
      const objective = String(prompt || "").trim();
      const root = String(workspaceRoot || "").trim();
      if (!objective) throw new Error("Enter a prompt first.");
      if (!root) throw new Error("Set workspace path first.");
      return api("/api/orchestrator/start", {
        method: "POST",
        body: JSON.stringify({
          objective,
          workspaceRoot: root,
          maxTasks: Number(maxTasks || 4),
          maxAgents: Number(maxAgents || 3),
          workflowId: String(workflowId || "swarm.blackboard.default").trim(),
          requireLlmPlan: true,
          allowLocalPlannerFallback: false,
          verificationMode: "strict",
        }),
      });
    },
    onSuccess: async (payload: any) => {
      const nextRunId = String(payload?.runId || "").trim();
      if (nextRunId) setSelectedRunId(nextRunId);
      setComposeMode(false);
      toast("ok", "Planning started.");
      await queryClient.invalidateQueries({ queryKey: ["swarm"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const actionMutation = useMutation({
    mutationFn: ({ path, body }: { path: string; body: any }) =>
      api(path, { method: "POST", body: JSON.stringify(body) }),
    onSuccess: async (payload: any, vars) => {
      if (vars.path === "/api/orchestrator/request_revision") {
        const nextRunId = String(payload?.runId || "").trim();
        if (nextRunId) {
          setSelectedRunId(nextRunId);
          setRevisionFeedback("");
        }
        toast("ok", "Reworked plan created.");
      }
      if (vars.path === "/api/orchestrator/approve") toast("ok", "Execution started.");
      await queryClient.invalidateQueries({ queryKey: ["swarm"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const discardMutation = useMutation({
    mutationFn: async (targetRunId: string) => {
      const id = String(targetRunId || "").trim();
      if (!id) throw new Error("Missing run id.");
      await api("/api/orchestrator/cancel", {
        method: "POST",
        body: JSON.stringify({ runId: id }),
      }).catch(() => null);
      await api("/api/orchestrator/runs/hide", {
        method: "POST",
        body: JSON.stringify({ runIds: [id] }),
      }).catch(() => null);
      return id;
    },
    onSuccess: async () => {
      clearSelectedRunId();
      setComposeMode(true);
      setRevisionFeedback("");
      setPrompt("");
      toast("ok", "Discarded pending plan. You can start a new prompt now.");
      await queryClient.invalidateQueries({ queryKey: ["swarm"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const goToStartView = useCallback(() => {
    setComposeMode(true);
    clearSelectedRunId();
    setHistoryOpen(false);
    setRevisionFeedback("");
  }, [clearSelectedRunId]);
  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    renderIcons(root);
  }, [
    composeMode,
    historyOpen,
    orderedRuns,
    runId,
    runStatus,
    runWorkspaceDir,
    runWorkspaceDirectories.length,
    runWorkspaceFiles.length,
    selectedWorkspaceFile,
    selectedWorkspaceText,
    taskRenderSignature,
  ]);

  const noRunYet = !runId;
  const isPlanning = runStatus === "planning" || runStatus === "queued";
  const isAwaitingApproval = runStatus === "awaiting_approval";
  const isTerminal = ["completed", "failed", "cancelled"].includes(runStatus);
  const canPause = runStatus === "running";
  const canResume = runStatus === "paused";
  const canCancel = [
    "queued",
    "planning",
    "awaiting_approval",
    "running",
    "paused",
    "blocked",
  ].includes(runStatus);
  const historyPanel = (
    <>
      <motion.aside
        className={`chat-sessions-panel ${historyOpen ? "open" : ""}`}
        initial={false}
        animate={
          reducedMotion
            ? { x: historyOpen ? 0 : "-104%" }
            : { x: historyOpen ? 0 : "-104%", transition: { duration: 0.18, ease: "easeOut" } }
        }
      >
        <div className="chat-sessions-header">
          <h3 className="chat-sessions-title">
            <i data-lucide="history"></i>
            History
          </h3>
          <div className="flex items-center gap-1">
            <button
              type="button"
              className="tcp-btn h-8 px-2.5 text-xs"
              onClick={() => {
                void queryClient.invalidateQueries({ queryKey: ["swarm", "runs"] });
              }}
            >
              <i data-lucide="refresh-cw"></i>
            </button>
          </div>
        </div>
        <div className="chat-session-list">
          <AnimatePresence>
            {orderedRuns.map((run: any, index: number) => {
              const id = String(run?.run_id || run?.runId || `run-${index}`);
              const active = id === runId;
              return (
                <motion.div
                  key={id}
                  className="chat-session-row"
                  initial={reducedMotion ? false : { opacity: 0, y: 6 }}
                  animate={reducedMotion ? undefined : { opacity: 1, y: 0 }}
                  exit={reducedMotion ? undefined : { opacity: 0, y: -6 }}
                >
                  <button
                    type="button"
                    className={`chat-session-btn ${active ? "active" : ""}`}
                    onClick={() => {
                      setComposeMode(false);
                      setSelectedRunId(id);
                      setHistoryOpen(false);
                    }}
                  >
                    <span className="mb-0.5 inline-flex items-center gap-1 text-xs font-medium">
                      <i data-lucide="history"></i>
                      <span>{runLabelFromTimestamp(runTimestamp(run))}</span>
                    </span>
                    <span className="tcp-subtle line-clamp-2 block text-[11px]">
                      {String(run?.objective || "").trim() || "No objective"}
                    </span>
                  </button>
                </motion.div>
              );
            })}
          </AnimatePresence>
          {!orderedRuns.length ? <p className="chat-rail-empty px-1 py-2">No runs yet.</p> : null}
        </div>
      </motion.aside>
      <AnimatePresence>
        {historyOpen ? (
          <motion.button
            type="button"
            className="chat-scrim open"
            aria-label="Close history"
            initial={reducedMotion ? false : { opacity: 0 }}
            animate={reducedMotion ? undefined : { opacity: 1 }}
            exit={reducedMotion ? undefined : { opacity: 0 }}
            onClick={() => setHistoryOpen(false)}
          />
        ) : null}
      </AnimatePresence>
    </>
  );

  if (noRunYet) {
    const canSend =
      String(prompt || "").trim().length > 0 && String(workspaceRoot || "").trim().length > 0;
    return (
      <>
        <div ref={rootRef} className="chat-layout min-w-0 min-h-0 h-full flex-1">
          {historyPanel}
          <div className="chat-workspace min-h-0 min-w-0">
            <PageCard
              title={
                <span className="inline-flex items-center gap-2">
                  <button
                    type="button"
                    className="chat-icon-btn h-8 w-8"
                    title="History"
                    onClick={() => setHistoryOpen((prev) => !prev)}
                  >
                    <i data-lucide="history"></i>
                  </button>
                  <span>Orchestrator</span>
                </span>
              }
              subtitle="Describe the goal. The planner will build a task board."
              className="flex h-full min-h-0 flex-col"
            >
              <div className="grid min-h-0 flex-1 w-full content-start gap-3">
                <textarea
                  className="tcp-input min-h-[360px] md:min-h-[52vh]"
                  placeholder="What do you want the agents to build?"
                  value={prompt}
                  onInput={(e) => setPrompt((e.target as HTMLTextAreaElement).value)}
                />
                <div className="grid gap-2 md:grid-cols-[auto_1fr]">
                  <button
                    className="tcp-btn"
                    onClick={() => {
                      const seed = String(
                        workspaceRoot || statusQuery.data?.workspaceRoot || "/"
                      ).trim();
                      setWorkspaceBrowserDir(seed || "/");
                      setWorkspaceBrowserSearch("");
                      setWorkspaceBrowserOpen(true);
                    }}
                  >
                    <i data-lucide="folder-open"></i>
                    Browse
                  </button>
                  <input
                    className="tcp-input"
                    readOnly
                    placeholder="No workspace selected. Use Browse."
                    value={workspaceRoot}
                  />
                </div>
                <div className="tcp-subtle text-xs">Selected folder: {workspaceRoot || "none"}</div>
                {!workspaceRoot ? (
                  <div className="rounded-lg border border-amber-400/40 bg-amber-950/20 p-2 text-xs text-amber-200">
                    Select a workspace folder before sending.
                  </div>
                ) : null}
                <div className="grid gap-2 md:grid-cols-2">
                  <button
                    className="tcp-btn-primary"
                    onClick={() => startMutation.mutate()}
                    disabled={startMutation.isPending || !canSend}
                  >
                    <i data-lucide="send"></i>
                    Send
                  </button>
                  <button
                    className="tcp-btn"
                    type="button"
                    onClick={() => setShowAdvanced((prev) => !prev)}
                  >
                    <i data-lucide="sliders-horizontal"></i>
                    {showAdvanced ? "Hide Advanced" : "Show Advanced"}
                  </button>
                </div>
                {showAdvanced ? (
                  <div className="grid gap-2 rounded-lg border border-slate-700/60 bg-slate-900/20 p-2 md:grid-cols-3">
                    <input
                      className="tcp-input"
                      type="number"
                      min="1"
                      value={maxTasks}
                      onInput={(e) => setMaxTasks((e.target as HTMLInputElement).value)}
                      title="max tasks"
                    />
                    <input
                      className="tcp-input"
                      type="number"
                      min="1"
                      max="16"
                      value={maxAgents}
                      onInput={(e) => setMaxAgents((e.target as HTMLInputElement).value)}
                      title="max agents"
                    />
                    <input
                      className="tcp-input"
                      value={workflowId}
                      onInput={(e) => setWorkflowId((e.target as HTMLInputElement).value)}
                      title="workflow id"
                    />
                    <div className="tcp-subtle md:col-span-3 text-xs">
                      Workflow id controls task routing template. Keep default unless you have a
                      custom workflow.
                    </div>
                  </div>
                ) : null}
              </div>
            </PageCard>
          </div>
        </div>
        {workspaceBrowserOpen ? (
          <div className="tcp-confirm-overlay">
            <div className="tcp-confirm-dialog max-w-2xl">
              <h3 className="tcp-confirm-title">Select Workspace Folder</h3>
              <p className="tcp-confirm-message">Current: {workspaceCurrentBrowseDir || "n/a"}</p>
              <div className="mb-2 flex flex-wrap gap-2">
                <button
                  className="tcp-btn"
                  onClick={() => {
                    if (!workspaceParentDir) return;
                    setWorkspaceBrowserDir(workspaceParentDir);
                  }}
                  disabled={!workspaceParentDir}
                >
                  <i data-lucide="arrow-up-circle"></i>
                  Up
                </button>
                <button
                  className="tcp-btn-primary"
                  onClick={() => {
                    if (!workspaceCurrentBrowseDir) return;
                    setWorkspaceRoot(workspaceCurrentBrowseDir);
                    setWorkspaceBrowserOpen(false);
                    setWorkspaceBrowserSearch("");
                    toast("ok", `Workspace selected: ${workspaceCurrentBrowseDir}`);
                  }}
                >
                  <i data-lucide="badge-check"></i>
                  Select This Folder
                </button>
                <button
                  className="tcp-btn"
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
                  onInput={(e) => setWorkspaceBrowserSearch((e.target as HTMLInputElement).value)}
                />
              </div>
              <div className="max-h-[360px] overflow-auto rounded-lg border border-slate-700/60 bg-slate-900/20 p-2">
                {filteredWorkspaceDirectories.length ? (
                  filteredWorkspaceDirectories.map((entry: any) => (
                    <button
                      key={String(entry?.path || entry?.name)}
                      className="tcp-list-item mb-1 w-full text-left"
                      onClick={() => setWorkspaceBrowserDir(String(entry?.path || ""))}
                    >
                      <i data-lucide="folder-open"></i>
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
            </div>
          </div>
        ) : null}
      </>
    );
  }

  return (
    <>
      <div ref={rootRef} className="chat-layout min-w-0 min-h-0 h-full flex-1">
        {historyPanel}
        <div className="chat-workspace min-h-0 min-w-0">
          <div className="grid h-full min-h-[calc(100vh-240px)] min-w-0 gap-4 xl:grid-cols-[1.05fr_1fr]">
            <PageCard
              title={
                <span className="inline-flex items-center gap-2">
                  <button
                    type="button"
                    className="chat-icon-btn h-8 w-8"
                    title="History"
                    onClick={() => setHistoryOpen((prev) => !prev)}
                  >
                    <i data-lucide="history"></i>
                  </button>
                  <button
                    type="button"
                    className="chat-icon-btn h-8 w-8"
                    title="Back to start"
                    onClick={goToStartView}
                  >
                    <i data-lucide="arrow-left-to-line"></i>
                  </button>
                  <span>Orchestration Run</span>
                </span>
              }
              subtitle="Plan review and execution"
              className="flex h-full min-h-0 flex-col"
            >
              <div className="mb-3 flex flex-wrap items-center gap-2 text-xs">
                <span className={statusBadgeClass(runStatus)}>{runStatus || "unknown"}</span>
                <span className="inline-flex items-center gap-1 tcp-subtle">
                  <i data-lucide="history"></i>
                  <span>
                    {runLabelFromTimestamp(
                      runQuery.data?.run?.updated_at_ms || runQuery.data?.run?.created_at_ms
                    )}
                  </span>
                </span>
                <span className="tcp-subtle">id: {runId}</span>
                <span className="tcp-subtle">plan: {planSource}</span>
              </div>

              {isPlanning ? (
                <div className="mb-3 rounded-xl border border-slate-700/60 bg-slate-900/25 p-3">
                  <div className="mb-1 text-sm font-medium">Planner is formulating a plan...</div>
                  <div className="tcp-subtle text-xs">Waiting for tasks to be generated.</div>
                </div>
              ) : null}

              {isAwaitingApproval ? (
                <div className="mb-3 rounded-xl border border-amber-500/40 bg-amber-950/20 p-3">
                  <div className="mb-2 text-sm font-medium text-amber-200">Plan Ready</div>
                  <div className="mb-2 text-xs text-amber-100/90">
                    Review the kanban. Request a rework or execute.
                  </div>
                  <textarea
                    className="tcp-input mb-2 min-h-[80px]"
                    placeholder="Feedback to rework the plan..."
                    value={revisionFeedback}
                    onInput={(e) => setRevisionFeedback((e.target as HTMLTextAreaElement).value)}
                  />
                  <div className="flex flex-wrap gap-2">
                    <button
                      className="tcp-btn"
                      disabled={!revisionFeedback.trim()}
                      onClick={() =>
                        actionMutation.mutate({
                          path: "/api/orchestrator/request_revision",
                          body: {
                            runId,
                            feedback: revisionFeedback,
                            maxTasks: Number(maxTasks || 4),
                            maxAgents: Number(maxAgents || 3),
                            workflowId,
                          },
                        })
                      }
                    >
                      <i data-lucide="pencil"></i>
                      Rework Plan
                    </button>
                    <button
                      className="tcp-btn-primary"
                      onClick={() =>
                        actionMutation.mutate({
                          path: "/api/orchestrator/approve",
                          body: { runId },
                        })
                      }
                    >
                      <i data-lucide="play"></i>
                      Execute Plan
                    </button>
                    <button
                      className="tcp-btn-danger"
                      disabled={discardMutation.isPending}
                      onClick={() => discardMutation.mutate(runId)}
                    >
                      <i data-lucide="trash-2"></i>
                      Discard Plan
                    </button>
                  </div>
                </div>
              ) : null}

              {!isPlanning && !isAwaitingApproval ? (
                <div className="mb-3 flex flex-wrap gap-2">
                  {canPause ? (
                    <button
                      className="tcp-btn"
                      onClick={() =>
                        actionMutation.mutate({ path: "/api/orchestrator/pause", body: { runId } })
                      }
                    >
                      <i data-lucide="square"></i>
                      Pause
                    </button>
                  ) : null}
                  {canResume ? (
                    <button
                      className="tcp-btn"
                      onClick={() =>
                        actionMutation.mutate({ path: "/api/orchestrator/resume", body: { runId } })
                      }
                    >
                      <i data-lucide="play"></i>
                      Resume
                    </button>
                  ) : null}
                  {canCancel ? (
                    <button
                      className="tcp-btn-danger"
                      onClick={() =>
                        actionMutation.mutate({ path: "/api/orchestrator/cancel", body: { runId } })
                      }
                    >
                      <i data-lucide="x"></i>
                      Cancel
                    </button>
                  ) : null}
                  <button className="tcp-btn" onClick={goToStartView}>
                    <i data-lucide="plus"></i>
                    New Prompt
                  </button>
                </div>
              ) : null}
              {isTerminal ? (
                <div className="mb-3 rounded-lg border border-slate-700/60 bg-slate-900/25 p-2 text-xs tcp-subtle">
                  This run is {runStatus}. Start a new prompt to continue.
                </div>
              ) : null}

              <div className="mt-3 rounded-xl border border-slate-700/60 bg-slate-900/30 p-3">
                <div className="mb-2 flex items-center justify-between gap-2">
                  <div className="font-medium">Workspace Files</div>
                  <button
                    type="button"
                    className="chat-icon-btn h-7 w-7"
                    title="Refresh"
                    aria-label="Refresh"
                    onClick={() => void runWorkspaceQuery.refetch()}
                    disabled={!activeWorkspaceRoot}
                  >
                    <i data-lucide="refresh-cw"></i>
                  </button>
                </div>
                <div className="mb-2 flex flex-wrap gap-2">
                  <button
                    type="button"
                    className="chat-icon-btn h-7 w-7"
                    title="Up"
                    aria-label="Up"
                    disabled={!runWorkspaceParent}
                    onClick={() => {
                      if (!runWorkspaceParent) return;
                      setRunWorkspaceDir(runWorkspaceParent);
                    }}
                  >
                    <i data-lucide="arrow-up-circle"></i>
                  </button>
                  <button
                    type="button"
                    className="chat-icon-btn h-7 w-7"
                    title="Root"
                    aria-label="Root"
                    disabled={!activeWorkspaceRoot}
                    onClick={() => {
                      if (!activeWorkspaceRoot) return;
                      setRunWorkspaceDir(activeWorkspaceRoot);
                    }}
                  >
                    <i data-lucide="home"></i>
                  </button>
                </div>
                <div className="mb-2 tcp-subtle text-[11px]" style={{ overflowWrap: "anywhere" }}>
                  {runWorkspaceDir || activeWorkspaceRoot || "No workspace"}
                </div>
                {!runWorkspaceFileAccessAllowed ? (
                  <div className="mb-2 rounded-lg border border-amber-400/40 bg-amber-950/20 p-2 text-xs text-amber-200">
                    File listing for this workspace is not exposed by the current server scope.
                    Directory navigation still works.
                  </div>
                ) : null}
                <div className="grid max-h-[220px] min-h-0 gap-1 overflow-auto">
                  {runWorkspaceDirectories.map((entry: any) => {
                    const path = String(entry?.path || "");
                    return (
                      <button
                        key={`dir-${path}`}
                        className="tcp-list-item text-left"
                        onClick={() => setRunWorkspaceDir(path)}
                      >
                        <span className="inline-flex items-center gap-2">
                          <i data-lucide="folder-open"></i>
                          <span>{String(entry?.name || path)}</span>
                        </span>
                      </button>
                    );
                  })}
                  {runWorkspaceFiles.map((entry: any) => {
                    const path = String(entry?.path || "");
                    const active = path === selectedWorkspaceFile;
                    return (
                      <button
                        key={`file-${path}`}
                        className={`tcp-list-item text-left ${active ? "border-amber-400/70" : ""}`}
                        onClick={() => setSelectedWorkspaceFile(path)}
                      >
                        <div className="flex items-center justify-between gap-2">
                          <span className="inline-flex min-w-0 items-center gap-2">
                            <i data-lucide="file-up"></i>
                            <span className="truncate">{String(entry?.name || path)}</span>
                          </span>
                          <span className="tcp-subtle shrink-0 text-[11px]">
                            {formatFileBytes(entry?.size)}
                          </span>
                        </div>
                      </button>
                    );
                  })}
                  {!runWorkspaceDirectories.length && !runWorkspaceFiles.length ? (
                    <EmptyState text="No files or folders in this workspace location." />
                  ) : null}
                </div>
              </div>
              {selectedWorkspaceFile ? (
                <div className="mt-3 rounded-xl border border-slate-700/60 bg-slate-900/30 p-3">
                  <div className="mb-2 flex items-center justify-between gap-2">
                    <div className="font-medium">File Preview</div>
                    <span className="tcp-subtle text-[11px]" style={{ overflowWrap: "anywhere" }}>
                      {selectedWorkspaceFile}
                    </span>
                  </div>
                  {runWorkspaceReadQuery.isLoading ? (
                    <div className="tcp-subtle text-xs">Loading file...</div>
                  ) : selectedIsHtml ? (
                    <iframe
                      className="h-[260px] w-full rounded-lg border border-slate-700/60 bg-black"
                      sandbox="allow-scripts allow-forms allow-pointer-lock"
                      srcDoc={selectedWorkspaceText}
                      title={selectedWorkspaceFile}
                    />
                  ) : selectedIsMarkdown ? (
                    <div
                      className="tcp-markdown tcp-markdown-ai max-h-[260px] overflow-auto rounded-lg border border-slate-700/60 bg-slate-950/40 p-3"
                      dangerouslySetInnerHTML={{
                        __html: renderMarkdownSafe(selectedWorkspaceText),
                      }}
                    />
                  ) : (
                    <pre className="tcp-code max-h-[260px] overflow-auto whitespace-pre-wrap break-words">
                      {selectedWorkspaceText || "No text content."}
                    </pre>
                  )}
                </div>
              ) : null}
            </PageCard>

            <PageCard
              title="Kanban + Budget"
              subtitle="Tasks activate after execute"
              className="flex h-full min-h-0 flex-col"
            >
              <div className="mb-3">
                <BudgetMeter budget={budget} />
              </div>

              <TaskBoard
                tasks={tasks}
                currentTaskId={String(runQuery.data?.run?.current_step_id || "")}
                onRetryTask={(task) =>
                  actionMutation.mutate({
                    path: "/api/orchestrator/retry",
                    body: { runId, stepId: task.id },
                  })
                }
                onTaskClick={(task) => {
                  if (!task.session_id) return;
                  saveStoredSessionId(task.session_id);
                  navigate("chat");
                }}
              />

              <div className="mt-3 rounded-xl border border-slate-700/60 bg-slate-900/30 p-3">
                <div className="mb-1 flex items-center justify-between gap-2">
                  <div className="font-medium">Executor Verification</div>
                </div>
                {verificationEvents.length ? (
                  <div className="grid max-h-40 gap-2 overflow-auto">
                    {verificationEvents.map((item) => (
                      <div key={item.id} className="rounded border border-slate-700/60 p-2 text-xs">
                        <div className="mb-1 flex items-center justify-between gap-2">
                          <span
                            className={statusBadgeClass(
                              item.type.includes("failed") ? "failed" : "running"
                            )}
                          >
                            {item.type}
                          </span>
                          <span className="tcp-subtle">mode: {item.mode}</span>
                        </div>
                        <div className="font-medium">{item.title}</div>
                        <div className="tcp-subtle">{item.reason || "No verification detail."}</div>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="tcp-subtle text-xs">No verification telemetry yet.</div>
                )}
              </div>

              <div className="mt-3 rounded-xl border border-slate-700/60 bg-slate-900/30 p-3">
                <div className="mb-1 flex items-center justify-between gap-2">
                  <div className="font-medium">Live Activity</div>
                  <span className="tcp-subtle text-xs">{eventTimeLabel(Date.now())}</span>
                </div>
                {activityEvents.length ? (
                  <div className="grid max-h-40 gap-2 overflow-auto">
                    {activityEvents.map((item) => (
                      <div key={item.id} className="rounded border border-slate-700/60 p-2 text-xs">
                        <div className="mb-1 flex items-center justify-between gap-2">
                          <span
                            className={statusBadgeClass(
                              item.type.includes("failed") ? "failed" : "running"
                            )}
                          >
                            {item.type}
                          </span>
                          <span className="tcp-subtle">{eventTimeLabel(item.at)}</span>
                        </div>
                        <div className="font-medium">{item.title}</div>
                        <div className="tcp-subtle">{item.detail || "No additional detail."}</div>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="tcp-subtle text-xs">No activity events yet.</div>
                )}
              </div>

              <div className="mt-3 rounded-xl border border-slate-700/60 bg-slate-900/30 p-3">
                <div className="mb-1 flex items-center justify-between gap-2">
                  <div className="font-medium">Latest Output</div>
                  {liveSessionId ? (
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() => {
                        saveStoredSessionId(String(liveSessionId));
                        navigate("chat");
                      }}
                    >
                      Open Session
                    </button>
                  ) : null}
                </div>
                {liveSessionId ? (
                  <>
                    <div className="tcp-code max-h-28 overflow-auto whitespace-pre-wrap break-words">
                      {latestAssistantOutput || "No assistant output text yet."}
                    </div>
                    <div className="mt-2 tcp-subtle text-xs">Recent tool calls</div>
                    <div className="tcp-code mt-1 max-h-24 overflow-auto whitespace-pre-wrap break-words">
                      {recentToolActivity.length
                        ? recentToolActivity.join("\n")
                        : "No tool call records found in current session yet."}
                    </div>
                  </>
                ) : (
                  <div className="tcp-subtle text-xs">No completed step output session yet.</div>
                )}
              </div>
            </PageCard>
          </div>
        </div>
      </div>
      {workspaceBrowserOpen ? (
        <div className="tcp-confirm-overlay">
          <div className="tcp-confirm-dialog max-w-2xl">
            <h3 className="tcp-confirm-title">Select Workspace Folder</h3>
            <p className="tcp-confirm-message">Current: {workspaceCurrentBrowseDir || "n/a"}</p>
            <div className="mb-2 flex flex-wrap gap-2">
              <button
                className="tcp-btn"
                onClick={() => {
                  if (!workspaceParentDir) return;
                  setWorkspaceBrowserDir(workspaceParentDir);
                }}
                disabled={!workspaceParentDir}
              >
                <i data-lucide="arrow-up-circle"></i>
                Up
              </button>
              <button
                className="tcp-btn-primary"
                onClick={() => {
                  if (!workspaceCurrentBrowseDir) return;
                  setWorkspaceRoot(workspaceCurrentBrowseDir);
                  setWorkspaceBrowserOpen(false);
                  setWorkspaceBrowserSearch("");
                  toast("ok", `Workspace selected: ${workspaceCurrentBrowseDir}`);
                }}
              >
                <i data-lucide="badge-check"></i>
                Select This Folder
              </button>
              <button
                className="tcp-btn"
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
                onInput={(e) => setWorkspaceBrowserSearch((e.target as HTMLInputElement).value)}
              />
            </div>
            <div className="max-h-[360px] overflow-auto rounded-lg border border-slate-700/60 bg-slate-900/20 p-2">
              {filteredWorkspaceDirectories.length ? (
                filteredWorkspaceDirectories.map((entry: any) => (
                  <button
                    key={String(entry?.path || entry?.name)}
                    className="tcp-list-item mb-1 w-full text-left"
                    onClick={() => setWorkspaceBrowserDir(String(entry?.path || ""))}
                  >
                    <i data-lucide="folder-open"></i>
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
          </div>
        </div>
      ) : null}
    </>
  );
}
