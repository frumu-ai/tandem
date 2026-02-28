#!/usr/bin/env node

import { spawn } from "child_process";
import { createServer } from "http";
import { readFileSync, existsSync, createReadStream, createWriteStream } from "fs";
import { mkdir, readdir, stat, rm } from "fs/promises";
import { randomBytes } from "crypto";
import { join, dirname, extname, normalize, resolve, basename } from "path";
import { Transform } from "stream";
import { pipeline } from "stream/promises";
import { fileURLToPath } from "url";
import { createRequire } from "module";
import { homedir } from "os";
import { ensureEnv } from "./init-env.js";

function parseDotEnv(content) {
  const out = {};
  for (const raw of String(content || "").split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const idx = line.indexOf("=");
    if (idx <= 0) continue;
    const key = line.slice(0, idx).trim();
    let value = line.slice(idx + 1).trim();
    if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
      value = value.slice(1, -1);
    }
    out[key] = value;
  }
  return out;
}

function loadDotEnvFile(pathname) {
  if (!existsSync(pathname)) return false;
  const parsed = parseDotEnv(readFileSync(pathname, "utf8"));
  for (const [key, value] of Object.entries(parsed)) {
    if (process.env[key] === undefined) process.env[key] = value;
  }
  return true;
}

const cliArgs = new Set(process.argv.slice(2));
const initRequested = cliArgs.has("--init") || cliArgs.has("init");
const resetTokenRequested = cliArgs.has("--reset-token");
const cwdEnvPath = resolve(process.cwd(), ".env");

if (initRequested) {
  const result = ensureEnv({ overwrite: resetTokenRequested });
  console.log("[Tandem Control Panel] Environment initialized.");
  console.log(`[Tandem Control Panel] .env:      ${result.envPath}`);
  console.log(`[Tandem Control Panel] Engine URL: ${result.engineUrl}`);
  console.log(`[Tandem Control Panel] Panel URL:  http://localhost:${result.panelPort}`);
  console.log(`[Tandem Control Panel] Token:      ${result.token}`);
  if (cliArgs.size === 1 || (cliArgs.size === 2 && resetTokenRequested)) process.exit(0);
}

loadDotEnvFile(cwdEnvPath);

const __dirname = dirname(fileURLToPath(import.meta.url));
const DIST_DIR = join(__dirname, "..", "dist");
const REPO_ROOT = resolve(__dirname, "..", "..", "..");

function resolveDefaultChannelUploadsRoot() {
  const explicitStateDir = String(process.env.TANDEM_STATE_DIR || "").trim();
  if (explicitStateDir) return resolve(explicitStateDir, "channel_uploads");

  const xdgDataHome = String(process.env.XDG_DATA_HOME || "").trim();
  if (xdgDataHome) return resolve(xdgDataHome, "tandem", "data", "channel_uploads");

  const appData = String(process.env.APPDATA || "").trim();
  if (appData) return resolve(appData, "tandem", "data", "channel_uploads");

  return resolve(homedir(), ".tandem", "data", "channel_uploads");
}

const PORTAL_PORT = Number.parseInt(process.env.TANDEM_CONTROL_PANEL_PORT || "39732", 10);
const ENGINE_HOST = (process.env.TANDEM_ENGINE_HOST || "127.0.0.1").trim();
const ENGINE_PORT = Number.parseInt(process.env.TANDEM_ENGINE_PORT || "39731", 10);
const ENGINE_URL = (process.env.TANDEM_ENGINE_URL || `http://${ENGINE_HOST}:${ENGINE_PORT}`).replace(/\/+$/, "");
const AUTO_START_ENGINE = (process.env.TANDEM_CONTROL_PANEL_AUTO_START_ENGINE || "1") !== "0";
const CONFIGURED_ENGINE_TOKEN = (
  process.env.TANDEM_CONTROL_PANEL_ENGINE_TOKEN ||
  process.env.TANDEM_API_TOKEN ||
  ""
).trim();
const SESSION_TTL_MS =
  Number.parseInt(process.env.TANDEM_CONTROL_PANEL_SESSION_TTL_MINUTES || "1440", 10) * 60 * 1000;
