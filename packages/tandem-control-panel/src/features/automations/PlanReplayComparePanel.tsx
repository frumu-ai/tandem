import { formatJson } from "../../pages/ui";

type PlanReplayComparePanelProps = {
  planPackageReplay: any | null;
};

type ReplayDiffEntry = {
  path?: string;
  previous_value?: unknown;
  next_value?: unknown;
  blocking?: boolean;
  preserved?: boolean;
};

type ReplayIssue = {
  code?: string;
  path?: string;
  message?: string;
  blocking?: boolean;
};

const CATEGORY_ORDER = [
  "plan metadata",
  "scope/runtime",
  "routines",
  "credentials",
  "context objects",
  "other",
];

function safeString(value: unknown) {
  return String(value || "").trim();
}

function toArray(value: unknown) {
  return Array.isArray(value) ? value : [];
}

function diffValue(value: unknown) {
  if (value === null || value === undefined) return "n/a";
  if (typeof value === "string") return value;
  return formatJson(value);
}

function compareTone(entry: ReplayDiffEntry) {
  if (entry.blocking) return "warning";
  if (entry.preserved) return "success";
  return "info";
}

function compareBadge(entry: ReplayDiffEntry) {
  const tone = compareTone(entry);
  const className =
    tone === "success"
      ? "tcp-badge-success"
      : tone === "warning"
        ? "tcp-badge-warning"
        : "tcp-badge-info";
  return <span className={className}>{entry.preserved ? "preserved" : "changed"}</span>;
}

function issueBadge(issue: ReplayIssue) {
  return (
    <span className={issue.blocking ? "tcp-badge-warning" : "tcp-badge-success"}>
      {issue.blocking ? "blocking" : "warning"}
    </span>
  );
}

function replayCompareCategory(path: string) {
  const normalized = safeString(path).toLowerCase();
  if (
    normalized === "plan_id" ||
    normalized === "plan_revision" ||
    normalized === "lifecycle_state" ||
    normalized.startsWith("owner") ||
    normalized.startsWith("mission") ||
    normalized.startsWith("metadata")
  ) {
    return "plan metadata";
  }
  if (
    normalized.startsWith("output_roots") ||
    normalized.startsWith("inter_routine_policy") ||
    normalized.startsWith("approval_policy") ||
    normalized.startsWith("trigger_policy") ||
    normalized.startsWith("validation_state")
  ) {
    return "scope/runtime";
  }
  if (
    normalized.startsWith("routine_graph") ||
    normalized.startsWith("routine_scopes") ||
    normalized.startsWith("dependency_resolution") ||
    normalized.startsWith("model_routing_resolution")
  ) {
    return "routines";
  }
  if (
    normalized.startsWith("connector_bindings") ||
    normalized.startsWith("connector_binding_resolution") ||
    normalized.startsWith("credential_envelopes") ||
    normalized.startsWith("credential")
  ) {
    return "credentials";
  }
  if (normalized.startsWith("context_object") || normalized.startsWith("context_objects")) {
    return "context objects";
  }
  return "other";
}

function groupReplayDiffEntries(entries: ReplayDiffEntry[]) {
  const buckets = new Map<string, ReplayDiffEntry[]>();
  entries.forEach((entry) => {
    const key = replayCompareCategory(safeString(entry.path));
    if (!buckets.has(key)) {
      buckets.set(key, []);
    }
    buckets.get(key)?.push(entry);
  });

  return CATEGORY_ORDER.map((label) => ({
    key: label,
    label,
    entries: buckets.get(label) || [],
  })).filter((category) => category.entries.length > 0);
}

function summaryValue(value: unknown) {
  if (value === null || value === undefined) return "n/a";
  if (typeof value === "boolean") return value ? "yes" : "no";
  return String(value);
}

