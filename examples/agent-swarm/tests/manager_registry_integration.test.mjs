import test from "node:test";
import assert from "node:assert/strict";
import { seedTasks } from "../src/swarm_orchestrator.mjs";
import { blankRegistry } from "../src/swarm_types.mjs";

test("manager seeds N tasks and updates registry", async () => {
  const registry = blankRegistry();
  const tasks = [
    { taskId: "a", title: "Task A" },
    { taskId: "b", title: "Task B" },
    { taskId: "c", title: "Task C" },
  ];

  let runCount = 0;
  await seedTasks({
    registry,
    taskDefs: tasks,
    createWorktree: async (taskId) => ({
      worktreePath: `/repo/.swarm/worktrees/${taskId}`,
      branch: `swarm/${taskId}`,
    }),
    createSession: async (taskId, worktreePath) => ({ id: `s-${taskId}`, directory: worktreePath }),
    startRun: async (_task, sessionId) => {
      runCount += 1;
      return `r-${sessionId}`;
    },
  });

  assert.equal(Object.keys(registry.tasks).length, 3);
  assert.equal(runCount, 3);
  assert.equal(registry.tasks.a.sessionId, "s-a");
  assert.equal(registry.tasks.b.status, "pending");
});