const FILES_ROOT = resolve(process.env.TANDEM_CONTROL_PANEL_FILES_ROOT || resolveDefaultChannelUploadsRoot());
const FILES_SCOPE = String(process.env.TANDEM_CONTROL_PANEL_FILES_SCOPE || "control-panel")
  .trim()
  .replace(/\\/g, "/")
  .replace(/^\/+/, "")
  .replace(/\/+$/, "");
const MAX_UPLOAD_BYTES = Math.max(
  1,
  Number.parseInt(process.env.TANDEM_CONTROL_PANEL_MAX_UPLOAD_BYTES || `${250 * 1024 * 1024}`, 10) ||
    250 * 1024 * 1024
);
const require = createRequire(import.meta.url);

const log = (msg) => console.log(`[Tandem Control Panel] ${msg}`);
const err = (msg) => console.error(`[Tandem Control Panel] ERROR: ${msg}`);

if (!Number.isFinite(PORTAL_PORT) || PORTAL_PORT <= 0) {
  err(`Invalid TANDEM_CONTROL_PANEL_PORT: ${process.env.TANDEM_CONTROL_PANEL_PORT || ""}`);
  process.exit(1);
}
if (!Number.isFinite(ENGINE_PORT) || ENGINE_PORT <= 0) {
  err(`Invalid TANDEM_ENGINE_PORT: ${process.env.TANDEM_ENGINE_PORT || ""}`);
  process.exit(1);
}

const MIME_TYPES = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".css": "text/css",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".json": "application/json",
  ".ico": "image/x-icon",
  ".txt": "text/plain",
};

const sessions = new Map();
let engineProcess = null;
let server = null;
let managedEngineToken = "";

const swarmState = {
  status: "idle",
  process: null,
  logs: [],
  reasons: [],
  monitorTimer: null,
  registryCache: null,
  startedAt: null,
  stoppedAt: null,
  objective: "",
  workspaceRoot: REPO_ROOT,
  maxTasks: 3,
  lastError: "",
};
const swarmSseClients = new Set();

const sleep = (ms) => new Promise((resolveFn) => setTimeout(resolveFn, ms));

function isLocalEngineUrl(url) {
  try {
    const u = new URL(url);
    const h = (u.hostname || "").toLowerCase();
    return h === "localhost" || h === "127.0.0.1" || h === "::1";
  } catch {
    return false;
  }
}

function pruneExpiredSessions() {
  const now = Date.now();
  for (const [sid, rec] of sessions.entries()) {
    if (now - rec.lastSeenAt > SESSION_TTL_MS) sessions.delete(sid);
  }
}

function parseCookies(req) {
  const header = req.headers.cookie || "";
  const out = {};
  for (const part of header.split(";")) {
    const trimmed = part.trim();
    if (!trimmed) continue;
    const idx = trimmed.indexOf("=");
    if (idx <= 0) continue;
    out[trimmed.slice(0, idx)] = decodeURIComponent(trimmed.slice(idx + 1));
  }
  return out;
}

function getSession(req) {
  pruneExpiredSessions();
  const sid = parseCookies(req).tcp_sid;
  if (!sid) return null;
  const rec = sessions.get(sid);
  if (!rec) return null;
  rec.lastSeenAt = Date.now();
  return { sid, ...rec };
}

function setSessionCookie(res, sid) {
  const attrs = [
    `tcp_sid=${encodeURIComponent(sid)}`,
    "HttpOnly",
    "SameSite=Lax",
    "Path=/",
    `Max-Age=${Math.floor(SESSION_TTL_MS / 1000)}`,
  ];
  res.setHeader("Set-Cookie", attrs.join("; "));
}

function clearSessionCookie(res) {
  res.setHeader("Set-Cookie", "tcp_sid=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0");
}

async function readJsonBody(req) {
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  if (chunks.length === 0) return {};
  const raw = Buffer.concat(chunks).toString("utf8").trim();
  if (!raw) return {};
  return JSON.parse(raw);
}

function sendJson(res, code, payload) {
  const body = JSON.stringify(payload);
  res.writeHead(code, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(body),
  });
  res.end(body);
}

function pushSwarmEvent(kind, payload = {}) {
  const event = {
    kind,
    ts: Date.now(),
    ...payload,
  };
  const line = `data: ${JSON.stringify(event)}\n\n`;
  for (const client of [...swarmSseClients]) {
    try {
      client.write(line);
    } catch {
      swarmSseClients.delete(client);
    }
  }
}

