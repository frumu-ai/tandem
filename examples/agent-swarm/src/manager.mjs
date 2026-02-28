import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { execFileSync } from "node:child_process";
import { createApi, loadDotEnv } from "./tandem_api.mjs";
import { applyEvent, createMapperContext } from "./event_mapper.mjs";
import { currentResourceKey, loadRegistry, saveRegistry } from "./swarm_registry.mjs";
import { TASK_STATUS } from "./swarm_types.mjs";
import { seedTasks } from "./swarm_orchestrator.mjs";

const here = path.dirname(new URL(import.meta.url).pathname);
const root = path.resolve(here, "..");
loadDotEnv(path.join(root, ".env"));

const BASE_URL = process.env.TANDEM_BASE_URL || "http://127.0.0.1:39731";
const API_TOKEN = process.env.TANDEM_API_TOKEN || "";
const MAX_TASKS = Number(process.env.SWARM_MAX_TASKS || "3");
const OBJECTIVE =
  process.argv.slice(2).join(" ") ||
  process.env.SWARM_OBJECTIVE ||
  "Ship a small feature end-to-end";

const api = createApi(BASE_URL, API_TOKEN);

function readPrompt(name) {
  return fs.readFileSync(path.join(root, "agents", name), "utf8");
}

function parseTasksFromText(text) {
  try {
    return JSON.parse(text)?.tasks || [];
  } catch {
    const m = text.match(/\{[\s\S]*\}/);
    if (m) {
      try {
        return JSON.parse(m[0])?.tasks || [];
      } catch {
        return [];
      }
    }
    return [];
  }
}

function extractAssistantTextFromSession(sessionWire) {
  const messages = sessionWire?.messages || [];
  for (let i = messages.length - 1; i >= 0; i--) {
    const msg = messages[i];
    const role = msg?.info?.role;
    if (role !== "assistant") continue;
    const parts = msg.parts || [];
    const text = parts
      .map((p) => p.text)
      .filter(Boolean)
      .join("\n");
    if (text) return text;
  }
  return "";
}

function runCreateWorktree(repoRoot, taskId) {
  const script = path.join(root, "scripts", "create_worktree.sh");
  const out = execFileSync(script, [repoRoot, taskId], { encoding: "utf8" });
  const rows = Object.fromEntries(
    out
      .trim()
      .split(/\r?\n/)
      .map((line) => line.split("="))
      .filter((pair) => pair.length === 2)
  );
  return { worktreePath: rows.worktreePath, branch: rows.branch };
}

async function createTaskSession(title, worktreePath) {
  return api.post("/session", {
    title,
    directory: worktreePath,
    workspace_root: worktreePath,
  });
}

async function startRun(sessionId, prompt) {
  const res = await api.post(`/session/${sessionId}/prompt_async?return=run`, {
    parts: [{ type: "text", text: prompt }],
  });
  return res.runID || res.runId || res.run_id;
}

async function sendManagerDecompose(objective) {
  const managerSession = await api.post("/session", {
    title: "Agent Swarm Manager",
    directory: process.cwd(),
    workspace_root: process.cwd(),
  });
  const managerPrompt = readPrompt("manager.md");
  const req = `${managerPrompt}\n\nObjective:\n${objective}\n\nReturn 1-${MAX_TASKS} tasks.`;
  const response = await api.post(`/session/${managerSession.id}/prompt_sync`, {
    parts: [{ type: "text", text: req }],
  });
  const text = extractAssistantTextFromSession(response);
  const tasks = parseTasksFromText(text).slice(0, MAX_TASKS);
  if (tasks.length > 0) return tasks;
  return [
    {
      taskId: "task-1",
      title: objective,
      ownerRole: "worker",
      description: objective,
      acceptanceCriteria: [],
    },
  ];
}

function buildWorkerPrompt(task, worktreePath, branch) {
  return `${readPrompt("worker.md")}\n\nTask: ${task.title}\nDescription: ${task.description || task.title}\nBranch: ${branch}\nWorktree: ${worktreePath}`;
}

function buildTesterPrompt(task, worktreePath) {
  return `${readPrompt("tester.md")}\n\nTask: ${task.title}\nWorktree: ${worktreePath}`;
}

