#!/usr/bin/env node

import { spawn } from "child_process";
import { createServer } from "http";
import { readFileSync, existsSync, createReadStream, createWriteStream } from "fs";
import { mkdir, readdir, stat, rm, readFile, writeFile } from "fs/promises";
import { createHash, randomBytes } from "crypto";
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

function parseCliArgs(argv) {
  const flags = new Set();
  const values = new Map();
  for (let i = 0; i < argv.length; i += 1) {
    const raw = String(argv[i] || "").trim();
    if (!raw) continue;
    if (!raw.startsWith("--")) {
      flags.add(raw);
      continue;
    }
    const eq = raw.indexOf("=");
    if (eq > 2) {
      values.set(raw.slice(2, eq), raw.slice(eq + 1));
      continue;
    }
    const key = raw.slice(2);
    const next = String(argv[i + 1] || "").trim();
    if (next && !next.startsWith("-")) {
      values.set(key, next);
      i += 1;
      continue;
    }
    flags.add(raw);
  }
  return {
    flags,
    values,
    has(flag) {
      return flags.has(flag) || flags.has(`--${flag}`) || values.has(flag);
    },
    value(key) {
      return values.get(key);
    },
  };
}

const cli = parseCliArgs(process.argv.slice(2));
const rawArgs = process.argv.slice(2);
const initRequested = cli.has("init");
const resetTokenRequested = cli.has("reset-token");
const installServicesRequested = cli.has("install-services");
const serviceModeRaw = String(cli.value("service-mode") || "both")
  .trim()
  .toLowerCase();
const serviceMode = ["both", "engine", "panel"].includes(serviceModeRaw) ? serviceModeRaw : "both";
const serviceUserArg = String(cli.value("service-user") || "").trim();
const serviceSetupOnly = rawArgs.length > 0 && rawArgs.every((arg) => {
  if (arg === "--install-services") return true;
  if (arg.startsWith("--service-mode")) return true;
  if (arg.startsWith("--service-user")) return true;
  return false;
});
const cwdEnvPath = resolve(process.cwd(), ".env");

if (initRequested) {
  const result = ensureEnv({ overwrite: resetTokenRequested });
  console.log("[Tandem Control Panel] Environment initialized.");
  console.log(`[Tandem Control Panel] .env:      ${result.envPath}`);
  console.log(`[Tandem Control Panel] Engine URL: ${result.engineUrl}`);
  console.log(`[Tandem Control Panel] Panel URL:  http://localhost:${result.panelPort}`);
  console.log(`[Tandem Control Panel] Token:      ${result.token}`);
  if (process.argv.slice(2).length === 1 || (process.argv.slice(2).length === 2 && resetTokenRequested)) {
    process.exit(0);
  }
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
const SWARM_RUNS_PATH = resolve(homedir(), ".tandem", "control-panel", "swarm-runs.json");
const SWARM_HIDDEN_RUNS_PATH = resolve(homedir(), ".tandem", "control-panel", "swarm-hidden-runs.json");
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
const SETUP_ENTRYPOINT = fileURLToPath(import.meta.url);

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
  modelProvider: "",
  modelId: "",
  mcpServers: [],
  repoRoot: "",
  preflight: {
    gitAvailable: null,
    repoReady: true,
    autoInitialized: false,
    code: "workspace_ready",
    reason: "",
    guidance: "",
  },
  lastError: "",
  executorState: "idle",
  executorReason: "",
  resolvedModelProvider: "",
  resolvedModelId: "",
  modelResolutionSource: "none",
  runId: "",
  attachedPid: null,
};
const swarmSseClients = new Set();

const sleep = (ms) => new Promise((resolveFn) => setTimeout(resolveFn, ms));

function shellEscape(token) {
  const text = String(token || "");
  if (/^[A-Za-z0-9_./:@-]+$/.test(text)) return text;
  return `"${text.replace(/(["\\$`])/g, "\\$1")}"`;
}

function runCmd(bin, args = [], options = {}) {
  return new Promise((resolveFn, reject) => {
    const child = spawn(bin, args, {
      stdio: options.stdio || "pipe",
      env: options.env || process.env,
      cwd: options.cwd || undefined,
    });
    let stdout = "";
    let stderr = "";
    if (child.stdout) {
      child.stdout.on("data", (chunk) => {
        stdout += chunk.toString("utf8");
      });
    }
    if (child.stderr) {
      child.stderr.on("data", (chunk) => {
        stderr += chunk.toString("utf8");
      });
    }
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) {
        resolveFn({ stdout, stderr });
        return;
      }
      reject(new Error(`${bin} ${args.join(" ")} exited ${code}: ${stderr || stdout}`));
    });
  });
}

async function loadSwarmRunsHistory() {
  try {
    const raw = await readFile(SWARM_RUNS_PATH, "utf8");
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed?.runs)) return [];
    return parsed.runs
      .filter((row) => row && typeof row === "object")
      .map((row) => ({
        runId: String(row.runId || "").trim(),
        objective: String(row.objective || "").trim(),
        workspaceRoot: String(row.workspaceRoot || "").trim(),
        status: String(row.status || "unknown").trim(),
        startedAt: Number(row.startedAt || 0) || 0,
        stoppedAt: Number(row.stoppedAt || 0) || 0,
        pid: Number(row.pid || 0) || null,
        attached: row.attached === true,
      }))
      .filter((row) => row.runId || row.workspaceRoot || row.pid);
  } catch {
    return [];
  }
}

async function saveSwarmRunsHistory(runs = []) {
  const payload = JSON.stringify({ version: 1, updatedAtMs: Date.now(), runs: runs.slice(-100) }, null, 2);
  await mkdir(dirname(SWARM_RUNS_PATH), { recursive: true });
  await writeFile(SWARM_RUNS_PATH, payload, "utf8");
}

async function loadHiddenSwarmRunIds() {
  try {
    const raw = await readFile(SWARM_HIDDEN_RUNS_PATH, "utf8");
    const parsed = JSON.parse(raw);
    const ids = Array.isArray(parsed?.runIds) ? parsed.runIds : [];
    return new Set(
      ids
        .map((id) => String(id || "").trim())
        .filter(Boolean)
        .slice(0, 5000)
    );
  } catch {
    return new Set();
  }
}

async function saveHiddenSwarmRunIds(runIdSet) {
  const runIds = Array.from(runIdSet)
    .map((id) => String(id || "").trim())
    .filter(Boolean)
    .sort((a, b) => a.localeCompare(b));
  await mkdir(dirname(SWARM_HIDDEN_RUNS_PATH), { recursive: true });
  await writeFile(
    SWARM_HIDDEN_RUNS_PATH,
    JSON.stringify({ updatedAt: Date.now(), runIds }, null, 2),
    "utf8"
  );
}

async function recordSwarmRun(update = {}) {
  try {
    const runId = String(update.runId || "").trim() || randomBytes(8).toString("hex");
    const runs = await loadSwarmRunsHistory();
    const idx = runs.findIndex((row) => row.runId === runId);
    const next = {
      runId,
      objective: String(update.objective || "").trim(),
      workspaceRoot: String(update.workspaceRoot || "").trim(),
      status: String(update.status || "unknown").trim(),
      startedAt: Number(update.startedAt || 0) || Date.now(),
      stoppedAt: Number(update.stoppedAt || 0) || 0,
      pid: Number(update.pid || 0) || null,
      attached: update.attached === true,
    };
    if (idx >= 0) runs[idx] = { ...runs[idx], ...next };
    else runs.push(next);
    await saveSwarmRunsHistory(runs);
    return runId;
  } catch {
    return String(update.runId || "").trim();
  }
}

