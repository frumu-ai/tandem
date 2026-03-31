import { formatJson } from "../../pages/ui";

type WorkflowTaskSignalsPanelProps = {
  selectedBoardTask: any;
  selectedBoardTaskTelemetry: any;
  selectedBoardTaskArtifactValidation: any;
  selectedBoardTaskTouchedFiles: string[];
  selectedBoardTaskUndeclaredFiles: string[];
  selectedBoardTaskRequestedQualityMode: string;
  selectedBoardTaskEmergencyRollbackEnabled: boolean | null;
  selectedBoardTaskBlockerCategory: string;
  selectedBoardTaskValidationBasis: any;
  selectedBoardTaskReceiptTimeline: any[];
};

export function WorkflowTaskSignalsPanel({
  selectedBoardTask,
  selectedBoardTaskTelemetry,
  selectedBoardTaskArtifactValidation,
  selectedBoardTaskTouchedFiles,
  selectedBoardTaskUndeclaredFiles,
  selectedBoardTaskRequestedQualityMode,
  selectedBoardTaskEmergencyRollbackEnabled,
  selectedBoardTaskBlockerCategory,
  selectedBoardTaskValidationBasis,
  selectedBoardTaskReceiptTimeline,
}: WorkflowTaskSignalsPanelProps) {
  return (
    <>
      {selectedBoardTaskArtifactValidation ||
      selectedBoardTaskTelemetry ||
      selectedBoardTaskTouchedFiles.length ||
      selectedBoardTaskUndeclaredFiles.length ? (
        <div className="rounded-lg border border-slate-700/60 bg-slate-950/20 p-3 text-xs text-slate-300">
          <div className="font-medium text-slate-200">Coding Signals</div>
          <div className="mt-3 grid gap-2 sm:grid-cols-2">
            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
              <div className="tcp-subtle">execution mode</div>
              <div className="mt-1 font-medium text-slate-100">
                {String(selectedBoardTaskArtifactValidation?.execution_policy?.mode || "").trim() ||
                  "n/a"}
              </div>
            </div>
            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
              <div className="tcp-subtle">git diff</div>
              <div className="mt-1 font-medium text-slate-100">
                {String(selectedBoardTaskArtifactValidation?.git_diff_summary?.stat || "").trim() ||
                  "n/a"}
              </div>
            </div>
            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
              <div className="tcp-subtle">workspace inspection</div>
              <div className="mt-1 font-medium text-slate-100">
                {selectedBoardTaskTelemetry?.workspace_inspection_used ? "yes" : "no"}
              </div>
            </div>
            <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
              <div className="tcp-subtle">web research</div>
              <div className="mt-1 font-medium text-slate-100">
                {selectedBoardTaskTelemetry?.web_research_used ? "yes" : "no"}
              </div>
            </div>
          </div>
          <div className="mt-3 space-y-2">
            <div>
              <div className="tcp-subtle mb-1">touched files</div>
              {selectedBoardTaskTouchedFiles.length ? (
                <div className="flex flex-wrap gap-1">
                  {selectedBoardTaskTouchedFiles.map((file) => (
                    <span
                      key={file}
                      className="rounded-full border border-slate-700/70 bg-slate-950/30 px-2 py-1 font-mono text-[11px] text-slate-300"
                    >
                      {file}
                    </span>
                  ))}
                </div>
              ) : (
                <div className="tcp-subtle">none</div>
              )}
            </div>
            <div>
              <div className="tcp-subtle mb-1">undeclared files</div>
              {selectedBoardTaskUndeclaredFiles.length ? (
                <div className="flex flex-wrap gap-1">
                  {selectedBoardTaskUndeclaredFiles.map((file) => (
                    <span
                      key={file}
                      className="rounded-full border border-amber-500/30 bg-amber-950/20 px-2 py-1 font-mono text-[11px] text-amber-100"
                    >
                      {file}
                    </span>
                  ))}
                </div>
              ) : (
                <div className="tcp-subtle">none</div>
              )}
            </div>
          </div>
        </div>
      ) : null}
      {String(selectedBoardTask?.state || "").toLowerCase() === "blocked" ? (
        <div className="rounded-lg border border-slate-700/60 bg-slate-950/30 p-3 text-xs text-slate-300">
          Continue resets this blocked step and its descendants, preserves valid upstream outputs,
          and clears stale descendant artifacts before requeue.
        </div>
      ) : null}
      {selectedBoardTaskTelemetry ? (
        <div className="rounded-lg border border-slate-700/60 bg-slate-950/30 p-3 text-xs text-slate-300">
          <div className="mb-2 font-medium text-slate-100">Node Tooling</div>
          <div className="grid gap-1">
            <div>
              offered:{" "}
              {Array.isArray(selectedBoardTaskTelemetry?.requested_tools)
                ? selectedBoardTaskTelemetry.requested_tools.join(", ") || "n/a"
                : "n/a"}
            </div>
            <div>
              executed:{" "}
              {Array.isArray(selectedBoardTaskTelemetry?.executed_tools)
                ? selectedBoardTaskTelemetry.executed_tools.join(", ") || "none"
                : "none"}
            </div>
            <div>
              workspace inspection:{" "}
              {selectedBoardTaskTelemetry?.workspace_inspection_used ? "yes" : "no"}
            </div>
            <div>web research: {selectedBoardTaskTelemetry?.web_research_used ? "yes" : "no"}</div>
          </div>
        </div>
      ) : null}
      {selectedBoardTaskArtifactValidation ? (
        <div className="rounded-lg border border-slate-700/60 bg-slate-950/30 p-3 text-xs text-slate-300">
          <div className="mb-2 font-medium text-slate-100">Artifact Validation</div>
          <div className="grid gap-1">
            <div>
              accepted path:{" "}
              {String(selectedBoardTaskArtifactValidation?.accepted_artifact_path || "").trim() ||
                "n/a"}
            </div>
            <div>
              rejected reason:{" "}
              {String(selectedBoardTaskArtifactValidation?.rejected_artifact_reason || "").trim() ||
                "none"}
            </div>
            <div>
              auto-cleaned: {String(Boolean(selectedBoardTaskArtifactValidation?.auto_cleaned))}
            </div>
            <div>
              undeclared files:{" "}
              {selectedBoardTaskUndeclaredFiles.length
                ? selectedBoardTaskUndeclaredFiles.join(", ")
                : "none"}
            </div>
            <div>
              execution policy:{" "}
              {String(selectedBoardTaskArtifactValidation?.execution_policy?.mode || "").trim() ||
                "n/a"}
            </div>
            <div>blocker category: {selectedBoardTaskBlockerCategory || "none"}</div>
            <div>
              validation basis:{" "}
              {selectedBoardTaskValidationBasis
                ? String(
                    selectedBoardTaskValidationBasis?.authority ||
                      selectedBoardTaskValidationBasis?.mode ||
                      selectedBoardTaskValidationBasis?.status ||
                      ""
                  ).trim() || "present"
                : "none"}
            </div>
            <div>
              touched files:{" "}
              {selectedBoardTaskTouchedFiles.length
                ? selectedBoardTaskTouchedFiles.join(", ")
                : "none"}
            </div>
            <div>
              git diff:{" "}
              {String(selectedBoardTaskArtifactValidation?.git_diff_summary?.stat || "").trim() ||
                "n/a"}
            </div>
          </div>
          <div className="mt-2 grid gap-2 sm:grid-cols-2">
            <div className="rounded-md border border-slate-700/60 bg-black/10 p-2">
              <div className="tcp-subtle">requested quality mode</div>
              <div className="mt-1 font-medium text-slate-100">
                {selectedBoardTaskRequestedQualityMode || "none"}
              </div>
            </div>
            <div className="rounded-md border border-slate-700/60 bg-black/10 p-2">
              <div className="tcp-subtle">emergency rollback</div>
              <div className="mt-1 font-medium text-slate-100">
                {selectedBoardTaskEmergencyRollbackEnabled === null
                  ? "n/a"
                  : selectedBoardTaskEmergencyRollbackEnabled
                    ? "enabled"
                    : "disabled"}
              </div>
            </div>
          </div>
          {selectedBoardTaskValidationBasis ? (
            <details className="mt-2 rounded-md border border-slate-700/60 bg-black/10 p-2">
              <summary className="cursor-pointer text-[11px] text-slate-400">
                Validation basis
              </summary>
              <pre className="tcp-code mt-2 max-h-40 overflow-auto text-[11px]">
                {formatJson(selectedBoardTaskValidationBasis)}
              </pre>
            </details>
          ) : null}
          <div className="mt-3">
            <div className="tcp-subtle mb-1">receipt timeline</div>
            {selectedBoardTaskReceiptTimeline.length ? (
              <div className="grid max-h-56 gap-1 overflow-auto pr-1">
                {selectedBoardTaskReceiptTimeline.slice(-12).map((entry: any, index: number) => (
                  <details
                    key={`${String(entry?.seq || index)}:${String(entry?.eventType || entry?.event_type || "receipt")}`}
                    className="rounded-md border border-slate-700/60 bg-slate-950/20 px-2 py-1.5"
                  >
                    <summary className="cursor-pointer list-none">
                      <div className="flex items-center justify-between gap-2">
                        <span className="text-[11px] font-medium text-slate-200">
                          {String(
                            entry?.eventType || entry?.event_type || entry?.receiptKind || "receipt"
                          )}
                        </span>
                        <span className="tcp-subtle text-[10px]">
                          seq {String(entry?.seq || index + 1)}
                        </span>
                      </div>
                      <div className="tcp-subtle mt-0.5 text-[11px]">
                        {String(entry?.detail || entry?.summary || "").trim() ||
                          "No summary available."}
                      </div>
                    </summary>
                    <pre className="tcp-code mt-2 max-h-32 overflow-auto text-[10px]">
                      {formatJson(entry?.raw || entry)}
                    </pre>
                  </details>
                ))}
              </div>
            ) : (
              <div className="tcp-subtle text-xs">none</div>
            )}
          </div>
        </div>
      ) : null}
    </>
  );
}