export function PlanReplayComparePanel({ planPackageReplay }: PlanReplayComparePanelProps) {
  const diffEntries = toArray(planPackageReplay?.diff_summary) as ReplayDiffEntry[];
  const issues = toArray(planPackageReplay?.issues) as ReplayIssue[];
  const blockingIssues = issues.filter((issue) => issue?.blocking);
  const groupedEntries = groupReplayDiffEntries(diffEntries);
  const sortedIssues = issues
    .slice()
    .sort((left, right) => Number(right?.blocking) - Number(left?.blocking));

  if (!planPackageReplay) return null;

  return (
    <div className="grid gap-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="font-medium text-slate-200">Revision compare</div>
        <span className={planPackageReplay.compatible ? "tcp-badge-success" : "tcp-badge-warning"}>
          {planPackageReplay.compatible ? "compatible" : "drift detected"}
        </span>
      </div>
      <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3">
        <div className="grid gap-2 sm:grid-cols-4">
          <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">previous plan</div>
            <div className="mt-1 break-words text-slate-100">
              {summaryValue(planPackageReplay?.previous_plan_id) || "n/a"}
            </div>
            <div className="tcp-subtle tcp-text-caption">
              rev {summaryValue(planPackageReplay?.previous_plan_revision)}
            </div>
          </div>
          <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">next plan</div>
            <div className="mt-1 break-words text-slate-100">
              {summaryValue(planPackageReplay?.next_plan_id) || "n/a"}
            </div>
            <div className="tcp-subtle tcp-text-caption">
              rev {summaryValue(planPackageReplay?.next_plan_revision)}
            </div>
          </div>
          <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">changed paths</div>
            <div className="mt-1 text-slate-100">{diffEntries.length}</div>
          </div>
          <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">blocking issues</div>
            <div className="mt-1 text-slate-100">{blockingIssues.length}</div>
          </div>
        </div>
        <div className="mt-2 grid gap-2 sm:grid-cols-3">
          <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">scope metadata</div>
            <div className="mt-1 text-slate-100">
              {summaryValue(planPackageReplay?.scope_metadata_preserved)}
            </div>
          </div>
          <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">handoff rules</div>
            <div className="mt-1 text-slate-100">
              {summaryValue(planPackageReplay?.handoff_rules_preserved)}
            </div>
          </div>
          <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
              credential isolation
            </div>
            <div className="mt-1 text-slate-100">
              {summaryValue(planPackageReplay?.credential_isolation_preserved)}
            </div>
          </div>
        </div>
      </div>
      {issues.length ? (
        <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="font-medium text-slate-100">Replay issues</div>
            <span className="tcp-badge-info">
              {issues.length} issue{issues.length === 1 ? "" : "s"}
            </span>
          </div>
          <div className="mt-2 grid gap-2">
            {sortedIssues.map((issue, index) => (
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
                  {issueBadge(issue)}
                </div>
                <div className="mt-1 text-slate-100">{safeString(issue?.message) || "n/a"}</div>
              </div>
            ))}
          </div>
        </div>
      ) : null}
      <div className="grid gap-2">
        {groupedEntries.length ? (
          groupedEntries.map((category) => {
            const blockingCount = category.entries.filter((entry) => entry.blocking).length;
            return (
              <div
                key={category.key}
                className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="font-medium text-slate-100">{category.label}</div>
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="tcp-badge-info">
                      {category.entries.length} change{category.entries.length === 1 ? "" : "s"}
                    </span>
                    {blockingCount ? (
                      <span className="tcp-badge-warning">{blockingCount} blocking</span>
                    ) : null}
                  </div>
                </div>
                <div className="mt-2 grid gap-2">
                  {category.entries.map((entry: ReplayDiffEntry, index: number) => (
                    <div
                      key={`${safeString(entry.path) || "diff"}-${index}`}
                      className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2"
                    >
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <div className="font-medium text-slate-100">
                          {safeString(entry.path) || "unknown path"}
                        </div>
                        {compareBadge(entry)}
                      </div>
                      <div className="mt-2 grid gap-2 sm:grid-cols-2">
                        <div>
                          <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                            previous
                          </div>
                          <pre className="tcp-code mt-1 max-h-24 overflow-auto tcp-text-caption">
                            {diffValue(entry.previous_value)}
                          </pre>
                        </div>
                        <div>
                          <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">next</div>
                          <pre className="tcp-code mt-1 max-h-24 overflow-auto tcp-text-caption">
                            {diffValue(entry.next_value)}
                          </pre>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            );
          })
        ) : (
          <div className="tcp-subtle text-xs">No replay diff summary is available.</div>
        )}
      </div>
    </div>
  );
}