function buildGitSafeEnv(baseEnv, safeDirectory) {
  const env = { ...(baseEnv || process.env) };
  const value = String(safeDirectory || "*").trim() || "*";
  env.GIT_CONFIG_COUNT = "1";
  env.GIT_CONFIG_KEY_0 = "safe.directory";
  env.GIT_CONFIG_VALUE_0 = value;
  return env;
}

function runGit(args = [], options = {}) {
  return runCmd("git", args, {
    ...options,
    env: buildGitSafeEnv(options.env || process.env, options.safeDirectory),
  });
}

function guidanceForGitInstall() {
  if (process.platform === "darwin") {
    return "Install Git: `xcode-select --install` or `brew install git`.";
  }
  if (process.platform === "win32") {
    return "Install Git for Windows from https://git-scm.com/download/win and restart the app.";
  }
  return "Install Git using your package manager (for example `sudo apt install git`) and restart the app.";
}

function setSwarmPreflight(patch = {}) {
  swarmState.preflight = {
    gitAvailable: swarmState.preflight?.gitAvailable ?? null,
    repoReady: swarmState.preflight?.repoReady ?? false,
    autoInitialized: swarmState.preflight?.autoInitialized ?? false,
    code: swarmState.preflight?.code || "",
    reason: swarmState.preflight?.reason || "",
    guidance: swarmState.preflight?.guidance || "",
    ...patch,
  };
}

async function detectGitAvailable() {
  try {
    await runGit(["--version"], { stdio: "pipe" });
    return true;
  } catch {
    return false;
  }
}

async function isGitRepo(cwd) {
  try {
    const out = await runGit(["-C", String(cwd || ""), "rev-parse", "--show-toplevel"], {
      stdio: "pipe",
      safeDirectory: "*",
    });
    const root = String(out.stdout || "").trim();
    return root ? { ok: true, root, error: "" } : { ok: false, root: "", error: "" };
  } catch (error) {
    return { ok: false, root: "", error: String(error?.message || error || "") };
  }
}

async function bootstrapEmptyGitRepo(workspaceRoot) {
  await runGit(["init"], { cwd: workspaceRoot, stdio: "pipe", safeDirectory: workspaceRoot });
  const readmePath = join(workspaceRoot, "README.md");
  const ignorePath = join(workspaceRoot, ".gitignore");
  if (!existsSync(readmePath)) {
    await writeFile(
      readmePath,
      "# Swarm Workspace\n\nInitialized automatically for Tandem Swarm.\n",
      "utf8"
    );
  }
  if (!existsSync(ignorePath)) {
    await writeFile(
      ignorePath,
      "node_modules/\n.DS_Store\n.swarm/worktrees/\n",
      "utf8"
    );
  }
  await runGit(["add", "."], { cwd: workspaceRoot, stdio: "pipe", safeDirectory: workspaceRoot });
  try {
    await runGit(["commit", "-m", "Initialize swarm workspace"], {
      cwd: workspaceRoot,
      stdio: "pipe",
      safeDirectory: workspaceRoot,
    });
  } catch (commitError) {
    const message = String(commitError?.message || "");
    if (!message.includes("Author identity unknown")) throw commitError;
    await runGit(["config", "user.name", "Swarm Bootstrap"], {
      cwd: workspaceRoot,
      stdio: "pipe",
      safeDirectory: workspaceRoot,
    });
    await runGit(["config", "user.email", "swarm@local"], {
      cwd: workspaceRoot,
      stdio: "pipe",
      safeDirectory: workspaceRoot,
    });
    await runGit(["commit", "-m", "Initialize swarm workspace"], {
      cwd: workspaceRoot,
      stdio: "pipe",
      safeDirectory: workspaceRoot,
    });
  }
}

async function preflightSwarmWorkspace(workspaceRoot, options = {}) {
  const allowInitNonEmpty = options.allowInitNonEmpty === true;
  const guidance = guidanceForGitInstall();
  const gitAvailable = await detectGitAvailable();
  if (!gitAvailable) {
    return {
      gitAvailable,
      repoReady: false,
      autoInitialized: false,
      code: "git_missing",
      repoRoot: "",
      reason: "Git executable not found",
      guidance,
    };
  }

  const normalized = resolve(workspaceRoot);
  if (!existsSync(normalized)) {
    throw new Error(`Workspace root does not exist: ${normalized}`);
  }
  const details = await stat(normalized);
  if (!details.isDirectory()) {
    throw new Error(`Workspace root is not a directory: ${normalized}`);
  }

  const repo = await isGitRepo(normalized);
  if (repo.ok) {
    return {
      gitAvailable: true,
      repoReady: true,
      autoInitialized: false,
      code: "ok",
      repoRoot: repo.root,
      reason: "",
      guidance,
    };
  }
  const repoErrorText = String(repo.error || "");
  const repoErrorLower = repoErrorText.toLowerCase();
  if (
    repoErrorText &&
    !repoErrorLower.includes("not a git repository") &&
    !repoErrorLower.includes("needed a single revision")
  ) {
    return {
      gitAvailable: true,
      repoReady: false,
      autoInitialized: false,
      code: "git_probe_failed",
      repoRoot: "",
      reason: `Git could not inspect the selected directory: ${repoErrorText}`,
      guidance,
    };
  }

  const entries = await readdir(normalized);
  if (entries.length > 0 && !allowInitNonEmpty) {
    const detail = repoErrorText ? ` Git probe output: ${repoErrorText}` : "";
    return {
      gitAvailable: true,
      repoReady: false,
      autoInitialized: false,
      code: "not_repo_non_empty",
      repoRoot: "",
      reason:
        `Selected directory is not a Git repository and is not empty: ${normalized}. Choose an existing repo or an empty directory.${detail}`,
      guidance,
    };
  }

  await bootstrapEmptyGitRepo(normalized);
  const initializedRepo = await isGitRepo(normalized);
  if (!initializedRepo.ok) {
    throw new Error(`Git repository initialization failed for ${normalized}`);
  }
  return {
    gitAvailable: true,
    repoReady: true,
    autoInitialized: true,
    code: allowInitNonEmpty ? "auto_initialized_non_empty" : "auto_initialized_empty",
    repoRoot: initializedRepo.root,
    reason: "",
    guidance,
  };
}