function appendSwarmLog(stream, text) {
  const lines = String(text || "").split(/\r?\n/).filter(Boolean);
  for (const line of lines) {
    swarmState.logs.push({ at: Date.now(), stream, line });
    if (swarmState.logs.length > 800) swarmState.logs.shift();
    pushSwarmEvent("log", { stream, line });
  }
}

function appendSwarmReason(reason) {
  const item = { at: Date.now(), ...reason };
  swarmState.reasons.push(item);
  if (swarmState.reasons.length > 500) swarmState.reasons.shift();
  pushSwarmEvent("reason", item);
}

function compareRegistryTransitions(previousRegistry, nextRegistry) {
  const prevTasks = previousRegistry?.tasks || {};
  const nextTasks = nextRegistry?.tasks || {};
  const transitions = [];

  for (const [taskId, next] of Object.entries(nextTasks)) {
    const prev = prevTasks[taskId];
    if (!prev) {
      transitions.push({
        kind: "task_transition",
        taskId,
        from: "new",
        to: next.status || "unknown",
        role: next.ownerRole || "",
        reason: next.statusReason || "task registered",
      });
      continue;
    }
    if ((prev.status || "") !== (next.status || "")) {
      transitions.push({
        kind: "task_transition",
        taskId,
        from: prev.status || "unknown",
        to: next.status || "unknown",
        role: next.ownerRole || "",
        reason: next.statusReason || `${prev.status || "unknown"} -> ${next.status || "unknown"}`,
      });
    } else if ((prev.statusReason || "") !== (next.statusReason || "") && next.statusReason) {
      transitions.push({
        kind: "task_reason",
        taskId,
        from: next.status || "unknown",
        to: next.status || "unknown",
        role: next.ownerRole || "",
        reason: next.statusReason,
      });
    }
  }

  return transitions;
}

async function monitorSwarmRegistry(token) {
  try {
    const latest = await readSwarmRegistry(token);
    const latestValue = latest?.value || { tasks: {} };
    const previous = swarmState.registryCache?.value || { tasks: {} };
    const transitions = compareRegistryTransitions(previous, latestValue);
    if (transitions.length > 0) {
      for (const t of transitions) appendSwarmReason(t);
      pushSwarmEvent("registry_update", { count: transitions.length });
    }
    swarmState.registryCache = latest;
  } catch (e) {
    appendSwarmLog("stderr", `[swarm-monitor] ${e instanceof Error ? e.message : String(e)}`);
  }
}

function clearSwarmMonitor() {
  if (swarmState.monitorTimer) {
    clearInterval(swarmState.monitorTimer);
    swarmState.monitorTimer = null;
  }
}

async function engineHealth(token = "") {
  try {
    const response = await fetch(`${ENGINE_URL}/global/health`, {
      headers: token
        ? {
            authorization: `Bearer ${token}`,
            "x-tandem-token": token,
          }
        : {},
      signal: AbortSignal.timeout(1800),
    });
    if (!response.ok) return null;
    return await response.json();
  } catch {
    return null;
  }
}

async function validateEngineToken(token) {
  try {
    const response = await fetch(`${ENGINE_URL}/config/providers`, {
      headers: {
        authorization: `Bearer ${token}`,
        "x-tandem-token": token,
      },
      signal: AbortSignal.timeout(1800),
    });
    return response.ok;
  } catch {
    return false;
  }
}

