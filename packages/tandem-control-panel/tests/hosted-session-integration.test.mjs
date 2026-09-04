import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

async function listen(t, handler) {
  const server = createServer(handler);
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => { server.closeAllConnections(); server.close(); });
  return `http://127.0.0.1:${server.address().port}`;
}

function send(res, status, body) {
  res.writeHead(status, { "content-type": "application/json" });
  res.end(JSON.stringify(body));
}

async function setup(t, { refresh = "valid", invalidExchange = false } = {}) {
  const root = await mkdtemp(join(tmpdir(), "tandem-hosted-identity-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const requests = [];
  const refreshCalls = [];
  const engine = await listen(t, (req, res) => {
    requests.push({ path: req.url, headers: req.headers });
    send(res, 200, { ready: true, healthy: true, version: "identity-fixture" });
  });
  const envelope = (user, { fresh = false } = {}) => ({
    deployment_id: "deployment-a", user: { id: user }, role: "member",
    roles: ["workspace:user"], org_units: [fresh ? "finance" : "engineering"],
    policy_version: fresh ? 2 : 1,
    panel_session_token: `server-session-${user}`,
    context_assertion: `server-assertion-${user}${fresh ? "-refreshed" : ""}`,
    context_assertion_expires_at: new Date(Date.now() + (fresh || user === "bob" ? 300_000 : 30_000)).toISOString(),
    session_expires_at: new Date(Date.now() + 3_600_000).toISOString(),
  });
  const controlPlane = await listen(t, async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const body = JSON.parse(Buffer.concat(chunks).toString() || "{}");
    if (req.headers.authorization !== "Bearer host-fixture-token") return send(res, 401, {});
    if (req.url === "/exchange") {
      const payload = envelope(body.code);
      if (invalidExchange) delete payload.user;
      return send(res, 200, payload);
    }
    if (req.url === "/refresh") {
      refreshCalls.push(body.panel_session_token);
      if (refresh === "revoked") return send(res, 403, { error: "fixture revocation" });
      const user = refresh === "changed-user" ? "mallory" : "alice";
      return send(res, 200, envelope(user, { fresh: true }));
    }
    send(res, 404, {});
  });
  const tokenPath = join(root, "host-token");
  await writeFile(tokenPath, "host-fixture-token", { mode: 0o600 });
  const configPath = join(root, "panel.json");
  await writeFile(configPath, JSON.stringify({
    version: 1,
    hosted: {
      managed: true, deployment_id: "deployment-a", public_url: "https://private.example.test",
      control_plane_url: controlPlane,
      auth: { mode: "hosted", panel_exchange_url: `${controlPlane}/exchange`, panel_refresh_url: `${controlPlane}/refresh`, host_agent_token_file: tokenPath },
    },
  }));
  // Reserve and release a port without fixed-port cross-test collisions.
  const reservation = createServer();
  await new Promise((resolve) => reservation.listen(0, "127.0.0.1", resolve));
  const port = reservation.address().port;
  await new Promise((resolve) => reservation.close(resolve));
  const url = `http://127.0.0.1:${port}`;
  const panel = spawn(process.execPath, ["bin/setup.js"], {
    cwd: new URL("..", import.meta.url),
    env: { ...process.env, TANDEM_CONTROL_PANEL_PORT: String(port), TANDEM_ENGINE_URL: engine,
      TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "0", TANDEM_API_TOKEN: "test-token",
      TANDEM_CONTROL_PANEL_CONFIG_FILE: configPath, TANDEM_CONTROL_PANEL_STATE_DIR: root },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let output = "";
  panel.stdout.on("data", (chunk) => { output += chunk; });
  panel.stderr.on("data", (chunk) => { output += chunk; });
  t.after(async () => {
    if (panel.exitCode !== null) return;
    const exited = new Promise((resolve) => panel.once("exit", resolve));
    panel.kill("SIGTERM");
    await exited;
  });
  let ready = false;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try { if ((await fetch(`${url}/api/system/health`)).ok) { ready = true; break; } } catch {}
    if (panel.exitCode !== null) break;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  assert.equal(ready, true, `panel did not start: ${output}`);
  async function login(user) {
    const response = await fetch(`${url}/api/auth/hosted/exchange`, {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ code: user, user: { id: "browser-forged-user" }, role: "owner" }),
    });
    const setCookie = response.headers.get("set-cookie") || "";
    const body = await response.json();
    return { status: response.status, cookie: setCookie.split(";")[0], setCookie, body };
  }
  return { url, login, requests, refreshCalls };
}

test("hosted requests use only the server identity and refreshed memberships", async (t) => {
  const app = await setup(t);
  const alice = await app.login("alice");
  assert.equal(alice.status, 200);
  assert.match(alice.setCookie, /; Secure/);
  assert.equal(alice.body.user.id, "alice");
  assert.equal(alice.body.role, "member");
  assert.equal(JSON.stringify(alice.body).includes("server-session"), false);
  const [first, second] = await Promise.all([
    fetch(`${app.url}/api/auth/me`, { headers: { cookie: alice.cookie } }),
    fetch(`${app.url}/api/engine/global/health`, { headers: {
      cookie: alice.cookie, "x-tandem-context-assertion": "browser-forged",
      "x-tandem-context-jws": "browser-forged-alias", "x-tandem-tenant-context-jws": "forged",
      "x-tandem-actor-id": "mallory", "x-user-id": "mallory",
      "x-tandem-agent-id": "forged-agent", "x-tandem-agent-test-mode": "true",
      "x-tandem-request-source": "agent",
    } }),
  ]);
  assert.equal(first.status, 200);
  assert.equal(second.status, 200);
  const me = await first.json();
  assert.equal(me.principal_id, "alice");
  assert.deepEqual(me.org_units, ["finance"]);
  assert.equal(me.policy_version, 2);
  assert.equal(app.refreshCalls.length, 1);
  const forwarded = app.requests.find((request) => request.headers["x-tandem-context-assertion"]);
  assert.equal(forwarded.headers["x-tandem-context-assertion"], "server-assertion-alice-refreshed");
  assert.equal(forwarded.headers["x-tandem-context-jws"], undefined);
  assert.equal(forwarded.headers["x-tandem-tenant-context-jws"], undefined);
  assert.equal(forwarded.headers["x-tandem-actor-id"], undefined);
  assert.equal(forwarded.headers["x-user-id"], undefined);
  assert.equal(forwarded.headers["x-tandem-agent-id"], undefined);
  assert.equal(forwarded.headers["x-tandem-request-source"], "control_panel");
  assert.equal(forwarded.headers.authorization, "Bearer test-token");
});

test("failed refresh removes only the affected user's panel session", async (t) => {
  const app = await setup(t, { refresh: "revoked" });
  const alice = await app.login("alice");
  const bob = await app.login("bob");
  assert.equal(alice.status, 200);
  assert.equal(bob.status, 200);
  const denied = await fetch(`${app.url}/api/auth/me`, { headers: { cookie: alice.cookie } });
  assert.equal(denied.status, 401);
  assert.match(denied.headers.get("set-cookie"), /Max-Age=0/);
  const again = await fetch(`${app.url}/api/engine/global/health`, { headers: { cookie: alice.cookie } });
  assert.equal(again.status, 401);
  const allowed = await fetch(`${app.url}/api/auth/me`, { headers: { cookie: bob.cookie } });
  assert.equal(allowed.status, 200);
  assert.equal((await allowed.json()).principal_id, "bob");
  assert.equal(app.refreshCalls.length, 1);
});

test("hosted members cannot change deployment auth or reach shared administrative handlers", async (t) => {
  const app = await setup(t);
  const bob = await app.login("bob");
  for (const [path, method] of [
    ["/api/control-panel/config", "GET"], ["/api/control-panel/config", "PATCH"],
    ["/api/system/scheduler-settings", "PATCH"], ["/api/system/search-settings/test", "POST"],
    ["/api/workspace/files", "GET"], ["/api/files", "GET"], ["/api/aca/run", "POST"],
    ["/api/swarm/start", "POST"], ["/api/knowledgebase/admin/collections", "GET"],
  ]) {
    const response = await fetch(`${app.url}${path}`, { method, headers: { cookie: bob.cookie } });
    assert.equal(response.status, 403, `${method} ${path}`);
  }
  const valid = await fetch(`${app.url}/api/auth/me`, { headers: { cookie: bob.cookie } });
  assert.equal(valid.status, 200);
});

test("refresh cannot replace a signed-in user with another user", async (t) => {
  const app = await setup(t, { refresh: "changed-user" });
  const alice = await app.login("alice");
  const response = await fetch(`${app.url}/api/engine/global/health`, { headers: { cookie: alice.cookie } });
  assert.equal(response.status, 401);
  assert.equal(app.requests.some((request) => request.headers["x-tandem-context-assertion"]), false);
});

test("incomplete control-plane exchange cannot create a token-backed fallback session", async (t) => {
  const app = await setup(t, { invalidExchange: true });
  const response = await app.login("alice");
  assert.equal(response.status, 401);
  assert.equal(response.cookie, "");
  const me = await fetch(`${app.url}/api/auth/me`);
  assert.equal(me.status, 401);
});