function buildReviewerPrompt(task, worktreePath, prUrl) {
  return `${readPrompt("reviewer.md")}\n\nTask: ${task.title}\nWorktree: ${worktreePath}\nPR: ${prUrl || "unknown"}`;
}

function extractPr(text) {
  const match = text.match(/https?:\/\/github\.com\/[^\s]+\/pull\/(\d+)/i);
  if (!match) return {};
  return { prUrl: match[0], prNumber: Number(match[1]) };
}

async function updatePrFromSession(task) {
  try {
    const messages = await api.get(`/session/${task.sessionId}/message`);
    const text = JSON.stringify(messages);
    const pr = extractPr(text);
    if (pr.prUrl) {
      task.prUrl = pr.prUrl;
      task.prNumber = pr.prNumber;
    }
  } catch {
    // ignore parse failure
  }
}

async function main() {
  const taskDefs = await sendManagerDecompose(OBJECTIVE);
  const registry = await loadRegistry(api);
  await seedTasks({
    registry,
    taskDefs,
    createWorktree: async (taskId) => runCreateWorktree(process.cwd(), taskId),
    createSession: async (taskId, worktreePath) =>
      createTaskSession(`Swarm Worker ${taskId}`, worktreePath),
    startRun: async (task, sessionId, worktreePath, branch) =>
      startRun(sessionId, buildWorkerPrompt(task, worktreePath, branch)),
  });

  await saveRegistry(api, registry);
  console.log(`Using swarm registry key: ${currentResourceKey()}`);

  const ctx = createMapperContext();
  const abortController = new AbortController();
  const done = new Promise((resolve) => setTimeout(resolve, 5 * 60 * 1000));

  const eventLoop = api
    .streamEvents(
      async (event) => {
        const out = applyEvent(registry, event, ctx);
        if (!out.changed) return;

        for (const action of out.actions) {
          if (action.type === "notify_auth_once") {
            console.log(`[auth-required] ${action.taskId}: ${action.reason}`);
          }
        }

        for (const task of Object.values(registry.tasks)) {
          if (
            task.status === TASK_STATUS.READY_FOR_REVIEW &&
            task.ownerRole === "worker" &&
            !task._testerStarted
          ) {
            await updatePrFromSession(task);
            const s = await createTaskSession(`Swarm Tester ${task.taskId}`, task.worktreePath);
            const r = await startRun(s.id, buildTesterPrompt(task, task.worktreePath));
            Object.assign(task, {
              ownerRole: "tester",
              status: TASK_STATUS.RUNNING,
              sessionId: s.id,
              runId: r,
              _testerStarted: true,
            });
          } else if (
            task.status === TASK_STATUS.READY_FOR_REVIEW &&
            task.ownerRole === "tester" &&
            !task._reviewerStarted
          ) {
            const s = await createTaskSession(`Swarm Reviewer ${task.taskId}`, task.worktreePath);
            const r = await startRun(
              s.id,
              buildReviewerPrompt(task, task.worktreePath, task.prUrl)
            );
            Object.assign(task, {
              ownerRole: "reviewer",
              status: TASK_STATUS.RUNNING,
              sessionId: s.id,
              runId: r,
              _reviewerStarted: true,
            });
          }
        }

        await saveRegistry(api, registry);

        const allTerminal = Object.values(registry.tasks).every((t) =>
          [TASK_STATUS.COMPLETE, TASK_STATUS.FAILED].includes(t.status)
        );
        if (allTerminal) abortController.abort();
      },
      { signal: abortController.signal }
    )
    .catch((err) => {
      if (!String(err).includes("AbortError")) {
        console.error(err);
      }
    });

  await Promise.race([done, eventLoop]);
  abortController.abort();
  await saveRegistry(api, registry);

  console.log("Swarm final status:");
  for (const task of Object.values(registry.tasks)) {
    console.log(`- ${task.taskId}: ${task.status} (${task.ownerRole}) ${task.prUrl || ""}`);
  }
  console.log("Merge gate: explicit user approval required. No automatic merge performed.");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
