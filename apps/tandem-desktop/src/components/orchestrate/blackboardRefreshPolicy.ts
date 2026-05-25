import type { RunEventRecord, RunStatus } from "./types.js";

const EXPLICIT_REFRESH_TYPES = new Set([
  "meta_next_step_selected",
  "todo_synced",
  "workspace_mismatch",
  "context_pack_built",
  "planning_started",
  "plan_generated",
  "plan_approved",
  "revision_requested",
  "task_started",
  "task_completed",
  "task_trace",
  "run_failed",
  "contract_warning",
  "contract_error",
]);

export function isRelevantBlackboardEventType(eventType: string): boolean {
  const normalized = eventType.trim().toLowerCase();
  if (EXPLICIT_REFRESH_TYPES.has(normalized)) return true;
  if (normalized.includes("checkpoint")) return true;
  if (normalized.startsWith("run_")) return true;
  return false;
}

export function relevantRefreshTriggerSeq(
  events: RunEventRecord[],
  lastBlackboardRefreshSeq: number
): number | null {
  let newest: number | null = null;
  for (const event of events) {
    if (event.seq <= lastBlackboardRefreshSeq) continue;
    if (!isRelevantBlackboardEventType(event.type)) continue;
    newest = newest === null ? event.seq : Math.max(newest, event.seq);
  }
  return newest;
}

export function shouldRefreshForRunStatusTransition(
  prevStatus: RunStatus | null | undefined,
  nextStatus: RunStatus | null | undefined
): boolean {
  if (!prevStatus || !nextStatus) return false;
  return prevStatus !== nextStatus;
}

export function computeDebounceDelayMs(
  nowMs: number,
  lastScheduleMs: number | null,
  debounceMs: number
): number {
  if (lastScheduleMs === null) return 0;
  const elapsed = nowMs - lastScheduleMs;
  if (elapsed >= debounceMs) return 0;
  return debounceMs - elapsed;
}
