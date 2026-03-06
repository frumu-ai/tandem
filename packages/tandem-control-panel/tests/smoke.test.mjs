import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import test from "node:test";

function getFreePort() {
  return new Promise((resolve, reject) => {
    const s = createServer();
    s.listen(0, "127.0.0.1", () => {
      const address = s.address();
      s.close(() => resolve(address.port));
    });
    s.on("error", reject);
  });
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitForReady(url, timeoutMs = 12000) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const res = await fetch(`${url}/api/system/health`);
      if (res.ok) return;
    } catch {
      // retry
    }
    await delay(150);
  }
  throw new Error(`Timed out waiting for ${url}`);
}

async function waitForCondition(checkFn, timeoutMs = 12000, intervalMs = 120) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const value = await checkFn();
    if (value) return value;
    await delay(intervalMs);
  }
  throw new Error("Timed out waiting for condition");
}

function textFromParts(parts) {
  return (Array.isArray(parts) ? parts : [])
    .map((part) => {
      if (!part) return "";
      if (typeof part === "string") return part;
      if (typeof part.text === "string") return part.text;
      if (typeof part.content === "string") return part.content;
      return "";
    })
    .filter(Boolean)
    .join("\n")
    .trim();
}

function lastRequiredToolPromptCall(promptSyncCalls) {
  return [...(Array.isArray(promptSyncCalls) ? promptSyncCalls : [])]
    .reverse()
    .find((call) => call?.body?.tool_mode === "required");
}

