import type {
  Blackboard,
  RunCheckpointSummary,
  RunEventRecord,
  RunReplaySummary,
  RunStatus,
  Task,
} from "./types.js";

export type ProjectionNodeKind = "decision" | "memory" | "task_sync" | "reliability" | "checkpoint";

export interface ProjectionNode {
  id: string;
  kind: ProjectionNodeKind;
  label: string;
  seq: number;
  tsMs: number;
  eventType?: string;
  stepId?: string;
  whyNextStep?: string;
  payload?: Record<string, unknown>;
  sourceEventId?: string;
  parentId?: string;
}

export interface BlackboardIndicators {
  doneCount: number;
  blockedCount: number;
  failedCount: number;
  hasWorkspaceMismatch: boolean;
  showAwaitingApproval: boolean;
  showReplayDrift: boolean;
  checkpointSeq: number | null;
}

const DECISION_EVENT_TYPES = new Set([
  "meta_next_step_selected",
  "context_pack_built",
  "planning_started",
  "plan_generated",
  "plan_approved",
  "revision_requested",
]);

const TASK_SYNC_EVENT_TYPES = new Set([
  "todo_synced",
  "task_started",
  "task_completed",
  "plan_generated",
  "task_trace",
]);

const RELIABILITY_EVENT_TYPES = new Set([
  "workspace_mismatch",
  "run_failed",
  "contract_error",
  "contract_warning",
  "session_error",
]);

export function isDecisionEventType(eventType: string): boolean {
  return DECISION_EVENT_TYPES.has(eventType.trim().toLowerCase());
}

function eventWhyNextStep(event: RunEventRecord): string | null {
  const directWhy = event.payload?.why_next_step;
  if (typeof directWhy === "string" && directWhy.trim().length > 0) {
    return directWhy;
  }
  if (event.type === "plan_generated") {
    return "Plan generated and awaiting review/approval.";
  }
  if (event.type === "planning_started") {
    return "Planning has started.";
  }
  return null;
}

function taskTraceLabel(event: RunEventRecord): string | null {
  const stage = event.payload?.stage;
  const detail = event.payload?.detail;
  const stageText = typeof stage === "string" && stage.trim().length > 0 ? stage : null;
  const detailText = typeof detail === "string" && detail.trim().length > 0 ? detail : null;
  if (!stageText && !detailText) {
    return null;
  }
  if (stageText && detailText) {
    return `${stageText}: ${detailText}`;
  }
  return stageText ?? detailText;
}

export function extractWhyNextFromEvents(events: RunEventRecord[]): string | null {
  const lastDecision = [...events].reverse().find((event) => isDecisionEventType(event.type));
  return lastDecision ? eventWhyNextStep(lastDecision) : null;
}

export function deriveIndicators(
  runStatus: RunStatus | null | undefined,
  tasks: Task[],
  events: RunEventRecord[],
  replay: RunReplaySummary | null | undefined,
  checkpoint: RunCheckpointSummary | null | undefined
): BlackboardIndicators {
  return {
    doneCount: tasks.filter((task) => task.state === "done").length,
    blockedCount: tasks.filter((task) => task.state === "blocked").length,
    failedCount: tasks.filter((task) => task.state === "failed").length,
    hasWorkspaceMismatch: events.some((event) => event.type === "workspace_mismatch"),
    showAwaitingApproval: runStatus === "awaiting_approval",
    showReplayDrift: !!replay?.drift?.mismatch,
    checkpointSeq: checkpoint?.seq ?? null,
  };
}

export function filterProjectedNodes(
  nodes: ProjectionNode[],
  kind: ProjectionNodeKind | "all",
  query: string
): ProjectionNode[] {
  const needle = query.trim().toLowerCase();
  return nodes.filter((node) => {
    if (kind !== "all" && node.kind !== kind) return false;
    if (!needle) return true;
    const stepText = node.stepId?.toLowerCase() ?? "";
    const labelText = node.label.toLowerCase();
    const whyText = node.whyNextStep?.toLowerCase() ?? "";
    const eventType = node.eventType?.toLowerCase() ?? "";
    return (
      stepText.includes(needle) ||
      labelText.includes(needle) ||
      whyText.includes(needle) ||
      eventType.includes(needle)
    );
  });
}

