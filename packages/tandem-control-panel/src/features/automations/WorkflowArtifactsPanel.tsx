import { DeferredJson } from "./LazyJson";
import { ExperimentalArtifactBadge } from "./ExecutionProfileBadges";
import { normalizeManagedFilesExplorerPath } from "../files/explorerHandoff";

type WorkflowArtifactEntry = {
  key: string;
  name: string;
  kind?: string;
  paths: string[];
  artifact: unknown;
};

function extractArtifactValidation(artifact: unknown): unknown {
  if (!artifact || typeof artifact !== "object") return null;
  const record = artifact as Record<string, unknown>;
  return (
    record.artifact_validation ||
    record.artifactValidation ||
    (record.validation && typeof record.validation === "object" ? record.validation : null) ||
    null
  );
}

type WorkflowArtifactsPanelProps = {
  artifactCount: number;
  artifactEntries: WorkflowArtifactEntry[];
  selectedArtifactKey: string;
  isWorkflowRun: boolean;
  onToggleArtifact: (key: string) => void;
  onOpenPath?: (path: string) => void;
};

export function WorkflowArtifactsPanel({
  artifactCount,
  artifactEntries,
  selectedArtifactKey,
  isWorkflowRun,
  onToggleArtifact,
  onOpenPath,
}: WorkflowArtifactsPanelProps) {
  return (
    <div className="tcp-list-item overflow-visible">
      <div className="font-medium">Artifacts ({artifactCount})</div>
      {artifactCount ? (
        <div className="mt-2 grid gap-2 overflow-auto pr-1 sm:max-h-40">
          {artifactEntries.map((entry) => (
            <details
              key={entry.key}
              open={selectedArtifactKey === entry.key ? true : undefined}
              className={
                selectedArtifactKey === entry.key
                  ? "rounded-lg border border-sky-500/40 bg-sky-950/10 p-2"
                  : "rounded-lg border border-slate-700/40 bg-slate-900/25 p-2"
              }
            >
              <summary
                className="cursor-pointer list-none"
                onClick={() => onToggleArtifact(entry.key)}
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="text-xs font-medium text-slate-200">{entry.name}</span>
                  <span className="flex items-center gap-1 tcp-text-caption">
                    <ExperimentalArtifactBadge
                      validation={extractArtifactValidation(entry.artifact)}
                    />
                    <span className="tcp-subtle">{entry.kind || "artifact"}</span>
                  </span>
                </div>
              </summary>
              {entry.paths.length ? (
                <div className="mt-2 flex flex-wrap gap-1">
                  {entry.paths.map((path) => (
                    <button
                      key={path}
                      type="button"
                      className={`rounded-full border border-slate-700/70 bg-slate-950/30 px-2 py-1 font-mono tcp-text-caption text-slate-300 ${normalizeManagedFilesExplorerPath(path) && onOpenPath ? "cursor-pointer hover:border-sky-500/40 hover:text-sky-100" : "cursor-default"}`.trim()}
                      onClick={() => {
                        const normalized = normalizeManagedFilesExplorerPath(path);
                        if (!normalized || !onOpenPath) return;
                        onOpenPath(normalized);
                      }}
                      disabled={!normalizeManagedFilesExplorerPath(path) || !onOpenPath}
                    >
                      {path}
                    </button>
                  ))}
                </div>
              ) : null}
              <DeferredJson
                value={entry.artifact}
                open={selectedArtifactKey === entry.key}
                className="tcp-code mt-2 max-h-32 overflow-auto tcp-text-caption"
              />
            </details>
          ))}
        </div>
      ) : (
        <div className="tcp-subtle mt-2 text-xs">
          {isWorkflowRun
            ? "No blackboard artifacts have been recorded for this workflow run yet."
            : "No run artifacts were persisted for this automation."}
        </div>
      )}
    </div>
  );
}
