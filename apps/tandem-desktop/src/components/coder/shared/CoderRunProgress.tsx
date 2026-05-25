import type { AutomationV2RunRecord } from "@/lib/tauri";
import { runProgress, runStatusTone } from "./coderRunUtils";

type CoderRunProgressProps = {
  run: AutomationV2RunRecord | null;
  showLabel?: boolean;
  className?: string;
};

export function CoderRunProgress({ run, showLabel = true, className = "" }: CoderRunProgressProps) {
  const progress = runProgress(run);
  if (progress.total === 0) return null;
  const tone = runStatusTone(run);
  const barColor =
    tone === "failed"
      ? "bg-red-500/70"
      : tone === "awaiting" || tone === "paused"
        ? "bg-amber-400/70"
        : tone === "completed"
          ? "bg-emerald-500/70"
          : "bg-primary/70";
  return (
    <div className={`space-y-1 ${className}`}>
      {showLabel ? (
        <div className="flex items-center justify-between text-[11px] text-text-subtle">
          <span>
            {progress.completed} of {progress.total} steps
            {progress.blocked > 0 ? ` • ${progress.blocked} blocked` : ""}
          </span>
          <span>{progress.percent}%</span>
        </div>
      ) : null}
      <div className="h-1 w-full overflow-hidden rounded-full bg-surface-elevated">
        <div
          className={`h-full rounded-full transition-all duration-500 ${barColor}`}
          style={{ width: `${Math.max(progress.percent, progress.completed > 0 ? 3 : 0)}%` }}
        />
      </div>
    </div>
  );
}
