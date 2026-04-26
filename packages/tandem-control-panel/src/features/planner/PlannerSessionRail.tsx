import { Badge } from "../../ui/index.tsx";
import {
  type PlannerSessionSummary,
  plannerSessionStatusLabel,
  plannerSessionStatusTone,
} from "./plannerShared";

type PlannerSessionRailProps = {
  sessions: PlannerSessionSummary[];
  selectedSessionId: string;
  onSelectSession: (sessionId: string) => void;
  onCreateSession: () => void;
  onRenameSession: (sessionId: string) => void;
  onDuplicateSession: (sessionId: string) => void;
  onDeleteSession: (sessionId: string) => void;
  className?: string;
};

export function PlannerSessionRail({
  sessions,
  selectedSessionId,
  onSelectSession,
  onCreateSession,
  onRenameSession,
  onDuplicateSession,
  onDeleteSession,
  className = "",
}: PlannerSessionRailProps) {
  return (
    <section
      className={`flex min-h-0 flex-col rounded-2xl border border-white/10 bg-black/20 p-3 ${className}`.trim()}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-xs uppercase tracking-wide text-slate-500">Planner sessions</div>
          <div className="tcp-subtle text-xs">
            Draft threads only. Model work starts when you generate or revise a plan.
          </div>
        </div>
        <button type="button" className="tcp-btn shrink-0" onClick={onCreateSession}>
          <i data-lucide="plus"></i>
          New plan
        </button>
      </div>

      <div className="mt-3 min-h-0 flex-1 space-y-2 overflow-auto pr-1">
        {sessions.length ? (
          sessions.map((session) => {
            const active = session.id === selectedSessionId;
            return (
              <article
                key={session.id}
                className={`rounded-xl border p-2 transition ${
                  active
                    ? "border-sky-500/40 bg-sky-500/10"
                    : "border-white/10 bg-black/10 hover:border-white/20 hover:bg-black/20"
                }`}
              >
                <button
                  type="button"
                  className="block w-full text-left"
                  onClick={() => onSelectSession(session.id)}
                  title={session.title}
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0">
                      <div className="truncate text-sm font-medium text-slate-100">
                        {session.title || "Untitled plan"}
                      </div>
                      <div className="mt-1 flex flex-wrap items-center gap-2 text-[11px] text-slate-500">
                        <span>{session.updatedAtLabel}</span>
                        {session.revisionCount ? <span>{session.revisionCount} rev</span> : null}
                      </div>
                    </div>
                    <Badge tone={plannerSessionStatusTone(session.status)} className="shrink-0">
                      {plannerSessionStatusLabel(session.status)}
                    </Badge>
                  </div>
                  <p className="mt-2 line-clamp-2 text-xs text-slate-400">{session.previewText}</p>
                </button>

                <div className="mt-2 flex flex-wrap gap-2">
                  <button
                    type="button"
                    className="tcp-btn h-7 px-2 text-[11px]"
                    onClick={() => onRenameSession(session.id)}
                    title="Rename session"
                  >
                    <i data-lucide="pencil"></i>
                  </button>
                  <button
                    type="button"
                    className="tcp-btn h-7 px-2 text-[11px]"
                    onClick={() => onDuplicateSession(session.id)}
                    title="Duplicate session"
                  >
                    <i data-lucide="copy"></i>
                  </button>
                  <button
                    type="button"
                    className="tcp-btn h-7 px-2 text-[11px]"
                    onClick={() => onDeleteSession(session.id)}
                    title="Delete session"
                  >
                    <i data-lucide="trash-2"></i>
                  </button>
                </div>
              </article>
            );
          })
        ) : (
          <div className="rounded-xl border border-dashed border-white/10 bg-black/10 p-3">
            <div className="text-sm text-slate-100">No planner sessions yet.</div>
            <div className="mt-1 text-xs text-slate-500">
              Start a new plan when you are ready to create the first draft thread.
            </div>
            <button type="button" className="tcp-btn mt-3" onClick={onCreateSession}>
              <i data-lucide="plus"></i>
              Start new plan
            </button>
          </div>
        )}
      </div>
    </section>
  );
}
