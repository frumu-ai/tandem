function normalizeValue(value: unknown) {
  return String(value || "").trim();
}

function normalizeStateKey(value: unknown) {
  return normalizeValue(value)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
}

function stateLabel(state: string) {
  const key = normalizeStateKey(state);
  if (key === "todo") return "Todo";
  if (key === "todos") return "TODOS";
  if (key === "in_progress") return "In Progress";
  return key
    .split("_")
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function itemReferences(item: any) {
  return new Set(
    [
      item?.id,
      item?.project_item_id,
      item?.projectItemId,
      item?.identifier,
      item?.issue_number,
      item?.issueNumber,
      item?.issue_id,
      item?.issueId,
      item?.issue_url,
      item?.issueUrl,
      item?.selector,
    ]
      .map(normalizeValue)
      .filter(Boolean)
  );
}

function itemMatches(item: any, refs: Set<string>) {
  for (const ref of itemReferences(item)) {
    if (refs.has(ref)) return true;
  }
  return false;
}

export function optimisticallyMoveBoardItems(rawBoard: any, items: any[], targetState: string) {
  if (!rawBoard || !Array.isArray(rawBoard.items) || !items.length) return rawBoard;
  const refs = new Set(items.flatMap((item) => Array.from(itemReferences(item))));
  if (!refs.size) return rawBoard;

  const statusKey = normalizeStateKey(targetState);
  const statusName = stateLabel(statusKey);
  const isRunning = ["active", "in_progress", "started", "working"].includes(statusKey);
  const launchState = isRunning ? "starting" : statusKey;

  return {
    ...rawBoard,
    items: rawBoard.items.map((item: any) => {
      if (!itemMatches(item, refs)) return item;
      return {
        ...item,
        actionable: isRunning ? false : item?.actionable,
        launch_state: launchState,
        launchState,
        runnable_now: isRunning ? false : item?.runnable_now,
        runnableNow: isRunning ? false : item?.runnableNow,
        run_state: isRunning ? "starting" : item?.run_state,
        runState: isRunning ? "starting" : item?.runState,
        status_key: statusKey,
        statusKey,
        status_name: statusName,
        statusName,
      };
    }),
  };
}
