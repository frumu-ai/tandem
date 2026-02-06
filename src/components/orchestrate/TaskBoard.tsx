import { useMemo } from "react";
import { motion } from "framer-motion";
import { Clock, CheckCircle2, XCircle, Loader2, AlertTriangle, ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import type { Task, TaskState } from "./types";

interface TaskBoardProps {
  tasks: Task[];
  currentTaskId?: string;
  onTaskClick?: (task: Task) => void;
  className?: string;
}

const STATE_CONFIG: Record<TaskState, { label: string; color: string; icon: React.ReactNode }> = {
  pending: {
    label: "Pending",
    color: "bg-slate-500/20 text-slate-400 border-slate-500/30",
    icon: <Clock className="h-3 w-3" />,
  },
  in_progress: {
    label: "In Progress",
    color: "bg-blue-500/20 text-blue-400 border-blue-500/30",
    icon: <Loader2 className="h-3 w-3 animate-spin" />,
  },
  blocked: {
    label: "Blocked",
    color: "bg-amber-500/20 text-amber-400 border-amber-500/30",
    icon: <AlertTriangle className="h-3 w-3" />,
  },
  done: {
    label: "Done",
    color: "bg-emerald-500/20 text-emerald-400 border-emerald-500/30",
    icon: <CheckCircle2 className="h-3 w-3" />,
  },
  failed: {
    label: "Failed",
    color: "bg-red-500/20 text-red-400 border-red-500/30",
    icon: <XCircle className="h-3 w-3" />,
  },
};

interface TaskCardProps {
  task: Task;
  isCurrent: boolean;
  onClick?: () => void;
}

function TaskCard({ task, isCurrent, onClick }: TaskCardProps) {
  const config = STATE_CONFIG[task.state];

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -10 }}
      onClick={onClick}
      className={cn(
        "group rounded-lg border p-3 transition-all cursor-pointer",
        "hover:bg-surface-elevated",
        isCurrent ? "ring-2 ring-primary ring-offset-2 ring-offset-background" : "",
        config.color
      )}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          {config.icon}
          <span className="text-sm font-medium truncate">{task.title}</span>
        </div>
        <ChevronRight className="h-4 w-4 opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0" />
      </div>

      {task.description && (
        <p className="mt-1 text-xs text-text-muted line-clamp-2">{task.description}</p>
      )}

      {task.runtime_status && (
        <p className="mt-1 text-[10px] text-text-subtle line-clamp-1">
          {task.runtime_status}
          {task.runtime_detail ? `: ${task.runtime_detail}` : ""}
        </p>
      )}

      {task.error_message && (
        <p className="mt-1 text-xs text-red-400 line-clamp-1">{task.error_message}</p>
      )}

      {task.dependencies.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1">
          {task.dependencies.map((depId) => (
            <span
              key={depId}
              className="inline-flex items-center rounded-full bg-surface px-1.5 py-0.5 text-[10px] text-text-muted"
            >
              ← {depId}
            </span>
          ))}
        </div>
      )}

      {task.retry_count > 0 && (
        <div className="mt-1 text-[10px] text-amber-400">
          Retried {task.retry_count} time{task.retry_count > 1 ? "s" : ""}
        </div>
      )}
    </motion.div>
  );
}

export function TaskBoard({ tasks, currentTaskId, onTaskClick, className }: TaskBoardProps) {
  const groupedTasks = useMemo(() => {
    const groups: Record<TaskState, Task[]> = {
      pending: [],
      in_progress: [],
      blocked: [],
      done: [],
      failed: [],
    };

    for (const task of tasks) {
      groups[task.state].push(task);
    }

    return groups;
  }, [tasks]);

  const columns = (
    [
      { state: "pending" as TaskState, tasks: groupedTasks.pending },
      { state: "in_progress" as TaskState, tasks: groupedTasks.in_progress },
      { state: "blocked" as TaskState, tasks: groupedTasks.blocked },
      { state: "done" as TaskState, tasks: groupedTasks.done },
      { state: "failed" as TaskState, tasks: groupedTasks.failed },
    ] as const
  ).filter((col) => col.tasks.length > 0 || col.state === "pending" || col.state === "done");

  if (tasks.length === 0) {
    return (
      <div className={cn("rounded-lg border border-border bg-surface p-6 text-center", className)}>
        <p className="text-sm text-text-muted">
          No tasks yet. Start the orchestrator to generate a plan.
        </p>
      </div>
    );
  }

  return (
    <div className={cn("space-y-4", className)}>
      {/* Summary bar */}
      <div className="flex items-center gap-4 text-xs text-text-muted">
        <span>
          <span className="font-medium text-text">{tasks.length}</span> tasks
        </span>
        <span>
          <span className="text-emerald-400">{groupedTasks.done.length}</span> done
        </span>
        {groupedTasks.in_progress.length > 0 && (
          <span>
            <span className="text-blue-400">{groupedTasks.in_progress.length}</span> in progress
          </span>
        )}
        {groupedTasks.failed.length > 0 && (
          <span>
            <span className="text-red-400">{groupedTasks.failed.length}</span> failed
          </span>
        )}
      </div>

      {/* Kanban columns */}
      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
        {columns.map((column) => (
          <div key={column.state} className="space-y-2">
            <div className="flex items-center gap-2">
              {STATE_CONFIG[column.state].icon}
              <span className="text-xs font-medium text-text-subtle uppercase tracking-wide">
                {STATE_CONFIG[column.state].label}
              </span>
              <span className="text-xs text-text-muted">({column.tasks.length})</span>
            </div>
            <div className="space-y-2">
              {column.tasks.map((task) => (
                <TaskCard
                  key={task.id}
                  task={task}
                  isCurrent={task.id === currentTaskId}
                  onClick={() => onTaskClick?.(task)}
                />
              ))}
              {column.tasks.length === 0 && (
                <div className="rounded-lg border border-dashed border-border p-4 text-center text-xs text-text-muted">
                  No tasks
                </div>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