async function startFakeEngine(options = {}) {
  const port = await getFreePort();
  const token = "smoke-token";
  const requests = [];
  const sessionCreates = [];
  const promptSyncCalls = [];
  const runs = new Map();
  const runEvents = new Map();
  const sessions = new Map();

  const server = createServer(async (req, res) => {
    const url = new URL(req.url || "/", `http://127.0.0.1:${port}`);
    const auth = req.headers.authorization || "";
    const xToken = req.headers["x-tandem-token"] || "";
    requests.push({ path: url.pathname, auth, xToken });

    if (url.pathname === "/global/health") {
      if (auth === `Bearer ${token}` || xToken === token) {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(
          JSON.stringify({
            ready: true,
            healthy: true,
            version: "test-engine",
            apiTokenRequired: true,
          })
        );
        return;
      }
      // /global/health is intentionally open in real engine auth-gate.
      res.writeHead(200, { "content-type": "application/json" });
      res.end(
        JSON.stringify({
          ready: true,
          healthy: true,
          version: "test-engine",
          apiTokenRequired: true,
        })
      );
      return;
    }

    if (url.pathname === "/config/providers") {
      if (auth !== `Bearer ${token}` && xToken !== token) {
        res.writeHead(401, { "content-type": "application/json" });
        res.end(JSON.stringify({ ok: false, error: "unauthorized" }));
        return;
      }
      res.writeHead(200, { "content-type": "application/json" });
      res.end(
        JSON.stringify({
          default: "openai",
          providers: { openai: { default_model: "gpt-4o-mini" } },
        })
      );
      return;
    }

    if (url.pathname === "/context/runs" && req.method === "POST") {
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      const input = chunks.length ? JSON.parse(Buffer.concat(chunks).toString("utf8")) : {};
      const runId = `run-${Math.random().toString(16).slice(2, 10)}`;
      const now = Date.now();
      const run = {
        run_id: runId,
        run_type: input.run_type || "interactive",
        source_client: input.source_client || "control_panel",
        model_provider: input.model_provider || null,
        model_id: input.model_id || null,
        mcp_servers: Array.isArray(input.mcp_servers) ? input.mcp_servers : [],
        status: "queued",
        objective: input.objective || "",
        workspace: input.workspace || {
          workspace_id: "ws-test",
          canonical_path: process.cwd(),
          lease_epoch: 1,
        },
        steps: [],
        tasks: [],
        why_next_step: null,
        revision: 1,
        created_at_ms: now,
        started_at_ms: null,
        ended_at_ms: null,
        last_error: null,
        updated_at_ms: now,
      };
      runs.set(runId, run);
      runEvents.set(runId, []);
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: true, run }));
      return;
    }

    if (url.pathname === "/session" && req.method === "POST") {
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      const input = chunks.length ? JSON.parse(Buffer.concat(chunks).toString("utf8")) : {};
      const sessionId = `sess-${Math.random().toString(16).slice(2, 10)}`;
      sessions.set(sessionId, { id: sessionId, messages: [] });
      sessionCreates.push({ path: url.pathname, method: req.method, body: input });
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ id: sessionId }));
      return;
    }

    if (url.pathname.match(/^\/session\/[^/]+\/prompt_sync$/) && req.method === "POST") {
      const sessionId = decodeURIComponent(url.pathname.split("/")[2] || "");
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      const input = chunks.length ? JSON.parse(Buffer.concat(chunks).toString("utf8")) : {};
      const snapshot = sessions.get(sessionId);
      const call = {
        sessionId,
        body: input,
        callIndex: promptSyncCalls.length + 1,
      };
      promptSyncCalls.push(call);
      if (snapshot) {
        snapshot.messages.push({
          role: "user",
          content: textFromParts(input?.parts),
        });
      }

      if (typeof options.onPromptSync === "function") {
        const response = await options.onPromptSync({ call, snapshot });
        const status = Number(response?.status || 200);
        const payload =
          response && Object.prototype.hasOwnProperty.call(response, "body") ? response.body : {};
        if (snapshot && Array.isArray(response?.appendMessages)) {
          snapshot.messages.push(...response.appendMessages);
        }
        res.writeHead(status, { "content-type": "application/json" });
        res.end(JSON.stringify(payload));
        return;
      }

      const message = { role: "assistant", content: "Attempted execution." };
      if (snapshot) snapshot.messages.push(message);
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(snapshot?.messages || [message]));
      return;
    }

    if (url.pathname.match(/^\/session\/[^/]+$/) && req.method === "GET") {
      const sessionId = decodeURIComponent(url.pathname.split("/")[2] || "");
      const snapshot = sessions.get(sessionId) || { id: sessionId, messages: [] };
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(snapshot));
      return;
    }

    if (url.pathname === "/context/runs" && req.method === "GET") {
      const workspace = url.searchParams.get("workspace") || "";
      const rows = [...runs.values()].filter(
        (run) => !workspace || run.workspace?.canonical_path === workspace
      );
      rows.sort((a, b) => Number(b.updated_at_ms || 0) - Number(a.updated_at_ms || 0));
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ runs: rows }));
      return;
    }

    if (url.pathname.match(/^\/context\/runs\/[^/]+$/) && req.method === "GET") {
      const runId = decodeURIComponent(url.pathname.split("/").at(-1) || "");
      const run = runs.get(runId);
      if (!run) {
        res.writeHead(404, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: "run not found" }));
        return;
      }
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ run }));
      return;
    }

    if (url.pathname.match(/^\/context\/runs\/[^/]+$/) && req.method === "PUT") {
      const runId = decodeURIComponent(url.pathname.split("/").at(-1) || "");
      const existing = runs.get(runId);
      if (!existing) {
        res.writeHead(404, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: "run not found" }));
        return;
      }
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      const input = chunks.length ? JSON.parse(Buffer.concat(chunks).toString("utf8")) : {};
      const merged = { ...existing, ...input, updated_at_ms: Date.now() };
      runs.set(runId, merged);
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: true, run: merged }));
      return;
    }

    if (url.pathname.match(/^\/context\/runs\/[^/]+\/events$/) && req.method === "POST") {
      const runId = decodeURIComponent(url.pathname.split("/")[3] || "");
      const run = runs.get(runId);
      if (!run) {
        res.writeHead(404, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: "run not found" }));
        return;
      }
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      const input = chunks.length ? JSON.parse(Buffer.concat(chunks).toString("utf8")) : {};
      const events = runEvents.get(runId) || [];
      const event = {
        event_id: `evt-${events.length + 1}`,
        run_id: runId,
        seq: events.length + 1,
        ts_ms: Date.now(),
        type: input.type || "event",
        status: input.status || run.status,
        step_id: input.step_id || null,
        payload: input.payload || {},
      };
      events.push(event);
      runEvents.set(runId, events);
      run.status = event.status;
      run.updated_at_ms = event.ts_ms;
      if (event.type === "planning_started") {
        run.status = "awaiting_approval";
        run.steps = [
          { step_id: "step-1", title: "Draft plan", status: "pending" },
          { step_id: "step-2", title: "Implement", status: "pending" },
        ];
      }
      if (event.type === "plan_approved") {
        run.status = "running";
        if (run.steps[0]) run.steps[0].status = "in_progress";
      }
      if (event.type === "step_started" && event.step_id) {
        const step = run.steps.find((row) => row.step_id === event.step_id);
        if (step) step.status = "in_progress";
      }
      if (event.type === "step_completed" && event.step_id) {
        const step = run.steps.find((row) => row.step_id === event.step_id);
        if (step) step.status = "done";
      }
      if (event.type === "step_failed" && event.step_id) {
        const step = run.steps.find((row) => row.step_id === event.step_id);
        if (step) step.status = "failed";
      }
      if (event.type === "run_cancelled") run.status = "cancelled";
      if (event.type === "task_retry_requested" && event.step_id) {
        const step = run.steps.find((row) => row.step_id === event.step_id);
        if (step) step.status = "runnable";
      }
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: true, event }));
      return;
    }

    if (url.pathname.match(/^\/context\/runs\/[^/]+\/events$/) && req.method === "GET") {
      const runId = decodeURIComponent(url.pathname.split("/")[3] || "");
      const events = runEvents.get(runId) || [];
      const sinceSeq = Number(url.searchParams.get("since_seq") || "0");
      const tail = Number(url.searchParams.get("tail") || "0");
      const filtered = events.filter((evt) => Number(evt.seq || 0) > sinceSeq);
      const output = tail > 0 ? filtered.slice(-tail) : filtered;
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ events: output }));
      return;
    }

    if (url.pathname.match(/^\/context\/runs\/[^/]+\/todos\/sync$/) && req.method === "POST") {
      const runId = decodeURIComponent(url.pathname.split("/")[3] || "");
      const run = runs.get(runId);
      if (!run) {
        res.writeHead(404, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: "run not found" }));
        return;
      }
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      const input = chunks.length ? JSON.parse(Buffer.concat(chunks).toString("utf8")) : {};
      const todos = Array.isArray(input.todos) ? input.todos : [];
      run.steps = todos.map((todo, idx) => ({
        step_id: String(todo.id || `step-${idx + 1}`),
        title: String(todo.content || "").trim(),
        status: String(todo.status || "pending"),
      }));
      run.updated_at_ms = Date.now();
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: true, run }));
      return;
    }

    if (url.pathname.match(/^\/context\/runs\/[^/]+\/driver\/next$/) && req.method === "POST") {
      const runId = decodeURIComponent(url.pathname.split("/")[3] || "");
      const run = runs.get(runId);
      if (!run) {
        res.writeHead(404, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: "run not found" }));
        return;
      }
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      const input = chunks.length ? JSON.parse(Buffer.concat(chunks).toString("utf8")) : {};
      const dryRun = input?.dry_run === true;
      const steps = Array.isArray(run.steps) ? run.steps : [];
      let selected = steps.find((step) => String(step?.status || "") === "in_progress");
      if (!selected) {
        selected = steps.find((step) => String(step?.status || "") === "pending");
        if (selected && !dryRun) selected.status = "in_progress";
      }
      run.updated_at_ms = Date.now();
      runs.set(runId, run);
      res.writeHead(200, { "content-type": "application/json" });
      res.end(
        JSON.stringify({
          selected_step_id: selected?.step_id || null,
          why_next_step: selected ? `execute ${selected.step_id}` : "no actionable steps",
          run,
        })
      );
      return;
    }

    if (url.pathname.match(/^\/context\/runs\/[^/]+\/tasks$/) && req.method === "POST") {
      const runId = decodeURIComponent(url.pathname.split("/")[3] || "");
      const run = runs.get(runId);
      if (!run) {
        res.writeHead(404, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: "run not found" }));
        return;
      }
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      const input = chunks.length ? JSON.parse(Buffer.concat(chunks).toString("utf8")) : {};
      const tasks = Array.isArray(input?.tasks) ? input.tasks : [];
      run.tasks = tasks.map((task, index) => ({
        id: String(task?.id || `task-${index + 1}`),
        task_type: String(task?.task_type || "inspection"),
        payload: task?.payload || {},
        status: String(task?.status || "pending"),
        workflow_id: task?.workflow_id || null,
        depends_on_task_ids: Array.isArray(task?.depends_on_task_ids)
          ? task.depends_on_task_ids
          : [],
        assigned_agent: null,
        attempt: 0,
        max_attempts: Number(task?.max_attempts || 3),
        last_error: null,
        lease_token: null,
        task_rev: 1,
      }));
      run.updated_at_ms = Date.now();
      runs.set(runId, run);
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: true, tasks: run.tasks }));
      return;
    }

    if (url.pathname.match(/^\/context\/runs\/[^/]+\/tasks\/claim$/) && req.method === "POST") {
      const runId = decodeURIComponent(url.pathname.split("/")[3] || "");
      const run = runs.get(runId);
      if (!run) {
        res.writeHead(404, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: "run not found" }));
        return;
      }
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      const input = chunks.length ? JSON.parse(Buffer.concat(chunks).toString("utf8")) : {};
      const selected = (Array.isArray(run.tasks) ? run.tasks : []).find((task) =>
        ["pending", "runnable"].includes(String(task?.status || ""))
      );
      if (selected) {
        selected.status = "in_progress";
        selected.assigned_agent = String(input?.agent_id || "").trim() || null;
        selected.lease_token = "lease-test";
        selected.task_rev = Number(selected.task_rev || 0) + 1;
      }
      run.updated_at_ms = Date.now();
      runs.set(runId, run);
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: true, task: selected || null, blackboard: { tasks: run.tasks } }));
      return;
    }

    if (
      url.pathname.match(/^\/context\/runs\/[^/]+\/tasks\/[^/]+\/transition$/) &&
      req.method === "POST"
    ) {
      const [, , , runId, , taskId] = url.pathname.split("/");
      const run = runs.get(decodeURIComponent(runId || ""));
      if (!run) {
        res.writeHead(404, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: "run not found" }));
        return;
      }
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      const input = chunks.length ? JSON.parse(Buffer.concat(chunks).toString("utf8")) : {};
      const task = (Array.isArray(run.tasks) ? run.tasks : []).find(
        (row) => row.id === decodeURIComponent(taskId || "")
      );
      if (!task) {
        res.writeHead(404, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: "task not found" }));
        return;
      }
      if (input?.action === "complete") task.status = "done";
      else if (input?.action === "fail") {
        task.status = "failed";
        task.last_error = input?.error || null;
      } else if (input?.action === "retry" || input?.action === "release") task.status = "runnable";
      else if (input?.status) task.status = String(input.status);
      task.task_rev = Number(task.task_rev || 0) + 1;
      run.updated_at_ms = Date.now();
      runs.set(runId, run);
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: true, task, blackboard: { tasks: run.tasks } }));
      return;
    }

    if (
      url.pathname.match(/^\/context\/runs\/[^/]+\/(blackboard|replay)$/) &&
      req.method === "GET"
    ) {
      const kind = url.pathname.split("/").at(-1);
      res.writeHead(200, { "content-type": "application/json" });
      if (kind === "blackboard") {
        res.end(
          JSON.stringify({
            blackboard: {
              facts: [],
              decisions: [],
              open_questions: [],
              artifacts: [],
              tasks: Array.isArray(runs.get(decodeURIComponent(url.pathname.split("/")[3] || ""))?.tasks)
                ? runs.get(decodeURIComponent(url.pathname.split("/")[3] || "")).tasks
                : [],
              summaries: { rolling: "" },
              revision: 0,
            },
          })
        );
      } else {
        res.end(JSON.stringify({ ok: true, replay: null, drift: { mismatch: false } }));
      }
      return;
    }

    if (url.pathname === "/global/event") {
      res.writeHead(200, {
        "content-type": "text/event-stream",
        "cache-control": "no-cache",
        connection: "keep-alive",
      });
      res.write(`data: ${JSON.stringify({ type: "test.event", runID: "run-1" })}\n\n`);
      setTimeout(() => res.end(), 50);
      return;
    }

    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ ok: true, path: url.pathname }));
  });

  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", resolve);
  });

  return {
    server,
    port,
    token,
    requests,
    sessionCreates,
    promptSyncCalls,
    close: () => new Promise((resolve) => server.close(() => resolve())),
  };
}

