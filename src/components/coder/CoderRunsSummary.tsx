import { useEffect, useState } from "react";
import { AlertCircle, CheckCircle2, Loader2, PauseCircle, XCircle } from "lucide-react";
import { runStatusTone, type DerivedCoderRun } from "./shared/coderRunUtils";

type CoderRunsSummaryProps = {
  runs: DerivedCoderRun[];
  lastUpdatedMs: number | null;
  isLoading?: boolean;
};

type Counter = {
  key: string;
  label: string;
  count: number;
  icon: React.ReactNode;
  tone: string;
  emphasize?: boolean;
};

export function CoderRunsSummary({ runs, lastUpdatedMs, isLoading }: CoderRunsSummaryProps) {
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNowMs(Date.now()), 15_000);
    return () => window.clearInterval(id);
  }, []);

  const tally = runs.reduce(
    (acc, row) => {
      const tone = runStatusTone(row.run);
      acc[tone] = (acc[tone] || 0) + 1;
      return acc;
    },
    {} as Record<string, number>
  );

  const counters: Counter[] = [
    {
      key: "running",
      label: "Running",
      count: (tally.running || 0) + (tally.queued || 0),
      icon: (
        <Loader2
          className={`h-3.5 w-3.5 ${(tally.running || 0) > 0 ? "animate-spin" : ""}`}
          aria-hidden
        />
      ),
      tone: "border-primary/40 bg-primary/10 text-primary",
      emphasize: (tally.running || 0) > 0,
    },
    {
      key: "awaiting",
      label: "Needs approval",
      count: tally.awaiting || 0,
      icon: <AlertCircle className="h-3.5 w-3.5" aria-hidden />,
      tone: "border-amber-300/50 bg-amber-300/10 text-amber-100",
      emphasize: (tally.awaiting || 0) > 0,
    },
    {
      key: "paused",
      label: "Paused",
      count: tally.paused || 0,
      icon: <PauseCircle className="h-3.5 w-3.5" aria-hidden />,
      tone: "border-amber-400/40 bg-amber-400/10 text-amber-200",
    },
    {
      key: "failed",
      label: "Failed",
      count: tally.failed || 0,
      icon: <XCircle className="h-3.5 w-3.5" aria-hidden />,
      tone: "border-red-500/40 bg-red-500/10 text-red-100",
      emphasize: (tally.failed || 0) > 0,
    },
    {
      key: "completed",
      label: "Completed",
      count: tally.completed || 0,
      icon: <CheckCircle2 className="h-3.5 w-3.5" aria-hidden />,
      tone: "border-emerald-500/30 bg-emerald-500/5 text-emerald-200",
    },
  ];

  const lastUpdatedLabel = (() => {
    if (isLoading) return "Refreshing…";
    if (!lastUpdatedMs) return "Not synced yet";
    const delta = Math.max(0, Math.floor((nowMs - lastUpdatedMs) / 1000));
    if (delta < 5) return "Updated just now";
    if (delta < 60) return `Updated ${delta}s ago`;
    const min = Math.floor(delta / 60);
    return `Updated ${min}m ago`;
  })();

  return (
    <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-border bg-surface-elevated/40 px-4 py-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-xs uppercase tracking-wide text-text-subtle">Coder runs</span>
        <span className="text-sm font-semibold text-text">{runs.length} total</span>
        <div className="ml-2 flex flex-wrap items-center gap-2">
          {counters
            .filter((counter) => counter.count > 0 || counter.emphasize)
            .map((counter) => (
              <span
                key={counter.key}
                className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs ${counter.tone}`}
              >
                {counter.icon}
                <span className="font-medium">{counter.count}</span>
                <span className="text-text-muted/80">{counter.label}</span>
              </span>
            ))}
        </div>
      </div>
      <div className="flex items-center gap-2 text-[11px] text-text-subtle">
        {isLoading ? <Loader2 className="h-3 w-3 animate-spin" aria-hidden /> : null}
        <span>{lastUpdatedLabel}</span>
      </div>
    </div>
  );
}
