import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import type { TandemClient } from "@frumu/tandem-client";

const WINDOW_OPTIONS: Array<{ label: string; hours: number }> = [
  { label: "24h", hours: 24 },
  { label: "7d", hours: 168 },
  { label: "30d", hours: 720 },
];

const VALIDATOR_CLASS_LABELS: Record<string, string> = {
  missing_required_section: "Missing required section",
  weak_markdown_structure: "Weak markdown structure",
  missing_optional_evidence: "Missing optional evidence",
  artifact_word_count_below_minimum: "Word count below minimum",
  missing_nonconsumed_workspace_files: "Workspace files missing",
  missing_required_artifact_path: "Missing required artifact path",
  validator_kind_specific_soft_check: "Validator soft check",
  repair_budget_exhausted: "Repair budget exhausted",
};

function classLabel(key: string) {
  return VALIDATOR_CLASS_LABELS[key] || key.replace(/_/g, " ");
}

type CountsRow = {
  accepted?: number;
  rejected?: number;
  re_ran_strict?: number;
  unmarked?: number;
};

function reviewedCount(counts: CountsRow): number {
  return (
    (Number(counts.accepted) || 0) +
    (Number(counts.rejected) || 0) +
    (Number(counts.re_ran_strict) || 0)
  );
}

function totalCount(counts: CountsRow): number {
  return reviewedCount(counts) + (Number(counts.unmarked) || 0);
}

function acceptRate(counts: CountsRow): number | null {
  const reviewed = reviewedCount(counts);
  if (reviewed === 0) return null;
  return (Number(counts.accepted) || 0) / reviewed;
}

function formatPercent(value: number | null) {
  if (value === null) return "—";
  return `${(value * 100).toFixed(1)}%`;
}

export function GraduationSummaryPanel({
  client,
  automationId,
  className = "",
}: {
  client: TandemClient | null;
  automationId?: string;
  className?: string;
}) {
  const [windowHours, setWindowHours] = useState<number>(168);

  const summaryQuery = useQuery({
    queryKey: ["graduation", "summary", windowHours, automationId || ""],
    enabled: !!client?.automationsV2?.graduationSummary,
    queryFn: async () => {
      if (!client?.automationsV2?.graduationSummary) return null;
      return client.automationsV2.graduationSummary({
        windowHours,
        automationId,
        limit: 200,
      });
    },
    refetchInterval: 60000,
    staleTime: 30000,
  });

  const data = summaryQuery.data || null;
  const summary = (data as any)?.summary || {};
  const byClass = (summary?.by_class || {}) as Record<string, CountsRow>;
  const sortedRows = useMemo(() => {
    return Object.entries(byClass)
      .map(([key, counts]) => ({
        key,
        counts,
        total: totalCount(counts),
        reviewed: reviewedCount(counts),
        acceptRate: acceptRate(counts),
      }))
      .sort((a, b) => {
        if (b.reviewed !== a.reviewed) return b.reviewed - a.reviewed;
        return b.total - a.total;
      });
  }, [byClass]);

  const totalScanned = Number(summary?.total_outputs_scanned || 0);
  const totalRelaxed = Number(summary?.total_relaxed_outputs || 0);
  const scannedRuns = Number((data as any)?.scanned_runs || 0);

  return (
    <div className={`grid gap-3 ${className}`}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <div className="text-sm font-medium text-slate-100">Graduation summary</div>
          <div className="tcp-subtle text-xs">
            Per-class accept rate over the last{" "}
            {WINDOW_OPTIONS.find((opt) => opt.hours === windowHours)?.label || `${windowHours}h`}.
          </div>
        </div>
        <div className="flex flex-wrap gap-1">
          {WINDOW_OPTIONS.map((option) => (
            <button
              key={option.hours}
              type="button"
              className={windowHours === option.hours ? "tcp-btn-primary" : "tcp-btn"}
              onClick={() => setWindowHours(option.hours)}
              disabled={summaryQuery.isFetching}
            >
              {option.label}
            </button>
          ))}
        </div>
      </div>

      <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
        <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
          <div className="tcp-subtle tcp-text-caption">runs scanned</div>
          <div className="mt-1 font-medium text-slate-100">{scannedRuns}</div>
        </div>
        <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
          <div className="tcp-subtle tcp-text-caption">node outputs scanned</div>
          <div className="mt-1 font-medium text-slate-100">{totalScanned}</div>
        </div>
        <div className="rounded-md border border-amber-700/40 bg-amber-950/20 p-2">
          <div className="tcp-subtle tcp-text-caption">relaxed outputs</div>
          <div className="mt-1 font-medium text-amber-200">{totalRelaxed}</div>
        </div>
      </div>

      {summaryQuery.isError ? (
        <div className="rounded-md border border-rose-700/40 bg-rose-950/20 p-2 text-xs text-rose-200">
          Failed to load graduation summary.
        </div>
      ) : null}

      {sortedRows.length === 0 ? (
        <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-3 text-sm text-slate-300">
          {summaryQuery.isLoading
            ? "Loading…"
            : totalScanned === 0
              ? "No automation runs scanned in this window."
              : "No relaxed outputs in this window. Strict-only runs do not show up here."}
        </div>
      ) : (
        <div className="overflow-hidden rounded-md border border-slate-800/80">
          <table className="w-full text-left text-xs">
            <thead className="bg-slate-900/40 tcp-text-caption uppercase tracking-wide text-slate-400">
              <tr>
                <th className="px-2 py-1.5">Validator class</th>
                <th className="px-2 py-1.5 text-right">Accepted</th>
                <th className="px-2 py-1.5 text-right">Rejected</th>
                <th className="px-2 py-1.5 text-right">Re-ran Strict</th>
                <th className="px-2 py-1.5 text-right">Unmarked</th>
                <th className="px-2 py-1.5 text-right">Accept rate</th>
              </tr>
            </thead>
            <tbody>
              {sortedRows.map((row) => {
                const insufficient = row.reviewed === 0;
                const rateText = insufficient ? (
                  <span className="tcp-subtle italic">insufficient signal</span>
                ) : (
                  <span
                    className={
                      (row.acceptRate ?? 0) >= 0.8
                        ? "font-semibold text-emerald-300"
                        : (row.acceptRate ?? 0) <= 0.4
                          ? "font-semibold text-rose-300"
                          : "font-semibold text-amber-200"
                    }
                  >
                    {formatPercent(row.acceptRate)}
                  </span>
                );
                return (
                  <tr key={row.key} className="border-t border-slate-800/60 text-slate-200">
                    <td className="px-2 py-1.5">
                      <div className="font-medium">{classLabel(row.key)}</div>
                      <code className="tcp-subtle tcp-text-micro">{row.key}</code>
                    </td>
                    <td className="px-2 py-1.5 text-right">{row.counts.accepted || 0}</td>
                    <td className="px-2 py-1.5 text-right">{row.counts.rejected || 0}</td>
                    <td className="px-2 py-1.5 text-right">{row.counts.re_ran_strict || 0}</td>
                    <td className="px-2 py-1.5 text-right tcp-subtle">
                      {row.counts.unmarked || 0}
                    </td>
                    <td className="px-2 py-1.5 text-right">{rateText}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