async function installServices() {
  if (process.platform !== "linux") {
    throw new Error("--install-services currently supports Linux/systemd only.");
  }
  if (typeof process.getuid === "function" && process.getuid() !== 0) {
    throw new Error("Service installation needs root privileges. Re-run with sudo.");
  }

  const serviceUser = serviceUserArg || String(process.env.SUDO_USER || process.env.USER || "root").trim();
  if (!serviceUser) throw new Error("Could not determine service user.");
  const serviceGroup = serviceUser;
  const installEngine = serviceMode === "both" || serviceMode === "engine";
  const installPanel = serviceMode === "both" || serviceMode === "panel";
  const stateDir = String(process.env.TANDEM_STATE_DIR || "/srv/tandem").trim();
  const engineEnvPath = "/etc/tandem/engine.env";
  const panelEnvPath = "/etc/tandem/control-panel.env";
  const engineServiceName = "tandem-engine";
  const panelServiceName = "tandem-control-panel";
  const engineBin = String(process.env.TANDEM_ENGINE_BIN || "tandem-engine").trim();
  const token =
    CONFIGURED_ENGINE_TOKEN ||
    (existsSync(engineEnvPath) ? parseDotEnv(readFileSync(engineEnvPath, "utf8")).TANDEM_API_TOKEN || "" : "") ||
    `tk_${randomBytes(16).toString("hex")}`;

  await mkdir("/etc/tandem", { recursive: true });
  await mkdir(stateDir, { recursive: true });
  try {
    await runCmd("chown", ["-R", `${serviceUser}:${serviceGroup}`, stateDir]);
  } catch (e) {
    log(`Warning: could not chown ${stateDir} to ${serviceUser}:${serviceGroup}: ${e.message}`);
  }

  const existingEngineEnv = existsSync(engineEnvPath) ? parseDotEnv(readFileSync(engineEnvPath, "utf8")) : {};
  const engineEnv = {
    ...existingEngineEnv,
    TANDEM_API_TOKEN: token,
    TANDEM_STATE_DIR: stateDir,
    TANDEM_MEMORY_DB_PATH: existingEngineEnv.TANDEM_MEMORY_DB_PATH || `${stateDir}/memory.sqlite`,
    TANDEM_ENABLE_GLOBAL_MEMORY: existingEngineEnv.TANDEM_ENABLE_GLOBAL_MEMORY || "1",
    TANDEM_DISABLE_TOOL_GUARD_BUDGETS:
      existingEngineEnv.TANDEM_DISABLE_TOOL_GUARD_BUDGETS || "1",
    TANDEM_TOOL_ROUTER_ENABLED:
      existingEngineEnv.TANDEM_TOOL_ROUTER_ENABLED || "0",
    TANDEM_PROMPT_CONTEXT_HOOK_TIMEOUT_MS:
      existingEngineEnv.TANDEM_PROMPT_CONTEXT_HOOK_TIMEOUT_MS || "5000",
    TANDEM_PROVIDER_STREAM_CONNECT_TIMEOUT_MS:
      existingEngineEnv.TANDEM_PROVIDER_STREAM_CONNECT_TIMEOUT_MS || "30000",
    TANDEM_PROVIDER_STREAM_IDLE_TIMEOUT_MS:
      existingEngineEnv.TANDEM_PROVIDER_STREAM_IDLE_TIMEOUT_MS || "90000",
    TANDEM_PERMISSION_WAIT_TIMEOUT_MS:
      existingEngineEnv.TANDEM_PERMISSION_WAIT_TIMEOUT_MS || "15000",
    TANDEM_TOOL_EXEC_TIMEOUT_MS:
      existingEngineEnv.TANDEM_TOOL_EXEC_TIMEOUT_MS || "45000",
    TANDEM_BASH_TIMEOUT_MS: existingEngineEnv.TANDEM_BASH_TIMEOUT_MS || "30000",
  };
  const engineEnvBody = Object.entries(engineEnv)
    .map(([k, v]) => `${k}=${v}`)
    .join("\n");
  await writeFile(engineEnvPath, `${engineEnvBody}\n`, "utf8");
  await runCmd("chmod", ["640", engineEnvPath]);

  const panelAutoStart = serviceMode === "panel" ? "1" : "0";
  const existingPanelEnv = existsSync(panelEnvPath) ? parseDotEnv(readFileSync(panelEnvPath, "utf8")) : {};
  const panelEnv = {
    ...existingPanelEnv,
    TANDEM_CONTROL_PANEL_PORT: String(PORTAL_PORT),
    TANDEM_ENGINE_URL: ENGINE_URL,
    TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: panelAutoStart,
    TANDEM_CONTROL_PANEL_ENGINE_TOKEN: token,
  };
  const panelEnvBody = Object.entries(panelEnv)
    .map(([k, v]) => `${k}=${v}`)
    .join("\n");
  await writeFile(panelEnvPath, `${panelEnvBody}\n`, "utf8");
  await runCmd("chmod", ["640", panelEnvPath]);

  if (installEngine) {
    const engineExec = [
      engineBin,
      "serve",
      "--hostname",
      ENGINE_HOST,
      "--port",
      String(ENGINE_PORT),
    ]
      .map(shellEscape)
      .join(" ");
    const engineUnit = `[Unit]
Description=Tandem Engine
After=network.target

[Service]
Type=simple
User=${serviceUser}
Group=${serviceGroup}
EnvironmentFile=-${engineEnvPath}
ExecStart=${engineExec}
Restart=always
RestartSec=2
WorkingDirectory=${REPO_ROOT}

[Install]
WantedBy=multi-user.target
`;
    await writeFile(`/etc/systemd/system/${engineServiceName}.service`, engineUnit, "utf8");
  }

  if (installPanel) {
    const panelExec = [process.execPath, SETUP_ENTRYPOINT].map(shellEscape).join(" ");
    const unitDependencies = installEngine
      ? `After=network.target ${engineServiceName}.service\nWants=${engineServiceName}.service`
      : "After=network.target";
    const panelUnit = `[Unit]
Description=Tandem Control Panel
${unitDependencies}

[Service]
Type=simple
User=${serviceUser}
Group=${serviceGroup}
EnvironmentFile=-${panelEnvPath}
ExecStart=${panelExec}
Restart=always
RestartSec=2
WorkingDirectory=${REPO_ROOT}

[Install]
WantedBy=multi-user.target
`;
    await writeFile(`/etc/systemd/system/${panelServiceName}.service`, panelUnit, "utf8");
  }

  await runCmd("systemctl", ["daemon-reload"], { stdio: "inherit" });
  if (installEngine) {
    await runCmd("systemctl", ["enable", "--now", `${engineServiceName}.service`], { stdio: "inherit" });
  }
  if (installPanel) {
    await runCmd("systemctl", ["enable", "--now", `${panelServiceName}.service`], { stdio: "inherit" });
  }

  log("Services installed.");
  log(`Mode: ${serviceMode}`);
  log(`Service user: ${serviceUser}`);
  log(`Engine env: ${engineEnvPath}`);
  log(`Panel env: ${panelEnvPath}`);
  if (installEngine) log(`Engine service: ${engineServiceName}.service`);
  if (installPanel) log(`Panel service:  ${panelServiceName}.service`);
  log(`Token: ${token}`);
}

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
  if (res.headersSent || res.writableEnded || res.destroyed) return;
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
    err("Reinstall with: npm i -g @frumu/tandem-panel");
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
        TANDEM_DISABLE_TOOL_GUARD_BUDGETS:
          process.env.TANDEM_DISABLE_TOOL_GUARD_BUDGETS || "1",
        TANDEM_TOOL_ROUTER_ENABLED:
          process.env.TANDEM_TOOL_ROUTER_ENABLED || "0",
        TANDEM_PROMPT_CONTEXT_HOOK_TIMEOUT_MS:
          process.env.TANDEM_PROMPT_CONTEXT_HOOK_TIMEOUT_MS || "5000",
        TANDEM_PROVIDER_STREAM_CONNECT_TIMEOUT_MS:
          process.env.TANDEM_PROVIDER_STREAM_CONNECT_TIMEOUT_MS || "30000",
        TANDEM_PROVIDER_STREAM_IDLE_TIMEOUT_MS:
          process.env.TANDEM_PROVIDER_STREAM_IDLE_TIMEOUT_MS || "90000",
        TANDEM_PERMISSION_WAIT_TIMEOUT_MS:
          process.env.TANDEM_PERMISSION_WAIT_TIMEOUT_MS || "15000",
        TANDEM_TOOL_EXEC_TIMEOUT_MS:
          process.env.TANDEM_TOOL_EXEC_TIMEOUT_MS || "45000",
        TANDEM_BASH_TIMEOUT_MS: process.env.TANDEM_BASH_TIMEOUT_MS || "30000",
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

  if (pathname === "/api/files/read" && req.method === "GET") {
    const rel = toSafeRelPath(url.searchParams.get("path") || "");
    if (!rel) {
      sendJson(res, 400, { ok: false, error: "Missing or invalid file path." });
      return true;
    }
    const full = resolve(FILES_ROOT, rel);
    try {
      const info = await stat(full);
      if (!info.isFile()) throw new Error("Not a file");
      if (info.size > MAX_UPLOAD_BYTES) {
        sendJson(res, 413, { ok: false, error: "File too large to read through API." });
        return true;
      }
      const text = await readFile(full, "utf8");
      sendJson(res, 200, {
        ok: true,
        root: FILES_ROOT,
        path: rel,
        absPath: full,
        size: info.size,
        text,
      });
    } catch {
      sendJson(res, 404, { ok: false, error: "File not found." });
    }
    return true;
  }

  if (pathname === "/api/files/write" && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      const rel = toSafeRelPath(body?.path || "");
      const text = String(body?.text ?? "");
      const overwrite = body?.overwrite !== false;
      if (!rel) {
        sendJson(res, 400, { ok: false, error: "Missing or invalid file path." });
        return true;
      }
      if (Buffer.byteLength(text, "utf8") > MAX_UPLOAD_BYTES) {
        sendJson(res, 413, { ok: false, error: "Text payload exceeds max upload bytes limit." });
        return true;
      }
      const full = resolve(FILES_ROOT, rel);
      await mkdir(dirname(full), { recursive: true });
      await writeFile(full, text, { encoding: "utf8", flag: overwrite ? "w" : "wx" });
      const info = await stat(full);
      sendJson(res, 200, {
        ok: true,
        root: FILES_ROOT,
        path: rel,
        absPath: full,
        size: info.size,
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

  try {
    res.writeHead(upstream.status, responseHeaders);
    if (!upstream.body) {
      res.end();
      return;
    }
    for await (const chunk of upstream.body) {
      if (res.writableEnded || res.destroyed) break;
      res.write(chunk);
    }
    if (!res.writableEnded && !res.destroyed) {
      res.end();
    }
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    if (res.headersSent) {
      const lower = message.toLowerCase();
      // SSE/streaming upstream can terminate normally from the engine side.
      if (lower.includes("terminated") || lower.includes("aborted")) {
        if (!res.writableEnded && !res.destroyed) {
          res.end();
        }
        return;
      }
      if (!res.destroyed && !res.writableEnded) {
        res.destroy(e instanceof Error ? e : undefined);
      }
      return;
    }
    sendJson(res, 502, {
      ok: false,
      error: `Engine proxy stream failed: ${message}`,
    });
  }
}

function contextStepStatusToLegacyTaskStatus(status) {
  switch (String(status || "").trim().toLowerCase()) {
    case "in_progress":
      return "running";
    case "runnable":
    case "pending":
      return "pending";
    case "blocked":
      return "blocked";
    case "done":
      return "complete";
    case "failed":
      return "failed";
    default:
      return "pending";
  }
}

function contextRunStatusToSwarmStatus(status) {
  const normalized = String(status || "").trim().toLowerCase();
  if (
    [
      "queued",
      "planning",
      "awaiting_approval",
      "running",
      "paused",
      "blocked",
      "completed",
      "failed",
      "cancelled",
    ].includes(normalized)
  ) {
    return normalized;
  }
  return "idle";
}

function buildWorkspaceId(workspaceRoot) {
  const digest = createHash("sha1").update(String(workspaceRoot || "")).digest("hex");
  return `ws-${digest.slice(0, 16)}`;
}

function workspaceExistsAsDirectory(workspaceRoot) {
  const normalized = resolve(String(workspaceRoot || "").trim());
  if (!existsSync(normalized)) return null;
  return stat(normalized)
    .then((info) => (info.isDirectory() ? normalized : null))
    .catch(() => null);
}

async function engineRequestJson(session, path, options = {}) {
  const method = String(options.method || "GET").toUpperCase();
  const body = options.body;
  const response = await fetch(`${ENGINE_URL}${path}`, {
    method,
    headers: {
      authorization: `Bearer ${session.token}`,
      "x-tandem-token": session.token,
      ...(body ? { "content-type": "application/json" } : {}),
      ...(options.headers || {}),
    },
    body: body ? JSON.stringify(body) : undefined,
    signal: AbortSignal.timeout(options.timeoutMs || 8000),
  });
  if (!response.ok) {
    let detail = `${method} ${path} failed: ${response.status}`;
    try {
      const payload = await response.json();
      detail = String(payload?.error || payload?.message || detail);
    } catch {
      try {
        detail = (await response.text()) || detail;
      } catch {
        // ignore
      }
    }
    throw new Error(detail);
  }
  if (response.status === 204) return {};
  const text = await response.text();
  if (!text.trim()) return {};
  return JSON.parse(text);
}

function contextRunToTasks(run) {
  const steps = Array.isArray(run?.steps) ? run.steps : [];
  return steps.map((step) => ({
    taskId: String(step.step_id || ""),
    title: String(step.title || step.step_id || "Untitled step"),
    ownerRole: "context_driver",
    status: contextStepStatusToLegacyTaskStatus(step.status),
    stepStatus: String(step.status || "pending"),
    statusReason: String(run?.why_next_step || ""),
    lastUpdateMs: Number(run?.updated_at_ms || Date.now()),
    runId: String(run?.run_id || ""),
    sessionId: `context-${String(run?.run_id || "")}`,
    branch: "",
    worktreePath: "",
  }));
}

function eventsToReasons(events = []) {
  return events
    .map((evt) => {
      const payload = evt?.payload && typeof evt.payload === "object" ? evt.payload : {};
      return {
        at: Number(evt?.ts_ms || Date.now()),
        kind: "task_transition",
        taskId: String(evt?.step_id || payload?.step_id || "run"),
        role: "context_driver",
        from: "",
        to: String(evt?.status || ""),
        reason: String(payload?.why_next_step || payload?.error || evt?.type || "updated"),
      };
    })
    .slice(-250);
}

function eventsToLogs(events = []) {
  return events
    .map((evt) => {
      const payload = evt?.payload && typeof evt.payload === "object" ? evt.payload : {};
      const details = payload?.error || payload?.why_next_step || "";
      const line = details
        ? `${evt?.type || "event"} ${evt?.status || ""}: ${details}`
        : `${evt?.type || "event"} ${evt?.status || ""}`.trim();
      return {
        at: Number(evt?.ts_ms || Date.now()),
        stream: "event",
        line,
      };
    })
    .slice(-300);
}

async function contextRunSnapshot(session, runId) {
  const [runPayload, eventsPayload, blackboardPayload, replayPayload, patchesPayload] = await Promise.all([
    engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}`),
    engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}/events?tail=300`),
    engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}/blackboard`).catch(() => ({})),
    engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}/replay`).catch(() => ({})),
    engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}/blackboard/patches?tail=300`).catch(
      () => ({})
    ),
  ]);
  const run = runPayload?.run || {};
  const events = Array.isArray(eventsPayload?.events) ? eventsPayload.events : [];
  const blackboardPatches = Array.isArray(patchesPayload?.patches) ? patchesPayload.patches : [];
  const tasks = contextRunToTasks(run);
  const taskMap = Object.fromEntries(tasks.map((task) => [task.taskId, task]));
  return {
    run,
    events,
    blackboard: blackboardPayload?.blackboard || null,
    blackboardPatches,
    replay: replayPayload || null,
    registry: {
      key: "context.run.steps",
      value: {
        version: 1,
        updatedAtMs: Number(run?.updated_at_ms || Date.now()),
        tasks: taskMap,
      },
    },
    reasons: eventsToReasons(events),
    logs: eventsToLogs(events),
  };
}

async function appendContextRunEvent(session, runId, eventType, status, payload = {}, stepId = null) {
  return engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}/events`, {
    method: "POST",
    body: {
      type: eventType,
      status,
      step_id: stepId || undefined,
      payload,
    },
  });
}

