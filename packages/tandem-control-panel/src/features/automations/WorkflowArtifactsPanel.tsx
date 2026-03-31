import { formatJson } from "../../pages/ui";

type WorkflowArtifactEntry = {
  key: string;
  name: string;
  kind?: string;
  paths: string[];
  artifact: unknown;
};

type WorkflowArtifactsPanelProps = {
  artifactCount: number;
  artifactEntries: WorkflowArtifactEntry[];
  selectedArtifactKey: string;
  isWorkflowRun: boolean;
  onToggleArtifact: (key: string) => void;
};

export function WorkflowArtifactsPanel({
  artifactCount,
  artifactEntries,
  selectedArtifactKey,
  isWorkflowRun,
  onToggleArtifact,
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
                  <span className="tcp-subtle text-[11px]">{entry.kind || "artifact"}</span>
                </div>
              </summary>
              {entry.paths.length ? (
                <div className="mt-2 flex flex-wrap gap-1">
                  {entry.paths.map((path) => (
                    <span
                      key={path}
                      className="rounded-full border border-slate-700/70 bg-slate-950/30 px-2 py-1 font-mono text-[11px] text-slate-300"
                    >
                      {path}
                    </span>
                  ))}
                </div>
              ) : null}
              <pre className="tcp-code mt-2 max-h-32 overflow-auto text-[11px]">
                {formatJson(entry.artifact)}
              </pre>
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
