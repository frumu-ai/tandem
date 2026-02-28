import { TASK_STATUS } from "./swarm_types.mjs";
import { upsertTask } from "./swarm_registry.mjs";

export async function seedTasks({ registry, taskDefs, createWorktree, createSession, startRun }) {
  for (let idx = 0; idx < taskDefs.length; idx++) {
    const raw = taskDefs[idx];
    const taskId = raw.taskId || `task-${idx + 1}`;
    if (registry.tasks[taskId]) continue;

    const { worktreePath, branch } = await createWorktree(taskId);
    const session = await createSession(taskId, worktreePath);
    const runId = await startRun(raw, session.id, worktreePath, branch);

    upsertTask(registry, {
      taskId,
      title: raw.title || taskId,
      ownerRole: "worker",
      status: TASK_STATUS.PENDING,
      sessionId: session.id,
      runId,
      worktreePath,
      branch,
      notifyOnComplete: true,
      _testerStarted: false,
      _reviewerStarted: false,
    });
  }
  return registry;
}
