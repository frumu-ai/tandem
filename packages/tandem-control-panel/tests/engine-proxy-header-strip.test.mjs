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
        forwardedPrefix: String(req.headers["x-forwarded-prefix"] || ""),
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
  assert.equal(forwarded?.forwardedPrefix, "/api/engine");
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

test("control panel proxies OAuth callbacks without a panel session", async (t) => {
  const enginePort = await getFreePort();
  const panelPort = await getFreePort();
  const engineToken = "engine-token";
  const seenRequests = [];

  const fakeEngine = await new Promise((resolve) => {
    const server = createServer((req, res) => {
      const url = new URL(req.url || "/", `http://127.0.0.1:${enginePort}`);
      seenRequests.push({
        path: url.pathname,
        search: url.search,
        auth: String(req.headers.authorization || ""),
        xToken: String(req.headers["x-tandem-token"] || ""),
        cookie: String(req.headers.cookie || ""),
      });

      if (url.pathname === "/global/health") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ ready: true, healthy: true, version: "fake-engine" }));
        return;
      }
      if (url.pathname === "/mcp/linear/auth/callback") {
        res.writeHead(200, { "content-type": "text/html" });
        res.end("<html><body>connected</body></html>");
        return;
      }

      res.writeHead(404, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: false, error: "not found" }));
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

  const response = await request(
    baseUrl,
    "/api/engine/mcp/linear/auth/callback?code=test-code&state=test-state",
    {
      headers: {
        cookie: "tcp_sid=browser-session-should-not-forward",
      },
    }
  );
  assert.equal(response.status, 200);
  assert.match(await response.text(), /connected/);

  const forwarded = seenRequests.find((row) => row.path === "/mcp/linear/auth/callback");
  assert.equal(forwarded?.search, "?code=test-code&state=test-state");
  assert.equal(forwarded?.auth, `Bearer ${engineToken}`);
  assert.equal(forwarded?.xToken, engineToken);
  assert.equal(forwarded?.cookie, "");
});

test("control panel proxies automation webhooks without a panel session", async (t) => {
  const enginePort = await getFreePort();
  const panelPort = await getFreePort();
  const engineToken = "engine-token";
  const seenRequests = [];

  const fakeEngine = await new Promise((resolve) => {
    const server = createServer(async (req, res) => {
      const url = new URL(req.url || "/", `http://127.0.0.1:${enginePort}`);
      let body = "";
      for await (const chunk of req) {
        body += chunk.toString("utf8");
      }
      seenRequests.push({
        path: url.pathname,
        search: url.search,
        method: req.method,
        auth: String(req.headers.authorization || ""),
        xToken: String(req.headers["x-tandem-token"] || ""),
        cookie: String(req.headers.cookie || ""),
        signature: String(req.headers["x-tandem-webhook-signature"] || ""),
        eventId: String(req.headers["x-tandem-webhook-event-id"] || ""),
        forwardedHost: String(req.headers["x-forwarded-host"] || ""),
        forwardedProto: String(req.headers["x-forwarded-proto"] || ""),
        forwardedPrefix: String(req.headers["x-forwarded-prefix"] || ""),
        origin: String(req.headers.origin || ""),
        requestedMethod: String(req.headers["access-control-request-method"] || ""),
        requestedHeaders: String(req.headers["access-control-request-headers"] || ""),
        body,
      });

      if (url.pathname === "/global/health") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ ready: true, healthy: true, version: "fake-engine" }));
        return;
      }
      if (url.pathname === "/webhooks/automations/whpub_test") {
        if (req.method === "OPTIONS") {
          res.writeHead(204, {
            "access-control-allow-origin": String(req.headers.origin || ""),
            "access-control-allow-methods": "POST, OPTIONS",
            "access-control-allow-headers": String(
              req.headers["access-control-request-headers"] || ""
            ),
          });
          res.end();
          return;
        }
        res.writeHead(202, { "content-type": "application/json" });
        res.end(JSON.stringify({ ok: true, status: "accepted" }));
        return;
      }

      res.writeHead(404, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: false, error: "not found" }));
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

  const response = await request(
    baseUrl,
    "/api/engine/webhooks/automations/whpub_test?source=provider",
    {
      method: "POST",
      cookie: "tcp_sid=browser-session-should-not-forward",
      headers: {
        authorization: "Bearer browser-token-should-not-forward",
        "x-tandem-token": "browser-token-should-not-forward",
        origin: "https://client.example",
        "x-forwarded-prefix": "/caller-controlled",
        "x-tandem-webhook-signature": "t=123,v1=abc",
        "x-tandem-webhook-event-id": "evt-public-proxy",
      },
      body: { ok: true },
    }
  );
  assert.equal(response.status, 202);

  const forwarded = seenRequests.find((row) => row.path === "/webhooks/automations/whpub_test");
  assert.equal(forwarded?.method, "POST");
  assert.equal(forwarded?.search, "?source=provider");
  assert.equal(forwarded?.auth, "");
  assert.equal(forwarded?.xToken, "");
  assert.equal(forwarded?.cookie, "");
  assert.equal(forwarded?.signature, "t=123,v1=abc");
  assert.equal(forwarded?.eventId, "evt-public-proxy");
  assert.equal(forwarded?.forwardedHost, "testing.tandem.ac");
  assert.equal(forwarded?.forwardedProto, "https");
  assert.equal(forwarded?.forwardedPrefix, "/api/engine");
  assert.equal(forwarded?.origin, "https://client.example");
  assert.equal(forwarded?.body, JSON.stringify({ ok: true }));

  const preflight = await request(
    baseUrl,
    "/api/engine/webhooks/automations/whpub_test",
    {
      method: "OPTIONS",
      headers: {
        origin: "https://client.example",
        "access-control-request-method": "POST",
        "access-control-request-headers":
          "content-type,x-tandem-webhook-secret,x-tandem-webhook-event-id",
      },
    }
  );
  assert.equal(preflight.status, 204);
  assert.equal(preflight.headers.get("access-control-allow-origin"), "https://client.example");

  const forwardedPreflight = seenRequests.find(
    (row) => row.path === "/webhooks/automations/whpub_test" && row.method === "OPTIONS"
  );
  assert.equal(forwardedPreflight?.auth, "");
  assert.equal(forwardedPreflight?.xToken, "");
  assert.equal(forwardedPreflight?.cookie, "");
  assert.equal(forwardedPreflight?.forwardedPrefix, "/api/engine");
  assert.equal(forwardedPreflight?.origin, "https://client.example");
  assert.equal(forwardedPreflight?.requestedMethod, "POST");
  assert.equal(
    forwardedPreflight?.requestedHeaders,
    "content-type,x-tandem-webhook-secret,x-tandem-webhook-event-id"
  );
  assert.equal(forwardedPreflight?.body, "");
});