async function ensureEngineRunning() {
  if (!AUTO_START_ENGINE || !isLocalEngineUrl(ENGINE_URL)) return;

  const healthy = await engineHealth();
  if (healthy?.ready || healthy?.healthy) {
    log(`Detected existing Tandem Engine at ${ENGINE_URL} (v${healthy.version || "unknown"}).`);
    if (CONFIGURED_ENGINE_TOKEN) {
      log(
        "Note: TANDEM_CONTROL_PANEL_ENGINE_TOKEN is only applied when control panel starts a new engine process."
      );
      log("Use the existing engine's token, or stop that engine to let control panel start one with your configured token.");
    }
    return;
  }

  let engineEntrypoint;
  try {
    engineEntrypoint = require.resolve("@frumu/tandem/bin/tandem-engine.js");
  } catch (e) {
    err("Could not resolve @frumu/tandem binary entrypoint.");
    err("Reinstall with: npm i -g @frumu/tandem-control-panel");
    throw e;
  }

  const url = new URL(ENGINE_URL);
  managedEngineToken = CONFIGURED_ENGINE_TOKEN || `tk_${randomBytes(16).toString("hex")}`;

  log(`Starting Tandem Engine at ${ENGINE_URL}...`);
  engineProcess = spawn(
    process.execPath,
    [engineEntrypoint, "serve", "--hostname", url.hostname, "--port", String(url.port || ENGINE_PORT)],
    {
      env: {
        ...process.env,
        TANDEM_API_TOKEN: managedEngineToken,
      },
      stdio: "inherit",
    }
  );
  log(`Engine API token for this process: ${managedEngineToken}`);
  if (!CONFIGURED_ENGINE_TOKEN) {
    log("Token was auto-generated. Set TANDEM_CONTROL_PANEL_ENGINE_TOKEN (or TANDEM_API_TOKEN) to keep it stable.");
  }

  engineProcess.on("error", (e) => err(`Failed to start engine: ${e.message}`));

  for (let i = 0; i < 30; i += 1) {
    const probe = await engineHealth();
    if (probe?.ready || probe?.healthy) {
      log(`Engine ready (v${probe.version || "unknown"}).`);
      return;
    }
    await sleep(300);
  }

  err("Engine did not become healthy in time.");
}

function sanitizeStaticPath(rawUrl) {
  const url = new URL(rawUrl || "/", `http://127.0.0.1:${PORTAL_PORT}`);
  const decoded = decodeURIComponent(url.pathname || "/");
  const relative = decoded === "/" ? "index.html" : decoded.replace(/^\/+/, "");
  const full = normalize(join(DIST_DIR, relative));
  if (!full.startsWith(DIST_DIR + "/") && full !== DIST_DIR) return null;
  return full;
}

function toSafeRelPath(raw) {
  const normalized = String(raw || "")
    .trim()
    .replace(/\\/g, "/")
    .replace(/^\/+/, "");
  if (!normalized) return "";
  if (normalized.includes("\0")) return null;
  const full = resolve(FILES_ROOT, normalized);
  if (full !== FILES_ROOT && !full.startsWith(`${FILES_ROOT}/`)) return null;
  if (FILES_SCOPE && normalized !== FILES_SCOPE && !normalized.startsWith(`${FILES_SCOPE}/`)) return null;
  return normalized;
}

function toSafeRelFileName(rawName) {
  const cleaned = basename(String(rawName || "").trim()).replace(/[\0]/g, "");
  if (!cleaned || cleaned === "." || cleaned === "..") return null;
  return cleaned;
}

async function ensureUniqueRelPath(relativePath) {
  const ext = extname(relativePath);
  const stem = ext ? relativePath.slice(0, -ext.length) : relativePath;
  let candidate = relativePath;
  let counter = 1;
  while (true) {
    const full = resolve(FILES_ROOT, candidate);
    try {
      await stat(full);
      counter += 1;
      candidate = `${stem}-${counter}${ext}`;
    } catch {
      return candidate;
    }
  }
}

