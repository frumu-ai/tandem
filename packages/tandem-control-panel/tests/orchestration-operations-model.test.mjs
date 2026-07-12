import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createRequire } from "node:module";
import test from "node:test";
import ts from "typescript";

function loadModel() {
  const directory = mkdtempSync(join(tmpdir(), "tandem-goal-operations-"));
  writeFileSync(join(directory, "package.json"), JSON.stringify({ type: "commonjs" }));
  const source = readFileSync(
    new URL("../src/features/orchestration-operations/model.ts", import.meta.url),
    "utf8"
  );
  const compiled = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
    fileName: "model.ts",
  });
  writeFileSync(join(directory, "model.js"), compiled.outputText);
  const require = createRequire(import.meta.url);
  return {
    model: require(join(directory, "model.js")),
    cleanup: () => rmSync(directory, { recursive: true, force: true }),
  };
}

function event(seq, eventId = `event-${seq}`) {
  return {
    cursor: seq,
    event: {
      event_id: eventId,
      goal_seq: seq,
      event_type: `step_${seq}`,
      occurred_at_ms: 1_700_000_000_000 + seq,
    },
  };
}

function projection(timeline, nodeIds = ["plan", "execute"]) {
  return {
    goal: { goal_id: "goal-1", updated_at_ms: 10 },
    orchestration: { nodes: [], edges: [] },
    graph: {
      nodes: nodeIds.map((node_id) => ({ node_id, name: node_id, kind: "workflow", state: "running", runs: [] })),
      edges: [{ edge: { edge_id: "plan-execute", from_node_id: "plan", to_node_id: "execute", transition_key: "next" }, state: "eligible" }],
    },
    budgets: {},
    timeline: { events: timeline, count: timeline.length, limit: 240, truncated: false },
    cursor: timeline.at(-1)?.cursor || 0,
    live_cursor: timeline.at(-1)?.cursor || 0,
    actions: [],
  };
}

test("goal projection reducer is deterministic for duplicate and reordered pages", () => {
  const { model, cleanup } = loadModel();
  try {
    const initial = model.initialGoalOperationsState();
    const first = model.goalOperationsReducer(initial, {
      type: "projection",
      projection: projection([event(2), event(1), event(2)]),
    });
    const second = model.goalOperationsReducer(initial, {
      type: "projection",
      projection: projection([event(1), event(2)]),
    });
    assert.deepEqual(first, second);
    assert.deepEqual(first.timeline.map((entry) => entry.cursor), [1, 2]);
  } finally {
    cleanup();
  }
});

test("bounded timeline dedupes and reports cursor gaps for canonical repair", () => {
  const { model, cleanup } = loadModel();
  try {
    const merged = model.mergeBoundedTimeline(
      [event(1), event(2), event(3)],
      [event(3), event(4), event(5)],
      3
    );
    assert.deepEqual(merged.map((entry) => entry.cursor), [3, 4, 5]);
    assert.equal(model.hasTimelineGap([event(5)], [event(7)]), true);
    assert.equal(model.hasTimelineGap([event(5)], [event(6), event(7)]), false);
    const cursorJump = event(6);
    cursorJump.cursor = 99;
    assert.equal(model.hasTimelineGap([event(5)], [cursorJump]), false);

    const hydrated = model.goalOperationsReducer(model.initialGoalOperationsState(), {
      type: "projection",
      projection: projection([event(1), event(2)]),
    });
    const gapped = model.goalOperationsReducer(hydrated, {
      type: "projection",
      projection: projection([event(4)]),
    });
    assert.equal(gapped.gapDetected, true);
    const repaired = model.goalOperationsReducer(gapped, {
      type: "projection",
      projection: projection([event(1), event(2), event(3), event(4)]),
      replace: true,
    });
    assert.deepEqual(repaired.timeline.map((entry) => entry.cursor), [1, 2, 3, 4]);
    assert.equal(repaired.gapDetected, false);
  } finally {
    cleanup();
  }
});

test("selection and replay position survive live projection replacement when valid", () => {
  const { model, cleanup } = loadModel();
  try {
    let state = model.goalOperationsReducer(model.initialGoalOperationsState(), {
      type: "projection",
      projection: projection([event(1), event(2), event(3)]),
    });
    state = model.goalOperationsReducer(state, { type: "select", selection: { kind: "node", id: "execute" } });
    state = model.goalOperationsReducer(state, { type: "mode", mode: "replay" });
    state = model.goalOperationsReducer(state, { type: "scrub", index: 1 });
    state = model.goalOperationsReducer(state, {
      type: "projection",
      projection: projection([event(4)]),
    });
    assert.deepEqual(state.selection, { kind: "node", id: "execute" });
    assert.equal(state.replayIndex, 1);
    assert.deepEqual(state.timeline.map((entry) => entry.cursor), [1, 2, 3, 4]);

    state = model.goalOperationsReducer(state, {
      type: "projection",
      projection: projection([event(5)], ["plan"]),
    });
    assert.equal(state.selection, null);
  } finally {
    cleanup();
  }
});
