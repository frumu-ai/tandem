type WorkflowRunSummaryPanelProps = {
  runSummaryRows: Array<{ label: string; value: string }>;
};

export function WorkflowRunSummaryPanel({ runSummaryRows }: WorkflowRunSummaryPanelProps) {
  return (
    <div className="tcp-list-item overflow-visible">
      <div className="font-medium">Run Summary</div>
      <div className="mt-2 grid gap-2 text-xs text-slate-300 sm:grid-cols-2 xl:grid-cols-4">
        {runSummaryRows.map((row) => (
          <div key={row.label} className="break-words">
            {row.label}: {row.value}
          </div>
        ))}
      </div>
    </div>
  );
}