function parseObjectiveTodos(objective, max = 6) {
  const lines = String(objective || "")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  const todos = [];
  for (const line of lines) {
    if (todos.length >= max) break;
    const normalized = line.replace(/^[-*#\d\.\)\[\]\s]+/, "").trim();
    if (normalized.length < 8) continue;
    todos.push(normalized);
  }
  if (!todos.length) {
    return ["Draft implementation plan", "Execute implementation", "Validate output"];
  }
  return todos;
}

function isRunTerminal(status) {
  const normalized = String(status || "").trim().toLowerCase();
  return ["completed", "failed", "cancelled"].includes(normalized);
}

async function seedContextRunSteps(session, runId, objective) {
  const todoRows = parseObjectiveTodos(objective, 8).map((content, idx) => ({
    id: `step-${idx + 1}`,
    content,
    status: "pending",
  }));
  await engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}/todos/sync`, {
    method: "POST",
    body: {
      replace: true,
      todos: todoRows,
      source_session_id: null,
      source_run_id: runId,
    },
  });
}

async function createExecutionSession(session, run) {
  const workspaceRoot = String(run?.workspace?.canonical_path || REPO_ROOT).trim();
  const resolved = await resolveExecutionModel(session, run);
  const modelProvider = resolved.provider;
  const modelId = resolved.model;
  swarmState.resolvedModelProvider = modelProvider;
  swarmState.resolvedModelId = modelId;
  swarmState.modelResolutionSource = resolved.source;
  if (swarmState.modelProvider !== modelProvider) swarmState.modelProvider = modelProvider;
  if (swarmState.modelId !== modelId) swarmState.modelId = modelId;
  const payload = await engineRequestJson(session, "/session", {
    method: "POST",
    body: {
      title: `Swarm ${String(run?.run_id || "").trim()}`,
      directory: workspaceRoot,
      workspace_root: workspaceRoot,
      provider: modelProvider || undefined,
      model:
        modelProvider && modelId
          ? {
              providerID: modelProvider,
              modelID: modelId,
            }
          : undefined,
    },
  });
  return String(payload?.id || "").trim();
}

let cachedEngineDefaultModel = {
  provider: "",
  model: "",
  fetchedAtMs: 0,
};

async function fetchEngineDefaultModel(session) {
  const now = Date.now();
  if (cachedEngineDefaultModel.fetchedAtMs && now - cachedEngineDefaultModel.fetchedAtMs < 15000) {
    return { provider: cachedEngineDefaultModel.provider, model: cachedEngineDefaultModel.model };
  }
  const payload = await engineRequestJson(session, "/config/providers").catch(() => ({}));
  const defaultProvider = String(payload?.default || "").trim();
  const providers = payload?.providers && typeof payload.providers === "object" ? payload.providers : {};
  const defaultModel = String(providers?.[defaultProvider]?.default_model || "").trim();
  cachedEngineDefaultModel = {
    provider: defaultProvider,
    model: defaultModel,
    fetchedAtMs: now,
  };
  return { provider: defaultProvider, model: defaultModel };
}

async function resolveExecutionModel(session, run) {
  const runProvider = String(run?.model_provider || "").trim();
  const runModel = String(run?.model_id || "").trim();
  if (runProvider && runModel) {
    return { provider: runProvider, model: runModel, source: "run" };
  }

  const swarmProvider = String(swarmState.modelProvider || "").trim();
  const swarmModel = String(swarmState.modelId || "").trim();
  if (swarmProvider && swarmModel) {
    return { provider: swarmProvider, model: swarmModel, source: "swarm_state" };
  }

  const defaults = await fetchEngineDefaultModel(session);
  if (defaults.provider && defaults.model) {
    return { provider: defaults.provider, model: defaults.model, source: "engine_default" };
  }

  throw new Error("MODEL_SELECTION_REQUIRED: no provider/model configured for swarm execution.");
}

function stepPromptText(run, step, stepIndex, totalSteps) {
  return [
    "Execute this swarm step.",
    "",
    `Objective: ${String(run?.objective || "").trim()}`,
    `Workspace: ${String(run?.workspace?.canonical_path || "").trim()}`,
    `Step (${stepIndex + 1}/${totalSteps}): ${String(step?.title || step?.step_id || "").trim()}`,
    "",
    "Requirements:",
    "- Make the required code/project changes for this step.",
    "- Keep scope limited to this step.",
    "- Return a concise summary of changes and blockers.",
  ].join("\n");
}

async function runStepWithLLM(session, run, step, stepIndex, totalSteps) {
  const sessionId = await createExecutionSession(session, run);
  if (!sessionId) throw new Error("Failed to create execution session.");
  const prompt = stepPromptText(run, step, stepIndex, totalSteps);
  const promptResponse = await engineRequestJson(session, `/session/${encodeURIComponent(sessionId)}/prompt_sync`, {
    method: "POST",
    timeoutMs: 10 * 60 * 1000,
    body: {
      parts: [{ type: "text", text: prompt }],
    },
  });
  const syncRows = Array.isArray(promptResponse) ? promptResponse : [];
  const hasAssistant = syncRows.some((row) => String(row?.info?.role || "").toLowerCase() === "assistant");
  if (!hasAssistant) {
    const sessionSnapshot = await engineRequestJson(session, `/session/${encodeURIComponent(sessionId)}`).catch(
      () => null
    );
    const messages = Array.isArray(sessionSnapshot?.messages) ? sessionSnapshot.messages : [];
    const persistedAssistant = messages.some(
      (message) => String(message?.info?.role || "").toLowerCase() === "assistant"
    );
    if (!persistedAssistant) {
      throw new Error(
        "PROMPT_DISPATCH_EMPTY_RESPONSE: prompt_sync returned no assistant output. Model route may be unresolved."
      );
    }
  }
  return sessionId;
}

const swarmExecutors = new Map();

function findStepByStatus(steps, status) {
  return (Array.isArray(steps) ? steps : []).find(
    (step) => String(step?.status || "").trim().toLowerCase() === status
  );
}

async function ensureStepMarkedDone(session, runId, stepId) {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const payload = await engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}`).catch(
      () => null
    );
    const run = payload?.run;
    const steps = Array.isArray(run?.steps) ? run.steps : [];
    const idx = steps.findIndex((step) => String(step?.step_id || "") === stepId);
    if (idx < 0) return true;
    const current = String(steps[idx]?.status || "").toLowerCase();
    if (current === "done") return true;
    if (attempt < 2) {
      await sleep(120);
      continue;
    }
    const patched = {
      ...run,
      status: "running",
      why_next_step: `reconciled completion for ${stepId}`,
      steps: steps.map((step, i) => (i === idx ? { ...step, status: "done" } : step)),
    };
    await engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}`, {
      method: "PUT",
      body: patched,
    });
    const verify = await engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}`).catch(() => null);
    const verifySteps = Array.isArray(verify?.run?.steps) ? verify.run.steps : [];
    const verifyStep = verifySteps.find((step) => String(step?.step_id || "") === stepId);
    return String(verifyStep?.status || "").toLowerCase() === "done";
  }
  return false;
}

