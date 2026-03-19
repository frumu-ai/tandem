import { useState } from "preact/hooks";
import type { BudgetUsage } from "./types";

function meterColor(percentage: number) {
  if (percentage >= 1) return "bg-rose-500";
  if (percentage >= 0.8) return "bg-amber-500";
  return "bg-emerald-500";
}

function meter(used: number, max: number) {
  if (!max || max <= 0) return 0;
  return Math.min(used / max, 1);
}

function MeterRow({
  label,
  used,
  max,
  unit,
  advisory = false,
}: {
  label: string;
  used: number;
  max: number;
  unit?: string;
  advisory?: boolean;
}) {
  const ratio = meter(used, max);
  const barClass = advisory ? (ratio >= 0.8 ? "bg-sky-400" : "bg-cyan-500") : meterColor(ratio);
  return (
    <div className="grid gap-1">
      <div className="flex items-center justify-between text-xs">
        <span className="tcp-subtle">{label}</span>
        <span className="font-mono text-[11px] text-slate-200">
          {used.toLocaleString()}
          {unit || ""} / {max.toLocaleString()}
          {unit || ""}
        </span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-slate-800/80">
        <div className={`h-full ${barClass}`} style={{ width: `${ratio * 100}%` }} />
      </div>
    </div>
  );
}

export function BudgetMeter({ budget }: { budget: BudgetUsage }) {
  const [expanded, setExpanded] = useState(false);
  const advisory = budget.limits_enforced === false;
  const overall = Math.max(
    meter(budget.iterations_used, budget.max_iterations),
    meter(budget.tokens_used, budget.max_tokens),
    meter(budget.wall_time_secs, budget.max_wall_time_secs),
    meter(budget.subagent_runs_used, budget.max_subagent_runs)
  );

  return (
    <div className="rounded-xl border border-slate-700/60 bg-slate-900/25 p-3">
      <button
        className="flex w-full items-center justify-between"
        onClick={() => setExpanded((v) => !v)}
      >
        <div className="text-sm font-medium">Budget + Tokens</div>
        <div className="flex items-center gap-2">
          <div className="h-2 w-24 overflow-hidden rounded-full bg-slate-800/80">
            <div
              className={`h-full ${advisory ? (overall >= 0.8 ? "bg-sky-400" : "bg-cyan-500") : meterColor(overall)}`}
              style={{ width: `${overall * 100}%` }}
            />
          </div>
          <span className="tcp-subtle text-xs">{Math.round(overall * 100)}%</span>
        </div>
      </button>
      {expanded ? (
        <div className="mt-3 grid gap-3 border-t border-slate-700/60 pt-3">
          <MeterRow
            label="Iterations"
            used={budget.iterations_used}
            max={budget.max_iterations}
            advisory={advisory}
          />
          <MeterRow
            label="Tokens"
            used={budget.tokens_used}
            max={budget.max_tokens}
            advisory={advisory}
          />
          <MeterRow
            label="Wall time"
            used={budget.wall_time_secs}
            max={budget.max_wall_time_secs}
            unit="s"
            advisory={advisory}
          />
          <MeterRow
            label="Agent calls"
            used={budget.subagent_runs_used}
            max={budget.max_subagent_runs}
            advisory={advisory}
          />
          {advisory ? (
            <div className="rounded border border-cyan-500/30 bg-cyan-950/20 p-2 text-xs text-cyan-100">
              Advisory usage only. This run does not expose explicit enforced budget caps, so these
              limits are relaxed defaults for visibility.
            </div>
          ) : null}
          {budget.exceeded && budget.exceeded_reason ? (
            <div className="rounded border border-rose-500/40 bg-rose-950/30 p-2 text-xs text-rose-200">
              {budget.exceeded_reason}
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
