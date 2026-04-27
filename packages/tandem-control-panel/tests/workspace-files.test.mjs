import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm, stat } from "node:fs/promises";
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
  const { method = "GET", body, cookie, headers = {}, json = true } = opts;
  const res = await fetch(new URL(path, url), {
    method,
    headers: {
      ...(cookie ? { cookie } : {}),
      ...(body != null && json ? { "content-type": "application/json" } : {}),
      ...headers,
    },
    ...(body != null ? { body: json ? JSON.stringify(body) : body } : {}),
  });
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

test("workspace files API supports scoped explorer operations", async (t) => {
  const workspaceRoot = await mkdtemp(join(tmpdir(), "tcp-workspace-files-"));
  t.after(() => rm(workspaceRoot, { recursive: true, force: true }));

  const fakeEnginePort = await getFreePort();
  const fakeEngine = await new Promise((resolve) => {
    const s = createServer((req, res) => {
      const url = new URL(req.url || "/", `http://127.0.0.1:${fakeEnginePort}`);
      if (url.pathname === "/global/health") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ ready: true, healthy: true, apiTokenRequired: false }));
        return;
      }
      res.writeHead(404);
      res.end();
    });
    s.listen(fakeEnginePort, "127.0.0.1", () => resolve(s));
  });
  t.after(() => fakeEngine.close());

  const panelPort = await getFreePort();
  const baseUrl = `http://127.0.0.1:${panelPort}`;
  const panel = spawn(process.execPath, ["bin/setup.js"], {
    cwd: new URL("..", import.meta.url),
    env: {
      ...process.env,
      TANDEM_CONTROL_PANEL_PORT: String(panelPort),
      TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "0",
      TANDEM_ENGINE_URL: `http://127.0.0.1:${fakeEnginePort}`,
      TANDEM_CONTROL_PANEL_WORKSPACE_ROOT: workspaceRoot,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let panelOutput = "";
  panel.stdout.on("data", (c) => { panelOutput += c.toString(); });
  panel.stderr.on("data", (c) => { panelOutput += c.toString(); });
  t.after(() => { if (!panel.killed) panel.kill("SIGTERM"); });

  await waitForReady(baseUrl);
  const login = await request(baseUrl, "/api/auth/login", {
    method: "POST",
    body: { token: "workspace-test" },
    json: true,
  });
  assert.equal(login.status, 200, `login failed: ${panelOutput}`);
  const cookie = extractCookie(login);
  assert.ok(cookie.startsWith("tcp_sid="), "session cookie should be set");

  const initial = await request(baseUrl, "/api/workspace/files/list", { cookie });
  assert.equal(initial.status, 200);
  assert.equal(initial.json().dir, "");

  const mkdir = await request(baseUrl, "/api/workspace/files/mkdir", {
    method: "POST",
    cookie,
    body: { path: "project/src" },
  });
  assert.equal(mkdir.status, 200);
  assert.equal(mkdir.json().path, "project/src");
  assert.equal((await stat(join(workspaceRoot, "project", "src"))).isDirectory(), true);

  const upload = await request(baseUrl, "/api/workspace/files/upload?dir=project", {
    method: "POST",
    cookie,
    headers: {
      "x-file-name": encodeURIComponent("info.txt"),
      "x-relative-path": encodeURIComponent("assets/info.txt"),
      "content-type": "text/plain",
    },
    body: "hello workspace",
    json: false,
  });
  assert.equal(upload.status, 200);
  const uploaded = await upload.json();
  assert.equal(uploaded.path, "project/assets/info.txt");
  assert.equal(await readFile(join(workspaceRoot, "project", "assets", "info.txt"), "utf8"), "hello workspace");

  const read = await request(baseUrl, "/api/workspace/files/read?path=project/assets/info.txt", { cookie });
  assert.equal(read.status, 200);
  assert.equal(read.json().text, "hello workspace");

  const download = await request(baseUrl, "/api/workspace/files/download?path=project/assets/info.txt", {
    cookie,
    json: false,
  });
  assert.equal(download.status, 200);
  assert.equal(await download.text(), "hello workspace");

  const nested = await request(baseUrl, "/api/workspace/files/list?dir=project/assets", { cookie });
  assert.equal(nested.status, 200);
  assert.deepEqual(nested.json().files.map((file) => file.path), ["project/assets/info.txt"]);

  const traversalList = await request(baseUrl, "/api/workspace/files/list?dir=../escape", { cookie });
  assert.equal(traversalList.status, 400);
  const traversalRead = await request(baseUrl, "/api/workspace/files/read?path=/tmp/escape.txt", { cookie });
  assert.equal(traversalRead.status, 400);

  const deleted = await request(baseUrl, "/api/workspace/files/delete", {
    method: "POST",
    cookie,
    body: { path: "project/assets/info.txt" },
  });
  assert.equal(deleted.status, 200);
});