export function projectNodes(
  events: RunEventRecord[],
  blackboard: Blackboard | null,
  checkpoint: RunCheckpointSummary | null
): ProjectionNode[] {
  const nodes: ProjectionNode[] = [];
  const decisionBySeq: Array<{ seq: number; nodeId: string }> = [];

  for (const event of events) {
    const normalizedType = event.type.trim().toLowerCase();
    if (isDecisionEventType(normalizedType)) {
      const whyText = eventWhyNextStep(event) ?? undefined;
      const label =
        event.step_id && event.step_id.trim().length > 0
          ? `select ${event.step_id}`
          : normalizedType === "plan_generated"
            ? "plan generated"
            : normalizedType === "planning_started"
              ? "planning started"
              : normalizedType.replace(/_/g, " ");
      const id = `decision:${event.event_id}`;
      nodes.push({
        id,
        kind: "decision",
        label,
        seq: event.seq,
        tsMs: event.ts_ms,
        eventType: normalizedType,
        stepId: event.step_id ?? undefined,
        whyNextStep: whyText,
        payload: event.payload,
        sourceEventId: event.event_id,
      });
      decisionBySeq.push({ seq: event.seq, nodeId: id });
    } else if (TASK_SYNC_EVENT_TYPES.has(normalizedType)) {
      const label =
        normalizedType === "task_trace"
          ? (taskTraceLabel(event) ?? "task trace")
          : normalizedType.replace(/_/g, " ");
      nodes.push({
        id: `task_sync:${event.event_id}`,
        kind: "task_sync",
        label,
        seq: event.seq,
        tsMs: event.ts_ms,
        eventType: normalizedType,
        payload: event.payload,
        sourceEventId: event.event_id,
      });
    } else if (
      RELIABILITY_EVENT_TYPES.has(normalizedType) ||
      normalizedType.includes("loop") ||
      normalizedType.includes("escalated")
    ) {
      nodes.push({
        id: `reliability:${event.event_id}`,
        kind: "reliability",
        label: normalizedType.replace(/_/g, " "),
        seq: event.seq,
        tsMs: event.ts_ms,
        eventType: normalizedType,
        stepId: event.step_id ?? undefined,
        payload: event.payload,
        sourceEventId: event.event_id,
      });
    }
  }

  const latestDecisionNodeId = decisionBySeq.length
    ? decisionBySeq[decisionBySeq.length - 1].nodeId
    : undefined;

  if (blackboard) {
    const memoryRows = [
      ...blackboard.facts.map((row) => ({
        label: `fact: ${row.text}`,
        tsMs: row.ts_ms,
        stepId: row.step_id ?? undefined,
        sourceEventId: row.source_event_id ?? undefined,
      })),
      ...blackboard.decisions.map((row) => ({
        label: `decision: ${row.text}`,
        tsMs: row.ts_ms,
        stepId: row.step_id ?? undefined,
        sourceEventId: row.source_event_id ?? undefined,
      })),
      ...blackboard.open_questions.map((row) => ({
        label: `question: ${row.text}`,
        tsMs: row.ts_ms,
        stepId: row.step_id ?? undefined,
        sourceEventId: row.source_event_id ?? undefined,
      })),
    ];
    for (const [index, row] of memoryRows.entries()) {
      nodes.push({
        id: `memory:${index}:${row.tsMs}`,
        kind: "memory",
        label: row.label,
        seq: 0,
        tsMs: row.tsMs,
        stepId: row.stepId,
        sourceEventId: row.sourceEventId,
        parentId: latestDecisionNodeId,
      });
    }
  }

  if (checkpoint) {
    nodes.push({
      id: `checkpoint:${checkpoint.checkpoint_id}`,
      kind: "checkpoint",
      label: `checkpoint (${checkpoint.reason})`,
      seq: checkpoint.seq,
      tsMs: checkpoint.ts_ms,
    });
  }

  const sorted = [...nodes].sort((a, b) => {
    if (a.seq !== b.seq) return b.seq - a.seq;
    return b.tsMs - a.tsMs;
  });
  const fallbackParent = sorted.find((node) => node.kind === "decision")?.id;
  return sorted.map((node) => ({
    ...node,
    parentId: node.parentId ?? (node.kind === "decision" ? undefined : fallbackParent),
  }));
}
