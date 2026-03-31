import type { RefObject } from "react";
import { formatJson } from "../../pages/ui";

type WorkflowLiveSessionLogPanelProps = {
  selectedSessionId: string;
  selectedSessionFilterId: string;
  availableSessionIds: string[];
  sessionLogEntries: any[];
  sessionLogRef: RefObject<HTMLDivElement | null>;
  compactIdentifier: (value: string, length?: number) => string;
  sessionLabel: (value: string) => string;
  onSessionFilterChange: (value: string) => void;
  onCopySessionLog: () => void;
  onJumpToLatest: () => void;
  onPinnedStateChange: (pinned: boolean) => void;
};

export function WorkflowLiveSessionLogPanel({
  selectedSessionId,
  selectedSessionFilterId,
  availableSessionIds,
  sessionLogEntries,
  sessionLogRef,
  compactIdentifier,
  sessionLabel,
  onSessionFilterChange,
  onCopySessionLog,
  onJumpToLatest,
  onPinnedStateChange,
}: WorkflowLiveSessionLogPanelProps) {
  return (
    <div className="tcp-list-item min-h-0 xl:order-2">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <div>
          <div className="font-medium">Live Session Log</div>
          <div className="tcp-subtle text-xs">
            {selectedSessionId
              ? selectedSessionFilterId === "all"
                ? `Merged timeline across ${availableSessionIds.length || 1} session${
                    availableSessionIds.length === 1 ? "" : "s"
                  }`
                : `Filtered to ${selectedSessionFilterId}`
              : "This run does not expose a session transcript."}
          </div>
        </div>
        <div className="flex flex-wrap gap-2">
          {availableSessionIds.length > 1 ? (
            <select
              className="tcp-select h-7 min-w-[12rem] max-w-full shrink-0 text-xs sm:min-w-[14rem]"
              value={selectedSessionFilterId}
              onInput={(e) => onSessionFilterChange((e.target as HTMLSelectElement).value)}
            >
              <option value="all">All sessions</option>
              {availableSessionIds.map((sessionId) => (
                <option key={sessionId} value={sessionId} title={sessionId}>
                  {sessionLabel(sessionId)}
                </option>
              ))}
            </select>
          ) : selectedSessionId ? (
            <span className="tcp-badge-info" title={selectedSessionId}>
              {sessionLabel(selectedSessionId)}
            </span>
          ) : null}
          {selectedSessionId ? (
            <span className="tcp-badge-info" title={selectedSessionId}>
              live: {compactIdentifier(selectedSessionId, 24)}
            </span>
          ) : null}
          <button
            className="tcp-btn h-7 px-2 text-xs"
            disabled={!sessionLogEntries.length}
            onClick={onCopySessionLog}
          >
            <i data-lucide="copy"></i>
            Copy session log
          </button>
          <button className="tcp-btn h-7 px-2 text-xs" onClick={onJumpToLatest}>
            <i data-lucide="arrow-down"></i>
            Jump to latest
          </button>
        </div>
      </div>
      <div
        ref={sessionLogRef}
        className="grid min-h-[12rem] gap-2 overflow-auto overscroll-contain pr-1 sm:min-h-[14rem] sm:max-h-[18rem]"
        onScroll={(event) => {
          const el = event.currentTarget;
          const pinned = el.scrollHeight - (el.scrollTop + el.clientHeight) < 48;
          onPinnedStateChange(pinned);
        }}
      >
        {sessionLogEntries.length ? (
          sessionLogEntries.map((entry) => (
            <div
              key={entry.id}
              className={`rounded-lg border p-3 ${
                entry.variant === "assistant"
                  ? "border-sky-500/30 bg-sky-950/10"
                  : entry.variant === "user"
                    ? "border-slate-600/60 bg-slate-900/35"
                    : entry.variant === "error"
                      ? "border-rose-500/35 bg-rose-950/20"
                      : "border-slate-700/50 bg-slate-900/25"
              }`}
            >
              <div className="mb-1 flex flex-wrap items-center justify-between gap-2">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-xs font-medium uppercase tracking-wide text-slate-200">
                    {entry.label}
                  </span>
                  {entry.sessionId ? (
                    <span className="tcp-badge-info text-[10px]">{entry.sessionLabel}</span>
                  ) : null}
                </div>
                <span className="tcp-subtle text-[11px]">
                  {new Date(entry.at).toLocaleTimeString()}
                </span>
              </div>
              {entry.body ? (
                <div className="whitespace-pre-wrap break-words text-sm text-slate-100">
                  {entry.body}
                </div>
              ) : (
                <div className="tcp-subtle text-xs">No textual body.</div>
              )}
              {entry.kind === "message" &&
              entry.parts.some((part: any) => String(part?.type || "") === "tool") ? (
                <details className="mt-2">
                  <summary className="cursor-pointer text-xs text-slate-400">Tool payloads</summary>
                  <pre className="tcp-code mt-2 max-h-40 overflow-auto text-[11px]">
                    {formatJson(entry.parts)}
                  </pre>
                </details>
              ) : null}
              {entry.kind === "event" ? (
                <details className="mt-2">
                  <summary className="cursor-pointer text-xs text-slate-400">Raw event</summary>
                  <pre className="tcp-code mt-2 max-h-40 overflow-auto text-[11px]">
                    {formatJson(entry.raw)}
                  </pre>
                </details>
              ) : null}
            </div>
          ))
        ) : (
          <div className="tcp-subtle text-xs">
            {availableSessionIds.length
              ? "Waiting for session transcript or live session events."
              : "This run does not expose a session transcript."}
          </div>
        )}
      </div>
    </div>
  );
}