async function driveContextRunExecution(session, runId) {
  if (swarmExecutors.has(runId)) return false;
  swarmState.executorState = "running";
  swarmState.executorReason = "";
  const runner = (async () => {
    let completionStreak = 0;
    let lastCompletedStepId = "";
    for (let cycle = 0; cycle < 24; cycle += 1) {
      const runPayload = await engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}`);
      const run = runPayload?.run || {};
      if (isRunTerminal(run.status)) return;

      const nextPayload = await engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}/driver/next`, {
        method: "POST",
        body: { dry_run: false },
      });
      const selectedStepId = String(nextPayload?.selected_step_id || "").trim();
      const latestRun = nextPayload?.run || run;
      const steps = Array.isArray(latestRun?.steps) ? latestRun.steps : [];
      const inProgressStep = findStepByStatus(steps, "in_progress");
      const executionStepId = selectedStepId || String(inProgressStep?.step_id || "").trim();

      if (!executionStepId) {
        if (steps.length && steps.every((step) => String(step?.status || "").toLowerCase() === "done")) {
          await appendContextRunEvent(session, runId, "run_completed", "completed", {
            why_next_step: "all steps completed",
          });
          swarmState.lastError = "";
          swarmState.executorState = "idle";
          swarmState.executorReason = "run completed";
          return;
        }
        swarmState.lastError = String(nextPayload?.why_next_step || latestRun?.why_next_step || "No actionable step selected.");
        swarmState.executorState = "blocked";
        swarmState.executorReason = swarmState.lastError;
        return;
      }

      const stepIndex = steps.findIndex((step) => String(step?.step_id || "") === executionStepId);
      const step = stepIndex >= 0 ? steps[stepIndex] : { step_id: executionStepId, title: executionStepId };

      try {
        await appendContextRunEvent(
          session,
          runId,
          "step_started",
          "running",
          {
            step_status: "in_progress",
            step_title: String(step?.title || executionStepId),
            why_next_step: selectedStepId
              ? `executing ${executionStepId}`
              : `resuming in_progress step ${executionStepId}`,
          },
          executionStepId
        );
        const sessionId = await runStepWithLLM(session, latestRun, step, Math.max(stepIndex, 0), Math.max(steps.length, 1));
        await appendContextRunEvent(
          session,
          runId,
          "step_completed",
          "running",
          {
            step_status: "done",
            step_title: String(step?.title || executionStepId),
            session_id: sessionId,
            why_next_step: `completed ${executionStepId}`,
          },
          executionStepId
        );
        const refresh = await engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}`).catch(() => null);
        const refreshedSteps = Array.isArray(refresh?.run?.steps) ? refresh.run.steps : [];
        const refreshedStep = refreshedSteps.find((item) => String(item?.step_id || "") === executionStepId);
        const refreshedStatus = String(refreshedStep?.status || "").toLowerCase();
        if (refreshedStatus !== "done") {
          const reconciled = await ensureStepMarkedDone(session, runId, executionStepId);
          if (reconciled) {
            await appendContextRunEvent(
              session,
              runId,
              "step_completion_reconciled",
              "running",
              {
                step_status: "done",
                why_next_step: `reconciled stale step state for ${executionStepId}`,
              },
              executionStepId
            );
          } else {
          throw new Error(
            `STEP_STATE_NOT_ADVANCING: step \`${executionStepId}\` remained \`${refreshedStatus || "unknown"}\` after completion`
          );
          }
        }
        if (executionStepId === lastCompletedStepId) completionStreak += 1;
        else {
          lastCompletedStepId = executionStepId;
          completionStreak = 1;
        }
        if (completionStreak > 2) {
          throw new Error(`STEP_LOOP_GUARD: repeated completion on step \`${executionStepId}\``);
        }
        swarmState.lastError = "";
        swarmState.executorState = "running";
        swarmState.executorReason = "";
      } catch (error) {
        const message = String(error?.message || error || "Unknown step failure");
        swarmState.lastError = message;
        swarmState.executorState = "error";
        swarmState.executorReason = message;
        await appendContextRunEvent(
          session,
          runId,
          "step_failed",
          "failed",
          {
            step_status: "failed",
            error: message,
            why_next_step: `step failed: ${executionStepId}`,
          },
          executionStepId
        );
        return;
      }
    }
  })()
    .catch((error) => {
      swarmState.lastError = String(error?.message || error || "Run executor failed");
      swarmState.executorState = "error";
      swarmState.executorReason = swarmState.lastError;
    })
    .finally(() => {
      swarmExecutors.delete(runId);
      if (swarmState.executorState === "running") {
        swarmState.executorState = "idle";
        swarmState.executorReason = "";
      }
    });
  swarmExecutors.set(runId, runner);
  return true;
}

