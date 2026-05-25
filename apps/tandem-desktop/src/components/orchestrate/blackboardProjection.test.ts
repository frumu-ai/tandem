import test from "node:test";
import assert from "node:assert/strict";
import {
  deriveIndicators,
  filterProjectedNodes,
  extractWhyNextFromEvents,
  projectNodes,
  type ProjectionNode,
} from "./blackboardProjection.js";
import type {
  Blackboard,
  RunCheckpointSummary,
  RunEventRecord,
  RunReplaySummary,
  Task,
} from "./types.js";

function event(
  seq: number,
  type: string,
  payload: Record<string, unknown> = {},
  stepId?: string
): RunEventRecord {
  return {
    event_id: `evt-${seq}`,
    run_id: "run-1",
    seq,
    ts_ms: 1000 + seq,
    type,
    status: "running",
    step_id: stepId ?? null,
    payload,
  };
}

function blackboard(): Blackboard {
  return {
    facts: [{ id: "f1", ts_ms: 1200, text: "fact one", step_id: "s1", source_event_id: "evt-2" }],
    decisions: [
      { id: "d1", ts_ms: 1300, text: "decision one", step_id: "s2", source_event_id: "evt-3" },
    ],
    open_questions: [],
    artifacts: [],
    summaries: { rolling: "", latest_context_pack: "" },
    revision: 1,
  };
}

test("projectNodes builds decision lineage and memory parent links", () => {
  const nodes = projectNodes(
    [
      event(1, "todo_synced"),
      event(2, "meta_next_step_selected", { why_next_step: "do s1" }, "s1"),
      event(3, "workspace_mismatch", {}, "s1"),
    ],
    blackboard(),
    null
  );

  const decision = nodes.find((node) => node.kind === "decision");
  assert.ok(decision);
  assert.equal(decision.stepId, "s1");

  const memory = nodes.find((node) => node.kind === "memory");
  assert.ok(memory);
  assert.equal(memory.parentId, decision.id);
});

test("filterProjectedNodes supports kind and step filters", () => {
  const nodes: ProjectionNode[] = [
    { id: "a", kind: "decision", label: "select s1", seq: 10, tsMs: 1, stepId: "s1" },
    { id: "b", kind: "reliability", label: "workspace_mismatch", seq: 11, tsMs: 2, stepId: "s2" },
  ];

  assert.equal(filterProjectedNodes(nodes, "decision", "").length, 1);
  assert.equal(filterProjectedNodes(nodes, "all", "s2")[0].id, "b");
});

test("projectNodes sorts lineage by descending seq", () => {
  const nodes = projectNodes(
    [
      event(5, "meta_next_step_selected", { why_next_step: "older" }, "s-old"),
      event(7, "meta_next_step_selected", { why_next_step: "newer" }, "s-new"),
    ],
    null,
    null
  );

  const decisions = nodes.filter((node) => node.kind === "decision");
  assert.equal(decisions[0].seq, 7);
  assert.equal(decisions[1].seq, 5);
});

test("extractWhyNextFromEvents supports orchestrator context-pack events", () => {
  const why = extractWhyNextFromEvents([
    event(1, "planning_started"),
    event(2, "context_pack_built", { why_next_step: "deps satisfied" }, "task_2"),
  ]);
  assert.equal(why, "deps satisfied");
});

test("projectNodes includes task_trace details in task-sync rail", () => {
  const nodes = projectNodes(
    [event(3, "task_trace", { stage: "FIRST_TOOL_CALL", detail: "glob" }, "task_1")],
    null,
    null
  );
  const trace = nodes.find((node) => node.kind === "task_sync");
  assert.ok(trace);
  assert.equal(trace.label, "FIRST_TOOL_CALL: glob");
});

test("deriveIndicators exposes drift/checkpoint/alert flags", () => {
  const tasks: Task[] = [
    {
      id: "s1",
      title: "Plan",
      description: "",
      dependencies: [],
      acceptance_criteria: [],
      state: "done",
      retry_count: 0,
    },
    {
      id: "s2",
      title: "Exec",
      description: "",
      dependencies: [],
      acceptance_criteria: [],
      state: "blocked",
      retry_count: 0,
    },
  ];
  const replay: RunReplaySummary = {
    ok: true,
    run_id: "run-1",
    from_checkpoint: true,
    checkpoint_seq: 8,
    events_applied: 3,
    drift: {
      mismatch: true,
      status_mismatch: false,
      why_next_step_mismatch: true,
      step_count_mismatch: false,
    },
  };
  const checkpoint: RunCheckpointSummary = {
    checkpoint_id: "cp-1",
    run_id: "run-1",
    seq: 8,
    ts_ms: 5000,
    reason: "heartbeat",
  };

  const indicators = deriveIndicators(
    "awaiting_approval",
    tasks,
    [event(9, "workspace_mismatch")],
    replay,
    checkpoint
  );

  assert.equal(indicators.doneCount, 1);
  assert.equal(indicators.blockedCount, 1);
  assert.equal(indicators.failedCount, 0);
  assert.equal(indicators.showAwaitingApproval, true);
  assert.equal(indicators.hasWorkspaceMismatch, true);
  assert.equal(indicators.showReplayDrift, true);
  assert.equal(indicators.checkpointSeq, 8);
});
