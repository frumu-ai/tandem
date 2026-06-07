type Props = {
  item: any;
  itemId: string;
  movingTaskStates: Record<string, string>;
  onMove: (item: any, state: string) => void;
};

const LINEAR_MOVE_STATE_OPTIONS = [
  { state: "backlog", label: "Backlog" },
  { state: "todo", label: "Todo" },
  { state: "ready", label: "Ready" },
  { state: "in_progress", label: "In Progress" },
  { state: "blocked", label: "Blocked" },
  { state: "done", label: "Done" },
];

export function CodingWorkflowsLinearTaskStateSelect({
  item,
  itemId,
  movingTaskStates,
  onMove,
}: Props) {
  const itemRef = String(item?.identifier || item?.issueNumber || item?.selector || itemId || "").trim();
  if (!itemRef) return null;
  const moving = LINEAR_MOVE_STATE_OPTIONS.some(
    (option) => movingTaskStates[`${itemId || itemRef}:${option.state}`]
  );
  return (
    <select
      className="tcp-input h-8 min-w-[8.5rem] px-2 py-1 text-xs"
      value=""
      disabled={moving}
      aria-label={`Move ${itemRef} to Linear state`}
      onChange={(event) => {
        const value = (event.target as HTMLSelectElement).value;
        event.currentTarget.value = "";
        if (value) onMove(item, value);
      }}
    >
      <option value="">Move to...</option>
      {LINEAR_MOVE_STATE_OPTIONS.map((option) => (
        <option key={option.state} value={option.state}>
          {option.label}
        </option>
      ))}
    </select>
  );
}