async function requeueInProgressSteps(session, runId) {
  const payload = await engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}`).catch(() => null);
  const run = payload?.run;
  const steps = Array.isArray(run?.steps) ? run.steps : [];
  const inProgress = steps.filter(
    (step) => String(step?.status || "").trim().toLowerCase() === "in_progress"
  );
  for (const step of inProgress) {
    const stepId = String(step?.step_id || "").trim();
    if (!stepId) continue;
    await appendContextRunEvent(session, runId, "task_retry_requested", "running", {
      why_next_step: `requeued stale in_progress step \`${stepId}\` before continue`,
    }, stepId);
  }
  return inProgress.length;
}

async function startSwarm(session, config = {}) {
  const objective = String(config.objective || "Ship a small feature end-to-end").trim();
  const workspaceRootRaw = String(config.workspaceRoot || REPO_ROOT).trim();
  const workspaceRoot = await workspaceExistsAsDirectory(workspaceRootRaw);
  if (!workspaceRoot) {
    throw new Error(`Workspace root does not exist or is not a directory: ${resolve(workspaceRootRaw)}`);
  }
  const maxTasks = Math.max(1, Number.parseInt(String(config.maxTasks || 3), 10) || 3);
  let modelProvider = String(config.modelProvider || "").trim();
  let modelId = String(config.modelId || "").trim();
  if (!modelProvider || !modelId) {
    const defaults = await fetchEngineDefaultModel(session);
    modelProvider = modelProvider || defaults.provider;
    modelId = modelId || defaults.model;
  }
  if (!modelProvider || !modelId) {
    throw new Error("MODEL_SELECTION_REQUIRED: configure a default provider/model before starting swarm runs.");
  }
  const mcpServers = (Array.isArray(config.mcpServers) ? config.mcpServers : [])
    .map((entry) => String(entry || "").trim())
    .filter(Boolean)
    .slice(0, 64);

  const created = await engineRequestJson(session, "/context/runs", {
    method: "POST",
    body: {
      objective,
      run_type: "interactive",
      source_client: "control_panel",
      model_provider: modelProvider || undefined,
      model_id: modelId || undefined,
      mcp_servers: mcpServers,
      workspace: {
        workspace_id: buildWorkspaceId(workspaceRoot),
        canonical_path: workspaceRoot,
        lease_epoch: 1,
      },
    },
  });
  const run = created?.run;
  const runId = String(run?.run_id || "").trim();
  if (!runId) throw new Error("Context run creation failed (missing run_id).");

  await appendContextRunEvent(session, runId, "planning_started", "planning", {
    source_client: "control_panel",
    max_tasks: maxTasks,
    model_provider: modelProvider || undefined,
    model_id: modelId || undefined,
    mcp_servers: mcpServers,
  });
  await seedContextRunSteps(session, runId, objective);
  await appendContextRunEvent(session, runId, "plan_seeded_local", "planning", {
    source: "local_objective_parser",
    note: "Initial steps were seeded from objective text. No model call yet.",
  });

  swarmState.status = "planning";
  swarmState.startedAt = Date.now();
  swarmState.stoppedAt = null;
  swarmState.objective = objective;
  swarmState.workspaceRoot = workspaceRoot;
  swarmState.maxTasks = maxTasks;
  swarmState.modelProvider = modelProvider;
  swarmState.modelId = modelId;
  swarmState.resolvedModelProvider = modelProvider;
  swarmState.resolvedModelId = modelId;
  swarmState.modelResolutionSource = "start";
  swarmState.mcpServers = mcpServers;
  swarmState.repoRoot = workspaceRoot;
  swarmState.lastError = "";
  swarmState.runId = runId;
  swarmState.attachedPid = null;
  swarmState.registryCache = null;
  setSwarmPreflight({
    gitAvailable: null,
    repoReady: true,
    autoInitialized: false,
    code: "workspace_ready",
    reason: "",
    guidance: "",
  });
  pushSwarmEvent("status", { status: swarmState.status, runId: runId });
  return runId;
}

