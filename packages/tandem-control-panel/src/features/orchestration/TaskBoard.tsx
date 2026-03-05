import { useMemo, useState } from "preact/hooks";
import type { OrchestrationTask, TaskState } from "./types";

const LABELS: Record<TaskState, string> = {
  pending: "Pending",
  runnable: "Runnable",
  in_progress: "In Progress",
  blocked: "Blocked",
  done: "Done",
  failed: "Failed",
};

function statusClass(state: TaskState) {
  if (state === "done") return "tcp-badge-ok";
  if (state === "failed") return "tcp-badge-err";
  if (state === "in_progress" || state === "runnable") return "tcp-badge-warn";
  return "tcp-badge-info";
}

function statusIcon(state: TaskState) {
  if (state === "in_progress") {
    return (
      <i data-lucide="loader-circle" className="h-3.5 w-3.5 animate-spin" aria-hidden="true"></i>
    );
  }
  return null;
}

function TaskCard({
  task,
  isCurrent,
  onTaskClick,
  onRetryTask,
}: {
  task: OrchestrationTask;
  isCurrent: boolean;
  onTaskClick?: (task: OrchestrationTask) => void;
  onRetryTask?: (task: OrchestrationTask) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const error =
    task.state === "failed" || task.state === "blocked"
      ? String(task.error_message || "").trim()
      : "";
  return (
    <div
      className={`rounded-lg border p-2 ${isCurrent ? "border-amber-400/70" : "border-slate-700/60 bg-slate-900/20"}`}
      onClick={() => onTaskClick?.(task)}
    >
      <div className="mb-1 flex items-start justify-between gap-2">
        <div className="text-xs font-medium leading-snug" title={task.title}>
          {task.title}
        </div>
        <span className={`${statusClass(task.state)} inline-flex shrink-0 items-center gap-1`}>
          {statusIcon(task.state)}
          <span>{LABELS[task.state]}</span>
        </span>
      </div>
      {task.description ? (
        <div className="tcp-subtle line-clamp-2 text-xs">{task.description}</div>
      ) : null}
      {task.assigned_role || task.gate ? (
        <div className="mt-1 flex flex-wrap gap-1 text-[10px] text-slate-300">
          {task.assigned_role ? (
            <span className="rounded border border-cyan-600/50 bg-cyan-950/30 px-1.5 py-0.5">
              role: {task.assigned_role}
            </span>
          ) : null}
          {task.gate ? (
            <span className="rounded border border-amber-600/50 bg-amber-950/30 px-1.5 py-0.5">
              gate: {task.gate}
            </span>
          ) : null}
        </div>
      ) : null}
      {error ? (
        <div className="mt-1 text-xs text-rose-300">
          <div className={expanded ? "whitespace-pre-wrap break-words" : "line-clamp-2"}>
            {error}
          </div>
          {error.length > 130 ? (
            <button
              className="tcp-btn mt-1 h-6 px-2 text-[11px]"
              onClick={() => setExpanded((v) => !v)}
            >
              {expanded ? "Less" : "More"}
            </button>
          ) : null}
        </div>
      ) : null}
      <div className="mt-2 flex flex-wrap gap-1">
        {task.dependencies.map((dep) => (
          <span
            key={dep}
            className="rounded border border-slate-700/60 px-1.5 py-0.5 text-[10px] text-slate-300"
          >
            {"<-"} {dep}
          </span>
        ))}
      </div>
      {task.state === "failed" && onRetryTask ? (
        <button
          className="tcp-btn mt-2 h-7 px-2 text-xs"
          onClick={(event) => {
            event.stopPropagation();
            onRetryTask(task);
          }}
        >
          Retry Task
        </button>
      ) : null}
    </div>
  );
}

export function TaskBoard({
  tasks,
  currentTaskId,
  onTaskClick,
  onRetryTask,
}: {
  tasks: OrchestrationTask[];
  currentTaskId?: string;
  onTaskClick?: (task: OrchestrationTask) => void;
  onRetryTask?: (task: OrchestrationTask) => void;
}) {
  const grouped = useMemo(() => {
    const rows: Record<TaskState, OrchestrationTask[]> = {
      pending: [],
      runnable: [],
      in_progress: [],
      blocked: [],
      done: [],
      failed: [],
    };
    for (const task of tasks) rows[task.state].push(task);
    const doneIds = new Set(rows.done.map((task) => task.id));
    const runnable = [...rows.runnable];
    const waiting: OrchestrationTask[] = [];
    for (const task of rows.pending) {
      const ready =
        task.dependencies.length === 0 || task.dependencies.every((dep) => doneIds.has(dep));
      if (ready) runnable.push(task);
      else waiting.push(task);
    }
    return {
      runnable,
      waiting,
      in_progress: rows.in_progress,
      blocked: rows.blocked,
      done: rows.done,
      failed: rows.failed,
    };
  }, [tasks]);

  const columns: Array<{ key: string; label: string; tasks: OrchestrationTask[] }> = [
    { key: "runnable", label: "Runnable", tasks: grouped.runnable },
    { key: "waiting", label: "Waiting on deps", tasks: grouped.waiting },
    { key: "in_progress", label: "In Progress", tasks: grouped.in_progress },
    { key: "blocked", label: "Blocked", tasks: grouped.blocked },
    { key: "done", label: "Done", tasks: grouped.done },
    { key: "failed", label: "Failed", tasks: grouped.failed },
  ];

  if (!tasks.length) {
    return (
      <div className="tcp-subtle rounded-lg border border-slate-700/60 bg-slate-900/20 p-4">
        No tasks yet.
      </div>
    );
  }

  return (
    <div className="grid gap-3 xl:grid-cols-3">
      {columns.map((column) => (
        <div key={column.key} className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-2">
          <div className="mb-2 flex items-center justify-between">
            <div className="text-xs font-semibold">{column.label}</div>
            <div className="tcp-subtle text-xs">{column.tasks.length}</div>
          </div>
          <div className="grid max-h-[280px] gap-2 overflow-auto">
            {column.tasks.map((task) => (
              <TaskCard
                key={task.id}
                task={task}
                isCurrent={task.id === currentTaskId}
                onTaskClick={onTaskClick}
                onRetryTask={onRetryTask}
              />
            ))}
            {!column.tasks.length ? <div className="tcp-subtle text-xs">No tasks</div> : null}
          </div>
        </div>
      ))}
    </div>
  );
}
