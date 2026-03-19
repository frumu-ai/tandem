import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { OrchestrationTask, TaskState } from "./types";

const LABELS: Record<TaskState, string> = {
  created: "Created",
  pending: "Pending",
  runnable: "Runnable",
  assigned: "Assigned",
  in_progress: "In Progress",
  blocked: "Blocked",
  done: "Done",
  failed: "Failed",
  validated: "Validated",
};

function statusClass(state: TaskState) {
  if (state === "done" || state === "validated") return "tcp-badge-ok";
  if (state === "failed") return "tcp-badge-err";
  if (state === "blocked") return "border border-emerald-400/60 bg-emerald-400/10 text-emerald-200";
  if (state === "in_progress" || state === "runnable" || state === "assigned")
    return "tcp-badge-warn";
  return "tcp-badge-info";
}

function taskCardClass(state: TaskState, isCurrent: boolean, isSelected: boolean) {
  if (isSelected) return "border-cyan-400/70 bg-cyan-950/20";
  if (state === "blocked") return "border-emerald-500/35 bg-emerald-950/18";
  if (isCurrent) return "border-emerald-400/70 bg-emerald-950/14";
  return "border-slate-700/60 bg-slate-900/20";
}

function statusIcon(state: TaskState) {
  if (state === "in_progress" || state === "assigned") {
    return (
      <i data-lucide="loader-circle" className="h-3.5 w-3.5 animate-spin" aria-hidden="true"></i>
    );
  }
  return null;
}

