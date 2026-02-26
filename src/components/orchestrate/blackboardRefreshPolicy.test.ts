import test from "node:test";
import assert from "node:assert/strict";
import {
  computeDebounceDelayMs,
  isRelevantBlackboardEventType,
  relevantRefreshTriggerSeq,
  shouldRefreshForRunStatusTransition,
} from "./blackboardRefreshPolicy.js";
import type { RunEventRecord } from "./types.js";

function event(seq: number, type: string): RunEventRecord {
  return {
    event_id: `evt-${seq}`,
    run_id: "run-1",
    seq,
    ts_ms: 1000 + seq,
    type,
    status: "running",
    step_id: null,
    payload: {},
  };
}

test("relevant event-family detection only matches blackboard-impacting families", () => {
  assert.equal(isRelevantBlackboardEventType("meta_next_step_selected"), true);
  assert.equal(isRelevantBlackboardEventType("todo_synced"), true);
  assert.equal(isRelevantBlackboardEventType("workspace_mismatch"), true);
  assert.equal(isRelevantBlackboardEventType("context_pack_built"), true);
  assert.equal(isRelevantBlackboardEventType("plan_generated"), true);
  assert.equal(isRelevantBlackboardEventType("task_completed"), true);
  assert.equal(isRelevantBlackboardEventType("task_trace"), true);
  assert.equal(isRelevantBlackboardEventType("checkpoint_created"), true);
  assert.equal(isRelevantBlackboardEventType("run_paused"), true);
  assert.equal(isRelevantBlackboardEventType("unrelated_event"), false);
});

test("relevant refresh trigger seq returns newest relevant seq past watermark", () => {
  const seq = relevantRefreshTriggerSeq(
    [event(10, "task_trace"), event(11, "todo_synced"), event(12, "workspace_mismatch")],
    10
  );
  assert.equal(seq, 12);
  assert.equal(relevantRefreshTriggerSeq([event(1, "task_trace")], 0), null);
});

test("status transition trigger only fires on actual status changes", () => {
  assert.equal(shouldRefreshForRunStatusTransition("running", "paused"), true);
  assert.equal(shouldRefreshForRunStatusTransition("running", "running"), false);
  assert.equal(shouldRefreshForRunStatusTransition(null, "running"), false);
});

test("debounce delay coalesces burst triggers", () => {
  assert.equal(computeDebounceDelayMs(1000, null, 300), 0);
  assert.equal(computeDebounceDelayMs(1200, 1000, 300), 100);
  assert.equal(computeDebounceDelayMs(1400, 1000, 300), 0);
});
