#!/usr/bin/env node

import { spawn } from "child_process";
import { createServer } from "http";
import { readFileSync, existsSync, createReadStream, createWriteStream } from "fs";
import { mkdir, readdir, stat, rm, readFile, writeFile } from "fs/promises";
import { createHash, randomBytes } from "crypto";
import { join, dirname, extname, normalize, resolve, basename, relative } from "path";
import { Transform } from "stream";
import { pipeline } from "stream/promises";
import { fileURLToPath } from "url";
import { createRequire } from "module";
import { homedir } from "os";
import { ensureEnv } from "./init-env.js";
import { createSwarmApiHandler } from "../server/routes/swarm.js";

function parseDotEnv(content) {
  const out = {};
  for (const raw of String(content || "").split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const idx = line.indexOf("=");
    if (idx <= 0) continue;
    const key = line.slice(0, idx).trim();
    let value = line.slice(idx + 1).trim();
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
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
const serviceOpRaw = String(cli.value("service-op") || "")
  .trim()
  .toLowerCase();
const serviceOp = ["status", "start", "stop", "restart", "enable", "disable", "logs"].includes(
  serviceOpRaw
)
  ? serviceOpRaw
  : "";
const serviceModeRaw = String(cli.value("service-mode") || "both")
  .trim()
  .toLowerCase();
const serviceMode = ["both", "engine", "panel"].includes(serviceModeRaw) ? serviceModeRaw : "both";
const serviceUserArg = String(cli.value("service-user") || "").trim();
const serviceSetupOnly =
  rawArgs.length > 0 &&
  rawArgs.every((arg) => {
    if (arg === "--install-services") return true;
    if (arg.startsWith("--service-op")) return true;
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
  if (
    process.argv.slice(2).length === 1 ||
    (process.argv.slice(2).length === 2 && resetTokenRequested)
  ) {
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
const ENGINE_URL = (
  process.env.TANDEM_ENGINE_URL || `http://${ENGINE_HOST}:${ENGINE_PORT}`
).replace(/\/+$/, "");
const SWARM_RUNS_PATH = resolve(homedir(), ".tandem", "control-panel", "swarm-runs.json");
const SWARM_HIDDEN_RUNS_PATH = resolve(
  homedir(),
  ".tandem",
  "control-panel",
  "swarm-hidden-runs.json"
);
const AUTO_START_ENGINE = (process.env.TANDEM_CONTROL_PANEL_AUTO_START_ENGINE || "1") !== "0";
const CONFIGURED_ENGINE_TOKEN = (
  process.env.TANDEM_CONTROL_PANEL_ENGINE_TOKEN ||
  process.env.TANDEM_API_TOKEN ||
  ""
).trim();
const SESSION_TTL_MS =
  Number.parseInt(process.env.TANDEM_CONTROL_PANEL_SESSION_TTL_MINUTES || "1440", 10) * 60 * 1000;
const FILES_ROOT = resolve(
  process.env.TANDEM_CONTROL_PANEL_FILES_ROOT || resolveDefaultChannelUploadsRoot()
);
const FILES_SCOPE = String(process.env.TANDEM_CONTROL_PANEL_FILES_SCOPE || "control-panel")
  .trim()
  .replace(/\\/g, "/")
  .replace(/^\/+/, "")
  .replace(/\/+$/, "");
const MAX_UPLOAD_BYTES = Math.max(
  1,
  Number.parseInt(
    process.env.TANDEM_CONTROL_PANEL_MAX_UPLOAD_BYTES || `${250 * 1024 * 1024}`,
    10
  ) || 250 * 1024 * 1024
);
const require = createRequire(import.meta.url);
const SETUP_ENTRYPOINT = fileURLToPath(import.meta.url);
const CONTROL_PANEL_PACKAGE = (() => {
  try {
    return JSON.parse(readFileSync(join(__dirname, "..", "package.json"), "utf8"));
  } catch {
    return {};
  }
})();
const CONTROL_PANEL_VERSION = String(CONTROL_PANEL_PACKAGE?.version || "0.0.0").trim() || "0.0.0";
const CONTROL_PANEL_BUILD_FINGERPRINT = (() => {
  try {
    const source = readFileSync(SETUP_ENTRYPOINT);
    const digest = createHash("sha1").update(source).digest("hex").slice(0, 8);
    return `${CONTROL_PANEL_VERSION}-${digest}`;
  } catch {
    return `${CONTROL_PANEL_VERSION}-unknown`;
  }
})();

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
  maxAgents: 3,
  workflowId: "swarm.blackboard.default",
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
  executorMode: "context_steps",
  resolvedModelProvider: "",
  resolvedModelId: "",
  modelResolutionSource: "none",
  verificationMode: "strict",
  runId: "",
  attachedPid: null,
  buildVersion: CONTROL_PANEL_VERSION,
  buildFingerprint: CONTROL_PANEL_BUILD_FINGERPRINT,
  buildStartedAt: Date.now(),
};
const swarmRunControllers = new Map();
const swarmSseClients = new Set();

function createSwarmRunController(runId = "", overrides = {}) {
  return {
    status: "idle",
    startedAt: null,
    stoppedAt: null,
    objective: "",
    workspaceRoot: REPO_ROOT,
    maxTasks: 3,
    maxAgents: 3,
    workflowId: "swarm.blackboard.default",
    modelProvider: "",
    modelId: "",
    mcpServers: [],
    repoRoot: "",
    lastError: "",
    executorState: "idle",
    executorReason: "",
    executorMode: "context_steps",
    resolvedModelProvider: "",
    resolvedModelId: "",
    modelResolutionSource: "none",
    verificationMode: "strict",
    runId: String(runId || "").trim(),
    attachedPid: null,
    registryCache: null,
    reasons: [],
    logs: [],
    ...overrides,
  };
}

function syncLegacySwarmState(controller) {
  if (!controller || typeof controller !== "object") return;
  swarmState.status = controller.status || swarmState.status;
  swarmState.startedAt = controller.startedAt ?? swarmState.startedAt;
  swarmState.stoppedAt = controller.stoppedAt ?? swarmState.stoppedAt;
  swarmState.objective = controller.objective || swarmState.objective;
  swarmState.workspaceRoot = controller.workspaceRoot || swarmState.workspaceRoot;
  swarmState.maxTasks = controller.maxTasks ?? swarmState.maxTasks;
  swarmState.maxAgents = controller.maxAgents ?? swarmState.maxAgents;
  swarmState.workflowId = controller.workflowId || swarmState.workflowId;
  swarmState.modelProvider = controller.modelProvider || swarmState.modelProvider;
  swarmState.modelId = controller.modelId || swarmState.modelId;
  swarmState.mcpServers = Array.isArray(controller.mcpServers)
    ? controller.mcpServers
    : swarmState.mcpServers;
  swarmState.repoRoot = controller.repoRoot || swarmState.repoRoot;
  swarmState.lastError = controller.lastError || "";
  swarmState.executorState = controller.executorState || swarmState.executorState;
  swarmState.executorReason = controller.executorReason || "";
  swarmState.executorMode = controller.executorMode || swarmState.executorMode;
  swarmState.resolvedModelProvider =
    controller.resolvedModelProvider || swarmState.resolvedModelProvider;
  swarmState.resolvedModelId = controller.resolvedModelId || swarmState.resolvedModelId;
  swarmState.modelResolutionSource =
    controller.modelResolutionSource || swarmState.modelResolutionSource;
  swarmState.verificationMode = controller.verificationMode || swarmState.verificationMode;
  swarmState.runId = controller.runId || swarmState.runId;
  swarmState.attachedPid = controller.attachedPid || null;
  swarmState.registryCache = controller.registryCache || null;
}

function getSwarmRunController(runId = "") {
  const key = String(runId || "").trim();
  if (!key) return null;
  return swarmRunControllers.get(key) || null;
}

function upsertSwarmRunController(runId = "", patch = {}) {
  const key = String(runId || "").trim();
  if (!key) return null;
  const current = getSwarmRunController(key) || createSwarmRunController(key);
  const next = {
    ...current,
    ...patch,
    runId: key,
  };
  swarmRunControllers.set(key, next);
  if (String(swarmState.runId || "").trim() === key) {
    syncLegacySwarmState(next);
  }
  return next;
}

function setActiveSwarmRunId(runId = "") {
  const key = String(runId || "").trim();
  swarmState.runId = key;
  if (!key) {
    swarmState.status = "idle";
    swarmState.executorState = "idle";
    swarmState.executorReason = "";
    swarmState.lastError = "";
    swarmState.attachedPid = null;
    swarmState.registryCache = null;
    return;
  }
  const controller = getSwarmRunController(key);
  if (controller) syncLegacySwarmState(controller);
}

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
  const payload = JSON.stringify(
    { version: 1, updatedAtMs: Date.now(), runs: runs.slice(-100) },
    null,
    2
  );
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
    await writeFile(ignorePath, "node_modules/\n.DS_Store\n.swarm/worktrees/\n", "utf8");
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
      reason: `Selected directory is not a Git repository and is not empty: ${normalized}. Choose an existing repo or an empty directory.${detail}`,
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

  const serviceUser =
    serviceUserArg || String(process.env.SUDO_USER || process.env.USER || "root").trim();
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
    (existsSync(engineEnvPath)
      ? parseDotEnv(readFileSync(engineEnvPath, "utf8")).TANDEM_API_TOKEN || ""
      : "") ||
    `tk_${randomBytes(16).toString("hex")}`;

  await mkdir("/etc/tandem", { recursive: true });
  await mkdir(stateDir, { recursive: true });
  try {
    await runCmd("chown", ["-R", `${serviceUser}:${serviceGroup}`, stateDir]);
  } catch (e) {
    log(`Warning: could not chown ${stateDir} to ${serviceUser}:${serviceGroup}: ${e.message}`);
  }

  const existingEngineEnv = existsSync(engineEnvPath)
    ? parseDotEnv(readFileSync(engineEnvPath, "utf8"))
    : {};
  const engineEnv = {
    ...existingEngineEnv,
    TANDEM_API_TOKEN: token,
    TANDEM_STATE_DIR: stateDir,
    TANDEM_MEMORY_DB_PATH: existingEngineEnv.TANDEM_MEMORY_DB_PATH || `${stateDir}/memory.sqlite`,
    TANDEM_ENABLE_GLOBAL_MEMORY: existingEngineEnv.TANDEM_ENABLE_GLOBAL_MEMORY || "1",
    TANDEM_DISABLE_TOOL_GUARD_BUDGETS: existingEngineEnv.TANDEM_DISABLE_TOOL_GUARD_BUDGETS || "1",
    TANDEM_TOOL_ROUTER_ENABLED: existingEngineEnv.TANDEM_TOOL_ROUTER_ENABLED || "0",
    TANDEM_PROMPT_CONTEXT_HOOK_TIMEOUT_MS:
      existingEngineEnv.TANDEM_PROMPT_CONTEXT_HOOK_TIMEOUT_MS || "5000",
    TANDEM_PROVIDER_STREAM_CONNECT_TIMEOUT_MS:
      existingEngineEnv.TANDEM_PROVIDER_STREAM_CONNECT_TIMEOUT_MS || "30000",
    TANDEM_PROVIDER_STREAM_IDLE_TIMEOUT_MS:
      existingEngineEnv.TANDEM_PROVIDER_STREAM_IDLE_TIMEOUT_MS || "90000",
    TANDEM_PERMISSION_WAIT_TIMEOUT_MS:
      existingEngineEnv.TANDEM_PERMISSION_WAIT_TIMEOUT_MS || "15000",
    TANDEM_TOOL_EXEC_TIMEOUT_MS: existingEngineEnv.TANDEM_TOOL_EXEC_TIMEOUT_MS || "45000",
    TANDEM_BASH_TIMEOUT_MS: existingEngineEnv.TANDEM_BASH_TIMEOUT_MS || "30000",
  };
  const engineEnvBody = Object.entries(engineEnv)
    .map(([k, v]) => `${k}=${v}`)
    .join("\n");
  await writeFile(engineEnvPath, `${engineEnvBody}\n`, "utf8");
  await runCmd("chmod", ["640", engineEnvPath]);

  const panelAutoStart = serviceMode === "panel" ? "1" : "0";
  const existingPanelEnv = existsSync(panelEnvPath)
    ? parseDotEnv(readFileSync(panelEnvPath, "utf8"))
    : {};
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
    await runCmd("systemctl", ["enable", "--now", `${engineServiceName}.service`], {
      stdio: "inherit",
    });
  }
  if (installPanel) {
    await runCmd("systemctl", ["enable", "--now", `${panelServiceName}.service`], {
      stdio: "inherit",
    });
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

function selectedServiceUnits(mode) {
  const normalized = String(mode || "both")
    .trim()
    .toLowerCase();
  if (normalized === "engine") return ["tandem-engine.service"];
  if (normalized === "panel") return ["tandem-control-panel.service"];
  return ["tandem-engine.service", "tandem-control-panel.service"];
}

async function operateServices(operation, mode) {
  const op = String(operation || "")
    .trim()
    .toLowerCase();
  if (!op) return;
  if (process.platform !== "linux") {
    throw new Error("--service-op currently supports Linux/systemd only.");
  }
  const units = selectedServiceUnits(mode);
  if (op === "logs") {
    const args = units
      .flatMap((unit) => ["-u", unit])
      .concat(["-n", "120", "-f", "-o", "short-iso"]);
    await runCmd("journalctl", args, { stdio: "inherit" });
    return;
  }
  if (op === "status") {
    await runCmd("systemctl", ["--no-pager", "--full", "status", ...units], { stdio: "inherit" });
    return;
  }
  if (!["start", "stop", "restart", "enable", "disable"].includes(op)) {
    throw new Error(
      "Invalid --service-op. Expected one of: status,start,stop,restart,enable,disable,logs"
    );
  }
  for (const unit of units) {
    await runCmd("systemctl", [op, unit], { stdio: "inherit" });
  }
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
  const lines = String(text || "")
    .split(/\r?\n/)
    .filter(Boolean);
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
      log(
        "Use the existing engine's token, or stop that engine to let control panel start one with your configured token."
      );
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
    [
      engineEntrypoint,
      "serve",
      "--hostname",
      url.hostname,
      "--port",
      String(url.port || ENGINE_PORT),
    ],
    {
      env: {
        ...process.env,
        TANDEM_API_TOKEN: managedEngineToken,
        TANDEM_DISABLE_TOOL_GUARD_BUDGETS: process.env.TANDEM_DISABLE_TOOL_GUARD_BUDGETS || "1",
        TANDEM_TOOL_ROUTER_ENABLED: process.env.TANDEM_TOOL_ROUTER_ENABLED || "0",
        TANDEM_PROMPT_CONTEXT_HOOK_TIMEOUT_MS:
          process.env.TANDEM_PROMPT_CONTEXT_HOOK_TIMEOUT_MS || "5000",
        TANDEM_PROVIDER_STREAM_CONNECT_TIMEOUT_MS:
          process.env.TANDEM_PROVIDER_STREAM_CONNECT_TIMEOUT_MS || "30000",
        TANDEM_PROVIDER_STREAM_IDLE_TIMEOUT_MS:
          process.env.TANDEM_PROVIDER_STREAM_IDLE_TIMEOUT_MS || "90000",
        TANDEM_PERMISSION_WAIT_TIMEOUT_MS: process.env.TANDEM_PERMISSION_WAIT_TIMEOUT_MS || "15000",
        TANDEM_TOOL_EXEC_TIMEOUT_MS: process.env.TANDEM_TOOL_EXEC_TIMEOUT_MS || "45000",
        TANDEM_BASH_TIMEOUT_MS: process.env.TANDEM_BASH_TIMEOUT_MS || "30000",
      },
      stdio: "inherit",
    }
  );
  log(`Engine API token for this process: ${managedEngineToken}`);
  if (!CONFIGURED_ENGINE_TOKEN) {
    log(
      "Token was auto-generated. Set TANDEM_CONTROL_PANEL_ENGINE_TOKEN (or TANDEM_API_TOKEN) to keep it stable."
    );
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
  if (FILES_SCOPE && normalized !== FILES_SCOPE && !normalized.startsWith(`${FILES_SCOPE}/`))
    return null;
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
      engine: {
        url: ENGINE_URL,
        version: health.version || "unknown",
        local: isLocalEngineUrl(ENGINE_URL),
      },
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
    sendJson(res, 502, {
      ok: false,
      error: `Engine unreachable: ${e instanceof Error ? e.message : String(e)}`,
    });
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
  switch (
    String(status || "")
      .trim()
      .toLowerCase()
  ) {
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
  const normalized = String(status || "")
    .trim()
    .toLowerCase();
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
  const digest = createHash("sha1")
    .update(String(workspaceRoot || ""))
    .digest("hex");
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
  const [runPayload, eventsPayload, blackboardPayload, replayPayload, patchesPayload] =
    await Promise.all([
      engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}`),
      engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}/events?tail=300`),
      engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}/blackboard`).catch(
        () => ({})
      ),
      engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}/replay`).catch(
        () => ({})
      ),
      engineRequestJson(
        session,
        `/context/runs/${encodeURIComponent(runId)}/blackboard/patches?tail=300`
      ).catch(() => ({})),
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

async function appendContextRunEvent(
  session,
  runId,
  eventType,
  status,
  payload = {},
  stepId = null
) {
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
  const compact = String(objective || "")
    .split(/\r?\n/)
    .map((line) => line.replace(/^[-*#\d\.\)\[\]\s]+/, "").trim())
    .filter(Boolean)
    .join(" ");
  const normalized = compact.replace(/\s+/g, " ").trim();
  if (!normalized) return ["Execute requested objective"];
  const maxChars = Math.max(80, Number(max || 6) * 80);
  const content =
    normalized.length > maxChars ? `${normalized.slice(0, maxChars).trimEnd()}...` : normalized;
  return [content];
}

function textFromMessageParts(parts) {
  if (!Array.isArray(parts)) return "";
  return parts
    .map((part) => {
      if (!part) return "";
      if (typeof part === "string") return part;
      if (typeof part.text === "string") return part.text;
      if (typeof part.delta === "string") return part.delta;
      if (typeof part.content === "string") return part.content;
      return "";
    })
    .filter(Boolean)
    .join("\n")
    .trim();
}

function roleOfMessage(row) {
  return String(
    row?.info?.role || row?.role || row?.message_role || row?.type || row?.author || "assistant"
  )
    .trim()
    .toLowerCase();
}

function textOfMessage(row) {
  const fromParts = textFromMessageParts(row?.parts);
  if (fromParts) return fromParts;
  const direct = [row?.content, row?.text, row?.message, row?.delta, row?.body].find(
    (value) => typeof value === "string" && value.trim().length > 0
  );
  if (typeof direct === "string") return direct.trim();
  if (Array.isArray(row?.content)) {
    return row.content
      .map((chunk) => {
        if (!chunk) return "";
        if (typeof chunk === "string") return chunk;
        if (typeof chunk?.text === "string") return chunk.text;
        if (typeof chunk?.content === "string") return chunk.content;
        return "";
      })
      .filter(Boolean)
      .join("\n")
      .trim();
  }
  return "";
}

function extractAssistantText(rows) {
  const list = Array.isArray(rows) ? rows : [];
  for (let i = list.length - 1; i >= 0; i -= 1) {
    if (roleOfMessage(list[i]) !== "assistant") continue;
    const text = textOfMessage(list[i]);
    if (text) return text;
  }
  return "";
}

function normalizePlannerTasks(rawTasks, maxTasks = 8, options = {}) {
  const normalizedMax = Math.max(1, Number(maxTasks) || 8);
  const linearFallback = options?.linearFallback === true;
  const candidates = Array.isArray(rawTasks) ? rawTasks : [];
  const provisional = candidates
    .map((row, index) => {
      if (typeof row === "string") {
        const title = String(row || "").trim();
        if (title.length < 6) return null;
        return {
          id: `task-${index + 1}`,
          title,
          dependsOnTaskIds: [],
        };
      }
      if (!row || typeof row !== "object") return null;
      const title = String(row?.title || row?.task || row?.content || "").trim();
      if (title.length < 6) return null;
      const rawId = String(row?.id || row?.task_id || row?.taskId || `task-${index + 1}`).trim();
      const id = rawId || `task-${index + 1}`;
      const dependencySource = Array.isArray(row?.depends_on_task_ids)
        ? row.depends_on_task_ids
        : Array.isArray(row?.dependsOnTaskIds)
          ? row.dependsOnTaskIds
          : Array.isArray(row?.dependsOn)
            ? row.dependsOn
            : Array.isArray(row?.dependencies)
              ? row.dependencies
              : [];
      const dependsOnTaskIds = dependencySource
        .map((dep) => String(dep || "").trim())
        .filter(Boolean);
      return {
        id,
        title,
        dependsOnTaskIds,
      };
    })
    .filter(Boolean)
    .slice(0, normalizedMax);
  const withUniqueIds = [];
  const idCounts = new Map();
  for (const row of provisional) {
    const base = String(row?.id || "task").trim() || "task";
    const count = Number(idCounts.get(base) || 0) + 1;
    idCounts.set(base, count);
    const uniqueId = count > 1 ? `${base}-${count}` : base;
    withUniqueIds.push({
      id: uniqueId,
      title: String(row?.title || "").trim(),
      dependsOnTaskIds: Array.isArray(row?.dependsOnTaskIds) ? row.dependsOnTaskIds : [],
    });
  }
  const knownIds = new Set(withUniqueIds.map((row) => row.id));
  return withUniqueIds.map((row, index) => {
    let dependsOnTaskIds = (Array.isArray(row?.dependsOnTaskIds) ? row.dependsOnTaskIds : [])
      .map((dep) => String(dep || "").trim())
      .filter((dep) => dep && dep !== row.id && knownIds.has(dep));
    if (!dependsOnTaskIds.length && linearFallback && index > 0) {
      dependsOnTaskIds = [withUniqueIds[index - 1].id];
    }
    return {
      id: row.id,
      title: row.title,
      dependsOnTaskIds,
    };
  });
}

function parsePlanTasksFromAssistant(assistantText, maxTasks = 8) {
  const normalizedMax = Math.max(1, Number(maxTasks) || 8);
  const fencedJson = String(assistantText || "").match(/```(?:json)?\s*([\s\S]*?)```/i);
  const candidateJson = (fencedJson?.[1] || String(assistantText || "")).trim();
  const parsedTodos = (() => {
    try {
      const payload = JSON.parse(candidateJson);
      if (Array.isArray(payload)) return payload;
      if (Array.isArray(payload?.tasks)) return payload.tasks;
      if (Array.isArray(payload?.plan)) return payload.plan;
      if (Array.isArray(payload?.steps)) return payload.steps;
      if (Array.isArray(payload?.items)) return payload.items;
      return [];
    } catch {
      return [];
    }
  })();
  const fromJson = normalizePlannerTasks(parsedTodos, normalizedMax, { linearFallback: false });
  if (fromJson.length) return fromJson;
  const fromText = String(assistantText || "")
    .split(/\r?\n/)
    .map((line) => line.replace(/^[-*#\d\.\)\[\]\s]+/, "").trim())
    .filter((line) => line.length >= 6)
    .slice(0, normalizedMax);
  return normalizePlannerTasks(fromText, normalizedMax, { linearFallback: true });
}

async function generatePlanTodosWithLLM(session, run, maxTasks) {
  const runId = String(run?.run_id || "").trim();
  if (!runId) throw new Error("Missing run id for plan generation.");
  const sessionId = await createExecutionSession(session, run);
  const prompt = [
    "You are planning a swarm run.",
    "",
    `Objective: ${String(run?.objective || "").trim()}`,
    `Workspace: ${String(run?.workspace?.canonical_path || "").trim()}`,
    "",
    `Generate ${Math.max(1, Number(maxTasks) || 3)} concise, execution-ready implementation steps.`,
    "Return strict JSON only in this shape:",
    '{"tasks":[{"id":"task-1","title":"...","depends_on_task_ids":[]},{"id":"task-2","title":"...","depends_on_task_ids":["task-1"]}]}',
    "Use depends_on_task_ids only when a task requires outputs from another task.",
    "Independent tasks must have an empty depends_on_task_ids array.",
    "Do not include explanations.",
  ].join("\n");
  const promptResponse = await engineRequestJson(
    session,
    `/session/${encodeURIComponent(sessionId)}/prompt_sync`,
    {
      method: "POST",
      timeoutMs: 3 * 60 * 1000,
      body: {
        parts: [{ type: "text", text: prompt }],
      },
    }
  );
  const syncRows = Array.isArray(promptResponse) ? promptResponse : [];
  const fromSync = extractAssistantText(syncRows);
  if (fromSync) {
    return {
      sessionId,
      tasks: parsePlanTasksFromAssistant(fromSync, maxTasks),
      assistantText: fromSync,
    };
  }
  const sessionSnapshot = await engineRequestJson(
    session,
    `/session/${encodeURIComponent(sessionId)}`
  ).catch(() => null);
  const messages = Array.isArray(sessionSnapshot?.messages) ? sessionSnapshot.messages : [];
  const fromSnapshot = extractAssistantText(messages);
  return {
    sessionId,
    tasks: parsePlanTasksFromAssistant(fromSnapshot, maxTasks),
    assistantText: fromSnapshot,
  };
}

function isRunTerminal(status) {
  const normalized = String(status || "")
    .trim()
    .toLowerCase();
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

async function seedContextRunStepsFromTitles(session, runId, titles = []) {
  const todoRows = (Array.isArray(titles) ? titles : [])
    .map((row) => String(row || "").trim())
    .filter((row) => row.length >= 3)
    .map((content, idx) => ({
      id: `step-${idx + 1}`,
      content,
      status: "pending",
    }));
  if (!todoRows.length) {
    throw new Error("No valid todo rows for context step seeding.");
  }
  await engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}/todos/sync`, {
    method: "POST",
    body: {
      replace: true,
      todos: todoRows,
      source_session_id: null,
      source_run_id: runId,
    },
  });
  return todoRows.length;
}

async function createExecutionSession(session, run) {
  const runId = String(run?.run_id || "").trim();
  const controller = getSwarmRunController(runId);
  const workspaceCandidates = [
    run?.workspace?.canonical_path,
    run?.workspace?.workspace_root,
    run?.workspace_root,
    controller?.workspaceRoot,
    controller?.repoRoot,
    swarmState.workspaceRoot,
    swarmState.repoRoot,
    REPO_ROOT,
  ];
  const workspaceRootRaw = workspaceCandidates
    .map((value) => String(value || "").trim())
    .find((value) => value.length > 0);
  const workspaceRoot = await workspaceExistsAsDirectory(workspaceRootRaw || REPO_ROOT);
  if (!workspaceRoot) {
    throw new Error(
      `Workspace root does not exist or is not a directory: ${resolve(String(workspaceRootRaw || REPO_ROOT))}`
    );
  }
  const resolved = await resolveExecutionModel(session, run);
  const modelProvider = resolved.provider;
  const modelId = resolved.model;
  upsertSwarmRunController(runId, {
    resolvedModelProvider: modelProvider,
    resolvedModelId: modelId,
    modelResolutionSource: resolved.source,
    modelProvider,
    modelId,
  });
  const payload = await engineRequestJson(session, "/session", {
    method: "POST",
    body: {
      title: `Swarm ${String(run?.run_id || "").trim()}`,
      directory: workspaceRoot,
      workspace_root: workspaceRoot,
      permission: [
        { permission: "write", pattern: "*", action: "allow" },
        { permission: "edit", pattern: "*", action: "allow" },
        { permission: "apply_patch", pattern: "*", action: "allow" },
        { permission: "read", pattern: "*", action: "allow" },
        { permission: "glob", pattern: "*", action: "allow" },
        { permission: "search", pattern: "*", action: "allow" },
        { permission: "grep", pattern: "*", action: "allow" },
        { permission: "codesearch", pattern: "*", action: "allow" },
        { permission: "ls", pattern: "*", action: "allow" },
        { permission: "list", pattern: "*", action: "allow" },
        { permission: "todowrite", pattern: "*", action: "allow" },
        { permission: "todo_write", pattern: "*", action: "allow" },
        { permission: "update_todo_list", pattern: "*", action: "allow" },
        { permission: "websearch", pattern: "*", action: "allow" },
        { permission: "webfetch", pattern: "*", action: "allow" },
        { permission: "webfetch_html", pattern: "*", action: "allow" },
        { permission: "bash", pattern: "*", action: "deny" },
        { permission: "task", pattern: "*", action: "deny" },
        { permission: "spawn_agent", pattern: "*", action: "deny" },
        { permission: "batch", pattern: "*", action: "deny" },
      ],
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

function workerExecutionToolAllowlist() {
  return [
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
  ];
}

function workerWriteRetryToolAllowlist() {
  return ["write", "edit", "apply_patch"];
}

const WRITE_TOOL_NAMES = new Set(["write", "edit", "apply_patch"]);
const REQUIRED_TOOL_MODE_REASON = "TOOL_MODE_REQUIRED_NOT_SATISFIED";

function normalizeVerificationMode(mode) {
  return String(mode || "")
    .trim()
    .toLowerCase() === "lenient"
    ? "lenient"
    : "strict";
}

function normalizeToolName(tool) {
  const raw = String(tool || "")
    .trim()
    .toLowerCase();
  if (!raw) return "";
  const parts = raw.split(":").filter(Boolean);
  return parts.length ? parts[parts.length - 1] : raw;
}

function isWriteToolName(tool) {
  return WRITE_TOOL_NAMES.has(normalizeToolName(tool));
}

function createExecutionError(message, details = {}) {
  const error = new Error(message);
  if (details?.sessionId) error.sessionId = String(details.sessionId || "").trim();
  if (details?.verification && typeof details.verification === "object") {
    error.verification = details.verification;
  }
  return error;
}

function normalizeWorkspaceRelativePath(pathname) {
  return String(pathname || "")
    .replace(/\\/g, "/")
    .replace(/^\.?\//, "")
    .replace(/^\/+/, "")
    .trim();
}

function shouldTrackWorkspacePath(pathname) {
  const normalized = normalizeWorkspaceRelativePath(pathname);
  return (
    !!normalized &&
    normalized !== ".git" &&
    normalized !== ".tandem" &&
    !normalized.startsWith(".git/") &&
    !normalized.startsWith(".tandem/") &&
    !normalized.startsWith(".swarm/") &&
    !normalized.startsWith("node_modules/") &&
    !normalized.startsWith("dist/") &&
    !normalized.startsWith("target/")
  );
}

async function fingerprintWorkspaceFile(pathname) {
  const info = await stat(pathname).catch(() => null);
  if (!info?.isFile()) return null;
  let contentHash = "";
  if (Number(info.size || 0) <= 1024 * 1024) {
    const bytes = await readFile(pathname).catch(() => null);
    if (bytes) {
      contentHash = createHash("sha1").update(bytes).digest("hex");
    }
  }
  return {
    size: Number(info.size || 0),
    modifiedMs: Number(info.mtimeMs || 0),
    contentHash,
  };
}

function parseGitStatusEntries(output) {
  const chunks = String(output || "").split("\0");
  const entries = [];
  for (let index = 0; index < chunks.length; index += 1) {
    const chunk = String(chunks[index] || "");
    if (!chunk) continue;
    const status = chunk.slice(0, 2);
    const primaryPath = normalizeWorkspaceRelativePath(chunk.slice(3));
    if (status.startsWith("R") || status.startsWith("C")) {
      const renamedPath = normalizeWorkspaceRelativePath(chunks[index + 1] || "");
      if (!renamedPath && !primaryPath) continue;
      entries.push({
        status,
        path: renamedPath || primaryPath,
        originalPath: primaryPath || "",
      });
      index += 1;
      continue;
    }
    if (!primaryPath) continue;
    entries.push({
      status,
      path: primaryPath,
      originalPath: "",
    });
  }
  return entries;
}

function buildGitStatusIndex(entries) {
  const statusByPath = Object.create(null);
  for (const entry of Array.isArray(entries) ? entries : []) {
    const rawStatus = String(entry?.status || "")
      .trim()
      .toUpperCase();
    let normalized = "M";
    if (rawStatus === "??") normalized = "A";
    else if (rawStatus.startsWith("R") || rawStatus.startsWith("C"))
      normalized = rawStatus.slice(0, 1);
    else if (rawStatus.includes("D")) normalized = "D";
    else if (rawStatus.includes("A")) normalized = "A";
    if (entry?.originalPath && shouldTrackWorkspacePath(entry.originalPath)) {
      statusByPath[normalizeWorkspaceRelativePath(entry.originalPath)] = "D";
    }
    if (entry?.path && shouldTrackWorkspacePath(entry.path)) {
      statusByPath[normalizeWorkspaceRelativePath(entry.path)] = normalized;
    }
  }
  return statusByPath;
}

function collectWorkspaceSnapshotSeedPaths(includePaths = []) {
  return Array.from(
    new Set(
      (Array.isArray(includePaths) ? includePaths : [])
        .map((pathname) => normalizeWorkspaceRelativePath(pathname))
        .filter((pathname) => shouldTrackWorkspacePath(pathname))
    )
  ).sort((a, b) => a.localeCompare(b));
}

async function captureGitWorkspaceSnapshot(workspaceRoot, includePaths = []) {
  const repo = await isGitRepo(workspaceRoot);
  if (!repo?.ok || !repo.root) return null;
  const gitStatus = await runGit(
    ["-C", repo.root, "status", "--porcelain=v1", "--untracked-files=all", "-z"],
    {
      stdio: "pipe",
      safeDirectory: repo.root,
    }
  ).catch(() => null);
  if (!gitStatus) return null;
  const entries = parseGitStatusEntries(gitStatus.stdout || "");
  const files = Object.create(null);
  const candidatePaths = collectWorkspaceSnapshotSeedPaths([
    ...entries.flatMap((entry) => [entry?.path || "", entry?.originalPath || ""]),
    ...includePaths,
  ]);
  for (const relativePath of candidatePaths) {
    const fingerprint = await fingerprintWorkspaceFile(join(repo.root, relativePath));
    if (fingerprint) files[relativePath] = fingerprint;
  }
  return {
    mode: "git_status",
    root: repo.root,
    files,
    statusByPath: buildGitStatusIndex(entries),
  };
}

async function captureWorkspaceSnapshot(workspaceRoot, options = {}) {
  const root = await workspaceExistsAsDirectory(workspaceRoot);
  if (!root) {
    throw new Error(
      `Workspace snapshot root not found: ${resolve(String(workspaceRoot || REPO_ROOT))}`
    );
  }
  const includePaths = collectWorkspaceSnapshotSeedPaths(options?.includePaths);
  const gitSnapshot = await captureGitWorkspaceSnapshot(root, includePaths);
  if (gitSnapshot) return gitSnapshot;
  const files = Object.create(null);
  async function walk(dirPath) {
    const entries = await readdir(dirPath, { withFileTypes: true }).catch(() => []);
    for (const entry of entries) {
      const absolutePath = join(dirPath, entry.name);
      const relativePath = normalizeWorkspaceRelativePath(relative(root, absolutePath));
      if (!shouldTrackWorkspacePath(relativePath)) continue;
      if (entry.isDirectory()) {
        await walk(absolutePath);
        continue;
      }
      if (!entry.isFile() && !entry.isSymbolicLink()) continue;
      const fingerprint = await fingerprintWorkspaceFile(absolutePath);
      if (fingerprint) files[relativePath] = fingerprint;
    }
  }
  await walk(root);
  return {
    mode: "filesystem_fingerprint",
    root,
    files,
    statusByPath: Object.create(null),
  };
}

function summarizeWorkspaceChanges(beforeSnapshot, afterSnapshot) {
  const beforeFiles =
    beforeSnapshot?.files && typeof beforeSnapshot.files === "object" ? beforeSnapshot.files : {};
  const afterFiles =
    afterSnapshot?.files && typeof afterSnapshot.files === "object" ? afterSnapshot.files : {};
  const beforeStatusByPath =
    beforeSnapshot?.statusByPath && typeof beforeSnapshot.statusByPath === "object"
      ? beforeSnapshot.statusByPath
      : {};
  const afterStatusByPath =
    afterSnapshot?.statusByPath && typeof afterSnapshot.statusByPath === "object"
      ? afterSnapshot.statusByPath
      : {};
  const created = [];
  const updated = [];
  const deleted = [];
  const allPaths = new Set([
    ...Object.keys(beforeFiles),
    ...Object.keys(afterFiles),
    ...Object.keys(beforeStatusByPath),
    ...Object.keys(afterStatusByPath),
  ]);
  for (const pathname of Array.from(allPaths)) {
    const beforeFingerprint = beforeFiles[pathname];
    const afterFingerprint = afterFiles[pathname];
    const afterStatus = String(afterStatusByPath[pathname] || "")
      .trim()
      .toUpperCase();
    const beforeStatus = String(beforeStatusByPath[pathname] || "")
      .trim()
      .toUpperCase();
    if (!beforeFingerprint && !afterFingerprint) {
      if (afterStatus === "D") deleted.push(pathname);
      else if (afterStatus || beforeStatus) updated.push(pathname);
      continue;
    }
    if (!beforeFingerprint && afterFingerprint) {
      if (afterStatus === "A" || afterStatus === "C" || afterStatus === "R") created.push(pathname);
      else updated.push(pathname);
      continue;
    }
    if (beforeFingerprint && !afterFingerprint) {
      deleted.push(pathname);
      continue;
    }
    if (
      Number(beforeFingerprint?.size || 0) !== Number(afterFingerprint?.size || 0) ||
      Number(beforeFingerprint?.modifiedMs || 0) !== Number(afterFingerprint?.modifiedMs || 0) ||
      String(beforeFingerprint?.contentHash || "") !== String(afterFingerprint?.contentHash || "")
    ) {
      updated.push(pathname);
    }
  }
  created.sort((a, b) => a.localeCompare(b));
  updated.sort((a, b) => a.localeCompare(b));
  deleted.sort((a, b) => a.localeCompare(b));
  const lines = [
    `Workspace changes: ${created.length} created, ${updated.length} updated, ${deleted.length} deleted`,
    ...created.slice(0, 20).map((pathname) => `+ ${pathname}`),
    ...updated.slice(0, 40).map((pathname) => `~ ${pathname}`),
    ...deleted.slice(0, 20).map((pathname) => `- ${pathname}`),
  ];
  return {
    mode: String(afterSnapshot?.mode || beforeSnapshot?.mode || "unknown"),
    hasChanges: created.length > 0 || updated.length > 0 || deleted.length > 0,
    created,
    updated,
    deleted,
    paths: [...created, ...updated, ...deleted],
    summary: lines.join("\n"),
  };
}

function emptyToolActivityAudit(source = "") {
  return {
    source: source || "",
    totalToolCalls: 0,
    writeToolCalls: 0,
    toolNames: [],
  };
}

function collectToolActivity(rows, source = "") {
  if (!Array.isArray(rows)) return emptyToolActivityAudit(source);
  const toolNames = new Set();
  let totalToolCalls = 0;
  let writeToolCalls = 0;
  const recordTool = (tool) => {
    const normalized = normalizeToolName(tool) || "tool";
    totalToolCalls += 1;
    toolNames.add(normalized);
    if (isWriteToolName(normalized)) writeToolCalls += 1;
  };
  for (const row of rows) {
    const parts = Array.isArray(row?.parts) ? row.parts : [];
    let rowRecorded = false;
    for (const part of parts) {
      const partType = String(part?.type || part?.part_type || "")
        .trim()
        .toLowerCase();
      const partTool = String(part?.tool || part?.name || "").trim();
      if (!partType.includes("tool") && !partTool) continue;
      recordTool(partTool);
      rowRecorded = true;
    }
    if (rowRecorded) continue;
    const rowType = String(row?.type || "")
      .trim()
      .toLowerCase();
    const rowTool = String(row?.tool || row?.name || "").trim();
    if (rowType.includes("tool") || rowTool) recordTool(rowTool);
  }
  return {
    source: source || "",
    totalToolCalls,
    writeToolCalls,
    toolNames: Array.from(toolNames).sort((a, b) => a.localeCompare(b)),
  };
}

function mergeToolActivityAudits(...audits) {
  const valid = audits.filter((audit) => audit && typeof audit === "object");
  const toolNames = new Set();
  const sources = [];
  let totalToolCalls = 0;
  let writeToolCalls = 0;
  for (const audit of valid) {
    totalToolCalls = Math.max(totalToolCalls, Number(audit?.totalToolCalls || 0));
    writeToolCalls = Math.max(writeToolCalls, Number(audit?.writeToolCalls || 0));
    if (audit?.source && (audit.totalToolCalls || audit.writeToolCalls)) {
      sources.push(String(audit.source));
    }
    for (const tool of Array.isArray(audit?.toolNames) ? audit.toolNames : []) {
      const normalized = normalizeToolName(tool);
      if (normalized) toolNames.add(normalized);
    }
  }
  return {
    totalToolCalls,
    writeToolCalls,
    toolNames: Array.from(toolNames).sort((a, b) => a.localeCompare(b)),
    sources: Array.from(new Set(sources)),
  };
}

function summarizeExecutionRows(rows, limit = 12) {
  const list = Array.isArray(rows) ? rows : [];
  const out = [];
  for (const row of list) {
    if (out.length >= limit) break;
    const role = roleOfMessage(row);
    const type = String(row?.type || "").trim().toLowerCase();
    const text = textOfMessage(row).trim();
    const parts = Array.isArray(row?.parts) ? row.parts : [];
    const tools = [];
    for (const part of parts) {
      const tool = String(part?.tool || part?.name || "").trim();
      if (tool) tools.push(normalizeToolName(tool) || tool);
    }
    const rowTool = String(row?.tool || row?.name || "").trim();
    if (rowTool) tools.push(normalizeToolName(rowTool) || rowTool);
    out.push({
      role,
      type: type || null,
      tools: Array.from(new Set(tools)).slice(0, 8),
      excerpt: text.slice(0, 240),
    });
  }
  return out;
}

function buildAttemptTelemetry(name, request, rows, messages, startedAtMs, error = null) {
  const syncAudit = collectToolActivity(rows, `${name}_prompt_sync`);
  const sessionAudit = collectToolActivity(messages, `${name}_session_snapshot`);
  const merged = mergeToolActivityAudits(syncAudit, sessionAudit);
  const assistantText = extractAssistantText(rows) || extractAssistantText(messages);
  return {
    name,
    started_at_ms: Number(startedAtMs || Date.now()),
    tool_mode: String(request?.tool_mode || "").trim() || null,
    tool_allowlist: Array.isArray(request?.tool_allowlist) ? request.tool_allowlist.slice() : [],
    prompt_excerpt: String(request?.parts?.[0]?.text || "")
      .trim()
      .slice(0, 600),
    assistant_present: !!assistantText.trim(),
    assistant_excerpt: assistantText.trim().slice(0, 400),
    total_tool_calls: merged.totalToolCalls,
    write_tool_calls: merged.writeToolCalls,
    tool_names: merged.toolNames,
    detection_sources: merged.sources,
    sync_row_count: Array.isArray(rows) ? rows.length : 0,
    session_message_count: Array.isArray(messages) ? messages.length : 0,
    sync_rows: summarizeExecutionRows(rows),
    session_rows: summarizeExecutionRows(messages),
    error: error ? String(error?.message || error || "").trim() : "",
  };
}

function buildVerificationSummary(syncRows, messages, workspaceChanges, sessionId, options = {}) {
  const syncAudit = collectToolActivity(syncRows, "prompt_sync");
  const sessionAudit = collectToolActivity(messages, "session_snapshot");
  const toolAudit = mergeToolActivityAudits(syncAudit, sessionAudit);
  const assistantText = extractAssistantText(syncRows) || extractAssistantText(messages);
  const mode = normalizeVerificationMode(options.verificationMode || swarmState.verificationMode);
  const requiredToolModeUnsatisfied = assistantText.includes(REQUIRED_TOOL_MODE_REASON);
  const workspaceChanged = workspaceChanges?.hasChanges === true;
  const strictMode = mode === "strict";
  const passed = strictMode
    ? workspaceChanged || toolAudit.writeToolCalls > 0
    : workspaceChanged || toolAudit.totalToolCalls > 0;
  let reason = "VERIFIED";
  if (!passed) {
    if (requiredToolModeUnsatisfied) reason = REQUIRED_TOOL_MODE_REASON;
    else if (strictMode && toolAudit.totalToolCalls > 0)
      reason = "NO_WRITE_ACTIVITY_NO_WORKSPACE_CHANGE";
    else reason = "NO_TOOL_ACTIVITY_NO_WORKSPACE_CHANGE";
  }
  return {
    mode,
    reason,
    passed,
    session_id: String(sessionId || "").trim() || null,
    assistant_present: !!assistantText.trim(),
    assistant_excerpt: assistantText.trim().slice(0, 280),
    any_tool_calls: toolAudit.totalToolCalls > 0,
    total_tool_calls: toolAudit.totalToolCalls,
    write_tool_calls: toolAudit.writeToolCalls,
    tool_names: toolAudit.toolNames,
    detection_sources: toolAudit.sources,
    workspace_changed: workspaceChanged,
    workspace_change_mode: String(workspaceChanges?.mode || "unknown"),
    workspace_change_paths: Array.isArray(workspaceChanges?.paths)
      ? workspaceChanges.paths.slice(0, 80)
      : [],
    workspace_change_summary: String(workspaceChanges?.summary || "").trim(),
    execution_trace:
      options?.executionTrace && typeof options.executionTrace === "object"
        ? options.executionTrace
        : undefined,
  };
}

function hasToolActivity(rows) {
  if (!Array.isArray(rows)) return false;
  return rows.some((row) => {
    const rowType = String(row?.type || "")
      .trim()
      .toLowerCase();
    if (rowType.includes("tool")) return true;
    const rowTool = String(row?.tool || "")
      .trim()
      .toLowerCase();
    if (rowTool) return true;
    const parts = Array.isArray(row?.parts) ? row.parts : [];
    return parts.some((part) => {
      const partType = String(part?.type || part?.part_type || "")
        .trim()
        .toLowerCase();
      if (partType.includes("tool")) return true;
      return String(part?.tool || "").trim().length > 0;
    });
  });
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
  const providers =
    payload?.providers && typeof payload.providers === "object" ? payload.providers : {};
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

  const controller = getSwarmRunController(String(run?.run_id || "").trim());
  const swarmProvider = String(controller?.modelProvider || swarmState.modelProvider || "").trim();
  const swarmModel = String(controller?.modelId || swarmState.modelId || "").trim();
  if (swarmProvider && swarmModel) {
    return { provider: swarmProvider, model: swarmModel, source: "swarm_state" };
  }

  const defaults = await fetchEngineDefaultModel(session);
  if (defaults.provider && defaults.model) {
    return { provider: defaults.provider, model: defaults.model, source: "engine_default" };
  }

  throw new Error("MODEL_SELECTION_REQUIRED: no provider/model configured for swarm execution.");
}

function summarizeRunStepsForPrompt(run, currentStepId, limit = 12) {
  const steps = Array.isArray(run?.steps) ? run.steps : [];
  return steps
    .slice(0, Math.max(1, Number(limit) || 12))
    .map((row, index) => {
      const stepId = String(row?.step_id || `step-${index + 1}`).trim();
      const title = String(row?.title || stepId).trim();
      const status = String(row?.status || "unknown").trim().toLowerCase();
      const marker = stepId === currentStepId ? "*" : "-";
      return `${marker} ${stepId} [${status}]: ${title}`;
    })
    .join("\n")
    .trim();
}

function stepPromptText(run, step, stepIndex, totalSteps) {
  const stepId = String(step?.step_id || "").trim() || `step-${stepIndex + 1}`;
  const stepTitle = String(step?.title || stepId).trim();
  const stepDetails =
    step && typeof step === "object" ? JSON.stringify(step, null, 2).trim() : "";
  const stepList = summarizeRunStepsForPrompt(run, stepId);
  return [
    "Execute this swarm step.",
    "",
    `Step (${stepIndex + 1}/${totalSteps}): ${stepTitle}`,
    `Step ID: ${stepId}`,
    `Workspace: ${String(run?.workspace?.canonical_path || "").trim()}`,
    "",
    "This step is already planned and assigned.",
    "Treat the current assigned step as the authority for what to implement.",
    "Use the original objective and step list only to clarify the assigned step, not to re-plan the run.",
    "Do not create a new plan, do not restate the task graph, and do not describe future work.",
    "Use workspace tools to implement this step now.",
    "",
    "Current assigned step payload:",
    stepDetails || "{}",
    "",
    "Run step list:",
    stepList || "(no step list available)",
    "",
    `Original objective: ${String(run?.objective || "").trim()}`,
    "",
    "Requirements:",
    "- First inspect the relevant workspace files with read/glob/list/search if needed.",
    "- Make the required code/project changes for this step right now.",
    "- Create or edit files as needed for this step only.",
    "- Use write/edit/apply_patch instead of a prose-only response.",
    "- Keep scope limited to this step.",
    "- Return a concise summary of the concrete file changes and blockers.",
  ].join("\n");
}

function rowsSinceAttemptStart(rows, startIndex = 0) {
  const list = Array.isArray(rows) ? rows : [];
  const offset = Math.max(0, Number(startIndex) || 0);
  if (!offset) return list;
  return list.length > offset ? list.slice(offset) : list;
}

async function runExecutionPromptWithVerification(session, run, prompt, sessionId = "") {
  const activeSessionId =
    String(sessionId || "").trim() || (await createExecutionSession(session, run));
  if (!activeSessionId) throw new Error("Failed to create execution session.");
  const runId = String(run?.run_id || "").trim();
  const controller = getSwarmRunController(runId);
  const workspaceRoot = await workspaceExistsAsDirectory(
    String(
      run?.workspace?.canonical_path ||
        run?.workspace_root ||
        controller?.workspaceRoot ||
        swarmState.workspaceRoot ||
        REPO_ROOT
    ).trim()
  );
  let workspaceBefore = null;
  let workspaceAfter = null;
  let workspaceChanges = {
    mode: "unavailable",
    hasChanges: false,
    paths: [],
    summary: "",
  };
  const resolvedModel = await resolveExecutionModel(session, run);
  const initialRequestBody = {
    parts: [{ type: "text", text: prompt }],
    tool_mode: "required",
    tool_allowlist: workerExecutionToolAllowlist(),
  };
  let initialAttemptRows = [];
  let retryAttemptRows = [];
  let retryAttemptMessages = [];
  let retryRequestBody = null;
  const workspaceSeedPaths = () => [
    ...Object.keys(workspaceBefore?.files || {}),
    ...Object.keys(workspaceBefore?.statusByPath || {}),
  ];
  if (workspaceRoot) {
    workspaceBefore = await captureWorkspaceSnapshot(workspaceRoot).catch((error) => ({
      mode: "capture_failed",
      root: workspaceRoot,
      files: {},
      statusByPath: {},
      error: String(error?.message || error || "workspace snapshot failed"),
    }));
  }
  let promptResponse = await engineRequestJson(
    session,
    `/session/${encodeURIComponent(activeSessionId)}/prompt_sync`,
    {
      method: "POST",
      timeoutMs: 10 * 60 * 1000,
      body: initialRequestBody,
    }
  );
  let syncRows = Array.isArray(promptResponse) ? promptResponse : [];
  initialAttemptRows = syncRows.slice();
  let verificationStartIndex = 0;
  let retryError = null;
  let hasAssistant = syncRows.some((row) => roleOfMessage(row) === "assistant");
  let hasToolSignal = hasToolActivity(syncRows);
  if (!hasAssistant || !hasToolSignal) {
    verificationStartIndex = syncRows.length;
    const retryNeedsInitialInspection = !hasToolSignal;
    const retryPrompt = retryNeedsInitialInspection
      ? [
          prompt,
          "",
          "Mandatory execution rule:",
          "- Before final text, execute at least one tool call.",
          "- You have not used any tools yet in this step.",
          "- Inspect the workspace first with ls/list/glob/search/read if needed.",
          "- Then make the required file changes in the same turn with write/edit/apply_patch.",
          "- Do not end with prose only.",
          "- Do not use bash.",
        ].join("\n")
      : [
          prompt,
          "",
          "Mandatory execution rule:",
          "- Before final text, execute at least one tool call.",
          "- Use write/edit/apply_patch to make workspace changes.",
          "- On this retry, do not inspect further with read/search/glob/ls/list.",
          "- Create or modify the target file directly with write/edit/apply_patch.",
          "- Do not use bash.",
        ].join("\n");
    retryRequestBody = {
      parts: [{ type: "text", text: retryPrompt }],
      tool_mode: "required",
      tool_allowlist: retryNeedsInitialInspection
        ? workerExecutionToolAllowlist()
        : workerWriteRetryToolAllowlist(),
    };
    promptResponse = await engineRequestJson(
      session,
      `/session/${encodeURIComponent(activeSessionId)}/prompt_sync`,
      {
        method: "POST",
        timeoutMs: 10 * 60 * 1000,
        body: retryRequestBody,
      }
    ).catch((error) => {
      retryError = error;
      return null;
    });
    syncRows = rowsSinceAttemptStart(promptResponse, verificationStartIndex);
    retryAttemptRows = syncRows.slice();
    hasAssistant = syncRows.some((row) => roleOfMessage(row) === "assistant");
    hasToolSignal = hasToolActivity(syncRows);
  }
  const sessionSnapshot = await engineRequestJson(
    session,
    `/session/${encodeURIComponent(activeSessionId)}`
  ).catch(() => null);
  const sessionMessages = Array.isArray(sessionSnapshot?.messages) ? sessionSnapshot.messages : [];
  const messages = rowsSinceAttemptStart(sessionMessages, verificationStartIndex);
  retryAttemptMessages = messages.slice();
  const persistedAssistant = messages.some((message) => roleOfMessage(message) === "assistant");
  if (workspaceRoot) {
    workspaceAfter = await captureWorkspaceSnapshot(workspaceRoot, {
      includePaths: workspaceSeedPaths(),
    }).catch((error) => ({
      mode: "capture_failed",
      root: workspaceRoot,
      files: {},
      statusByPath: {},
      error: String(error?.message || error || "workspace snapshot failed"),
    }));
  }
  if (workspaceBefore && workspaceAfter) {
    workspaceChanges = summarizeWorkspaceChanges(workspaceBefore, workspaceAfter);
    if (workspaceBefore?.error || workspaceAfter?.error) {
      const detail = [workspaceBefore?.error, workspaceAfter?.error].filter(Boolean).join(" | ");
      workspaceChanges.summary = [
        workspaceChanges.summary,
        detail ? `Workspace snapshot warnings: ${detail}` : "",
      ]
        .filter(Boolean)
        .join("\n");
    }
  }
  const executionTrace = {
    session_id: activeSessionId,
    model: {
      provider: String(resolvedModel?.provider || "").trim(),
      model_id: String(resolvedModel?.model || "").trim(),
      source: String(resolvedModel?.source || "").trim(),
    },
    attempts: [
      buildAttemptTelemetry(
        "initial",
        initialRequestBody,
        initialAttemptRows,
        retryRequestBody ? [] : messages,
        Date.now()
      ),
      retryRequestBody
        ? buildAttemptTelemetry(
            "retry",
            retryRequestBody,
            retryAttemptRows,
            retryAttemptMessages,
            Date.now(),
            retryError
          )
        : null,
    ].filter(Boolean),
  };
  const verification = buildVerificationSummary(
    syncRows,
    messages,
    workspaceChanges,
    activeSessionId,
    {
      verificationMode: controller?.verificationMode || swarmState.verificationMode,
      executionTrace,
    }
  );
  if (retryError && !hasAssistant && !persistedAssistant) {
    throw createExecutionError(`PROMPT_RETRY_FAILED: ${retryError.message}`, {
      sessionId: activeSessionId,
      verification: {
        ...verification,
        reason: "PROMPT_RETRY_FAILED",
        passed: false,
        assistant_present: false,
        retry_error: String(retryError?.message || retryError || "").trim(),
      },
    });
  }
  if (!hasAssistant && !persistedAssistant) {
    throw createExecutionError(
      "PROMPT_DISPATCH_EMPTY_RESPONSE: prompt_sync returned no assistant output. Model route may be unresolved.",
      {
        sessionId: activeSessionId,
        verification: {
          ...verification,
          reason: "PROMPT_DISPATCH_EMPTY_RESPONSE",
          passed: false,
          assistant_present: false,
        },
      }
    );
  }
  if (!verification.passed) {
    throw createExecutionError(`TASK_NOT_VERIFIED: ${verification.reason}`, {
      sessionId: activeSessionId,
      verification,
    });
  }
  return { sessionId: activeSessionId, verification };
}

async function runStepWithLLM(session, run, step, stepIndex, totalSteps, sessionId = "") {
  const prompt = stepPromptText(run, step, stepIndex, totalSteps);
  return runExecutionPromptWithVerification(session, run, prompt, sessionId);
}

function extractBlackboardTasks(blackboardPayload) {
  const board =
    blackboardPayload?.blackboard && typeof blackboardPayload.blackboard === "object"
      ? blackboardPayload.blackboard
      : {};
  return Array.isArray(board?.tasks) ? board.tasks : [];
}

function isTerminalTaskStatus(status) {
  const normalized = String(status || "")
    .trim()
    .toLowerCase();
  return ["done", "completed", "failed", "cancelled", "canceled"].includes(normalized);
}

function isCompletedTaskStatus(status) {
  const normalized = String(status || "")
    .trim()
    .toLowerCase();
  return ["done", "completed"].includes(normalized);
}

function extractClaimedTask(payload) {
  if (payload?.task && typeof payload.task === "object") return payload.task;
  if (Array.isArray(payload?.tasks) && payload.tasks[0] && typeof payload.tasks[0] === "object") {
    return payload.tasks[0];
  }
  if (payload && typeof payload === "object" && String(payload.id || "").trim()) {
    return payload;
  }
  return null;
}

function taskTitleFromRecord(task) {
  const payload = task?.payload && typeof task.payload === "object" ? task.payload : {};
  return String(payload?.title || task?.title || task?.task_type || task?.id || "task").trim();
}

function summarizeBlackboardTasksForPrompt(tasks, currentTaskId, limit = 16) {
  const list = Array.isArray(tasks) ? tasks : [];
  return list
    .slice(0, Math.max(1, Number(limit) || 16))
    .map((task, index) => {
      const taskId = String(task?.id || `task-${index + 1}`).trim();
      const title = taskTitleFromRecord(task);
      const status = String(task?.status || "unknown").trim().toLowerCase();
      const marker = taskId === currentTaskId ? "*" : "-";
      return `${marker} ${taskId} [${status}]: ${title}`;
    })
    .join("\n")
    .trim();
}

function taskPromptText(run, task, workerId, workflowId) {
  const taskId = String(task?.id || "").trim();
  const taskTitle = taskTitleFromRecord(task);
  const taskDetails =
    task && typeof task === "object" ? JSON.stringify(task, null, 2).trim() : "";
  const taskList = summarizeBlackboardTasksForPrompt(run?.tasks, taskId);
  return [
    "Execute this swarm blackboard task.",
    "",
    `Task: ${taskTitle}`,
    `Task ID: ${taskId}`,
    `Workflow: ${String(workflowId || task?.workflow_id || "swarm.blackboard.default").trim()}`,
    `Agent: ${workerId}`,
    `Workspace: ${String(run?.workspace?.canonical_path || "").trim()}`,
    "",
    "This task is already planned and assigned.",
    "Treat the current assigned task as the authority for what to implement.",
    "Use the original objective and task list only to clarify the assigned task, not to re-plan the run.",
    "Do not create a new plan, do not restate the task graph, and do not describe future work.",
    "Use workspace tools to implement this task now.",
    "",
    "Current assigned task payload:",
    taskDetails || "{}",
    "",
    "Run blackboard task list:",
    taskList || "(no task list available)",
    "",
    `Original objective: ${String(run?.objective || "").trim()}`,
    "",
    "Requirements:",
    "- First inspect the relevant workspace files with read/glob/list/search if needed.",
    "- Implement this task in the workspace right now.",
    "- Create or edit files as needed for this task only.",
    "- Use write/edit/apply_patch instead of a prose-only response.",
    "- Keep scope limited to this task.",
    "- Return a concise summary of the concrete file changes and blockers.",
  ].join("\n");
}

async function runTaskWithLLM(session, run, task, workerId, workflowId, sessionId = "") {
  const prompt = taskPromptText(run, task, workerId, workflowId);
  return runExecutionPromptWithVerification(session, run, prompt, sessionId);
}

async function fetchBlackboardTasks(session, runId) {
  const payload = await engineRequestJson(
    session,
    `/context/runs/${encodeURIComponent(runId)}/blackboard`
  ).catch(() => ({}));
  return extractBlackboardTasks(payload);
}

async function seedBlackboardTasks(session, runId, objective, taskRows, workflowId) {
  const normalizedTasks = normalizePlannerTasks(taskRows, 128, { linearFallback: true });
  const validTaskIds = new Set(normalizedTasks.map((task) => task.id));
  const prepared = normalizedTasks
    .map((task, idx, list) => ({
      id: String(task?.id || `task-${idx + 1}`).trim(),
      task_type: "implementation",
      workflow_id: workflowId,
      depends_on_task_ids: (Array.isArray(task?.dependsOnTaskIds) ? task.dependsOnTaskIds : [])
        .map((dep) => String(dep || "").trim())
        .filter((dep) => dep && validTaskIds.has(dep)),
      payload: {
        title: String(task?.title || "").trim(),
        objective,
        step_index: idx + 1,
        total_steps: list.length,
      },
    }))
    .filter((task) => String(task?.payload?.title || "").trim().length >= 6);
  if (!prepared.length) {
    throw new Error("No valid tasks to seed.");
  }
  try {
    await engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}/tasks`, {
      method: "POST",
      body: { tasks: prepared },
    });
  } catch (error) {
    const detail = String(error?.message || error || "").toLowerCase();
    const endpointMissing = detail.includes("404") || detail.includes("not found");
    if (!endpointMissing) throw error;
    const todoCount = await seedContextRunStepsFromTitles(
      session,
      runId,
      prepared.map((row) => String(row?.payload?.title || "").trim())
    );
    return { mode: "steps_compat", count: todoCount };
  }
  const seeded = await fetchBlackboardTasks(session, runId);
  if (!seeded.length) {
    throw new Error("Task seeding returned no blackboard tasks.");
  }
  return { mode: "blackboard", count: seeded.length };
}

async function transitionBlackboardTask(session, runId, task, update = {}) {
  const taskId = String(task?.id || update.taskId || "").trim();
  if (!taskId) throw new Error("Missing task id for transition.");
  const expectedTaskRev = task?.task_rev ?? update.expectedTaskRev;
  const leaseToken = task?.lease_token || task?.leaseToken || update.leaseToken;
  const agentId = update.agentId || task?.assigned_agent || task?.lease_owner || undefined;
  return engineRequestJson(
    session,
    `/context/runs/${encodeURIComponent(runId)}/tasks/${encodeURIComponent(taskId)}/transition`,
    {
      method: "POST",
      body: {
        action: String(update.action || "status").trim() || "status",
        status: update.status || undefined,
        error: update.error || undefined,
        command_id: update.commandId || undefined,
        expected_task_rev: expectedTaskRev ?? undefined,
        lease_token: leaseToken || undefined,
        agent_id: agentId || undefined,
      },
    }
  );
}

async function detectExecutorMode(session, runId) {
  const blackboardTasks = await fetchBlackboardTasks(session, runId);
  if (blackboardTasks.length) return "blackboard";
  const payload = await engineRequestJson(
    session,
    `/context/runs/${encodeURIComponent(runId)}`
  ).catch(() => null);
  const steps = Array.isArray(payload?.run?.steps) ? payload.run.steps : [];
  if (steps.length) return "context_steps";
  return String(
    getSwarmRunController(runId)?.executorMode || swarmState.executorMode || "context_steps"
  );
}

const swarmExecutors = new Map();

function findStepByStatus(steps, status) {
  return (Array.isArray(steps) ? steps : []).find(
    (step) =>
      String(step?.status || "")
        .trim()
        .toLowerCase() === status
  );
}

async function ensureStepMarkedDone(session, runId, stepId) {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const payload = await engineRequestJson(
      session,
      `/context/runs/${encodeURIComponent(runId)}`
    ).catch(() => null);
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
    const verify = await engineRequestJson(
      session,
      `/context/runs/${encodeURIComponent(runId)}`
    ).catch(() => null);
    const verifySteps = Array.isArray(verify?.run?.steps) ? verify.run.steps : [];
    const verifyStep = verifySteps.find((step) => String(step?.step_id || "") === stepId);
    return String(verifyStep?.status || "").toLowerCase() === "done";
  }
  return false;
}

async function driveContextRunExecution(session, runId) {
  if (swarmExecutors.has(runId)) return false;
  upsertSwarmRunController(runId, {
    executorState: "running",
    executorReason: "",
  });
  const runner = (async () => {
    let completionStreak = 0;
    let lastCompletedStepId = "";
    for (let cycle = 0; cycle < 24; cycle += 1) {
      const runPayload = await engineRequestJson(
        session,
        `/context/runs/${encodeURIComponent(runId)}`
      );
      const run = runPayload?.run || {};
      if (isRunTerminal(run.status)) return;

      const nextPayload = await engineRequestJson(
        session,
        `/context/runs/${encodeURIComponent(runId)}/driver/next`,
        {
          method: "POST",
          body: { dry_run: false },
        }
      );
      const selectedStepId = String(nextPayload?.selected_step_id || "").trim();
      const latestRun = nextPayload?.run || run;
      const steps = Array.isArray(latestRun?.steps) ? latestRun.steps : [];
      const inProgressStep = findStepByStatus(steps, "in_progress");
      const executionStepId = selectedStepId || String(inProgressStep?.step_id || "").trim();

      if (!executionStepId) {
        if (
          steps.length &&
          steps.every((step) => String(step?.status || "").toLowerCase() === "done")
        ) {
          await appendContextRunEvent(session, runId, "run_completed", "completed", {
            why_next_step: "all steps completed",
          });
          upsertSwarmRunController(runId, {
            lastError: "",
            status: "completed",
            stoppedAt: Date.now(),
            executorState: "idle",
            executorReason: "run completed",
          });
          return;
        }
        const blockedReason = String(
          nextPayload?.why_next_step || latestRun?.why_next_step || "No actionable step selected."
        );
        upsertSwarmRunController(runId, {
          lastError: blockedReason,
          status: "blocked",
          executorState: "blocked",
          executorReason: blockedReason,
        });
        return;
      }

      const stepIndex = steps.findIndex((step) => String(step?.step_id || "") === executionStepId);
      const step =
        stepIndex >= 0 ? steps[stepIndex] : { step_id: executionStepId, title: executionStepId };
      let stepSessionId = "";

      try {
        stepSessionId = await createExecutionSession(session, latestRun);
        await appendContextRunEvent(
          session,
          runId,
          "step_started",
          "running",
          {
            step_status: "in_progress",
            step_title: String(step?.title || executionStepId),
            session_id: stepSessionId || null,
            why_next_step: selectedStepId
              ? `executing ${executionStepId}`
              : `resuming in_progress step ${executionStepId}`,
          },
          executionStepId
        );
        const { sessionId, verification } = await runStepWithLLM(
          session,
          latestRun,
          step,
          Math.max(stepIndex, 0),
          Math.max(steps.length, 1),
          stepSessionId
        );
        await appendContextRunEvent(
          session,
          runId,
          "step_completed",
          "running",
          {
            step_status: "done",
            step_title: String(step?.title || executionStepId),
            session_id: sessionId,
            verification,
            why_next_step: `completed ${executionStepId}`,
          },
          executionStepId
        );
        const refresh = await engineRequestJson(
          session,
          `/context/runs/${encodeURIComponent(runId)}`
        ).catch(() => null);
        const refreshedSteps = Array.isArray(refresh?.run?.steps) ? refresh.run.steps : [];
        const refreshedStep = refreshedSteps.find(
          (item) => String(item?.step_id || "") === executionStepId
        );
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
        upsertSwarmRunController(runId, {
          lastError: "",
          status: "running",
          executorState: "running",
          executorReason: "",
        });
      } catch (error) {
        const message = String(error?.message || error || "Unknown step failure");
        const failureSessionId = String(error?.sessionId || stepSessionId || "").trim();
        const verification =
          error?.verification && typeof error.verification === "object"
            ? error.verification
            : undefined;
        upsertSwarmRunController(runId, {
          lastError: message,
          status: "failed",
          executorState: "error",
          executorReason: message,
        });
        await appendContextRunEvent(
          session,
          runId,
          "step_failed",
          "failed",
          {
            step_status: "failed",
            session_id: failureSessionId || null,
            verification,
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
      const message = String(error?.message || error || "Run executor failed");
      upsertSwarmRunController(runId, {
        lastError: message,
        status: "failed",
        executorState: "error",
        executorReason: message,
      });
    })
    .finally(() => {
      swarmExecutors.delete(runId);
      const controller = getSwarmRunController(runId);
      if (String(controller?.executorState || "") === "running") {
        upsertSwarmRunController(runId, {
          executorState: "idle",
          executorReason: "",
        });
      }
    });
  swarmExecutors.set(runId, runner);
  return true;
}

async function driveBlackboardRunExecution(session, runId, options = {}) {
  if (swarmExecutors.has(runId)) return false;
  const controller = getSwarmRunController(runId);
  const workflowId = String(
    options.workflowId || controller?.workflowId || swarmState.workflowId || "swarm.blackboard.default"
  ).trim();
  const maxAgents = Math.max(
    1,
    Math.min(
      16,
      Number.parseInt(String(options.maxAgents || controller?.maxAgents || swarmState.maxAgents || 3), 10) || 3
    )
  );
  upsertSwarmRunController(runId, {
    executorState: "running",
    executorReason: "",
    executorMode: "blackboard",
  });

  const runner = (async () => {
    let completionAnnounced = false;
    const markRunComplete = async (reason) => {
      if (completionAnnounced) return;
      completionAnnounced = true;
      await appendContextRunEvent(session, runId, "run_completed", "completed", {
        why_next_step: String(reason || "all tasks completed"),
      });
      upsertSwarmRunController(runId, {
        lastError: "",
        status: "completed",
        stoppedAt: Date.now(),
        executorState: "idle",
        executorReason: "run completed",
      });
    };

    const workers = Array.from({ length: maxAgents }).map((_, index) => {
      const agentId = `swarm-agent-${index + 1}`;
      return (async () => {
        let idleSpins = 0;
        for (let cycle = 0; cycle < 96; cycle += 1) {
          const runPayload = await engineRequestJson(
            session,
            `/context/runs/${encodeURIComponent(runId)}`
          ).catch(() => null);
          const run = runPayload?.run || {};
          if (isRunTerminal(run.status)) return;

          const boardTasks = await fetchBlackboardTasks(session, runId);
          const openTasks = boardTasks.filter((task) => !isTerminalTaskStatus(task?.status));
          if (!openTasks.length && boardTasks.length) {
            await markRunComplete("all blackboard tasks completed");
            return;
          }

          const claimedPayload = await engineRequestJson(
            session,
            `/context/runs/${encodeURIComponent(runId)}/tasks/claim`,
            {
              method: "POST",
              body: {
                agent_id: agentId,
                workflow_id: workflowId || undefined,
                lease_ms: 45000,
              },
            }
          ).catch(() => null);
          const task = extractClaimedTask(claimedPayload);
          if (!task) {
            idleSpins += 1;
            if (idleSpins > 6) {
              const refreshedBoard = await fetchBlackboardTasks(session, runId);
              if (
                refreshedBoard.length &&
                refreshedBoard.every((row) => isCompletedTaskStatus(row?.status))
              ) {
                await markRunComplete("all blackboard tasks completed");
                return;
              }
              idleSpins = 0;
            }
            await sleep(500);
            continue;
          }
          idleSpins = 0;
          const taskId = String(task?.id || "").trim();
          const title = taskTitleFromRecord(task);
          let taskSessionId = "";
          try {
            taskSessionId = await createExecutionSession(session, run);
            await appendContextRunEvent(
              session,
              runId,
              "task_started",
              "running",
              {
                step_status: "in_progress",
                step_title: title,
                session_id: taskSessionId || null,
                workflow_id: String(task?.workflow_id || workflowId || "").trim(),
                assigned_agent: agentId,
                why_next_step: `executing ${taskId || title}`,
              },
              taskId || null
            );
            const runSnapshot = await engineRequestJson(
              session,
              `/context/runs/${encodeURIComponent(runId)}`
            ).catch(() => ({ run: {} }));
            const { sessionId, verification } = await runTaskWithLLM(
              session,
              runSnapshot?.run || run,
              task,
              agentId,
              workflowId,
              taskSessionId
            );
            await transitionBlackboardTask(session, runId, task, {
              status: "done",
              agentId,
              commandId: sessionId,
            }).catch(() => null);
            await appendContextRunEvent(
              session,
              runId,
              "task_completed",
              "running",
              {
                step_status: "done",
                step_title: title,
                session_id: sessionId,
                verification,
                workflow_id: String(task?.workflow_id || workflowId || "").trim(),
                assigned_agent: agentId,
                why_next_step: `completed ${taskId || title}`,
              },
              taskId || null
            );
            upsertSwarmRunController(runId, {
              lastError: "",
              status: "running",
              executorState: "running",
              executorReason: "",
            });
          } catch (error) {
            const message = String(error?.message || error || "Unknown task failure");
            const failureSessionId = String(error?.sessionId || taskSessionId || "").trim();
            const verification =
              error?.verification && typeof error.verification === "object"
                ? error.verification
                : undefined;
            await transitionBlackboardTask(session, runId, task, {
              status: "failed",
              error: message,
              agentId,
            }).catch(() => null);
            upsertSwarmRunController(runId, {
              lastError: message,
              status: "failed",
              executorState: "error",
              executorReason: message,
            });
            await appendContextRunEvent(
              session,
              runId,
              "task_failed",
              "failed",
              {
                step_status: "failed",
                step_title: title,
                session_id: failureSessionId || null,
                verification,
                workflow_id: String(task?.workflow_id || workflowId || "").trim(),
                assigned_agent: agentId,
                error: message,
                why_next_step: `task failed: ${taskId || title}`,
              },
              taskId || null
            );
            return;
          }
        }
      })();
    });
    await Promise.all(workers);
  })()
    .catch((error) => {
      const message = String(error?.message || error || "Run executor failed");
      upsertSwarmRunController(runId, {
        lastError: message,
        status: "failed",
        executorState: "error",
        executorReason: message,
      });
    })
    .finally(() => {
      swarmExecutors.delete(runId);
      const current = getSwarmRunController(runId);
      if (String(current?.executorState || "") === "running") {
        upsertSwarmRunController(runId, {
          executorState: "idle",
          executorReason: "",
        });
      }
    });
  swarmExecutors.set(runId, runner);
  return true;
}

async function startRunExecutor(session, runId, options = {}) {
  const mode = String(options.mode || "").trim() || (await detectExecutorMode(session, runId));
  upsertSwarmRunController(runId, { executorMode: mode });
  if (mode === "blackboard") {
    return driveBlackboardRunExecution(session, runId, options);
  }
  return driveContextRunExecution(session, runId);
}

async function requeueInProgressSteps(session, runId) {
  const payload = await engineRequestJson(
    session,
    `/context/runs/${encodeURIComponent(runId)}`
  ).catch(() => null);
  const run = payload?.run;
  const steps = Array.isArray(run?.steps) ? run.steps : [];
  const inProgress = steps.filter(
    (step) =>
      String(step?.status || "")
        .trim()
        .toLowerCase() === "in_progress"
  );
  for (const step of inProgress) {
    const stepId = String(step?.step_id || "").trim();
    if (!stepId) continue;
    await appendContextRunEvent(
      session,
      runId,
      "task_retry_requested",
      "running",
      {
        why_next_step: `requeued stale in_progress step \`${stepId}\` before continue`,
      },
      stepId
    );
  }
  return inProgress.length;
}

async function startSwarm(session, config = {}) {
  const objective = String(config.objective || "Ship a small feature end-to-end").trim();
  const workspaceCandidates = [
    config.workspaceRoot,
    config.workspace_root,
    config.workspace?.canonical_path,
    config.workspace?.workspace_root,
    config.repoRoot,
    config.repo_root,
  ];
  const workspaceRootRaw = workspaceCandidates
    .map((value) => String(value || "").trim())
    .find((value) => value.length > 0);
  if (!workspaceRootRaw) {
    throw new Error(
      "WORKSPACE_SELECTION_REQUIRED: select a workspace folder before starting a swarm run."
    );
  }
  const workspaceRoot = await workspaceExistsAsDirectory(workspaceRootRaw);
  if (!workspaceRoot) {
    throw new Error(
      `Workspace root does not exist or is not a directory: ${resolve(String(workspaceRootRaw || REPO_ROOT))}`
    );
  }
  const maxTasks = Math.max(1, Number.parseInt(String(config.maxTasks || 3), 10) || 3);
  const maxAgents = Math.max(
    1,
    Math.min(16, Number.parseInt(String(config.maxAgents || 3), 10) || 3)
  );
  const workflowId =
    String(config.workflowId || "swarm.blackboard.default").trim() || "swarm.blackboard.default";
  const verificationMode = normalizeVerificationMode(
    config?.verificationMode || config?.verification_mode
  );
  const requireLlmPlan = config?.requireLlmPlan === true || config?.require_llm_plan === true;
  const allowLocalPlannerFallback =
    config?.allowLocalPlannerFallback === true || config?.allow_local_planner_fallback === true;
  let modelProvider = String(config.modelProvider || "").trim();
  let modelId = String(config.modelId || "").trim();
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
    max_agents: maxAgents,
    workflow_id: workflowId,
    verification_mode: verificationMode,
    model_provider: modelProvider || undefined,
    model_id: modelId || undefined,
    mcp_servers: mcpServers,
  });
  const synced = (() =>
    engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}/todos/sync`, {
      method: "POST",
      body: {
        replace: true,
        todos: [],
        source_session_id: null,
        source_run_id: runId,
      },
    }))();
  await synced;
  let plannerTasks = [];
  let planSeedMode = "fallback_local";
  try {
    const llmPlan = await generatePlanTodosWithLLM(session, run, maxTasks);
    plannerTasks = normalizePlannerTasks(llmPlan.tasks, maxTasks, { linearFallback: false });
    if (!plannerTasks.length) throw new Error("LLM planner returned no valid tasks.");
    const seeded = await seedBlackboardTasks(session, runId, objective, plannerTasks, workflowId);
    planSeedMode = seeded?.mode === "steps_compat" ? "steps_llm_compat" : "blackboard_llm";
    await appendContextRunEvent(session, runId, "plan_seeded_llm", "planning", {
      source: "llm_objective_planner",
      session_id: llmPlan.sessionId || null,
      task_count: Number(seeded?.count || 0),
      dependency_edges: plannerTasks.reduce(
        (sum, task) =>
          sum + (Array.isArray(task?.dependsOnTaskIds) ? task.dependsOnTaskIds.length : 0),
        0
      ),
      workflow_id: workflowId,
      planner_target: planSeedMode,
    });
  } catch (planningError) {
    if (requireLlmPlan && !allowLocalPlannerFallback) {
      await appendContextRunEvent(session, runId, "plan_failed_llm_required", "failed", {
        reason: String(planningError?.message || planningError || "unknown planning failure"),
      }).catch(() => null);
      throw new Error(
        `LLM planning failed and fallback is disabled: ${String(planningError?.message || planningError || "unknown planning failure")}`
      );
    }
    plannerTasks = normalizePlannerTasks(parseObjectiveTodos(objective, maxTasks), maxTasks, {
      linearFallback: true,
    });
    if (!plannerTasks.length)
      plannerTasks = normalizePlannerTasks(["Execute requested objective"], 1, {
        linearFallback: false,
      });
    try {
      const seeded = await seedBlackboardTasks(session, runId, objective, plannerTasks, workflowId);
      planSeedMode = seeded?.mode === "steps_compat" ? "steps_local_compat" : "blackboard_local";
      await appendContextRunEvent(session, runId, "plan_seeded_local", "planning", {
        source: "local_objective_parser",
        task_count: Number(seeded?.count || 0),
        dependency_edges: plannerTasks.reduce(
          (sum, task) =>
            sum + (Array.isArray(task?.dependsOnTaskIds) ? task.dependsOnTaskIds.length : 0),
          0
        ),
        workflow_id: workflowId,
        planner_target: planSeedMode,
        note: `Fallback planner used: ${String(planningError?.message || planningError || "unknown planning failure")}`,
      });
    } catch (blackboardError) {
      await seedContextRunSteps(session, runId, objective);
      planSeedMode = "steps_fallback";
      await appendContextRunEvent(session, runId, "plan_seeded_local", "planning", {
        source: "local_objective_parser",
        note: `Fallback planner used: ${String(blackboardError?.message || blackboardError || "unknown planning failure")}`,
      });
    }
  }
  await appendContextRunEvent(session, runId, "plan_ready_for_approval", "awaiting_approval", {
    source_client: "control_panel",
    workflow_id: workflowId,
    max_agents: maxAgents,
    verification_mode: verificationMode,
    planner_mode: planSeedMode,
  });
  upsertSwarmRunController(runId, {
    status: "awaiting_approval",
    startedAt: Date.now(),
    stoppedAt: null,
    objective,
    workspaceRoot,
    maxTasks,
    maxAgents,
    workflowId,
    modelProvider,
    modelId,
    resolvedModelProvider: "",
    resolvedModelId: "",
    modelResolutionSource: "deferred",
    mcpServers,
    repoRoot: workspaceRoot,
    verificationMode,
    lastError: "",
    executorMode: planSeedMode.startsWith("blackboard") ? "blackboard" : "context_steps",
    executorState: "idle",
    executorReason: "",
    attachedPid: null,
    registryCache: null,
  });
  setActiveSwarmRunId(runId);
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

const handleSwarmApi = createSwarmApiHandler({
  PORTAL_PORT,
  REPO_ROOT,
  ENGINE_URL,
  swarmState,
  isLocalEngineUrl,
  sendJson,
  readJsonBody,
  workspaceExistsAsDirectory,
  loadHiddenSwarmRunIds,
  saveHiddenSwarmRunIds,
  engineRequestJson,
  appendContextRunEvent,
  contextRunStatusToSwarmStatus,
  startSwarm,
  detectExecutorMode,
  startRunExecutor,
  requeueInProgressSteps,
  transitionBlackboardTask,
  contextRunSnapshot,
  contextRunToTasks,
  getSwarmRunController,
  upsertSwarmRunController,
  setActiveSwarmRunId,
});

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
      sendJson(res, 401, {
        ok: false,
        error: "Session token is no longer valid for the configured engine.",
      });
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

  if (pathname.startsWith("/api/swarm") || pathname.startsWith("/api/orchestrator")) {
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
  if (serviceOp) {
    await operateServices(serviceOp, serviceMode);
    if (serviceSetupOnly && !installServicesRequested) {
      return;
    }
  }

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
    log(`Build:         ${CONTROL_PANEL_BUILD_FINGERPRINT}`);
    log("=========================================");
  });
}

main().catch((e) => {
  err(e instanceof Error ? e.message : String(e));
  process.exit(1);
});
