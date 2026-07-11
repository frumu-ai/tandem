import type {
  GoalActionDescriptor,
  GoalProjection as CanonicalGoalProjection,
  GoalProjectionEdge as CanonicalGoalProjectionEdge,
  GoalProjectionMode,
  GoalProjectionNode as CanonicalGoalProjectionNode,
  GoalProjectionTimelineEntry as CanonicalGoalProjectionTimelineEntry,
} from "@frumu/tandem-client";

export type GoalOperationMode = GoalProjectionMode;
export type GoalSelection = { kind: "node" | "edge"; id: string } | null;
export type GoalTimelineEntry = CanonicalGoalProjectionTimelineEntry;
export type GoalProjection = CanonicalGoalProjection;
export type GoalActionPayloadOption = string | { value: string; label: string };
export type GoalActionPayloadField = {
  name: string;
  label: string;
  required: boolean;
  format?: string | null;
  options?: GoalActionPayloadOption[] | null;
};
export type GoalProjectionAction = GoalActionDescriptor & {
  payload_fields?: GoalActionPayloadField[] | null;
};
export type GoalProjectionNode = CanonicalGoalProjectionNode;
export type GoalProjectionEdge = CanonicalGoalProjectionEdge;
export type ProjectionConnection = "connecting" | "live" | "polling" | "offline";

export type GoalOperationsState = {
  projection: GoalProjection | null;
  timeline: GoalTimelineEntry[];
  cursor: number | null;
  mode: GoalOperationMode;
  replayIndex: number;
  selection: GoalSelection;
  gapDetected: boolean;
};
