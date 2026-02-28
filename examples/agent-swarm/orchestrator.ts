import { readFileSync } from "fs";
import { resolve, join } from "path";

/**
 * Tandem Agent Swarm Orchestrator
 *
 * Requirements:
 * 1. Tandem Engine (running locally on port 8000)
 * 2. `swarm.active_tasks` Shared Resource (auto-initialized by this script)
 * 3. `check_swarm_health` Routine (must be registered in Tandem)
 * 4. `github` MCP server configured
 */

const ENGINE_URL = process.env.VITE_ENGINE_URL || "http://127.0.0.1:8000";
const API_URL = `${ENGINE_URL}/api`;

const log = (msg: string) => console.log(`[Orchestrator] ${msg}`);
const err = (msg: string, e?: any) => console.error(`[!ERROR!] ${msg}`, e);

// Swarm Task States:
// pending -> scoping -> implementing -> testing -> ready_for_review -> completed | failed
interface SwarmTask {
  id: string;
  status: string;
  worker_id?: string;
  worktree_path?: string;
  pr_url?: string;
  error?: string;
}

/**
 * 1. Ensure the shared resource exists
 */
async function initializeSharedResource() {
  try {
    const res = await fetch(`${API_URL}/resources/swarm.active_tasks`);
    if (res.status === 404) {
      log("Initializing swarm.active_tasks shared resource...");
      await fetch(`${API_URL}/resources/swarm.active_tasks`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          type: "json",
          value: "[]",
          description: "Tracks active agent swarm worktrees and PRs.",
        }),
      });
    } else {
      log("Shared resource 'swarm.active_tasks' verified.");
    }
  } catch (e) {
    err("Failed to verify shared resource. Is Tandem running?", e);
    process.exit(1);
  }
}

async function getTasks(): Promise<SwarmTask[]> {
  const res = await fetch(`${API_URL}/resources/swarm.active_tasks`);
  const data = await res.json();
  return JSON.parse(data.value || "[]");
}

async function saveTasks(tasks: SwarmTask[]) {
  await fetch(`${API_URL}/resources/swarm.active_tasks`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      type: "json",
      value: JSON.stringify(tasks),
      description: "Tracks active agent swarm worktrees and PRs.",
    }),
  });
}

/**
 * 2. Listen to the Engine Event Bus (/v1/events)
 * This is where the magic happens: we map Tandem base events
 * into our custom Swarm State Machine.
 */
function startEventLoop() {
  log(`Connecting to Tandem Event Bus at ${ENGINE_URL}/v1/events...`);

  // NOTE: Tandem SSE events are broadcast globally on /v1/events
  const { spawn } = require("child_process");

  // We use curl for SSE because Node's native fetch doesn't support
  // streaming as easily without external dependencies like 'eventsource'
  const curl = spawn("curl", ["-N", "-s", `${ENGINE_URL}/v1/events`]);

  curl.stdout.on("data", async (data: Buffer) => {
    const lines = data.toString().split("\n");
    for (const line of lines) {
      if (!line.startsWith("data: ")) continue;

      try {
        const event = JSON.parse(line.substring(6));
        await handleEvent(event);
      } catch (e) {
        // Ignore JSON parse errors for incomplete chunks
      }
    }
  });

  curl.on("close", () => {
    err("Event bus connection lost. Reconnecting in 5s...");
    setTimeout(startEventLoop, 5000);
  });
}

/**
 * 3. The State Machine Reducer
 * Reacts to Tool Calls and Run transitions
 */
async function handleEvent(event: any) {
  if (event.type === "tool.call.completed") {
    // Detect when the Manager finishes creating a worktree
    if (
      event.properties.tool === "execute_shell_command" &&
      event.properties.result.includes("WORKTREE_PATH=")
    ) {
      const stdout = event.properties.result;
      const pathMatch = stdout.match(/WORKTREE_PATH=(.+)/);
      const branchMatch = stdout.match(/BRANCH=(.+)/);

      if (pathMatch && branchMatch) {
        const path = pathMatch[1].trim();
        const branch = branchMatch[1].trim();
        const taskId = branch.replace("swarm/", "");

        log(`Manager created worktree for task ${taskId}: ${path}`);

        const tasks = await getTasks();
        const existing = tasks.find((t) => t.id === taskId);

        if (!existing) {
          tasks.push({
            id: taskId,
            status: "implementing",
            worktree_path: path,
          });
          await saveTasks(tasks);
          log(`Registered new swarm task: ${taskId}. Spawning worker...`);

          await triggerWorkerAgent(taskId, path);
        }
      }
    }

    // Detect GitHub MCP PR creation
    if (event.properties.tool === "create_pull_request") {
      log(`Detected PR creation. Extracting PR URL...`);
      // Extrapolate logic: find the task by checking which agent session fired this event
      // Mark it ready_for_review!
    }
  }

  // Handle MCP Auth Loops (CRITICAL TANDEM PATTERN)
  // If an MCP tool returns an auth error, we must block the loop, notify the user,
  // and set the state to blocked so it doesn't spin infinitely.
  if (event.type === "tool.call.failed" && event.properties?.error?.includes("mcpAuth.required")) {
    log(
      `[BLOCKED] Swarm hit an MCP Auth gate. Waiting for user approval on ${event.properties.tool}`
    );
    // In a full implementation, send a Telegram ping here using the Tandem native webhook!
  }
}

/**
 * 4. Triggering the downstream agents
 */
async function triggerWorkerAgent(taskId: string, worktreePath: string) {
  // Read the system prompt from disk
  const prompt = readFileSync(join(__dirname, "agents/worker.md"), "utf8");

  // Inject context
  const contextAwarePrompt = prompt
    .replace("{{TASK_ID}}", taskId)
    .replace("{{WORKTREE_PATH}}", worktreePath);

  // 1. Create a transient session for the worker
  const sessionRes = await fetch(`${API_URL}/sessions`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ title: `Swarm Worker: ${taskId}` }),
  });
  const session = await sessionRes.json();

  // 2. Instruct the engine to start the run
  log(`Spawning Worker Agent in Session ${session.id}...`);
  await fetch(`${API_URL}/sessions/${session.id}/run`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      prompt: contextAwarePrompt,
      // Tell Tandem to tightly sandbox this worker down to just this path!
      fs_allowlist: [worktreePath],
    }),
  });
}

// Bootstrap
(async () => {
  log("Starting Tandem Swarm Orchestrator...");
  await initializeSharedResource();
  startEventLoop();
})();
