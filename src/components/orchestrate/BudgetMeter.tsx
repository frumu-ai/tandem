import { useState } from "react";
import { Gauge, Clock, Zap, Users } from "lucide-react";
import { cn } from "@/lib/utils";
import type { Budget } from "./types";

interface BudgetMeterProps {
  budget: Budget;
  className?: string;
}

interface MeterItemProps {
  label: string;
  used: number;
  max: number;
  icon: React.ReactNode;
  unit?: string;
  warningThreshold?: number;
}

function MeterItem({ label, used, max, icon, unit = "", warningThreshold = 0.8 }: MeterItemProps) {
  const percentage = max > 0 ? Math.min(used / max, 1) : 0;
  const isWarning = percentage >= warningThreshold;
  const isExceeded = percentage >= 1;

  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between text-xs">
        <div className="flex items-center gap-1.5 text-text-subtle">
          {icon}
          <span>{label}</span>
        </div>
        <span
          className={cn(
            "font-mono text-[10px]",
            isExceeded ? "text-red-400" : isWarning ? "text-amber-400" : "text-text-muted"
          )}
        >
          {used.toLocaleString()}
          {unit} / {max.toLocaleString()}
          {unit}
        </span>
      </div>
      <div className="h-1.5 rounded-full bg-surface-elevated overflow-hidden">
        <div
          className={cn(
            "h-full rounded-full transition-all duration-300",
            isExceeded ? "bg-red-500" : isWarning ? "bg-amber-500" : "bg-primary"
          )}
          style={{ width: `${percentage * 100}%` }}
        />
      </div>
    </div>
  );
}

export function BudgetMeter({ budget, className }: BudgetMeterProps) {
  const [expanded, setExpanded] = useState(false);

  const overallPercentage = Math.max(
    budget.max_iterations > 0 ? budget.iterations_used / budget.max_iterations : 0,
    budget.max_tokens > 0 ? budget.tokens_used / budget.max_tokens : 0,
    budget.max_wall_time_secs > 0 ? budget.wall_time_secs / budget.max_wall_time_secs : 0,
    budget.max_subagent_runs > 0 ? budget.subagent_runs_used / budget.max_subagent_runs : 0
  );

  return (
    <div className={cn("rounded-lg border border-border bg-surface p-3", className)}>
      {/* Compact view */}
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center justify-between text-left"
      >
        <div className="flex items-center gap-2">
          <Gauge className="h-4 w-4 text-primary" />
          <span className="text-sm font-medium text-text">Budget</span>
        </div>
        <div className="flex items-center gap-2">
          <div className="h-2 w-24 rounded-full bg-surface-elevated overflow-hidden">
            <div
              className={cn(
                "h-full rounded-full transition-all",
                overallPercentage >= 1
                  ? "bg-red-500"
                  : overallPercentage >= 0.8
                    ? "bg-amber-500"
                    : "bg-primary"
              )}
              style={{ width: `${Math.min(overallPercentage * 100, 100)}%` }}
            />
          </div>
          <span className="text-xs text-text-muted">{Math.round(overallPercentage * 100)}%</span>
        </div>
      </button>

      {/* Expanded view */}
      {expanded && (
        <div className="mt-3 space-y-3 border-t border-border pt-3">
          <MeterItem
            label="Iterations"
            used={budget.iterations_used}
            max={budget.max_iterations}
            icon={<Zap className="h-3 w-3" />}
          />
          <MeterItem
            label="Tokens"
            used={budget.tokens_used}
            max={budget.max_tokens}
            icon={<span className="text-[10px]">TOK</span>}
            unit="k"
          />
          <MeterItem
            label="Wall Time"
            used={budget.wall_time_secs}
            max={budget.max_wall_time_secs}
            icon={<Clock className="h-3 w-3" />}
          />
          <MeterItem
            label="Agent calls"
            used={budget.subagent_runs_used}
            max={budget.max_subagent_runs}
            icon={<Users className="h-3 w-3" />}
          />

          <p className="text-[11px] text-text-subtle">
            Agent calls include planning + (typically) build + validation per task.
          </p>

          {budget.exceeded && budget.exceeded_reason && (
            <div className="rounded-md bg-red-500/10 border border-red-500/30 p-2 text-xs text-red-300">
              Budget exceeded: {budget.exceeded_reason}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