function extractCookie(res) {
  const direct = res.headers.get("set-cookie");
  if (direct) return direct.split(";")[0];
  if (typeof res.headers.getSetCookie === "function") {
    const cookies = res.headers.getSetCookie();
    if (cookies[0]) return cookies[0].split(";")[0];
  }
  return "";
}

async function request(baseUrl, path, { method = "GET", body, cookie } = {}) {
  return fetch(`${baseUrl}${path}`, {
    method,
    headers: {
      ...(body ? { "content-type": "application/json" } : {}),
      ...(cookie ? { cookie } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });
}

test("control panel auth/proxy/swarm smoke", async (t) => {
  const fake = await startFakeEngine({
    onPromptSync: async ({ call, snapshot }) => {
      if (call?.body?.tool_mode === "required") {
        const assistant = { role: "assistant", content: "Attempted execution." };
        if (snapshot) snapshot.messages.push(assistant);
        return { status: 200, body: snapshot?.messages || [assistant] };
      }
      const assistant = {
        role: "assistant",
        content: JSON.stringify([
          {
            id: "task-1",
            title: "Create game shell",
            description: "Create the initial game.html shell.",
            dependencies: [],
            acceptance_criteria: ["game.html exists"],
            assigned_role: "worker",
            output_target: {
              path: "game.html",
              kind: "source",
              operation: "create_or_update",
            },
          },
        ]),
      };
      if (snapshot) snapshot.messages.push(assistant);
      return { status: 200, body: snapshot?.messages || [assistant] };
    },
  });
  t.after(async () => {
    await fake.close();
  });

  const panelPort = await getFreePort();
  const baseUrl = `http://127.0.0.1:${panelPort}`;

  const panel = spawn(process.execPath, ["bin/setup.js"], {
    cwd: new URL("..", import.meta.url),
    env: {
      ...process.env,
      TANDEM_CONTROL_PANEL_PORT: String(panelPort),
      TANDEM_ENGINE_URL: `http://127.0.0.1:${fake.port}`,
      TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "0",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  let panelOutput = "";
  panel.stdout.on("data", (chunk) => {
    panelOutput += chunk.toString();
  });
  panel.stderr.on("data", (chunk) => {
    panelOutput += chunk.toString();
  });

  t.after(() => {
    if (!panel.killed) panel.kill("SIGTERM");
  });

  await waitForReady(baseUrl);

  const shell = await request(baseUrl, "/");
  assert.equal(shell.status, 200);
  const shellHtml = await shell.text();
  assert.ok(shellHtml.includes('id="app"'), "missing app mount");
  assert.ok(shellHtml.includes("toasts"), "missing toast host");

  const unauthProxy = await request(baseUrl, "/api/engine/global/health");
  assert.equal(unauthProxy.status, 401);

  const badLogin = await request(baseUrl, "/api/auth/login", {
    method: "POST",
    body: { token: "wrong-token" },
  });
  assert.equal(badLogin.status, 401);

  const login = await request(baseUrl, "/api/auth/login", {
    method: "POST",
    body: { token: fake.token },
  });
  assert.equal(login.status, 200, panelOutput);
  const cookie = extractCookie(login);
  assert.ok(cookie.includes("tcp_sid="), "missing session cookie");

  const me = await request(baseUrl, "/api/auth/me", { cookie });
  assert.equal(me.status, 200);

  const proxy = await request(baseUrl, "/api/engine/global/health", { cookie });
  assert.equal(proxy.status, 200);
  const proxyJson = await proxy.json();
  assert.equal(proxyJson.version, "test-engine");

  const swarmStatus = await request(baseUrl, "/api/swarm/status", { cookie });
  assert.equal(swarmStatus.status, 200);
  const swarmStatusJson = await swarmStatus.json();
  assert.equal(typeof swarmStatusJson.status, "string");

  const swarmSnapshot = await request(baseUrl, "/api/swarm/snapshot", { cookie });
  assert.equal(swarmSnapshot.status, 200);
  const snapshotJson = await swarmSnapshot.json();
  assert.ok(snapshotJson.registry?.value?.tasks, "missing registry tasks");

  const swarmStart = await request(baseUrl, "/api/swarm/start", {
    method: "POST",
    cookie,
    body: {
      workspaceRoot: process.cwd(),
      objective: "Create `game.html` for the test objective",
      maxTasks: 2,
      verificationMode: "strict",
      requireLlmPlan: true,
      allowLocalPlannerFallback: true,
    },
  });
  const swarmStartJson = await swarmStart.json();
  assert.equal(swarmStart.status, 200, JSON.stringify(swarmStartJson));
  assert.ok(String(swarmStartJson.runId || "").length > 0, "missing run id");

  const runsRes = await request(
    baseUrl,
    `/api/swarm/runs?workspace=${encodeURIComponent(process.cwd())}`,
    { cookie }
  );
  assert.equal(runsRes.status, 200);
  const runsJson = await runsRes.json();
  assert.ok(Array.isArray(runsJson.runs) && runsJson.runs.length > 0, "missing runs");

  const runRes = await request(
    baseUrl,
    `/api/swarm/run/${encodeURIComponent(swarmStartJson.runId)}`,
    { cookie }
  );
  assert.equal(runRes.status, 200);
  const runJson = await runRes.json();
  assert.equal(runJson.run?.run_id, swarmStartJson.runId);
  assert.ok(Array.isArray(runJson.tasks), "missing tasks array");

  const approveRes = await request(baseUrl, "/api/swarm/approve", {
    method: "POST",
    cookie,
    body: { runId: swarmStartJson.runId },
  });
  assert.equal(approveRes.status, 200);

  await waitForCondition(
    () => fake.promptSyncCalls.some((call) => call?.body?.tool_mode === "required"),
    12000
  );
  assert.ok(
    fake.promptSyncCalls.some((call) => call?.body?.tool_mode === "required"),
    "expected execution prompt_sync calls to set tool_mode=required"
  );
  assert.ok(
    fake.promptSyncCalls.some((call) =>
      call?.body?.tool_mode === "required" && call?.body?.write_required === true
    ),
    "expected execution prompt_sync calls to set write_required=true"
  );
  assert.ok(
    fake.sessionCreates.some(
      (call) =>
        Array.isArray(call?.body?.permission) &&
        call.body.permission.some((rule) => rule.permission === "bash" && rule.action === "deny")
    ),
    "expected session permission rules to deny bash"
  );
  assert.ok(
    fake.sessionCreates.some(
      (call) =>
        Array.isArray(call?.body?.permission) &&
        call.body.permission.some((rule) => rule.permission === "write" && rule.action === "allow")
    ),
    "expected worker session permission rules to allow write"
  );

  await waitForCondition(async () => {
    const runState = await request(
      baseUrl,
      `/api/swarm/run/${encodeURIComponent(swarmStartJson.runId)}`,
      {
        cookie,
      }
    );
    if (!runState.ok) return false;
    const payload = await runState.json();
    const status = String(payload?.run?.status || "").toLowerCase();
    return status === "failed";
  }, 12000);

  const failedRunRes = await request(
    baseUrl,
    `/api/swarm/run/${encodeURIComponent(swarmStartJson.runId)}`,
    {
      cookie,
    }
  );
  assert.equal(failedRunRes.status, 200);
  const failedRunJson = await failedRunRes.json();
  const events = Array.isArray(failedRunJson.events) ? failedRunJson.events : [];
  const stepStarted = events.find(
    (event) => event?.type === "step_started" || event?.type === "task_started"
  );
  const stepFailed = events.find(
    (event) => event?.type === "step_failed" || event?.type === "task_failed"
  );
  assert.ok(stepStarted, "missing step_started event");
  assert.ok(stepFailed, "missing step_failed event");
  assert.ok(
    String(stepStarted?.payload?.session_id || "").length > 0,
    "step_started missing session_id"
  );
  assert.ok(
    String(stepFailed?.payload?.session_id || "").length > 0,
    "step_failed missing session_id"
  );
  assert.deepEqual(stepFailed?.payload?.verification?.mode, "strict");
  assert.deepEqual(
    stepFailed?.payload?.verification?.reason,
    "NO_TOOL_ACTIVITY_NO_WORKSPACE_CHANGE"
  );
  assert.equal(stepFailed?.payload?.verification?.passed, false);
  assert.equal(stepFailed?.payload?.verification?.workspace_changed, false);

  const proxiedAuthSeen = fake.requests.some(
    (r) => r.path === "/global/health" && r.auth === `Bearer ${fake.token}`
  );
  assert.ok(proxiedAuthSeen, "proxy did not forward token auth header");
});

test("swarm retry verification does not reuse stale assistant output", async (t) => {
  let requiredCallCount = 0;
  const fake = await startFakeEngine({
    onPromptSync: async ({ call, snapshot }) => {
      if (call?.body?.tool_mode !== "required") {
        const assistant = {
          role: "assistant",
          content: JSON.stringify([
            {
              id: "task-1",
              title: "Create game shell",
              description: "Create the initial game.html shell.",
              dependencies: [],
              acceptance_criteria: ["game.html exists"],
              assigned_role: "worker",
              output_target: {
                path: "game.html",
                kind: "source",
                operation: "create_or_update",
              },
            },
          ]),
        };
        if (snapshot) snapshot.messages.push(assistant);
        return { status: 200, body: snapshot?.messages || [assistant] };
      }
      requiredCallCount += 1;
      if (requiredCallCount === 1) {
        const assistant = { role: "assistant", content: "Planned only. No files changed." };
        if (snapshot) snapshot.messages.push(assistant);
        return { status: 200, body: snapshot?.messages || [assistant] };
      }
      return { status: 500, body: { error: "retry dispatch failed" } };
    },
  });
  t.after(async () => {
    await fake.close();
  });

  const panelPort = await getFreePort();
  const baseUrl = `http://127.0.0.1:${panelPort}`;

  const panel = spawn(process.execPath, ["bin/setup.js"], {
    cwd: new URL("..", import.meta.url),
    env: {
      ...process.env,
      TANDEM_CONTROL_PANEL_PORT: String(panelPort),
      TANDEM_ENGINE_URL: `http://127.0.0.1:${fake.port}`,
      TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "0",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  t.after(() => {
    if (!panel.killed) panel.kill("SIGTERM");
  });

  await waitForReady(baseUrl);

  const login = await request(baseUrl, "/api/auth/login", {
    method: "POST",
    body: { token: fake.token },
  });
  assert.equal(login.status, 200);
  const cookie = extractCookie(login);

  const swarmStart = await request(baseUrl, "/api/swarm/start", {
    method: "POST",
    cookie,
    body: {
      workspaceRoot: process.cwd(),
      objective: "Create `game.html` for the test objective",
      maxTasks: 2,
      verificationMode: "strict",
      requireLlmPlan: true,
      allowLocalPlannerFallback: true,
    },
  });
  const swarmStartJson = await swarmStart.json();
  assert.equal(swarmStart.status, 200, JSON.stringify(swarmStartJson));

  const approveRes = await request(baseUrl, "/api/swarm/approve", {
    method: "POST",
    cookie,
    body: { runId: swarmStartJson.runId },
  });
  assert.equal(approveRes.status, 200);

  await waitForCondition(async () => {
    const runState = await request(
      baseUrl,
      `/api/swarm/run/${encodeURIComponent(swarmStartJson.runId)}`,
      {
        cookie,
      }
    );
    if (!runState.ok) return false;
    const payload = await runState.json();
    return String(payload?.run?.status || "").toLowerCase() === "failed";
  }, 12000);

  const failedRunRes = await request(
    baseUrl,
    `/api/swarm/run/${encodeURIComponent(swarmStartJson.runId)}`,
    {
      cookie,
    }
  );
  assert.equal(failedRunRes.status, 200);
  const failedRunJson = await failedRunRes.json();
  const events = Array.isArray(failedRunJson.events) ? failedRunJson.events : [];
  const stepFailed = events.find(
    (event) => event?.type === "step_failed" || event?.type === "task_failed"
  );
  assert.ok(stepFailed, "missing step_failed event");
  assert.deepEqual(lastRequiredToolPromptCall(fake.promptSyncCalls)?.body?.tool_allowlist, [
    "ls",
    "list",
    "glob",
    "search",
    "grep",
    "codesearch",
    "read",
    "write",
    "edit",
    "apply_patch",
  ]);
  assert.match(String(stepFailed?.payload?.error || ""), /retry dispatch failed/i);
  assert.doesNotMatch(
    String(stepFailed?.payload?.error || ""),
    /NO_TOOL_ACTIVITY_NO_WORKSPACE_CHANGE/
  );
});

test("swarm retry preserves inspection tools when the first required attempt made no tool calls", async (t) => {
  let requiredCallCount = 0;
  const fake = await startFakeEngine({
    onPromptSync: async ({ call, snapshot }) => {
      if (call?.body?.tool_mode !== "required") {
        const assistant = {
          role: "assistant",
          content: JSON.stringify([
            {
              id: "task-1",
              title: "Create game shell",
              description: "Create the initial game.html shell.",
              dependencies: [],
              acceptance_criteria: ["game.html exists"],
              assigned_role: "worker",
              output_target: {
                path: "game.html",
                kind: "source",
                operation: "create_or_update",
              },
            },
          ]),
        };
        if (snapshot) snapshot.messages.push(assistant);
        return { status: 200, body: snapshot?.messages || [assistant] };
      }
      requiredCallCount += 1;
      if (requiredCallCount === 1) {
        const assistant = { role: "assistant", content: "Inspected the workspace only." };
        if (snapshot) snapshot.messages.push(assistant);
        return { status: 200, body: snapshot?.messages || [assistant] };
      }
      const assistant = {
        role: "assistant",
        content:
          "TOOL_MODE_REQUIRED_NOT_SATISFIED: tool_mode=required but the model ended without executing any tool calls.",
      };
      if (snapshot) snapshot.messages.push(assistant);
      return { status: 200, body: snapshot?.messages || [assistant] };
    },
  });
  t.after(async () => {
    await fake.close();
  });

  const panelPort = await getFreePort();
  const baseUrl = `http://127.0.0.1:${panelPort}`;

  const panel = spawn(process.execPath, ["bin/setup.js"], {
    cwd: new URL("..", import.meta.url),
    env: {
      ...process.env,
      TANDEM_CONTROL_PANEL_PORT: String(panelPort),
      TANDEM_ENGINE_URL: `http://127.0.0.1:${fake.port}`,
      TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "0",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  t.after(() => {
    if (!panel.killed) panel.kill("SIGTERM");
  });

  await waitForReady(baseUrl);

  const login = await request(baseUrl, "/api/auth/login", {
    method: "POST",
    body: { token: fake.token },
  });
  assert.equal(login.status, 200);
  const cookie = extractCookie(login);

  const swarmStart = await request(baseUrl, "/api/swarm/start", {
    method: "POST",
    cookie,
    body: {
      workspaceRoot: process.cwd(),
      objective: "Create `game.html` for the test objective",
      maxTasks: 2,
      verificationMode: "strict",
      requireLlmPlan: true,
      allowLocalPlannerFallback: true,
    },
  });
  const swarmStartJson = await swarmStart.json();
  assert.equal(swarmStart.status, 200, JSON.stringify(swarmStartJson));

  const approveRes = await request(baseUrl, "/api/swarm/approve", {
    method: "POST",
    cookie,
    body: { runId: swarmStartJson.runId },
  });
  assert.equal(approveRes.status, 200);

  await waitForCondition(async () => {
    const runState = await request(
      baseUrl,
      `/api/swarm/run/${encodeURIComponent(swarmStartJson.runId)}`,
      {
        cookie,
      }
    );
    if (!runState.ok) return false;
    const payload = await runState.json();
    return String(payload?.run?.status || "").toLowerCase() === "failed";
  }, 12000);

  const failedRunRes = await request(
    baseUrl,
    `/api/swarm/run/${encodeURIComponent(swarmStartJson.runId)}`,
    {
      cookie,
    }
  );
  assert.equal(failedRunRes.status, 200);
  const failedRunJson = await failedRunRes.json();
  const events = Array.isArray(failedRunJson.events) ? failedRunJson.events : [];
  const stepFailed = events.find(
    (event) => event?.type === "step_failed" || event?.type === "task_failed"
  );
  assert.ok(stepFailed, "missing step_failed event");
  assert.deepEqual(lastRequiredToolPromptCall(fake.promptSyncCalls)?.body?.tool_allowlist, [
    "ls",
    "list",
    "glob",
    "search",
    "grep",
    "codesearch",
    "read",
    "write",
    "edit",
    "apply_patch",
  ]);
  assert.equal(stepFailed?.payload?.verification?.reason, "TOOL_MODE_REQUIRED_NOT_SATISFIED");
});

test("swarm verification prefers provider-specific malformed write reasons", async (t) => {
  const fake = await startFakeEngine({
    onPromptSync: async ({ call, snapshot }) => {
      if (call?.body?.tool_mode !== "required") {
        const assistant = {
          role: "assistant",
          content: JSON.stringify([
            {
              id: "task-1",
              title: "Create game shell",
              description: "Create the initial game.html shell.",
              dependencies: [],
              acceptance_criteria: ["game.html exists"],
              assigned_role: "worker",
              output_target: {
                path: "game.html",
                kind: "source",
                operation: "create_or_update",
              },
            },
          ]),
        };
        if (snapshot) snapshot.messages.push(assistant);
        return { status: 200, body: snapshot?.messages || [assistant] };
      }
      return {
        status: 200,
        body: [
          {
            role: "user",
            parts: [
              { type: "text", text: textFromParts(call?.body?.parts) },
              { type: "tool_invocation", tool: "glob", args: {}, result: "README.md" },
              {
                type: "tool_invocation",
                tool: "write",
                args: {},
                error: "WRITE_ARGS_EMPTY_FROM_PROVIDER",
              },
            ],
          },
          {
            role: "assistant",
            content:
              "TOOL_MODE_REQUIRED_NOT_SATISFIED: WRITE_REQUIRED_NOT_SATISFIED: tool_mode=required but the model ended without executing a productive tool call.",
          },
        ],
      };
    },
  });
  t.after(async () => {
    await fake.close();
  });

  const panelPort = await getFreePort();
  const baseUrl = `http://127.0.0.1:${panelPort}`;

  const panel = spawn(process.execPath, ["bin/setup.js"], {
    cwd: new URL("..", import.meta.url),
    env: {
      ...process.env,
      TANDEM_CONTROL_PANEL_PORT: String(panelPort),
      TANDEM_ENGINE_URL: `http://127.0.0.1:${fake.port}`,
      TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "0",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  t.after(() => {
    if (!panel.killed) panel.kill("SIGTERM");
  });

  await waitForReady(baseUrl);

  const login = await request(baseUrl, "/api/auth/login", {
    method: "POST",
    body: { token: fake.token },
  });
  assert.equal(login.status, 200);
  const cookie = extractCookie(login);

  const swarmStart = await request(baseUrl, "/api/swarm/start", {
    method: "POST",
    cookie,
    body: {
      workspaceRoot: process.cwd(),
      objective: "Build `game.html`",
      maxTasks: 1,
      verificationMode: "strict",
      requireLlmPlan: true,
      allowLocalPlannerFallback: true,
    },
  });
  const swarmStartJson = await swarmStart.json();
  assert.equal(swarmStart.status, 200, JSON.stringify(swarmStartJson));

  const approveRes = await request(baseUrl, "/api/swarm/approve", {
    method: "POST",
    cookie,
    body: { runId: swarmStartJson.runId },
  });
  assert.equal(approveRes.status, 200);

  await waitForCondition(async () => {
    const runState = await request(
      baseUrl,
      `/api/swarm/run/${encodeURIComponent(swarmStartJson.runId)}`,
      {
        cookie,
      }
    );
    if (!runState.ok) return false;
    const payload = await runState.json();
    return String(payload?.run?.status || "").toLowerCase() === "failed";
  }, 12000);

  const failedRunRes = await request(
    baseUrl,
    `/api/swarm/run/${encodeURIComponent(swarmStartJson.runId)}`,
    {
      cookie,
    }
  );
  assert.equal(failedRunRes.status, 200);
  const failedRunJson = await failedRunRes.json();
  const events = Array.isArray(failedRunJson.events) ? failedRunJson.events : [];
  const stepFailed = events.find(
    (event) => event?.type === "step_failed" || event?.type === "task_failed"
  );
  assert.ok(stepFailed, "missing step_failed event");
  assert.equal(
    stepFailed?.payload?.verification?.reason,
    "WRITE_ARGS_EMPTY_FROM_PROVIDER"
  );
});

test("swarm strict write retries malformed write failures up to configured budget", async (t) => {
  let requiredCallCount = 0;
  const fake = await startFakeEngine({
    onPromptSync: async ({ call, snapshot }) => {
      if (call?.body?.tool_mode !== "required") {
        const assistant = {
          role: "assistant",
          content: JSON.stringify([
            {
              id: "task-1",
              title: "Create game shell",
              description: "Create the initial game.html shell.",
              dependencies: [],
              acceptance_criteria: ["game.html exists"],
              assigned_role: "worker",
              output_target: {
                path: "game.html",
                kind: "source",
                operation: "create_or_update",
              },
            },
          ]),
        };
        if (snapshot) snapshot.messages.push(assistant);
        return { status: 200, body: snapshot?.messages || [assistant] };
      }
      requiredCallCount += 1;
      return {
        status: 200,
        body: [
          {
            role: "user",
            parts: [
              { type: "text", text: textFromParts(call?.body?.parts) },
              { type: "tool_invocation", tool: "glob", args: {}, result: "README.md" },
              {
                type: "tool_invocation",
                tool: "write",
                args: {},
                error: "WRITE_ARGS_EMPTY_FROM_PROVIDER",
              },
            ],
          },
          {
            role: "assistant",
            content:
              "TOOL_MODE_REQUIRED_NOT_SATISFIED: WRITE_REQUIRED_NOT_SATISFIED: tool_mode=required but the model ended without executing a productive tool call.",
          },
        ],
      };
    },
  });
  t.after(async () => {
    await fake.close();
  });

  const panelPort = await getFreePort();
  const baseUrl = `http://127.0.0.1:${panelPort}`;

  const panel = spawn(process.execPath, ["bin/setup.js"], {
    cwd: new URL("..", import.meta.url),
    env: {
      ...process.env,
      TANDEM_CONTROL_PANEL_PORT: String(panelPort),
      TANDEM_ENGINE_URL: `http://127.0.0.1:${fake.port}`,
      TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "0",
      TANDEM_STRICT_WRITE_RETRY_MAX_ATTEMPTS: "3",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  t.after(() => {
    if (!panel.killed) panel.kill("SIGTERM");
  });

  await waitForReady(baseUrl);

  const login = await request(baseUrl, "/api/auth/login", {
    method: "POST",
    body: { token: fake.token },
  });
  assert.equal(login.status, 200);
  const cookie = extractCookie(login);

  const swarmStart = await request(baseUrl, "/api/swarm/start", {
    method: "POST",
    cookie,
    body: {
      workspaceRoot: process.cwd(),
      objective: "Build `game.html`",
      maxTasks: 1,
      verificationMode: "strict",
      requireLlmPlan: true,
      allowLocalPlannerFallback: true,
    },
  });
  const swarmStartJson = await swarmStart.json();
  assert.equal(swarmStart.status, 200, JSON.stringify(swarmStartJson));

  const approveRes = await request(baseUrl, "/api/swarm/approve", {
    method: "POST",
    cookie,
    body: { runId: swarmStartJson.runId },
  });
  assert.equal(approveRes.status, 200);

  await waitForCondition(async () => {
    const runState = await request(
      baseUrl,
      `/api/swarm/run/${encodeURIComponent(swarmStartJson.runId)}`,
      {
        cookie,
      }
    );
    if (!runState.ok) return false;
    const payload = await runState.json();
    return String(payload?.run?.status || "").toLowerCase() === "failed";
  }, 12000);

  const failedRunRes = await request(
    baseUrl,
    `/api/swarm/run/${encodeURIComponent(swarmStartJson.runId)}`,
    {
      cookie,
    }
  );
  assert.equal(failedRunRes.status, 200);
  const failedRunJson = await failedRunRes.json();
  const events = Array.isArray(failedRunJson.events) ? failedRunJson.events : [];
  const stepFailed = events.find(
    (event) => event?.type === "step_failed" || event?.type === "task_failed"
  );
  assert.ok(stepFailed, "missing step_failed event");
  assert.equal(stepFailed?.payload?.verification?.reason, "WRITE_ARGS_EMPTY_FROM_PROVIDER");
  assert.equal(
    stepFailed?.payload?.verification?.execution_trace?.attempts?.length,
    3
  );
  assert.equal(requiredCallCount, 3);
});

test("swarm start seeds fallback tasks when llm planning is required but returns no valid tasks", async (t) => {
  const fake = await startFakeEngine({
    onPromptSync: async ({ call, snapshot }) => {
      if (call?.body?.tool_mode === "required") {
        const assistant = {
          role: "assistant",
          content:
            "TOOL_MODE_REQUIRED_NOT_SATISFIED: tool_mode=required but the model ended without executing any tool calls.",
        };
        if (snapshot) snapshot.messages.push(assistant);
        return { status: 200, body: snapshot?.messages || [assistant] };
      }
      const assistant = {
        role: "assistant",
        content: "Planner summary only. No valid JSON task payload returned.",
      };
      if (snapshot) snapshot.messages.push(assistant);
      return { status: 200, body: snapshot?.messages || [assistant] };
    },
  });
  t.after(async () => {
    await fake.close();
  });

  const panelPort = await getFreePort();
  const baseUrl = `http://127.0.0.1:${panelPort}`;

  const panel = spawn(process.execPath, ["bin/setup.js"], {
    cwd: new URL("..", import.meta.url),
    env: {
      ...process.env,
      TANDEM_CONTROL_PANEL_PORT: String(panelPort),
      TANDEM_ENGINE_URL: `http://127.0.0.1:${fake.port}`,
      TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "0",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  t.after(() => {
    if (!panel.killed) panel.kill("SIGTERM");
  });

  await waitForReady(baseUrl);

  const login = await request(baseUrl, "/api/auth/login", {
    method: "POST",
    body: { token: fake.token },
  });
  assert.equal(login.status, 200);
  const cookie = extractCookie(login);

  const swarmStart = await request(baseUrl, "/api/swarm/start", {
    method: "POST",
    cookie,
    body: {
      workspaceRoot: process.cwd(),
      objective: "Build a neon arcade browser game with HTML, CSS, and JS",
      maxTasks: 3,
      verificationMode: "strict",
      requireLlmPlan: true,
      allowLocalPlannerFallback: false,
    },
  });
  const swarmStartJson = await swarmStart.json();
  assert.equal(swarmStart.status, 200, JSON.stringify(swarmStartJson));
  assert.ok(String(swarmStartJson.runId || "").length > 0, "missing run id");

  const runRes = await request(
    baseUrl,
    `/api/swarm/run/${encodeURIComponent(swarmStartJson.runId)}`,
    { cookie }
  );
  assert.equal(runRes.status, 200);
  const runJson = await runRes.json();
  assert.ok(Array.isArray(runJson.tasks) && runJson.tasks.length > 0, "missing fallback tasks");
  assert.match(
    String(runJson.tasks[0]?.title || ""),
    /neon arcade browser game|execute requested objective/i
  );

  const events = Array.isArray(runJson.events) ? runJson.events : [];
  assert.ok(
    events.some((event) => event?.type === "plan_failed_llm_required"),
    "missing llm planner failure event"
  );
  assert.ok(
    events.some((event) => event?.type === "plan_seeded_local"),
    "missing local recovery plan event"
  );
});
