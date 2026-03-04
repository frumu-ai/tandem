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

async function startFakeEngine() {
  const port = await getFreePort();
  const token = "smoke-token";
  const requests = [];
  const runs = new Map();
  const runEvents = new Map();

  const server = createServer(async (req, res) => {
    const url = new URL(req.url || "/", `http://127.0.0.1:${port}`);
    const auth = req.headers.authorization || "";
    const xToken = req.headers["x-tandem-token"] || "";
    requests.push({ path: url.pathname, auth, xToken });

    if (url.pathname === "/global/health") {
      if (auth === `Bearer ${token}` || xToken === token) {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ ready: true, healthy: true, version: "test-engine", apiTokenRequired: true }));
        return;
      }
      // /global/health is intentionally open in real engine auth-gate.
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ready: true, healthy: true, version: "test-engine", apiTokenRequired: true }));
      return;
    }

    if (url.pathname === "/config/providers") {
      if (auth !== `Bearer ${token}` && xToken !== token) {
        res.writeHead(401, { "content-type": "application/json" });
        res.end(JSON.stringify({ ok: false, error: "unauthorized" }));
        return;
      }
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ default: "openai", providers: {} }));
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
        workspace: input.workspace || { workspace_id: "ws-test", canonical_path: process.cwd(), lease_epoch: 1 },
        steps: [],
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

    if (url.pathname === "/context/runs" && req.method === "GET") {
      const workspace = url.searchParams.get("workspace") || "";
      const rows = [...runs.values()].filter((run) => !workspace || run.workspace?.canonical_path === workspace);
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

    if (url.pathname.match(/^\/context\/runs\/[^/]+\/(blackboard|replay)$/) && req.method === "GET") {
      const kind = url.pathname.split("/").at(-1);
      res.writeHead(200, { "content-type": "application/json" });
      if (kind === "blackboard") {
        res.end(
          JSON.stringify({
            blackboard: { facts: [], decisions: [], open_questions: [], artifacts: [], summaries: { rolling: "" }, revision: 0 },
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
  const fake = await startFakeEngine();
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
      objective: "Test objective",
      maxTasks: 2,
    },
  });
  assert.equal(swarmStart.status, 200);
  const swarmStartJson = await swarmStart.json();
  assert.ok(String(swarmStartJson.runId || "").length > 0, "missing run id");

  const runsRes = await request(baseUrl, `/api/swarm/runs?workspace=${encodeURIComponent(process.cwd())}`, { cookie });
  assert.equal(runsRes.status, 200);
  const runsJson = await runsRes.json();
  assert.ok(Array.isArray(runsJson.runs) && runsJson.runs.length > 0, "missing runs");

  const runRes = await request(baseUrl, `/api/swarm/run/${encodeURIComponent(swarmStartJson.runId)}`, { cookie });
  assert.equal(runRes.status, 200);
  const runJson = await runRes.json();
  assert.equal(runJson.run?.run_id, swarmStartJson.runId);
  assert.ok(Array.isArray(runJson.tasks), "missing tasks array");

  const proxiedAuthSeen = fake.requests.some((r) => r.path === "/global/health" && r.auth === `Bearer ${fake.token}`);
  assert.ok(proxiedAuthSeen, "proxy did not forward token auth header");
});
