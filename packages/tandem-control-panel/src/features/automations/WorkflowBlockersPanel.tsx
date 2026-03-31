type WorkflowBlockersPanelProps = {
  blockers: Array<{
    key: string;
    title: string;
    source: string;
    at?: number;
    reason: string;
  }>;
};

export function WorkflowBlockersPanel({ blockers }: WorkflowBlockersPanelProps) {
  if (!blockers.length) return null;

  return (
    <div className="tcp-list-item overflow-visible">
      <div className="mb-2 font-medium">Blockers</div>
      <div className="grid gap-2">
        {blockers.map((blocker) => (
          <div
            key={blocker.key}
            className="rounded-lg border border-emerald-500/30 bg-emerald-950/20 p-3"
          >
            <div className="mb-1 flex flex-wrap items-center gap-2">
              <strong>{blocker.title}</strong>
              <span className="border border-emerald-400/60 bg-emerald-400/10 text-emerald-200 tcp-badge">
                {blocker.source}
              </span>
              {blocker.at ? (
                <span className="tcp-subtle text-[11px]">
                  {new Date(blocker.at).toLocaleTimeString()}
                </span>
              ) : null}
            </div>
            <div className="whitespace-pre-wrap break-words text-sm text-emerald-100/90">
              {blocker.reason}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