async function handleFilesApi(req, res, _session) {
  const url = new URL(req.url, `http://127.0.0.1:${PORTAL_PORT}`);
  const pathname = url.pathname;

  if (pathname === "/api/files/list" && req.method === "GET") {
    const incomingDir = url.searchParams.get("dir") || "";
    const defaultDir = FILES_SCOPE || "";
    const dirRelRaw = toSafeRelPath(incomingDir || defaultDir);
    if (dirRelRaw === null) {
      sendJson(res, 400, { ok: false, error: "Invalid directory path." });
      return true;
    }
    const dirRel = dirRelRaw || "";
    const dirFull = resolve(FILES_ROOT, dirRel);
    try {
      await mkdir(dirFull, { recursive: true });
      const entries = await readdir(dirFull, { withFileTypes: true });
      const files = [];
      for (const entry of entries) {
        const childRel = dirRel ? `${dirRel}/${entry.name}` : entry.name;
        if (!entry.isFile()) continue;
        const info = await stat(resolve(FILES_ROOT, childRel)).catch(() => null);
        files.push({
          name: entry.name,
          path: childRel,
          size: info?.size || 0,
          updatedAt: info?.mtimeMs || 0,
        });
      }
      files.sort((a, b) => b.updatedAt - a.updatedAt);
      sendJson(res, 200, { ok: true, root: FILES_ROOT, files });
    } catch (e) {
      sendJson(res, 500, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname === "/api/files/upload" && req.method === "POST") {
    const nameHeader = req.headers["x-file-name"];
    const rawName = decodeURIComponent(
      Array.isArray(nameHeader) ? String(nameHeader[0] || "") : String(nameHeader || "")
    );
    const safeName = toSafeRelFileName(rawName);
    if (!safeName) {
      sendJson(res, 400, { ok: false, error: "Missing or invalid x-file-name header." });
      return true;
    }

    const incomingDir = url.searchParams.get("dir") || "";
    const defaultDir = FILES_SCOPE || "";
    const dirRelRaw = toSafeRelPath(incomingDir || defaultDir);
    if (dirRelRaw === null) {
      sendJson(res, 400, { ok: false, error: "Invalid upload directory." });
      return true;
    }
    const dirRel = dirRelRaw || "";
    let relPath = dirRel ? `${dirRel}/${safeName}` : safeName;
    relPath = await ensureUniqueRelPath(relPath);
    const fullPath = resolve(FILES_ROOT, relPath);
    const folder = dirname(fullPath);

    try {
      await mkdir(folder, { recursive: true });
      let bytes = 0;
      const guard = new Transform({
        transform(chunk, _enc, cb) {
          bytes += chunk.length;
          if (bytes > MAX_UPLOAD_BYTES) {
            cb(new Error(`Upload exceeds limit of ${MAX_UPLOAD_BYTES} bytes.`));
            return;
          }
          cb(null, chunk);
        },
      });
      await pipeline(req, guard, createWriteStream(fullPath, { flags: "wx" }));
      const meta = await stat(fullPath);
      sendJson(res, 200, {
        ok: true,
        root: FILES_ROOT,
        name: safeName,
        path: relPath,
        absPath: fullPath,
        size: meta.size,
        downloadUrl: `/api/files/download?path=${encodeURIComponent(relPath)}`,
      });
    } catch (e) {
      if (e && typeof e === "object" && "code" in e && e.code === "EEXIST") {
        sendJson(res, 409, { ok: false, error: "File already exists." });
      } else {
        sendJson(res, 500, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
    }
    return true;
  }

  if (pathname === "/api/files/download" && req.method === "GET") {
    const rel = toSafeRelPath(url.searchParams.get("path") || "");
    if (!rel) {
      sendJson(res, 400, { ok: false, error: "Missing or invalid file path." });
      return true;
    }
    const full = resolve(FILES_ROOT, rel);
    try {
      const info = await stat(full);
      if (!info.isFile()) throw new Error("Not a file");
      const ext = extname(full);
      const mime = MIME_TYPES[ext] || "application/octet-stream";
      res.writeHead(200, {
        "content-type": mime,
        "content-length": String(info.size),
        "content-disposition": `attachment; filename="${basename(full).replace(/"/g, "")}"`,
      });
      createReadStream(full).pipe(res);
    } catch {
      sendJson(res, 404, { ok: false, error: "File not found." });
    }
    return true;
  }

  if (pathname === "/api/files/delete" && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      const rel = toSafeRelPath(body?.path || "");
      if (!rel) {
        sendJson(res, 400, { ok: false, error: "Missing or invalid file path." });
        return true;
      }
      await rm(resolve(FILES_ROOT, rel), { force: true });
      sendJson(res, 200, { ok: true, path: rel });
    } catch (e) {
      sendJson(res, 500, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  return false;
}

async function handleAuthLogin(req, res) {
  try {
    const body = await readJsonBody(req);
    const token = String(body?.token || "").trim();
    if (!token) {
      sendJson(res, 400, { ok: false, error: "Token required" });
      return;
    }
    const health = await engineHealth();
    if (!health) {
      sendJson(res, 502, { ok: false, error: "Engine unavailable" });
      return;
    }
    if (health.apiTokenRequired) {
      const valid = await validateEngineToken(token);
      if (!valid) {
        sendJson(res, 401, { ok: false, error: "Invalid engine API token" });
        return;
      }
    }
    const sid = randomBytes(24).toString("hex");
    sessions.set(sid, { token, createdAt: Date.now(), lastSeenAt: Date.now() });
    setSessionCookie(res, sid);
    sendJson(res, 200, {
      ok: true,
      requiresToken: !!health.apiTokenRequired,
      engine: { url: ENGINE_URL, version: health.version || "unknown", local: isLocalEngineUrl(ENGINE_URL) },
    });
  } catch (e) {
    sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
  }
}

function requireSession(req, res) {
  const session = getSession(req);
  if (!session) {
    sendJson(res, 401, { ok: false, error: "Unauthorized" });
    return null;
  }
  return session;
}

async function proxyEngineRequest(req, res, session) {
  const incoming = new URL(req.url, `http://127.0.0.1:${PORTAL_PORT}`);
  const targetPath = incoming.pathname.replace(/^\/api\/engine/, "") || "/";
  const targetUrl = `${ENGINE_URL}${targetPath}${incoming.search}`;

  const headers = new Headers();
  for (const [key, value] of Object.entries(req.headers)) {
    if (!value) continue;
    const lower = key.toLowerCase();
    if (["host", "content-length", "cookie", "authorization", "x-tandem-token"].includes(lower)) {
      continue;
    }
    if (Array.isArray(value)) headers.set(key, value.join(", "));
    else headers.set(key, value);
  }
  headers.set("authorization", `Bearer ${session.token}`);
  headers.set("x-tandem-token", session.token);

  const hasBody = !["GET", "HEAD"].includes(req.method || "GET");

  let upstream;
  try {
    upstream = await fetch(targetUrl, {
      method: req.method,
      headers,
      body: hasBody ? req : undefined,
      duplex: hasBody ? "half" : undefined,
    });
  } catch (e) {
    sendJson(res, 502, { ok: false, error: `Engine unreachable: ${e instanceof Error ? e.message : String(e)}` });
    return;
  }

  const responseHeaders = {};
  upstream.headers.forEach((value, key) => {
    const lower = key.toLowerCase();
    if (["content-encoding", "transfer-encoding", "connection"].includes(lower)) return;
    responseHeaders[key] = value;
  });

  res.writeHead(upstream.status, responseHeaders);
  if (!upstream.body) {
    res.end();
    return;
  }
  for await (const chunk of upstream.body) {
    res.write(chunk);
  }
  res.end();
}

async function readSwarmRegistry(token) {
  const keys = ["swarm.active_tasks", "project/swarm.active_tasks"];
  for (const key of keys) {
    try {
      const response = await fetch(`${ENGINE_URL}/resource/${encodeURIComponent(key)}`, {
        headers: {
          authorization: `Bearer ${token}`,
          "x-tandem-token": token,
        },
        signal: AbortSignal.timeout(1200),
      });
      if (!response.ok) continue;
      const record = await response.json();
      if (record?.value && typeof record.value === "object") {
        return { key, value: record.value };
      }
    } catch {
      // ignore
    }
  }
  return { key: "swarm.active_tasks", value: { version: 1, updatedAtMs: Date.now(), tasks: {} } };
}

function stopSwarm() {
  if (!swarmState.process) return;
  swarmState.status = "stopping";
  clearSwarmMonitor();
  swarmState.process.kill("SIGTERM");
  pushSwarmEvent("status", { status: swarmState.status });
}

function startSwarm(session, config = {}) {
  if (!isLocalEngineUrl(ENGINE_URL)) {
    throw new Error("Swarm orchestration is disabled when using a remote engine URL.");
  }
  if (swarmState.process) {
    throw new Error("Swarm runtime is already running.");
  }

  const objective = String(config.objective || "Ship a small feature end-to-end").trim();
  const workspaceRoot = String(config.workspaceRoot || REPO_ROOT).trim();
  const maxTasks = Math.max(1, Number.parseInt(String(config.maxTasks || 3), 10) || 3);

  const managerPath = join(REPO_ROOT, "examples", "agent-swarm", "src", "manager.mjs");
  if (!existsSync(managerPath)) {
    throw new Error(`Missing swarm manager at ${managerPath}`);
  }

  swarmState.logs = [];
  swarmState.reasons = [];
  swarmState.status = "starting";
  swarmState.startedAt = Date.now();
  swarmState.stoppedAt = null;
  swarmState.objective = objective;
  swarmState.workspaceRoot = workspaceRoot;
  swarmState.maxTasks = maxTasks;
  swarmState.lastError = "";
  swarmState.registryCache = null;

  pushSwarmEvent("status", { status: swarmState.status, objective, workspaceRoot, maxTasks });

  const child = spawn(process.execPath, [managerPath, objective], {
    cwd: workspaceRoot,
    env: {
      ...process.env,
      TANDEM_BASE_URL: ENGINE_URL,
      TANDEM_API_TOKEN: session.token,
      SWARM_MAX_TASKS: String(maxTasks),
      SWARM_OBJECTIVE: objective,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  swarmState.process = child;

  child.stdout.on("data", (chunk) => appendSwarmLog("stdout", chunk));
  child.stderr.on("data", (chunk) => appendSwarmLog("stderr", chunk));

  child.on("spawn", () => {
    swarmState.status = "running";
    pushSwarmEvent("status", { status: swarmState.status });
    void monitorSwarmRegistry(session.token);
    swarmState.monitorTimer = setInterval(() => {
      if (swarmState.status === "running") void monitorSwarmRegistry(session.token);
    }, 2000);
  });

  child.on("error", (e) => {
    swarmState.status = "error";
    swarmState.lastError = e.message;
    clearSwarmMonitor();
    appendSwarmLog("stderr", e.message);
    pushSwarmEvent("status", { status: swarmState.status, error: e.message });
  });

  child.on("exit", (code, signal) => {
    const failed = code && code !== 0;
    swarmState.status = failed ? "error" : "idle";
    swarmState.stoppedAt = Date.now();
    swarmState.lastError = failed ? `Exited with code ${code}` : "";
    swarmState.process = null;
    clearSwarmMonitor();
    pushSwarmEvent("status", {
      status: swarmState.status,
      code,
      signal,
      error: swarmState.lastError || undefined,
    });
  });
}

async function handleSwarmApi(req, res, session) {
  const pathname = new URL(req.url, `http://127.0.0.1:${PORTAL_PORT}`).pathname;

  if (pathname === "/api/swarm/status" && req.method === "GET") {
    sendJson(res, 200, {
      ok: true,
      status: swarmState.status,
      objective: swarmState.objective,
      workspaceRoot: swarmState.workspaceRoot,
      maxTasks: swarmState.maxTasks,
      startedAt: swarmState.startedAt,
      stoppedAt: swarmState.stoppedAt,
      localEngine: isLocalEngineUrl(ENGINE_URL),
      lastError: swarmState.lastError || null,
    });
    return true;
  }

  if (pathname === "/api/swarm/start" && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      startSwarm(session, body || {});
      sendJson(res, 200, { ok: true });
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname === "/api/swarm/stop" && req.method === "POST") {
    stopSwarm();
    sendJson(res, 200, { ok: true, status: swarmState.status });
    return true;
  }

  if (pathname === "/api/swarm/snapshot" && req.method === "GET") {
    const registry = await readSwarmRegistry(session.token);
    sendJson(res, 200, {
      ok: true,
      status: swarmState.status,
      registry,
      logs: swarmState.logs.slice(-300),
      reasons: swarmState.reasons.slice(-250),
      startedAt: swarmState.startedAt,
      stoppedAt: swarmState.stoppedAt,
      lastError: swarmState.lastError || null,
      localEngine: isLocalEngineUrl(ENGINE_URL),
    });
    return true;
  }

  if (pathname === "/api/swarm/events" && req.method === "GET") {
    res.writeHead(200, {
      "content-type": "text/event-stream",
      "cache-control": "no-cache",
      connection: "keep-alive",
    });
    res.write(`data: ${JSON.stringify({ kind: "hello", ts: Date.now(), status: swarmState.status })}\n\n`);
    swarmSseClients.add(res);
    req.on("close", () => swarmSseClients.delete(res));
    return true;
  }

  return false;
}

async function handleApi(req, res) {
  const pathname = new URL(req.url, `http://127.0.0.1:${PORTAL_PORT}`).pathname;

  if (pathname === "/api/system/health" && req.method === "GET") {
    const health = await engineHealth();
    sendJson(res, 200, {
      ok: true,
      engineUrl: ENGINE_URL,
      engine: health || null,
      localEngine: isLocalEngineUrl(ENGINE_URL),
      autoStartEngine: AUTO_START_ENGINE,
    });
    return true;
  }

  if (pathname === "/api/auth/login" && req.method === "POST") {
    await handleAuthLogin(req, res);
    return true;
  }

  if (pathname === "/api/auth/logout" && req.method === "POST") {
    const current = getSession(req);
    if (current?.sid) sessions.delete(current.sid);
    clearSessionCookie(res);
    sendJson(res, 200, { ok: true });
    return true;
  }

  if (pathname === "/api/auth/me" && req.method === "GET") {
    const session = requireSession(req, res);
    if (!session) return true;
    const health = await engineHealth(session.token);
    if (!health) {
      sessions.delete(session.sid);
      clearSessionCookie(res);
      sendJson(res, 401, { ok: false, error: "Session token is no longer valid for the configured engine." });
      return true;
    }
    sendJson(res, 200, {
      ok: true,
      engineUrl: ENGINE_URL,
      localEngine: isLocalEngineUrl(ENGINE_URL),
      engine: health,
    });
    return true;
  }

  if (pathname.startsWith("/api/swarm")) {
    const session = requireSession(req, res);
    if (!session) return true;
    return handleSwarmApi(req, res, session);
  }

  if (pathname.startsWith("/api/files")) {
    const session = requireSession(req, res);
    if (!session) return true;
    return handleFilesApi(req, res, session);
  }

  if (pathname.startsWith("/api/engine")) {
    const session = requireSession(req, res);
    if (!session) return true;
    await proxyEngineRequest(req, res, session);
    return true;
  }

  return false;
}

function serveStatic(req, res) {
  const filePath = sanitizeStaticPath(req.url);
  if (!filePath) {
    res.writeHead(403);
    res.end("Forbidden");
    return;
  }

  let target = filePath;
  if (!existsSync(target)) {
    if (!extname(target)) target = join(DIST_DIR, "index.html");
    else {
      res.writeHead(404);
      res.end("Not Found");
      return;
    }
  }

  const ext = extname(target);
  const mime = MIME_TYPES[ext] || "application/octet-stream";
  res.writeHead(200, { "content-type": mime });
  createReadStream(target).pipe(res);
}

function shutdown(signal) {
  log(`Shutting down (${signal})...`);
  if (server) {
    try {
      server.close();
    } catch {}
  }
  if (swarmState.process && !swarmState.process.killed) {
    try {
      clearSwarmMonitor();
      swarmState.process.kill("SIGTERM");
    } catch {}
  }
  if (engineProcess && !engineProcess.killed) {
    try {
      engineProcess.kill(signal);
    } catch {}
  }
  process.exit(0);
}

process.on("SIGINT", () => shutdown("SIGINT"));
process.on("SIGTERM", () => shutdown("SIGTERM"));

async function main() {
  if (!existsSync(DIST_DIR)) {
    err(`Missing build output at ${DIST_DIR}`);
    err("Run: npm run build");
    process.exit(1);
  }

  await ensureEngineRunning();

  server = createServer(async (req, res) => {
    try {
      if (await handleApi(req, res)) return;
      serveStatic(req, res);
    } catch (e) {
      err(e instanceof Error ? e.stack || e.message : String(e));
      sendJson(res, 500, { ok: false, error: "Internal server error" });
    }
  });

  server.on("error", (e) => {
    err(`Failed to bind control panel port ${PORTAL_PORT}: ${e.message}`);
    process.exit(1);
  });

  server.listen(PORTAL_PORT, () => {
    log("=========================================");
    log(`Control Panel: http://localhost:${PORTAL_PORT}`);
    log(`Engine URL:    ${ENGINE_URL}`);
    log(`Engine mode:   ${isLocalEngineUrl(ENGINE_URL) ? "local" : "remote"}`);
    log(`Files root:    ${FILES_ROOT}`);
    log(`Files scope:   ${FILES_SCOPE || "(full root)"}`);
    log("=========================================");
  });
}

main().catch((e) => {
  err(e instanceof Error ? e.message : String(e));
  process.exit(1);
});
