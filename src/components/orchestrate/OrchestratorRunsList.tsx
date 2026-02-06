import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Sparkles } from "lucide-react";
import { type RunSummary } from "./types";

interface OrchestratorRunsListProps {
  onRunSelect: (runId: string) => void;
  currentRunId?: string;
}

export function OrchestratorRunsList({ onRunSelect, currentRunId }: OrchestratorRunsListProps) {
  const [runs, setRuns] = useState<RunSummary[]>([]);

  const loadRuns = useCallback(async () => {
    try {
      const runs = await invoke<RunSummary[]>("orchestrator_list_runs");
      setRuns(runs);
    } catch (error) {
      console.error("Failed to load orchestrator runs:", error);
    }
  }, []);

  useEffect(() => {
    const initialTimeout = setTimeout(() => {
      void loadRuns();
    }, 0);
    // Poll for updates every 5 seconds
    const interval = setInterval(() => {
      void loadRuns();
    }, 5000);
    return () => {
      clearTimeout(initialTimeout);
      clearInterval(interval);
    };
  }, [loadRuns]);

  // Group runs by status
  const activeRuns = runs.filter((r) =>
    ["planning", "awaiting_approval", "executing", "paused"].includes(r.status)
  );

  const completedRuns = runs
    .filter(
      (r) =>
        ["completed", "failed", "cancelled"].includes(r.status) ||
        // Include any other statuses as recently completed if not active
        !["planning", "awaiting_approval", "executing", "paused"].includes(r.status)
    )
    .slice(0, 5); // Limit to 5 most recent

  if (activeRuns.length === 0 && completedRuns.length === 0) return null;

  const RunItem = ({ run }: { run: RunSummary }) => (
    <button
      key={run.run_id}
      onClick={() => onRunSelect(run.run_id)}
      className={`w-full rounded px-2 py-1.5 text-left transition-colors hover:bg-surface-elevated ${
        currentRunId === run.run_id ? "bg-surface-elevated" : ""
      }`}
    >
      <div className="flex items-center gap-2">
        <Sparkles
          className={`h-3 w-3 flex-shrink-0 ${
            ["completed", "done"].includes(run.status)
              ? "text-success"
              : ["failed", "cancelled"].includes(run.status)
                ? "text-destructive"
                : "text-primary"
          }`}
        />
        <span className="truncate text-xs text-text">
          {run.objective.length > 40 ? `${run.objective.substring(0, 40)}...` : run.objective}
        </span>
      </div>
      <div className="mt-0.5 text-[10px] text-text-muted flex justify-between">
        <span>{run.status.replace("_", " ")}</span>
        <span>{new Date(run.updated_at).toLocaleDateString()}</span>
      </div>
    </button>
  );

  return (
    <div className="space-y-3">
      {activeRuns.length > 0 && (
        <div className="space-y-1">
          <div className="px-2 text-xs font-medium text-text-subtle">Active Orchestrations</div>
          {activeRuns.map((run) => (
            <RunItem key={run.run_id} run={run} />
          ))}
        </div>
      )}

      {completedRuns.length > 0 && (
        <div className="space-y-1">
          <div className="px-2 text-xs font-medium text-text-muted">Recent</div>
          {completedRuns.map((run) => (
            <RunItem key={run.run_id} run={run} />
          ))}
        </div>
      )}
    </div>
  );
}
