import { AlertCircle, ExternalLink, GitBranch } from "lucide-react";
import { CoderRunStatusBadge } from "./CoderRunStatusBadge";
import { CoderRunProgress } from "./CoderRunProgress";
import {
  extractSessionIdsFromRun,
  relativeTimeFromMs,
  runAwaitingGate,
  runSortTimestamp,
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
    <div className="space-y-2">
      {runs.map(({ automation, run, coderMetadata }) => {
        const selected = run.run_id === selectedRunId;
        const summary = runSummary(run);
        const awaitingGate = runAwaitingGate(run);
        const sessionCount = extractSessionIdsFromRun(run).length;
        const workflowKind = coderMetadata.workflow_kind.replace(/_/g, " ");
        const workspace = automation.workspace_root || "";
        const workspaceLabel = workspace
          ? workspace.split("/").filter(Boolean).slice(-2).join("/")
          : "Workspace not set";

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
            aria-selected={selected}
            className={`group relative cursor-pointer rounded-xl border bg-surface-elevated/30 text-left transition-all ${
              selected
                ? "border-primary/70 bg-primary/10 ring-1 ring-primary/40"
                : "border-border hover:border-border-subtle hover:bg-surface-elevated/60"
            }`}
          >
            {selected ? (
              <span
                className="absolute inset-y-2 left-0 w-1 rounded-r-full bg-primary"
                aria-hidden
              />
            ) : null}
            <div className="space-y-3 p-3 pl-4">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-semibold text-text">
                    {automation.name || run.run_id}
                  </div>
                  <div className="mt-0.5 flex items-center gap-1.5 text-[11px] text-text-subtle">
                    <span className="capitalize">{workflowKind}</span>
                    <span aria-hidden>·</span>
                    <GitBranch className="h-3 w-3" aria-hidden />
                    <span className="truncate">{workspaceLabel}</span>
                  </div>
                </div>
                <CoderRunStatusBadge run={run} />
              </div>

              {awaitingGate ? (
                <div className="flex items-start gap-2 rounded-lg border border-amber-300/40 bg-amber-300/10 px-2.5 py-1.5 text-[11px] text-amber-100">
                  <AlertCircle className="mt-0.5 h-3 w-3 flex-shrink-0" aria-hidden />
                  <span className="truncate">
                    Waiting on you:{" "}
                    {String(awaitingGate.title || awaitingGate.node_id || "operator decision")}
                  </span>
                </div>
              ) : summary ? (
                <div className="text-[11px] leading-4 text-text-muted">
                  {shortText(summary, 140)}
                </div>
              ) : null}

              <CoderRunProgress run={run} showLabel={false} />

              <div className="flex flex-wrap items-center justify-between gap-2 text-[10px] text-text-subtle">
                <span>{relativeTimeFromMs(runSortTimestamp(run))}</span>
                <div className="flex items-center gap-2">
                  {sessionCount > 0 ? (
                    <span>
                      {sessionCount} session{sessionCount === 1 ? "" : "s"}
                    </span>
                  ) : null}
                  {onOpenAutomationRun || onOpenContextRun ? (
                    <span className="flex items-center gap-1">
                      {onOpenAutomationRun ? (
                        <button
                          type="button"
                          onClick={(event) => {
                            event.stopPropagation();
                            onOpenAutomationRun(run.run_id);
                          }}
                          title="Open in Agent Automation"
                          className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-text-muted/80 transition-colors hover:bg-surface hover:text-text"
                        >
                          <ExternalLink className="h-3 w-3" aria-hidden />
                          Automation
                        </button>
                      ) : null}
                      {onOpenContextRun ? (
                        <button
                          type="button"
                          onClick={(event) => {
                            event.stopPropagation();
                            onOpenContextRun(run.run_id);
                          }}
                          title="Open in Command Center"
                          className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-text-muted/80 transition-colors hover:bg-surface hover:text-text"
                        >
                          <ExternalLink className="h-3 w-3" aria-hidden />
                          Command
                        </button>
                      ) : null}
                    </span>
                  ) : null}
                </div>
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}
