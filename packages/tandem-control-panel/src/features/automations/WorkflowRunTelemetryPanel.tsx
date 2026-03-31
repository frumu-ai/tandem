import { formatJson } from "../../pages/ui";

type WorkflowRunTelemetryPanelProps = {
  selectedLogSource: "all" | "automations" | "context" | "global";
  telemetryEvents: any[];
  isWorkflowRun: boolean;
  filteredRunEventEntries: any[];
  formatTimestampLabel: (value: number) => string;
  onSelectLogSource: (value: "all" | "automations" | "context" | "global") => void;
};

export function WorkflowRunTelemetryPanel({
  selectedLogSource,
  telemetryEvents,
  isWorkflowRun,
  filteredRunEventEntries,
  formatTimestampLabel,
  onSelectLogSource,
}: WorkflowRunTelemetryPanelProps) {
  return (
    <div className="tcp-list-item min-h-0 xl:order-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <div className="font-medium">Run Telemetry</div>
        <div className="flex w-full flex-wrap gap-1 sm:w-auto">
          <button
            className={`tcp-btn h-7 flex-1 px-2 text-[11px] sm:flex-none ${
              selectedLogSource === "all"
                ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                : ""
            }`}
            onClick={() => onSelectLogSource("all")}
          >
            all ({telemetryEvents.length})
          </button>
          <button
            className={`tcp-btn h-7 flex-1 px-2 text-[11px] sm:flex-none ${
              selectedLogSource === "automations"
                ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                : ""
            }`}
            onClick={() => onSelectLogSource("automations")}
          >
            automations
          </button>
          {isWorkflowRun ? (
            <button
              className={`tcp-btn h-7 flex-1 px-2 text-[11px] sm:flex-none ${
                selectedLogSource === "context"
                  ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                  : ""
              }`}
              onClick={() => onSelectLogSource("context")}
            >
              context
            </button>
          ) : null}
          <button
            className={`tcp-btn h-7 flex-1 px-2 text-[11px] sm:flex-none ${
              selectedLogSource === "global"
                ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                : ""
            }`}
            onClick={() => onSelectLogSource("global")}
          >
            global
          </button>
        </div>
      </div>
      {filteredRunEventEntries.length ? (
        <div className="grid gap-2 overflow-auto pr-1 sm:max-h-[12rem]">
          {filteredRunEventEntries
            .slice(-40)
            .reverse()
            .map((item) => (
              <details
                key={item.id}
                className="rounded-lg border border-slate-700/40 bg-slate-900/30 p-2"
              >
                <summary className="cursor-pointer list-none">
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-xs font-medium text-slate-200">{item.label}</span>
                    <span className="tcp-subtle text-[11px]">
                      {formatTimestampLabel(item.at)} · {item.source}
                    </span>
                  </div>
                  <div className="tcp-subtle mt-1 text-xs">{item.detail}</div>
                </summary>
                <pre className="tcp-code mt-2 max-h-40 overflow-auto text-[11px]">
                  {formatJson(item.raw)}
                </pre>
              </details>
            ))}
        </div>
      ) : (
        <div className="tcp-subtle text-xs">No telemetry entries captured yet.</div>
      )}
    </div>
  );
}
