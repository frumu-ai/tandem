import assert from "node:assert/strict";
import test from "node:test";
import {
  appendRunTimelineLiveEvent,
  buildRunTimeline,
  legacyRunTimelineRequestPath,
  nextRunTimelineAfterSeq,
  normalizeRunTimelineEntry,
  normalizeRunTimelinePage,
  runTimelinePageEventCount,
  runTimelineRequestPath,
} from "../lib/runs/run-timeline.js";

function runtimeRow(overrides = {}) {
  return {
    event_id: "evt-1",
    seq: 1,
    event_type: "workflow.run.started",
    occurred_at_ms: 1_000,
    run_id: "run-a",
    tenant_context: { org_id: "org-a", workspace_id: "workspace-a" },
    payload: { status: "running", actor: { kind: "user", id: "user-a" } },
    ...overrides,
  };
}

test("run timeline normalizes persisted runtime events with actor scope and summary", () => {
  const [entry] = normalizeRunTimelinePage({
    run_id: "run-a",
    events: [
      runtimeRow({
        event_type: "workflow.governance.gate_decided",
        phase_id: "review",
        payload: { decision: "approved", reason: "operator accepted" },
      }),
    ],
  });

  assert.equal(entry.id, "evt-1");
  assert.equal(entry.source, "runtime");
  assert.equal(entry.runId, "run-a");
  assert.equal(entry.title, "Workflow Governance Gate Decided");
  assert.equal(entry.summary, "operator accepted");
  assert.equal(entry.tone, "ok");
  assert.equal(entry.phase, "review");
  assert.equal(entry.tenantOrg, "org-a");
  assert.equal(entry.tenantWorkspace, "workspace-a");
});

test("run timeline orders persisted pages and exposes the next sequence cursor", () => {
  const entries = buildRunTimeline({
    persistedPages: [
      {
        run_id: "run-a",
        events: [
          runtimeRow({ event_id: "evt-5", seq: 5, occurred_at_ms: 2_000 }),
          runtimeRow({ event_id: "evt-3", seq: 3, occurred_at_ms: 2_000 }),
        ],
      },
      {
        run_id: "run-a",
        events: [runtimeRow({ event_id: "evt-7", seq: 7, occurred_at_ms: 2_000 })],
      },
    ],
  });

  assert.deepEqual(
    entries.map((entry) => entry.sequence),
    [3, 5, 7]
  );
  assert.equal(nextRunTimelineAfterSeq(entries), 7);
  assert.equal(
    runTimelineRequestPath("run/a", { afterSeq: 7, limit: 50 }),
    "/api/engine/stateful-runtime/runs/run%2Fa/events?after_seq=7&limit=50"
  );
  assert.equal(
    runTimelineRequestPath("run/a", { beforeSeq: 3, limit: 50, tail: true }),
    "/api/engine/stateful-runtime/runs/run%2Fa/events?before_seq=3&limit=50&tail=50"
  );
  assert.equal(
    legacyRunTimelineRequestPath("run/a", { beforeSeq: 3, limit: 50, tail: true }),
    "/api/engine/runs/run%2Fa/events?before_seq=3&limit=50&tail=50"
  );
  assert.equal(
    runTimelineRequestPath("run/a", { tail: 25 }),
    "/api/engine/stateful-runtime/runs/run%2Fa/events?limit=250&tail=25"
  );
  assert.equal(runTimelinePageEventCount({ events: [] }), 0);
  assert.equal(runTimelinePageEventCount({ rows: [{ seq: 1 }] }), 1);
});

test("run timeline limits keep the newest entries", () => {
  const entries = buildRunTimeline({
    persistedPages: [
      {
        run_id: "run-a",
        events: [1, 2, 3, 4, 5].map((seq) =>
          runtimeRow({ event_id: `evt-${seq}`, seq, occurred_at_ms: 1_000 + seq })
        ),
      },
    ],
    limit: 2,
  });

  assert.deepEqual(
    entries.map((entry) => entry.sequence),
    [4, 5]
  );
});

test("run timeline dedupes live and persisted copies by canonical event id", () => {
  const liveCopy = {
    type: "workflow.run.started",
    envelope: {
      event_id: "evt-1",
      seq: 1,
      occurred_at_ms: 1_000,
      run_id: "run-a",
    },
    properties: { runID: "run-a", status: "running" },
  };

  const entries = buildRunTimeline({
    persistedPages: [{ events: [runtimeRow()] }],
    liveEvents: [liveCopy],
  });

  assert.equal(entries.length, 1);
  assert.equal(entries[0].source, "runtime");
  assert.equal(entries[0].eventId, "evt-1");
});

test("run timeline appends live updates without duplicating existing sequence keys", () => {
  const initial = [
    normalizeRunTimelineEntry(
      {
        type: "workflow.run.started",
        envelope: { seq: 4, occurred_at_ms: 3_000, run_id: "run-a" },
        properties: { runID: "run-a", status: "running" },
      },
      { source: "live" }
    ),
  ];
  const updated = appendRunTimelineLiveEvent(initial, {
    type: "workflow.run.failed",
    envelope: { seq: 4, occurred_at_ms: 3_000, run_id: "run-a" },
    properties: { runID: "run-a", error: "provider failed" },
  });
  const next = appendRunTimelineLiveEvent(updated, {
    type: "workflow.run.completed",
    envelope: { event_id: "evt-5", seq: 5, occurred_at_ms: 4_000, run_id: "run-a" },
    properties: { runID: "run-a", status: "completed" },
  });

  assert.equal(updated.length, 1);
  assert.equal(updated[0].tone, "failed");
  assert.deepEqual(
    next.map((entry) => entry.sequence),
    [4, 5]
  );
  assert.equal(next[1].tone, "ok");
});
