type WorkflowMissionObjectivePanelProps = {
  objective: string;
};

export function WorkflowMissionObjectivePanel({ objective }: WorkflowMissionObjectivePanelProps) {
  return (
    <div className="tcp-list-item overflow-visible">
      <div className="font-medium">Mission Objective</div>
      <pre className="tcp-code mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-words">
        {objective}
      </pre>
    </div>
  );
}
