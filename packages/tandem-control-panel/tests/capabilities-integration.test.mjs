import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { join } from "node:path";
import { tmpdir } from "node:os";
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

async function waitForReady(url, timeoutMs = 15000) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const res = await fetch(`${url}/api/system/health`);
      if (res.ok) return;
    } catch {
      // retry
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`Timed out waiting for ${url}`);
}

async function request(url, path, opts = {}) {
  const { method = "GET", body, cookie, json = true } = opts;
  const u = new URL(path, url);
  const init = {
    method,
    headers: {
      ...(cookie ? { cookie } : {}),
      ...(body != null ? { "content-type": "application/json" } : {}),
    },
    ...(body != null ? { body: JSON.stringify(body) } : {}),
  };
  const res = await fetch(u, init);
  if (!json) return res;
  const text = await res.text();
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch {
    parsed = { raw: text };
  }
  return { status: res.status, ok: res.ok, headers: res.headers, json: () => parsed, text: () => text };
}

function extractCookie(res) {
  const setCookie = res.headers.get("set-cookie") || "";
  return setCookie.split(",")[0].split(";")[0].trim();
}

test("Engine up + ACA absent — /api/capabilities returns coding_workflows=true, aca_integration=false", async (t) => {
  const fakeEnginePort = await getFreePort();
  const engineToken = "test-token";

  const fake = await new Promise((resolve) => {
    const s = createServer(async (req, res) => {
      const url = new URL(req.url || "/", `http://127.0.0.1:${fakeEnginePort}`);
      const auth = req.headers.authorization || "";
      if (url.pathname === "/global/health") {
        if (auth === `Bearer ${engineToken}` || req.headers["x-tandem-token"] === engineToken) {
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify({ ready: true, healthy: true, version: "test-engine" }));
          return;
        }
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ ready: true, healthy: true, version: "test-engine" }));
        return;
      }
      res.writeHead(404);
      res.end();
    });
    s.listen(fakeEnginePort, "127.0.0.1", () => resolve(s));
  });

  t.after(() => fake.close());

  const panelPort = await getFreePort();
  const baseUrl = `http://127.0.0.1:${panelPort}`;

  const panel = spawn(process.execPath, ["bin/setup.js"], {
    cwd: new URL("..", import.meta.url),
    env: {
      ...process.env,
      TANDEM_CONTROL_PANEL_PORT: String(panelPort),
      TANDEM_ENGINE_URL: `http://127.0.0.1:${fakeEnginePort}`,
      TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "0",
      TANDEM_API_TOKEN: engineToken,
      // ACA_BASE_URL intentionally unset — simulates ACA absent
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  let panelOutput = "";
  panel.stdout.on("data", (c) => { panelOutput += c.toString(); });
  panel.stderr.on("data", (c) => { panelOutput += c.toString(); });

  t.after(() => { if (!panel.killed) panel.kill("SIGTERM"); });

  await waitForReady(baseUrl);

  // Verify /api/capabilities
  const caps = await request(baseUrl, "/api/capabilities");
  assert.equal(caps.status, 200, `/api/capabilities failed: ${panelOutput}`);
  const body = caps.json();

  assert.equal(body.aca_integration, false, "ACA should not be detected");
  assert.equal(body.coding_workflows, true, "Coding workflows should be true when engine is up");
  assert.equal(body.engine_healthy, true, "Engine should be healthy");
  assert.equal(body.missions, true, "Missions should be true");
  assert.equal(body.agent_teams, true, "Agent teams should be true");
  assert.equal(body.coder, true, "Coder should be true");
  assert.equal(typeof body.cached_at_ms, "number", "cached_at_ms should be a number");

  // Verify /api/system/health still works
  const health = await request(baseUrl, "/api/system/health");
  assert.equal(health.status, 200);
  const healthBody = health.json();
  assert.equal(healthBody.ok, true);

  const login = await request(baseUrl, "/api/auth/login", {
    method: "POST",
    body: { token: engineToken },
    json: false,
  });
  assert.equal(login.status, 200, `login failed: ${panelOutput}`);
  assert.ok(extractCookie(login).startsWith("tcp_sid="), "session cookie should be set");
});

test("Engine down — /api/capabilities returns all features false, no crash", async (t) => {
  const panelPort = await getFreePort();
  const baseUrl = `http://127.0.0.1:${panelPort}`;

  const panel = spawn(process.execPath, ["bin/setup.js"], {
    cwd: new URL("..", import.meta.url),
    env: {
      ...process.env,
      TANDEM_CONTROL_PANEL_PORT: String(panelPort),
      TANDEM_ENGINE_URL: "http://127.0.0.1:59999",
      TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "0",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  let panelOutput = "";
  panel.stdout.on("data", (c) => { panelOutput += c.toString(); });
  panel.stderr.on("data", (c) => { panelOutput += c.toString(); });

  t.after(() => { if (!panel.killed) panel.kill("SIGTERM"); });

  await waitForReady(baseUrl);

  const caps = await request(baseUrl, "/api/capabilities");
  assert.equal(caps.status, 200, `capabilities failed when engine down: ${panelOutput}`);
  const body = caps.json();

  assert.equal(body.engine_healthy, false, "Engine should be unhealthy");
  assert.equal(body.coding_workflows, false, "Coding workflows should be false when engine is down");
  assert.equal(body.missions, false, "Missions should be false");
  assert.equal(body.agent_teams, false, "Agent teams should be false");
  assert.equal(body.coder, false, "Coder should be false");
  assert.equal(body.aca_integration, false, "ACA should be false");

  // /api/system/health should still return 200 with engine: null
  const health = await request(baseUrl, "/api/system/health");
  assert.equal(health.status, 200);
});

test("ACA absent + engine up — /api/capabilities/metrics has error counts", async (t) => {
  const fakeEnginePort = await getFreePort();
  const engineToken = "test-token";

  const fake = await new Promise((resolve) => {
    const s = createServer(async (req, res) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ready: true, healthy: true, version: "test" }));
    });
    s.listen(fakeEnginePort, "127.0.0.1", () => resolve(s));
  });

  t.after(() => fake.close());

  const panelPort = await getFreePort();
  const baseUrl = `http://127.0.0.1:${panelPort}`;

  const panel = spawn(process.execPath, ["bin/setup.js"], {
    cwd: new URL("..", import.meta.url),
    env: {
      ...process.env,
      TANDEM_CONTROL_PANEL_PORT: String(panelPort),
      TANDEM_ENGINE_URL: `http://127.0.0.1:${fakeEnginePort}`,
      TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "0",
      TANDEM_API_TOKEN: engineToken,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  t.after(() => { if (!panel.killed) panel.kill("SIGTERM"); });

  await waitForReady(baseUrl);

  // Force a capability probe by resetting the cache
  // The metrics endpoint should reflect the first probe result
  const metrics = await request(baseUrl, "/api/capabilities/metrics");
  assert.equal(metrics.status, 200);
  const m = metrics.json();

  assert.equal(typeof m.aca_probe_error_counts, "object");
  assert.equal(m.aca_probe_error_counts.aca_not_configured >= 0, true);
  assert.equal(typeof m.detect_duration_ms, "number");
  assert.equal(typeof m.last_detect_at_ms, "number");

  // /api/system/orchestrator-metrics should also be present
  const orchMetrics = await request(baseUrl, "/api/system/orchestrator-metrics");
  assert.equal(orchMetrics.status, 200);
  const om = orchMetrics.json();
  assert.equal(typeof om.streams_active, "number");
  assert.equal(typeof om.stream_errors, "number");
});

test("Hosted install profile exposes hosted-managed config to the control panel", async (t) => {
  const fakeEnginePort = await getFreePort();
  const engineToken = "test-token";
  const engine = await new Promise((resolve) => {
    const s = createServer(async (req, res) => {
      if (new URL(req.url || "/", "http://127.0.0.1").pathname === "/global/health") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ ready: true, healthy: true, version: "test-engine" }));
        return;
      }
      res.writeHead(404);
      res.end();
    });
    s.listen(fakeEnginePort, "127.0.0.1", () => resolve(s));
  });
  t.after(() => engine.close());

  const tempDir = await mkdtemp(join(tmpdir(), "tcp-hosted-config-"));
  t.after(() => rm(tempDir, { recursive: true, force: true }));

  const configPath = join(tempDir, "control-panel-config.json");
  await writeFile(
    configPath,
    JSON.stringify(
      {
        version: 1,
        control_panel: {
          mode: "auto",
          aca_compact_nav: true,
          public_url: "https://t-12345.hosted.tandem.ac",
        },
        hosted: {
          managed: true,
          provider: "tandem-hosted",
          deployment_id: "deployment-123",
          deployment_slug: "t-12345",
          hostname: "t-12345.hosted.tandem.ac",
          public_url: "https://t-12345.hosted.tandem.ac",
          control_plane_url: "https://tandem.ac",
          release_version: "v0.4.32",
          release_channel: "stable",
          update_policy: "manual",
        },
        repository: {
          path: "/workspace/repo",
          slug: "repo",
          clone_url: "https://example.com/repo.git",
          default_branch: "main",
          worktree_root: "/workspace/repo",
          remote_name: "origin",
        },
        task_source: {
          type: "manual",
          source_name: "hosted",
          prompt: "",
          payload: {},
        },
        provider: {
          id: "openai",
          model: "gpt-4.1-mini",
        },
      },
      null,
      2
    )
  );

  const panelPort = await getFreePort();
  const baseUrl = `http://127.0.0.1:${panelPort}`;
  const panel = spawn(process.execPath, ["bin/setup.js"], {
    cwd: new URL("..", import.meta.url),
    env: {
      ...process.env,
      TANDEM_CONTROL_PANEL_PORT: String(panelPort),
      TANDEM_ENGINE_URL: `http://127.0.0.1:${fakeEnginePort}`,
      TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "0",
      TANDEM_API_TOKEN: engineToken,
      TANDEM_CONTROL_PANEL_CONFIG_FILE: configPath,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let output = "";
  panel.stdout.on("data", (c) => {
    output += c.toString();
  });
  panel.stderr.on("data", (c) => {
    output += c.toString();
  });
  t.after(() => {
    if (!panel.killed) panel.kill("SIGTERM");
  });

  await waitForReady(baseUrl);

  const profile = await request(baseUrl, "/api/install/profile");
  assert.equal(profile.status, 200, `install profile failed: ${output}`);
  const body = profile.json();
  assert.equal(body.hosted_managed, true);
  assert.equal(body.hosted_control_plane_url, "https://tandem.ac");
  assert.equal(body.hosted_public_url, "https://t-12345.hosted.tandem.ac");
  assert.equal(body.hosted_deployment_slug, "t-12345");
  assert.equal(body.hosted_auth_available, false);
  assert.equal(body.control_panel_config_ready, true);

  const caps = await request(baseUrl, "/api/capabilities");
  assert.equal(caps.status, 200, `capabilities failed: ${output}`);
  const capsBody = caps.json();
  assert.equal(capsBody.hosted_managed, true);
  assert.equal(capsBody.hosted_control_plane_url, "https://tandem.ac");
  assert.equal(capsBody.hosted_public_url, "https://t-12345.hosted.tandem.ac");
  assert.equal(capsBody.hosted_auth_available, false);
});

test("Hosted install profile enables search and scheduler settings without a local engine URL", async (t) => {
  const fakeEnginePort = await getFreePort();
  const engineToken = "test-token";
  const engine = await new Promise((resolve) => {
    const s = createServer(async (req, res) => {
      if (new URL(req.url || "/", "http://127.0.0.1").pathname === "/global/health") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ ready: true, healthy: true, version: "test-engine" }));
        return;
      }
      res.writeHead(404);
      res.end();
    });
    s.listen(fakeEnginePort, "127.0.0.1", () => resolve(s));
  });
  t.after(() => engine.close());

  const tempDir = await mkdtemp(join(tmpdir(), "tcp-hosted-search-settings-"));
  t.after(() => rm(tempDir, { recursive: true, force: true }));

  const configPath = join(tempDir, "control-panel-config.json");
  await writeFile(
    configPath,
    JSON.stringify(
      {
        version: 1,
        control_panel: {
          mode: "auto",
          aca_compact_nav: true,
          public_url: "https://t-12345.hosted.tandem.ac",
        },
        hosted: {
          managed: true,
          provider: "tandem-hosted",
          deployment_id: "deployment-123",
          deployment_slug: "t-12345",
          hostname: "t-12345.hosted.tandem.ac",
          public_url: "https://t-12345.hosted.tandem.ac",
          control_plane_url: "https://tandem.ac",
          release_version: "v0.4.32",
          release_channel: "stable",
          update_policy: "manual",
        },
        repository: {
          path: "/workspace/repo",
          slug: "repo",
          clone_url: "https://example.com/repo.git",
          default_branch: "main",
          worktree_root: "/workspace/repo",
          remote_name: "origin",
        },
        task_source: {
          type: "manual",
          source_name: "hosted",
          prompt: "",
          payload: {},
        },
        provider: {
          id: "openai",
          model: "gpt-4.1-mini",
        },
      },
      null,
      2
    )
  );

  const panelPort = await getFreePort();
  const baseUrl = `http://127.0.0.1:${panelPort}`;
  const panel = spawn(process.execPath, ["bin/setup.js"], {
    cwd: new URL("..", import.meta.url),
    env: {
      ...process.env,
      TANDEM_CONTROL_PANEL_PORT: String(panelPort),
      TANDEM_ENGINE_URL: `http://engine.localtest.me:${fakeEnginePort}`,
      TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "0",
      TANDEM_API_TOKEN: engineToken,
      TANDEM_CONTROL_PANEL_CONFIG_FILE: configPath,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let output = "";
  panel.stdout.on("data", (c) => {
    output += c.toString();
  });
  panel.stderr.on("data", (c) => {
    output += c.toString();
  });
  t.after(() => {
    if (!panel.killed) panel.kill("SIGTERM");
  });

  await waitForReady(baseUrl);

  const login = await request(baseUrl, "/api/auth/login", {
    method: "POST",
    body: { token: engineToken },
  });
  assert.equal(login.status, 200, `login failed: ${output}`);
  const cookie = extractCookie(login);

  const searchSettings = await request(baseUrl, "/api/system/search-settings", { cookie });
  assert.equal(searchSettings.status, 200, `search settings failed: ${output}`);
  const searchBody = searchSettings.json();
  assert.equal(searchBody.available, true);
  assert.equal(searchBody.local_engine, false);
  assert.equal(searchBody.writable, true);
  assert.ok(
    ["auto", "brave", "exa", "searxng", "tandem", "none"].includes(searchBody.settings.backend),
    `unexpected backend: ${searchBody.settings.backend}`
  );

  const schedulerSettings = await request(baseUrl, "/api/system/scheduler-settings", { cookie });
  assert.equal(schedulerSettings.status, 200, `scheduler settings failed: ${output}`);
  const schedulerBody = schedulerSettings.json();
  assert.equal(schedulerBody.available, true);
  assert.equal(schedulerBody.local_engine, false);
  assert.equal(schedulerBody.writable, true);
  assert.equal(schedulerBody.settings.mode, "multi");
});
