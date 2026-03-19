import type { OrchestrationTask, TaskState } from "../orchestration/types";

export type TaskProjectionSource = "blackboard" | "context_steps" | "hybrid" | "empty";

export interface OrchestrationProjection {
  currentTaskId: string;
  taskSource: TaskProjectionSource;
  tasks: OrchestrationTask[];
}

type TaskRuntimeMeta = {
  assignedRole: string;
  errorMessage: string;
  runtimeDetail: string;
  runtimeStatus: string;
  sessionId: string;
};

function normalizeTaskState(status: unknown): TaskState {
  const value = String(status || "")
    .trim()
    .toLowerCase();
  if (value === "assigned") return "assigned";
  if (value === "validated") return "validated";
  if (value === "in_progress" || value === "running") return "in_progress";
  if (value === "done" || value === "completed") return "done";
  if (value === "failed" || value === "error" || value === "cancelled" || value === "canceled") {
    return "failed";
  }
  if (value === "blocked") return "blocked";
  if (value === "runnable" || value === "ready") return "runnable";
  if (value === "created") return "created";
  return "pending";
}

function buildRuntimeMeta(events: any[]): Record<string, TaskRuntimeMeta> {
  const metaByTaskId: Record<string, TaskRuntimeMeta> = {};
  for (const event of Array.isArray(events) ? events : []) {
    const taskId = String(event?.step_id || event?.payload?.task_id || "").trim();
    if (!taskId) continue;
    const payload = event?.payload && typeof event.payload === "object" ? event.payload : {};
    const previous = metaByTaskId[taskId] || {
      assignedRole: "",
      errorMessage: "",
      runtimeDetail: "",
      runtimeStatus: "",
      sessionId: "",
    };
    metaByTaskId[taskId] = {
      assignedRole: String(
        payload?.assigned_agent || payload?.agent_id || previous.assignedRole || ""
      ),
      errorMessage: String(payload?.error || previous.errorMessage || ""),
      runtimeDetail: String(payload?.why_next_step || previous.runtimeDetail || ""),
      runtimeStatus: String(payload?.step_status || previous.runtimeStatus || ""),
      sessionId: String(payload?.session_id || previous.sessionId || ""),
    };
  }
  return metaByTaskId;
}

function projectBlackboardTask(task: any, runtime: TaskRuntimeMeta | undefined): OrchestrationTask {
  const state = normalizeTaskState(task?.status);
  const payload = task?.payload && typeof task.payload === "object" ? task.payload : {};
  const leaseExpiresAtMs = Number(task?.lease_expires_at_ms || 0) || 0;
  const nextRetryAtMs = Number(task?.next_retry_at_ms || 0) || 0;
  const now = Date.now();
  return {
    id: String(task?.id || ""),
    title: String(payload?.title || task?.task_type || task?.id || "Untitled task"),
    description: String(payload?.description || payload?.objective || ""),
    dependencies: Array.isArray(task?.depends_on_task_ids)
      ? task.depends_on_task_ids.map((dep: unknown) => String(dep || "")).filter(Boolean)
      : [],
    state,
    retry_count: Number(task?.attempt || task?.retry_count || 0),
    error_message:
      state === "failed" || state === "blocked"
        ? String(task?.last_error || runtime?.errorMessage || "")
        : "",
    runtime_status: String(runtime?.runtimeStatus || ""),
    runtime_detail: String(runtime?.runtimeDetail || ""),
    assigned_role: String(task?.assigned_agent || task?.lease_owner || runtime?.assignedRole || ""),
    workflow_id: String(task?.workflow_id || ""),
    session_id: String(runtime?.sessionId || ""),
    task_kind: String(payload?.task_kind || ""),
    backlog_task_id: String(payload?.backlog_task_id || payload?.task_id || ""),
    repo_root: String(payload?.repo_root || ""),
    write_scope: String(payload?.write_scope || ""),
    acceptance_criteria: String(payload?.acceptance_criteria || ""),
    task_dependencies: Array.isArray(payload?.task_dependencies)
      ? payload.task_dependencies.join(", ")
      : String(payload?.task_dependencies || ""),
    verification_state: String(payload?.verification_state || ""),
    task_owner: String(payload?.task_owner || payload?.owner || ""),
    verification_command: String(payload?.verification_command || ""),
    output_path: String(payload?.output_path || ""),
    parent_task_id: String(task?.parent_task_id || ""),
    task_type: String(task?.task_type || ""),
    projects_backlog_tasks: Boolean(payload?.projects_backlog_tasks),
    lease_owner: String(task?.lease_owner || ""),
    lease_expires_at_ms: leaseExpiresAtMs || undefined,
    next_retry_at_ms: nextRetryAtMs || undefined,
    max_attempts: Number(task?.max_attempts || 0) || undefined,
    is_stale: state === "in_progress" && leaseExpiresAtMs > 0 && leaseExpiresAtMs <= now,
  };
}

