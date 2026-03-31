type WorkflowDebugHintsPanelProps = {
  runHints: string[];
};

export function WorkflowDebugHintsPanel({ runHints }: WorkflowDebugHintsPanelProps) {
  if (!runHints.length) return null;

  return (
    <div className="tcp-list-item overflow-visible">
      <div className="mb-1 font-medium">Debug hints</div>
      <div className="grid gap-1 text-xs text-slate-300">
        {runHints.map((hint) => (
          <div key={hint}>{hint}</div>
        ))}
      </div>
    </div>
  );
}
