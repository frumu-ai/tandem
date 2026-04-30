import {
  formatTimestamp,
  extractSessionIdsFromRun,
  runSortTimestamp,
  runStatusLabel,
  runSummary,
  shortText,
  type DerivedCoderRun,
} from "./coderRunUtils";

type CoderRunListProps = {
  runs: DerivedCoderRun[];
  selectedRunId: string;
  onSelectRun: (runId: string) => void;
  onOpenAutomationRun?: (runId: string) => void;
  onOpenContextRun?: (runId: string) => void;
};

export function CoderRunList({
  runs,
  selectedRunId,
  onSelectRun,
  onOpenAutomationRun,
  onOpenContextRun,
}: CoderRunListProps) {
  return (
    <div className="space-y-3">
      {runs.map(({ automation, run, coderMetadata }) => {
        const selected = run.run_id === selectedRunId;
        const summary = runSummary(run);
        return (
          <div
            key={run.run_id}
            onClick={() => onSelectRun(run.run_id)}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                onSelectRun(run.run_id);
              }
            }}
            role="button"
            tabIndex={0}
            className={`w-full rounded-xl border p-4 text-left transition-colors ${
              selected
                ? "border-primary bg-primary/10"
                : "border-border bg-surface-elevated/30 hover:bg-surface-elevated/50"
            }`}
          >
            <div className="flex items-start justify-between gap-3">
              <div>
                <div className="text-sm font-semibold text-text">
                  {automation.name || run.run_id}
                </div>
                <div className="mt-1 text-xs uppercase tracking-wide text-text-subtle">
                  {coderMetadata.workflow_kind.replace(/_/g, " ")}
                </div>
              </div>
              <div className="rounded-full border border-border px-2 py-1 text-[11px] uppercase tracking-wide text-text-muted">
                {runStatusLabel(run)}
              </div>
            </div>
            <div className="mt-3 text-xs text-text-muted">
              Workspace: {automation.workspace_root || "Not set"}
            </div>
            {summary ? (
              <div className="mt-2 text-xs leading-5 text-text-muted">
                {shortText(summary, 180)}
              </div>
            ) : null}
            <div className="mt-3 flex flex-wrap gap-2 text-[11px] text-text-subtle">
              <span className="rounded-full border border-border px-2 py-1">Run {run.run_id}</span>
              <span className="rounded-full border border-border px-2 py-1">
                Updated {formatTimestamp(runSortTimestamp(run))}
              </span>
              <span className="rounded-full border border-border px-2 py-1">
                Sessions {extractSessionIdsFromRun(run).length}
              </span>
            </div>
            {onOpenAutomationRun || onOpenContextRun ? (
              <div className="mt-3 flex flex-wrap gap-2">
                {onOpenAutomationRun ? (
                  <button
                    type="button"
                    onClick={(event) => {
                      event.stopPropagation();
                      onOpenAutomationRun(run.run_id);
                    }}
                    className="rounded-full border border-border px-2 py-1 text-[11px] font-medium text-text-muted transition-colors hover:bg-surface hover:text-text"
                  >
                    Agent Automation
                  </button>
                ) : null}
                {onOpenContextRun ? (
                  <button
                    type="button"
                    onClick={(event) => {
                      event.stopPropagation();
                      onOpenContextRun(run.run_id);
                    }}
                    className="rounded-full border border-border px-2 py-1 text-[11px] font-medium text-text-muted transition-colors hover:bg-surface hover:text-text"
                  >
                    Command Center
                  </button>
                ) : null}
              </div>
            ) : null}
          </div>
        );
      })}
    </div>
  );
}