function TaskCard({
  task,
  isCurrent,
  isSelected,
  workflowSummary,
  onTaskSelect,
  onRetryTask,
}: {
  task: OrchestrationTask;
  isCurrent: boolean;
  isSelected?: boolean;
  workflowSummary?: { runs: number; failed: number };
  onTaskSelect?: (task: OrchestrationTask) => void;
  onRetryTask?: (task: OrchestrationTask) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const error =
    task.state === "failed" || task.state === "blocked"
      ? String(task.error_message || "").trim()
      : "";
  return (
    <div
      className={`cursor-pointer rounded-lg border p-2 ${taskCardClass(
        task.state,
        isCurrent,
        Boolean(isSelected)
      )}`}
      onClick={() => onTaskSelect?.(task)}
    >
      <div className="mb-1 flex items-start justify-between gap-2">
        <div className="line-clamp-6 text-xs font-medium leading-snug" title={task.title}>
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
            <span className="rounded border border-emerald-600/50 bg-emerald-950/30 px-1.5 py-0.5 text-emerald-200">
              gate: {task.gate}
            </span>
          ) : null}
        </div>
      ) : null}
      {workflowSummary && workflowSummary.runs > 0 ? (
        <div className="mt-1 flex flex-wrap gap-1 text-[10px] text-slate-300">
          <span className="rounded border border-indigo-600/50 bg-indigo-950/30 px-1.5 py-0.5">
            workflow runs: {workflowSummary.runs}
          </span>
          {workflowSummary.failed > 0 ? (
            <span className="rounded border border-rose-600/50 bg-rose-950/30 px-1.5 py-0.5 text-rose-200">
              workflow failed: {workflowSummary.failed}
            </span>
          ) : null}
        </div>
      ) : null}
      {error ? (
        <div
          className={`mt-1 text-xs ${
            task.state === "blocked" ? "text-emerald-200/90" : "text-rose-300"
          }`}
        >
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
        {task.dependencies.slice(0, 2).map((dep) => (
          <span
            key={dep}
            className="rounded border border-slate-700/60 px-1.5 py-0.5 text-[10px] text-slate-300"
          >
            {"<-"} {dep}
          </span>
        ))}
        {task.dependencies.length > 2 ? (
          <span className="rounded border border-slate-700/60 px-1.5 py-0.5 text-[10px] text-slate-300">
            +{task.dependencies.length - 2} more
          </span>
        ) : null}
      </div>
      <div className="mt-2 tcp-subtle text-[10px]">Details</div>
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
  selectedTaskId,
  workflowSummaryByTaskId,
  onTaskSelect,
  onRetryTask,
}: {
  tasks: OrchestrationTask[];
  currentTaskId?: string;
  selectedTaskId?: string;
  workflowSummaryByTaskId?: Record<string, { runs: number; failed: number }>;
  onTaskSelect?: (task: OrchestrationTask) => void;
  onRetryTask?: (task: OrchestrationTask) => void;
}) {
  const [mobileColumnKey, setMobileColumnKey] = useState("runnable");
  const desktopBoardRef = useRef<HTMLDivElement | null>(null);
  const columnRefs = useRef<Record<string, HTMLDivElement | null>>({});
  const grouped = useMemo(() => {
    const rows: Record<TaskState, OrchestrationTask[]> = {
      created: [],
      pending: [],
      runnable: [],
      assigned: [],
      in_progress: [],
      blocked: [],
      done: [],
      failed: [],
      validated: [],
    };
    for (const task of tasks) rows[task.state].push(task);
    const doneIds = new Set([...rows.done, ...rows.validated].map((task) => task.id));
    const runnable = [...rows.runnable];
    const waiting: OrchestrationTask[] = [];
    for (const task of [...rows.created, ...rows.pending]) {
      const ready =
        task.dependencies.length === 0 || task.dependencies.every((dep) => doneIds.has(dep));
      if (ready) runnable.push(task);
      else waiting.push(task);
    }
    return {
      runnable,
      waiting,
      assigned: rows.assigned,
      in_progress: rows.in_progress,
      blocked: rows.blocked,
      done: [...rows.done, ...rows.validated],
      failed: rows.failed,
    };
  }, [tasks]);

  const columns: Array<{ key: string; label: string; tasks: OrchestrationTask[] }> = [
    { key: "runnable", label: "Ready", tasks: grouped.runnable },
    { key: "waiting", label: "Waiting on deps", tasks: grouped.waiting },
    { key: "assigned", label: "Assigned", tasks: grouped.assigned },
    { key: "in_progress", label: "In Progress", tasks: grouped.in_progress },
    { key: "blocked", label: "Blocked", tasks: grouped.blocked },
    { key: "done", label: "Done", tasks: grouped.done },
    { key: "failed", label: "Failed", tasks: grouped.failed },
  ];
  const recommendedMobileColumnKey = useMemo(() => {
    const currentTask = columns.find((column) =>
      column.tasks.some((task) => task.id === currentTaskId)
    );
    if (currentTask) return currentTask.key;
    return columns.find((column) => column.tasks.length > 0)?.key || "runnable";
  }, [columns, currentTaskId]);
  const activeColumnKey = recommendedMobileColumnKey;

  useEffect(() => {
    setMobileColumnKey((current) => {
      if (columns.some((column) => column.key === current)) return current;
      return recommendedMobileColumnKey;
    });
  }, [columns, recommendedMobileColumnKey]);

  const scrollToColumn = (columnKey: string) => {
    const node = columnRefs.current[columnKey];
    if (!node) return;
    node.scrollIntoView({ behavior: "smooth", inline: "start", block: "nearest" });
  };

  const scrollToCurrentTask = () => {
    if (!activeColumnKey) return;
    scrollToColumn(activeColumnKey);
  };

  if (!tasks.length) {
    return (
      <div className="tcp-subtle rounded-lg border border-slate-700/60 bg-slate-900/20 p-4">
        No tasks yet.
      </div>
    );
  }

  return (
    <>
      <div className="grid gap-3 xl:hidden">
        <div className="flex flex-wrap items-center gap-2">
          {currentTaskId ? (
            <button
              type="button"
              className="rounded-full border border-emerald-400/60 bg-emerald-400/10 px-3 py-1.5 text-[11px] font-medium text-emerald-200"
              onClick={() => setMobileColumnKey(activeColumnKey)}
            >
              Jump to active task
            </button>
          ) : null}
        </div>
        <div className="flex gap-2 overflow-x-auto pb-1">
          {columns.map((column) => {
            const active = column.key === mobileColumnKey;
            return (
              <button
                key={column.key}
                type="button"
                className={`shrink-0 rounded-full border px-3 py-1.5 text-[11px] font-medium ${
                  active
                    ? "border-emerald-400/60 bg-emerald-400/10 text-emerald-200"
                    : "border-slate-700/60 bg-slate-900/20 text-slate-300"
                }`}
                onClick={() => setMobileColumnKey(column.key)}
              >
                {column.label} ({column.tasks.length})
              </button>
            );
          })}
        </div>
        {columns
          .filter((column) => column.key === mobileColumnKey)
          .map((column) => (
            <div
              key={column.key}
              className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-2"
            >
              <div className="mb-2 flex items-center justify-between">
                <div className="text-xs font-semibold">{column.label}</div>
                <div className="tcp-subtle text-xs">{column.tasks.length}</div>
              </div>
              <div className="grid gap-2">
                {column.tasks.map((task) => (
                  <TaskCard
                    key={task.id}
                    task={task}
                    isCurrent={task.id === currentTaskId}
                    isSelected={task.id === selectedTaskId}
                    workflowSummary={workflowSummaryByTaskId?.[task.id]}
                    onTaskSelect={onTaskSelect}
                    onRetryTask={onRetryTask}
                  />
                ))}
                {!column.tasks.length ? <div className="tcp-subtle text-xs">No tasks</div> : null}
              </div>
            </div>
          ))}
      </div>
      <div className="hidden gap-3 xl:grid">
        <div className="flex flex-wrap items-center gap-2">
          {currentTaskId ? (
            <button
              type="button"
              className="rounded-full border border-emerald-400/60 bg-emerald-400/10 px-3 py-1.5 text-[11px] font-medium text-emerald-200"
              onClick={scrollToCurrentTask}
            >
              Jump to active task
            </button>
          ) : null}
          <div className="flex flex-wrap gap-2">
            {columns.map((column) => {
              const active = column.key === activeColumnKey;
              return (
                <button
                  key={`desktop-tab-${column.key}`}
                  type="button"
                  className={`rounded-full border px-3 py-1.5 text-[11px] font-medium ${
                    active
                      ? "border-emerald-400/60 bg-emerald-400/10 text-emerald-200"
                      : "border-slate-700/60 bg-slate-900/20 text-slate-300"
                  }`}
                  onClick={() => scrollToColumn(column.key)}
                >
                  {column.label} ({column.tasks.length})
                </button>
              );
            })}
          </div>
        </div>
        <div ref={desktopBoardRef} className="overflow-x-auto pb-2">
          <div className="flex min-w-max gap-3">
            {columns.map((column) => (
              <div
                key={column.key}
                ref={(node) => {
                  columnRefs.current[column.key] = node;
                }}
                className={`min-h-[16rem] w-[320px] shrink-0 rounded-xl border p-2 ${
                  column.key === activeColumnKey
                    ? "border-emerald-400/60 bg-emerald-400/5"
                    : "border-slate-700/60 bg-slate-900/20"
                }`}
              >
                <div className="mb-2 flex items-center justify-between">
                  <div className="text-xs font-semibold">{column.label}</div>
                  <div className="tcp-subtle text-xs">{column.tasks.length}</div>
                </div>
                <div className="grid gap-2">
                  {column.tasks.map((task) => (
                    <TaskCard
                      key={task.id}
                      task={task}
                      isCurrent={task.id === currentTaskId}
                      isSelected={task.id === selectedTaskId}
                      workflowSummary={workflowSummaryByTaskId?.[task.id]}
                      onTaskSelect={onTaskSelect}
                      onRetryTask={onRetryTask}
                    />
                  ))}
                  {!column.tasks.length ? <div className="tcp-subtle text-xs">No tasks</div> : null}
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </>
  );
}
