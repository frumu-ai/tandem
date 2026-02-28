import test from "node:test";
import assert from "node:assert/strict";
import { applyEvent, createMapperContext } from "../src/event_mapper.mjs";

function baseRegistry() {
  return {
    version: 1,
    updatedAtMs: 0,
    tasks: {
      t1: {
        taskId: "t1",
        title: "T1",
        ownerRole: "worker",
        status: "pending",
        sessionId: "s1",
        runId: "r1",
        worktreePath: "/tmp/w1",
        branch: "swarm/t1",
        lastUpdateMs: 0,
        notifyOnComplete: true,
      },
    },
  };
}

test("state transitions are deterministic", () => {
  const events = [
    { type: "session.run.started", properties: { sessionID: "s1", runID: "r1" } },
    {
      type: "message.part.updated",
      properties: { sessionID: "s1", runID: "r1", part: { type: "tool", state: "running" } },
    },
    { type: "permission.asked", properties: { sessionID: "s1", runID: "r1", requestID: "p1" } },
    {
      type: "permission.replied",
      properties: { sessionID: "s1", runID: "r1", requestID: "p1", reply: "allow" },
    },
    {
      type: "session.run.finished",
      properties: { sessionID: "s1", runID: "r1", status: "completed" },
    },
  ];

  const run = () => {
    const registry = baseRegistry();
    const ctx = createMapperContext();
    for (const event of events) applyEvent(registry, event, ctx);
    return registry.tasks.t1.status;
  };

  assert.equal(run(), "ready_for_review");
  assert.equal(run(), "ready_for_review");
});
