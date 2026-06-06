import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import test from "node:test";

function getFreePort() {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close(() => resolve(address.port));
    });
    server.on("error", reject);
  });
}

async function waitForReady(url, timeoutMs = 15000) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    try {
      const res = await fetch(`${url}/api/system/health`);
      if (res.ok) return;
    } catch {
      // retry
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`Timed out waiting for ${url}`);
}

async function request(url, path, opts = {}) {
  const { method = "GET", body, cookie, headers = {} } = opts;
  const target = new URL(path, url);
  const res = await fetch(target, {
    method,
    headers: {
      ...(cookie ? { cookie } : {}),
      ...headers,
      ...(body != null ? { "content-type": "application/json" } : {}),
    },
    ...(body != null ? { body: JSON.stringify(body) } : {}),
  });
  return res;
}

function extractCookie(res) {
  const setCookie = res.headers.get("set-cookie") || "";
  return setCookie.split(",")[0].split(";")[0].trim();
}

test("control panel engine proxy strips browser agent headers", async (t) => {
  const enginePort = await getFreePort();
  const panelPort = await getFreePort();
  const engineToken = "engine-token";
  const seenRequests = [];

  const fakeEngine = await new Promise((resolve) => {
    const server = createServer((req, res) => {
      const url = new URL(req.url || "/", `http://127.0.0.1:${enginePort}`);
      seenRequests.push({
        path: url.pathname,
        agentId: String(req.headers["x-tandem-agent-id"] || ""),
        requestSource: String(req.headers["x-tandem-request-source"] || ""),
        auth: String(req.headers.authorization || ""),
        xToken: String(req.headers["x-tandem-token"] || ""),
      });

      if (url.pathname === "/global/health") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ ready: true, healthy: true, version: "fake-engine" }));
        return;
      }

      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: true, path: url.pathname }));
    });
    server.listen(enginePort, "127.0.0.1", () => resolve(server));
  });
  t.after(() => fakeEngine.close());

  const baseUrl = `http://127.0.0.1:${panelPort}`;
  const panel = spawn(process.execPath, ["bin/setup.js"], {
    cwd: new URL("..", import.meta.url),
    env: {
      ...process.env,
      TANDEM_CONTROL_PANEL_PORT: String(panelPort),
      TANDEM_ENGINE_URL: `http://127.0.0.1:${enginePort}`,
      TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "0",
      TANDEM_API_TOKEN: engineToken,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  t.after(() => {
    if (!panel.killed) panel.kill("SIGTERM");
  });

  await waitForReady(baseUrl);

  const login = await request(baseUrl, "/api/auth/login", {
    method: "POST",
    body: { token: engineToken },
  });
  assert.equal(login.status, 200);
  const cookie = extractCookie(login);

  const response = await request(baseUrl, "/api/engine/global/health", {
    cookie,
    headers: {
      "x-tandem-agent-id": "agent-should-not-forward",
    },
  });
  assert.equal(response.status, 200);

  const forwarded = seenRequests.at(-1);
  assert.equal(forwarded?.path, "/global/health");
  assert.equal(forwarded?.auth, `Bearer ${engineToken}`);
  assert.equal(forwarded?.xToken, engineToken);
  assert.equal(forwarded?.agentId, "");
  assert.equal(forwarded?.requestSource, "control_panel");
});

test("control panel engine proxy forwards public origin for MCP OAuth", async (t) => {
  const enginePort = await getFreePort();
  const panelPort = await getFreePort();
  const engineToken = "engine-token";
  const seenRequests = [];

  const fakeEngine = await new Promise((resolve) => {
    const server = createServer((req, res) => {
      const url = new URL(req.url || "/", `http://127.0.0.1:${enginePort}`);
      seenRequests.push({
        path: url.pathname,
        forwardedHost: String(req.headers["x-forwarded-host"] || ""),
        forwardedProto: String(req.headers["x-forwarded-proto"] || ""),
        origin: String(req.headers.origin || ""),
        referer: String(req.headers.referer || ""),
      });

      if (url.pathname === "/global/health") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ ready: true, healthy: true, version: "fake-engine" }));
        return;
      }

      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: false, pendingAuth: true }));
    });
    server.listen(enginePort, "127.0.0.1", () => resolve(server));
  });
  t.after(() => fakeEngine.close());

  const baseUrl = `http://127.0.0.1:${panelPort}`;
  const panel = spawn(process.execPath, ["bin/setup.js"], {
    cwd: new URL("..", import.meta.url),
    env: {
      ...process.env,
      TANDEM_CONTROL_PANEL_PORT: String(panelPort),
      TANDEM_CONTROL_PANEL_PUBLIC_URL: "https://testing.tandem.ac",
      TANDEM_ENGINE_URL: `http://127.0.0.1:${enginePort}`,
      TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "0",
      TANDEM_API_TOKEN: engineToken,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  t.after(() => {
    if (!panel.killed) panel.kill("SIGTERM");
  });

  await waitForReady(baseUrl);

  const login = await request(baseUrl, "/api/auth/login", {
    method: "POST",
    body: { token: engineToken },
  });
  assert.equal(login.status, 200);
  const cookie = extractCookie(login);

  const response = await request(baseUrl, "/api/engine/mcp/linear/auth/authenticate", {
    method: "POST",
    cookie,
  });
  assert.equal(response.status, 200);

  const forwarded = seenRequests.find((row) => row.path === "/mcp/linear/auth/authenticate");
  assert.equal(forwarded?.forwardedHost, "testing.tandem.ac");
  assert.equal(forwarded?.forwardedProto, "https");
  assert.equal(forwarded?.origin, "https://testing.tandem.ac");
  assert.equal(forwarded?.referer, "https://testing.tandem.ac/");
});
