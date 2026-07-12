import type {
  GoalOperationsState,
  GoalProjection,
  GoalSelection,
  GoalTimelineEntry,
} from "./types";

export const GOAL_TIMELINE_LIMIT = 240;

export type GoalOperationsEvent =
  | { type: "projection"; projection: GoalProjection; replace?: boolean }
  | { type: "select"; selection: GoalSelection }
  | { type: "mode"; mode: "live" | "replay" }
  | { type: "scrub"; index: number }
  | { type: "repair-start" };

export function timelineCursor(entry: GoalTimelineEntry): number | null {
  const value = entry.cursor;
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export function timelineKey(entry: GoalTimelineEntry): string {
  if (entry.event.event_id) return entry.event.event_id;
  const cursor = timelineCursor(entry);
  return cursor === null
    ? `${entry.event.event_type}:${entry.event.occurred_at_ms}`
    : `cursor:${cursor}`;
}

export function mergeBoundedTimeline(
  current: readonly GoalTimelineEntry[],
  incoming: readonly GoalTimelineEntry[],
  limit = GOAL_TIMELINE_LIMIT
): GoalTimelineEntry[] {
  const merged = new Map<string, GoalTimelineEntry>();
  for (const entry of [...current, ...incoming]) merged.set(timelineKey(entry), entry);
  return [...merged.values()]
    .sort((left, right) => {
      const leftCursor = timelineCursor(left);
      const rightCursor = timelineCursor(right);
      if (leftCursor !== null && rightCursor !== null && leftCursor !== rightCursor) {
        return leftCursor - rightCursor;
      }
      if (left.event.occurred_at_ms !== right.event.occurred_at_ms) {
        return left.event.occurred_at_ms - right.event.occurred_at_ms;
      }
      return timelineKey(left).localeCompare(timelineKey(right));
    })
    .slice(-Math.max(1, limit));
}

export function nextProjectionCursor(projection: GoalProjection): number | null {
  const candidates = [
    projection.cursor,
    projection.live_cursor,
    ...projection.timeline.events.map(timelineCursor),
  ].filter((value): value is number => typeof value === "number" && Number.isFinite(value));
  return candidates.length ? Math.max(...candidates) : null;
}

export function hasTimelineGap(
  current: readonly GoalTimelineEntry[],
  incoming: readonly GoalTimelineEntry[]
): boolean {
  const sequences = new Map<string, number>();
  const sequence = (entry: GoalTimelineEntry) => {
    const event = entry.event as typeof entry.event & { goal_seq?: number };
    if (typeof event.goal_seq === "number") return { key: "goal", value: event.goal_seq };
    return typeof event.seq === "number" && event.run_id
      ? { key: `run:${event.run_id}`, value: event.seq }
      : null;
  };
  for (const entry of current) {
    const item = sequence(entry);
    if (item) sequences.set(item.key, Math.max(sequences.get(item.key) ?? item.value, item.value));
  }
  for (const entry of [...incoming].sort((left, right) => (timelineCursor(left) ?? 0) - (timelineCursor(right) ?? 0))) {
    const item = sequence(entry);
    if (!item) continue;
    const previous = sequences.get(item.key);
    if (previous !== undefined && item.value > previous + 1) return true;
    sequences.set(item.key, Math.max(previous ?? item.value, item.value));
  }
  return false;
}

function selectionExists(selection: GoalSelection, projection: GoalProjection): boolean {
  if (!selection) return true;
  return selection.kind === "node"
    ? projection.graph.nodes.some((node) => node.node_id === selection.id)
    : projection.graph.edges.some((edge) => edge.edge.edge_id === selection.id);
}

export function initialGoalOperationsState(): GoalOperationsState {
  return {
    projection: null,
    timeline: [],
    cursor: null,
    mode: "live",
    replayIndex: 0,
    selection: null,
    gapDetected: false,
  };
}

export function goalOperationsReducer(
  state: GoalOperationsState,
  event: GoalOperationsEvent
): GoalOperationsState {
  if (event.type === "select") return { ...state, selection: event.selection };
  if (event.type === "mode") {
    return {
      ...state,
      mode: event.mode,
      replayIndex:
        event.mode === "live" ? Math.max(0, state.timeline.length - 1) : state.replayIndex,
    };
  }
  if (event.type === "scrub") {
    const maximum = Math.max(0, state.timeline.length - 1);
    return { ...state, replayIndex: Math.min(maximum, Math.max(0, event.index)) };
  }
  if (event.type === "repair-start") return { ...state, gapDetected: true };

  const replace = event.replace || state.projection === null;
  const gapDetected = !replace && hasTimelineGap(state.timeline, event.projection.timeline.events);
  const timeline = mergeBoundedTimeline(
    replace ? [] : state.timeline,
    event.projection.timeline.events
  );
  const projection = {
    ...event.projection,
    timeline: { ...event.projection.timeline, events: timeline, count: timeline.length },
  };
  const cursor = nextProjectionCursor(projection) ?? state.cursor;
  const replayIndex =
    state.mode === "live"
      ? Math.max(0, timeline.length - 1)
      : Math.min(state.replayIndex, Math.max(0, timeline.length - 1));
  return {
    ...state,
    projection,
    timeline,
    cursor,
    replayIndex,
    selection: selectionExists(state.selection, projection) ? state.selection : null,
    gapDetected,
  };
}
