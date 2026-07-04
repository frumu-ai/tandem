import { diffValue, safeString, toArray } from "./scopeInspectorPrimitives";

type ScopeInspectorReplayCheckProps = {
  planPackageReplay: any;
  replayIssues: any[];
  replayRecommendation: any;
  showReplayIssues: boolean;
  onToggleShowReplayIssues: () => void;
};

export function ScopeInspectorReplayCheck({
  planPackageReplay,
  replayIssues,
  replayRecommendation,
  showReplayIssues,
  onToggleShowReplayIssues,
}: ScopeInspectorReplayCheckProps) {
  return (
    <div className="grid gap-1 rounded-lg border border-slate-800/70 bg-slate-950/20 p-2 tcp-text-caption">
      <div className="tcp-subtle uppercase tracking-wide">Replay check</div>
      <div className="flex flex-wrap items-center gap-2 text-slate-100">
        <span className={planPackageReplay.compatible ? "text-emerald-300" : "text-amber-300"}>
          {planPackageReplay.compatible ? "Compatible" : "Drift detected"}
        </span>
        <span>scope: {String(planPackageReplay.scope_metadata_preserved ?? "n/a")}</span>
        <span>handoff: {String(planPackageReplay.handoff_rules_preserved ?? "n/a")}</span>
        <span>
          credentials: {String(planPackageReplay.credential_isolation_preserved ?? "n/a")}
        </span>
        <span>issues: {replayIssues.length}</span>
      </div>
      {replayRecommendation ? (
        <div className="mt-1 flex flex-wrap items-center gap-2 text-slate-200">
          <span
            className={
              replayRecommendation.tone === "success"
                ? "tcp-badge-success"
                : replayRecommendation.tone === "warning"
                  ? "tcp-badge-warning"
                  : "tcp-badge-info"
            }
          >
            recommend: {replayRecommendation.label}
          </span>
          <span className="tcp-subtle">{replayRecommendation.reason}</span>
        </div>
      ) : null}
      {safeString(planPackageReplay?.previous_plan_id) ||
      safeString(planPackageReplay?.next_plan_id) ? (
        <div className="tcp-subtle tcp-text-caption">
          {safeString(planPackageReplay?.previous_plan_id) || "unknown"} · rev{" "}
          {String(planPackageReplay?.previous_plan_revision ?? "n/a")} {"->"}{" "}
          {safeString(planPackageReplay?.next_plan_id) || "unknown"} · rev{" "}
          {String(planPackageReplay?.next_plan_revision ?? "n/a")}
        </div>
      ) : null}
      {toArray(planPackageReplay?.diff_summary).length ? (
        <div className="mt-2 grid gap-2">
          <div className="tcp-subtle uppercase tracking-wide">Plan diff summary</div>
          {toArray(planPackageReplay.diff_summary).map((entry: any, index: number) => (
            <div
              key={`${safeString(entry?.path) || "diff"}-${index}`}
              className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2"
            >
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="font-medium text-slate-100">
                  {safeString(entry?.path) || "unknown path"}
                </div>
                <span className={entry?.blocking ? "tcp-badge-warning" : "tcp-badge-success"}>
                  {entry?.preserved ? "preserved" : "changed"}
                </span>
              </div>
              <div className="mt-2 grid gap-2 sm:grid-cols-2">
                <div>
                  <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">previous</div>
                  <pre className="tcp-code mt-1 max-h-24 overflow-auto tcp-text-caption">
                    {diffValue(entry?.previous_value)}
                  </pre>
                </div>
                <div>
                  <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">next</div>
                  <pre className="tcp-code mt-1 max-h-24 overflow-auto tcp-text-caption">
                    {diffValue(entry?.next_value)}
                  </pre>
                </div>
              </div>
            </div>
          ))}
        </div>
      ) : null}
      {replayIssues.length ? (
        <div className="mt-2 grid gap-2">
          <button
            type="button"
            onClick={onToggleShowReplayIssues}
            className="inline-flex w-fit items-center gap-2 rounded-md border border-slate-700/80 bg-slate-900/70 px-2 py-1 tcp-text-caption font-medium text-slate-100 transition hover:border-slate-500 hover:bg-slate-800"
          >
            <i data-lucide={showReplayIssues ? "chevron-up" : "chevron-down"}></i>
            {showReplayIssues ? "Hide issues" : "Show issues"}
          </button>
          {showReplayIssues ? (
            <div className="grid gap-2">
              {replayIssues
                .slice()
                .sort((left: any, right: any) => Number(right?.blocking) - Number(left?.blocking))
                .map((issue: any, index: number) => (
                  <div
                    key={`${safeString(issue?.code) || "issue"}-${index}`}
                    className={
                      issue?.blocking
                        ? "rounded-md border border-amber-500/40 bg-amber-500/10 p-2 tcp-text-caption text-amber-50"
                        : "rounded-md border border-slate-800/80 bg-slate-950/30 p-2 tcp-text-caption text-slate-200"
                    }
                  >
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-medium">{safeString(issue?.code) || "issue"}</span>
                      <span className="tcp-subtle">path: {safeString(issue?.path) || "n/a"}</span>
                      <span className={issue?.blocking ? "tcp-badge-warning" : "tcp-badge-success"}>
                        {issue?.blocking ? "blocking" : "warning"}
                      </span>
                    </div>
                    <div className="mt-1 text-slate-100">{safeString(issue?.message) || "n/a"}</div>
                  </div>
                ))}
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