async function handleSwarmApi(req, res, session) {
  const url = new URL(req.url, `http://127.0.0.1:${PORTAL_PORT}`);
  const pathname = url.pathname;
  const statusFromRun = async (runId) => {
    if (!runId) return null;
    try {
      const payload = await engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}`);
      return payload?.run || null;
    } catch {
      return null;
    }
  };

  if (pathname === "/api/swarm/status" && req.method === "GET") {
    const run = await statusFromRun(swarmState.runId);
    if (run) {
      swarmState.status = contextRunStatusToSwarmStatus(run.status);
      swarmState.objective = String(run.objective || swarmState.objective || "");
      swarmState.workspaceRoot = String(run.workspace?.canonical_path || swarmState.workspaceRoot || REPO_ROOT);
      swarmState.repoRoot = swarmState.workspaceRoot;
    } else if (swarmState.runId) {
      swarmState.status = "idle";
      swarmState.stoppedAt = Date.now();
    }
    sendJson(res, 200, {
      ok: true,
      status: swarmState.status,
      objective: swarmState.objective,
      workspaceRoot: swarmState.workspaceRoot,
      maxTasks: swarmState.maxTasks,
      modelProvider: swarmState.modelProvider || "",
      modelId: swarmState.modelId || "",
      resolvedModelProvider: swarmState.resolvedModelProvider || "",
      resolvedModelId: swarmState.resolvedModelId || "",
      modelResolutionSource: swarmState.modelResolutionSource || "none",
      mcpServers: Array.isArray(swarmState.mcpServers) ? swarmState.mcpServers : [],
      repoRoot: swarmState.repoRoot || "",
      preflight: swarmState.preflight || null,
      startedAt: swarmState.startedAt,
      stoppedAt: swarmState.stoppedAt,
      runId: swarmState.runId || "",
      attachedPid: swarmState.attachedPid || null,
      localEngine: isLocalEngineUrl(ENGINE_URL),
      lastError: swarmState.lastError || null,
      executorState: swarmState.executorState || "idle",
      executorReason: swarmState.executorReason || null,
      currentRunId: swarmState.runId || "",
    });
    return true;
  }

  if (pathname === "/api/swarm/runs" && req.method === "GET") {
    const workspace = String(url.searchParams.get("workspace") || "").trim();
    const query = workspace ? `?workspace=${encodeURIComponent(resolve(workspace))}&limit=100` : "?limit=100";
    const payload = await engineRequestJson(session, `/context/runs${query}`).catch(() => ({ runs: [] }));
    const includeHidden = String(url.searchParams.get("include_hidden") || "").trim() === "1";
    const hiddenRunIds = await loadHiddenSwarmRunIds();
    const allRuns = Array.isArray(payload?.runs) ? payload.runs : [];
    const runs = includeHidden
      ? allRuns
      : allRuns.filter((run) => !hiddenRunIds.has(String(run?.run_id || "").trim()));
    const active = runs.filter((run) => {
      const status = String(run?.status || "").toLowerCase();
      return !["completed", "failed", "cancelled"].includes(status);
    });
    sendJson(res, 200, {
      ok: true,
      runs,
      active,
      recent: runs.slice(0, 30),
      hiddenCount: hiddenRunIds.size,
    });
    return true;
  }

  if (pathname === "/api/swarm/runs/hide" && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      const runIds = (Array.isArray(body?.runIds) ? body.runIds : [])
        .map((id) => String(id || "").trim())
        .filter(Boolean)
        .slice(0, 500);
      if (!runIds.length) throw new Error("Missing runIds");
      const hidden = await loadHiddenSwarmRunIds();
      for (const runId of runIds) hidden.add(runId);
      await saveHiddenSwarmRunIds(hidden);
      if (runIds.includes(String(swarmState.runId || "").trim())) {
        swarmState.runId = "";
      }
      sendJson(res, 200, { ok: true, hiddenCount: hidden.size, hiddenRunIds: runIds });
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname === "/api/swarm/runs/unhide" && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      const runIds = (Array.isArray(body?.runIds) ? body.runIds : [])
        .map((id) => String(id || "").trim())
        .filter(Boolean)
        .slice(0, 500);
      if (!runIds.length) throw new Error("Missing runIds");
      const hidden = await loadHiddenSwarmRunIds();
      for (const runId of runIds) hidden.delete(runId);
      await saveHiddenSwarmRunIds(hidden);
      sendJson(res, 200, { ok: true, hiddenCount: hidden.size, unhiddenRunIds: runIds });
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname === "/api/swarm/runs/hide_completed" && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      const workspace = String(body?.workspace || "").trim();
      const query = workspace ? `?workspace=${encodeURIComponent(resolve(workspace))}&limit=1000` : "?limit=1000";
      const payload = await engineRequestJson(session, `/context/runs${query}`).catch(() => ({ runs: [] }));
      const allRuns = Array.isArray(payload?.runs) ? payload.runs : [];
      const completedRunIds = allRuns
        .filter((run) => {
          const status = String(run?.status || "").toLowerCase();
          return ["completed", "failed", "cancelled"].includes(status);
        })
        .map((run) => String(run?.run_id || "").trim())
        .filter(Boolean);
      const hidden = await loadHiddenSwarmRunIds();
      for (const runId of completedRunIds) hidden.add(runId);
      await saveHiddenSwarmRunIds(hidden);
      if (completedRunIds.includes(String(swarmState.runId || "").trim())) {
        swarmState.runId = "";
      }
      sendJson(res, 200, { ok: true, hiddenCount: hidden.size, hiddenNow: completedRunIds.length });
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname === "/api/swarm/start" && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      const runId = await startSwarm(session, body || {});
      sendJson(res, 200, { ok: true, runId });
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname === "/api/swarm/approve" && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      const runId = String(body?.runId || swarmState.runId || "").trim();
      if (!runId) throw new Error("Missing runId");
      await appendContextRunEvent(session, runId, "plan_approved", "running", {});
      void driveContextRunExecution(session, runId);
      sendJson(res, 200, { ok: true, runId });
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname === "/api/swarm/pause" && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      const runId = String(body?.runId || swarmState.runId || "").trim();
      if (!runId) throw new Error("Missing runId");
      await appendContextRunEvent(session, runId, "run_paused", "paused", {});
      sendJson(res, 200, { ok: true, runId });
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname === "/api/swarm/resume" && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      const runId = String(body?.runId || swarmState.runId || "").trim();
      if (!runId) throw new Error("Missing runId");
      await appendContextRunEvent(session, runId, "run_resumed", "running", {});
      const requeued = await requeueInProgressSteps(session, runId);
      const started = await driveContextRunExecution(session, runId);
      const preview = await engineRequestJson(
        session,
        `/context/runs/${encodeURIComponent(runId)}/driver/next`,
        { method: "POST", body: { dry_run: true } }
      ).catch(() => null);
      sendJson(res, 200, {
        ok: true,
        runId,
        started,
        requeued,
        sessionDispatchOutcome: started ? "started" : "already_running",
        selectedStepId: preview?.selected_step_id || null,
        whyNextStep: preview?.why_next_step || null,
        executorState: swarmState.executorState || "idle",
        executorReason: swarmState.executorReason || null,
      });
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname === "/api/swarm/continue" && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      const runId = String(body?.runId || swarmState.runId || "").trim();
      if (!runId) throw new Error("Missing runId");
      await appendContextRunEvent(session, runId, "run_resumed", "running", {
        why_next_step: "manual continue requested",
      });
      const requeued = await requeueInProgressSteps(session, runId);
      const started = await driveContextRunExecution(session, runId);
      const preview = await engineRequestJson(
        session,
        `/context/runs/${encodeURIComponent(runId)}/driver/next`,
        { method: "POST", body: { dry_run: true } }
      ).catch(() => null);
      sendJson(res, 200, {
        ok: true,
        runId,
        started,
        requeued,
        sessionDispatchOutcome: started ? "started" : "already_running",
        selectedStepId: preview?.selected_step_id || null,
        whyNextStep: preview?.why_next_step || null,
        executorState: swarmState.executorState || "idle",
        executorReason: swarmState.executorReason || null,
      });
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if ((pathname === "/api/swarm/cancel" || pathname === "/api/swarm/stop") && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      const runId = String(body?.runId || swarmState.runId || "").trim();
      if (!runId) throw new Error("Missing runId");
      await appendContextRunEvent(session, runId, "run_cancelled", "cancelled", {});
      if (swarmState.runId === runId) {
        swarmState.status = "cancelled";
        swarmState.stoppedAt = Date.now();
      }
      sendJson(res, 200, { ok: true, runId });
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname === "/api/swarm/retry" && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      const runId = String(body?.runId || swarmState.runId || "").trim();
      const stepId = String(body?.stepId || "").trim();
      if (!runId || !stepId) throw new Error("Missing runId or stepId");
      await appendContextRunEvent(session, runId, "task_retry_requested", "running", {}, stepId);
      sendJson(res, 200, { ok: true, runId, stepId });
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname === "/api/swarm/tasks/create" && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      const runId = String(body?.runId || swarmState.runId || "").trim();
      const tasks = Array.isArray(body?.tasks) ? body.tasks : [];
      if (!runId || !tasks.length) throw new Error("Missing runId or tasks");
      const payload = await engineRequestJson(
        session,
        `/context/runs/${encodeURIComponent(runId)}/tasks`,
        {
          method: "POST",
          body: { tasks },
        }
      );
      sendJson(res, 200, payload);
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname === "/api/swarm/tasks/claim" && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      const runId = String(body?.runId || swarmState.runId || "").trim();
      if (!runId) throw new Error("Missing runId");
      const claimBody = {
        agent_id: String(body?.agentId || "control_panel").trim(),
        command_id: body?.commandId || undefined,
        task_type: body?.taskType || undefined,
        workflow_id: body?.workflowId || undefined,
        lease_ms: Number(body?.leaseMs || 30000),
      };
      const payload = await engineRequestJson(
        session,
        `/context/runs/${encodeURIComponent(runId)}/tasks/claim`,
        {
          method: "POST",
          body: claimBody,
        }
      );
      sendJson(res, 200, payload);
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname === "/api/swarm/tasks/transition" && req.method === "POST") {
    try {
      const body = await readJsonBody(req);
      const runId = String(body?.runId || swarmState.runId || "").trim();
      const taskId = String(body?.taskId || "").trim();
      if (!runId || !taskId) throw new Error("Missing runId or taskId");
      const transitionBody = {
        action: body?.action || "status",
        command_id: body?.commandId || undefined,
        expected_task_rev: body?.expectedTaskRev ?? undefined,
        lease_token: body?.leaseToken || undefined,
        agent_id: body?.agentId || undefined,
        status: body?.status || undefined,
        error: body?.error || undefined,
        lease_ms: body?.leaseMs || undefined,
      };
      const payload = await engineRequestJson(
        session,
        `/context/runs/${encodeURIComponent(runId)}/tasks/${encodeURIComponent(taskId)}/transition`,
        {
          method: "POST",
          body: transitionBody,
        }
      );
      sendJson(res, 200, payload);
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname.startsWith("/api/swarm/run/") && req.method === "GET") {
    const runId = decodeURIComponent(pathname.replace("/api/swarm/run/", "").trim());
    if (!runId) {
      sendJson(res, 400, { ok: false, error: "Missing run id." });
      return true;
    }
    try {
      const snapshot = await contextRunSnapshot(session, runId);
      swarmState.runId = runId;
      swarmState.status = contextRunStatusToSwarmStatus(snapshot.run?.status);
      sendJson(res, 200, {
        ok: true,
        run: snapshot.run,
        events: snapshot.events,
        blackboard: snapshot.blackboard,
        blackboardPatches: snapshot.blackboardPatches,
        replay: snapshot.replay,
        tasks: contextRunToTasks(snapshot.run),
      });
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname === "/api/swarm/snapshot" && req.method === "GET") {
    const runId = String(url.searchParams.get("runId") || swarmState.runId || "").trim();
    if (!runId) {
      sendJson(res, 200, {
        ok: true,
        status: "idle",
        registry: { key: "context.run.steps", value: { version: 1, updatedAtMs: Date.now(), tasks: {} } },
        logs: [],
        reasons: [],
        startedAt: swarmState.startedAt,
        stoppedAt: swarmState.stoppedAt,
        lastError: swarmState.lastError || null,
        localEngine: isLocalEngineUrl(ENGINE_URL),
        runId: "",
      });
      return true;
    }
    try {
      const snapshot = await contextRunSnapshot(session, runId);
      swarmState.registryCache = snapshot.registry;
      swarmState.logs = snapshot.logs;
      swarmState.reasons = snapshot.reasons;
      swarmState.status = contextRunStatusToSwarmStatus(snapshot.run?.status);
      sendJson(res, 200, {
        ok: true,
        status: swarmState.status,
        runId,
        registry: snapshot.registry,
        logs: snapshot.logs,
        reasons: snapshot.reasons,
        startedAt: Number(snapshot.run?.started_at_ms || swarmState.startedAt || Date.now()),
        stoppedAt: ["completed", "failed", "cancelled"].includes(String(snapshot.run?.status || ""))
          ? Number(snapshot.run?.ended_at_ms || Date.now())
          : null,
        lastError: String(snapshot.run?.last_error || ""),
        localEngine: isLocalEngineUrl(ENGINE_URL),
      });
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
    }
    return true;
  }

  if (pathname === "/api/swarm/events" && req.method === "GET") {
    res.writeHead(200, {
      "content-type": "text/event-stream",
      "cache-control": "no-cache",
      connection: "keep-alive",
    });
    const runId = String(url.searchParams.get("runId") || swarmState.runId || "").trim();
    let closed = false;
    let sinceSeq = 0;
    let sincePatchSeq = 0;
    const close = () => {
      closed = true;
    };
    req.on("close", close);
    res.write(
      `data: ${JSON.stringify({ kind: "hello", ts: Date.now(), status: swarmState.status, runId })}\n\n`
    );
    const tick = async () => {
      if (closed || !runId) return;
      try {
        const [eventsPayload, patchesPayload] = await Promise.all([
          engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}/events?since_seq=${sinceSeq}`),
          engineRequestJson(
            session,
            `/context/runs/${encodeURIComponent(runId)}/blackboard/patches?since_seq=${sincePatchSeq}`
          ).catch(() => ({ patches: [] })),
        ]);
        const events = Array.isArray(eventsPayload?.events) ? eventsPayload.events : [];
        for (const event of events) {
          sinceSeq = Math.max(sinceSeq, Number(event?.seq || 0));
          res.write(`data: ${JSON.stringify({ kind: "event", ts: Date.now(), runId, event })}\n\n`);
        }
        const patches = Array.isArray(patchesPayload?.patches) ? patchesPayload.patches : [];
        for (const patch of patches) {
          sincePatchSeq = Math.max(sincePatchSeq, Number(patch?.seq || 0));
          res.write(
            `data: ${JSON.stringify({ kind: "blackboard_patch", ts: Date.now(), runId, patch })}\n\n`
          );
        }
      } catch {
        // ignore transient poll failures
      }
    };
    const interval = setInterval(tick, 1500);
    tick();
    req.on("close", () => clearInterval(interval));
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
  if (installServicesRequested) {
    await installServices();
    if (serviceSetupOnly) {
      return;
    }
  }

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
      if (!res.headersSent && !res.writableEnded && !res.destroyed) {
        sendJson(res, 500, { ok: false, error: "Internal server error" });
      } else if (!res.destroyed) {
        res.destroy(e instanceof Error ? e : undefined);
      }
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