function projectContextTask(step: any, runtime: TaskRuntimeMeta | undefined): OrchestrationTask {
  const state = normalizeTaskState(step?.stepStatus || step?.status);
  return {
    id: String(step?.taskId || step?.step_id || ""),
    title: String(step?.title || step?.step_id || "Untitled step"),
    description: String(step?.description || ""),
    dependencies: Array.isArray(step?.dependsOn)
      ? step.dependsOn.map((dep: unknown) => String(dep || "")).filter(Boolean)
      : [],
    state,
    retry_count: Number(step?.retry_count || 0),
    error_message:
      state === "failed" || state === "blocked"
        ? String(step?.error_message || runtime?.errorMessage || "")
        : "",
    runtime_status: String(step?.runtime_status || runtime?.runtimeStatus || ""),
    runtime_detail: String(step?.runtime_detail || runtime?.runtimeDetail || ""),
    assigned_role: String(step?.assignedAgent || runtime?.assignedRole || ""),
    workflow_id: String(step?.workflowId || ""),
    session_id: String(step?.sessionId || step?.session_id || runtime?.sessionId || ""),
  };
}

export function projectOrchestrationRun(payload: any): OrchestrationProjection {
  const runtimeMeta = buildRuntimeMeta(payload?.events);
  const canonicalRunTasks = Array.isArray(payload?.run?.tasks) ? payload.run.tasks : [];
  const blackboardTasks = canonicalRunTasks.length
    ? canonicalRunTasks
    : Array.isArray(payload?.blackboard?.tasks)
      ? payload.blackboard.tasks
      : [];
  const contextTasks = Array.isArray(payload?.tasks) ? payload.tasks : [];
  const projectedBlackboard = blackboardTasks
    .map((task: any) => projectBlackboardTask(task, runtimeMeta[String(task?.id || "").trim()]))
    .filter((task) => task.id);
  const projectedContext = contextTasks
    .map((step: any) =>
      projectContextTask(step, runtimeMeta[String(step?.taskId || step?.step_id || "").trim()])
    )
    .filter((task) => task.id);
  const tasksById = new Map<string, OrchestrationTask>();
  for (const task of projectedBlackboard) tasksById.set(task.id, task);
  for (const task of projectedContext) {
    if (!tasksById.has(task.id)) {
      tasksById.set(task.id, task);
      continue;
    }
    const current = tasksById.get(task.id);
    if (!current) continue;
    tasksById.set(task.id, {
      ...current,
      session_id: current.session_id || task.session_id,
      runtime_status: current.runtime_status || task.runtime_status,
      runtime_detail: current.runtime_detail || task.runtime_detail,
      assigned_role: current.assigned_role || task.assigned_role,
      error_message: current.error_message || task.error_message,
    });
  }
  const tasks = [...tasksById.values()];
  const currentTaskId =
    String(payload?.run?.current_step_id || "").trim() ||
    tasks.find((task) => task.state === "in_progress" || task.state === "assigned")?.id ||
    "";
  const taskSource: TaskProjectionSource = projectedBlackboard.length
    ? projectedContext.length
      ? "hybrid"
      : canonicalRunTasks.length
        ? "blackboard"
        : "blackboard"
    : projectedContext.length
      ? "context_steps"
      : "empty";
  return {
    currentTaskId,
    taskSource,
    tasks,
  };
}
