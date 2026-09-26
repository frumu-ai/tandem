#!/usr/bin/env node

const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawn } = require("child_process");

const packageInfo = require("../package.json");

const ENGINE_ENTRYPOINT = path.join(__dirname, "tandem-engine.js");
const ENGINE_PACKAGE = "@frumu/tandem";
const PANEL_PACKAGE = "@frumu/tandem-panel";
const PANEL_COMMANDS = ["tandem-setup", "tandem-control-panel"];
const DEFAULT_ENGINE_HOST = "127.0.0.1";
const DEFAULT_ENGINE_PORT = 39731;
const DEFAULT_PANEL_HOST = "127.0.0.1";
const DEFAULT_PANEL_PORT = 39732;
const ENGINE_UNIT_NAME = "tandem-engine.service";
const ENGINE_LAUNCHD_LABEL = "ai.frumu.tandem.engine";
const WINDOWS_TASK_NAME = "TandemEngine";

function parseArgs(argv) {
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

function findCommandOnPath(command, env = process.env) {
  const pathValue = String(env.PATH || env.Path || "").trim();
  if (!pathValue) return "";
  const dirs = pathValue.split(path.delimiter).filter(Boolean);
  const candidates = [command];
  if (process.platform === "win32" && !path.extname(command)) {
    const exts = String(env.PATHEXT || ".EXE;.CMD;.BAT;.COM")
      .split(";")
      .map((ext) => ext.trim())
      .filter(Boolean);
    for (const ext of exts) candidates.push(`${command}${ext.toLowerCase()}`);
  }
  for (const dir of dirs) {
    for (const candidate of candidates) {
      const full = path.join(dir, candidate);
      try {
        const stat = fs.statSync(full);
        if (stat.isFile()) return full;
      } catch {}
    }
  }
  return "";
}

function detectPackageManager(env = process.env) {
  const agent = String(env.npm_config_user_agent || "").trim();
  if (agent.startsWith("pnpm/")) {
    return {
      name: "pnpm",
      installArgs: ["add", "-g"],
      updateArgs: ["add", "-g"],
      removeArgs: ["remove", "-g"],
    };
  }
  if (agent.startsWith("yarn/")) {
    return {
      name: "yarn",
      installArgs: ["global", "add"],
      updateArgs: ["global", "add"],
      removeArgs: ["global", "remove"],
    };
  }
  if (agent.startsWith("bun/")) {
    return {
      name: "bun",
      installArgs: ["add", "-g"],
      updateArgs: ["add", "-g"],
      removeArgs: ["remove", "-g"],
    };
  }
  return {
    name: "npm",
    installArgs: ["install", "-g"],
    updateArgs: ["install", "-g"],
    removeArgs: ["uninstall", "-g"],
  };
}

function resolveTandemHomeDir(env = process.env, platform = process.platform) {
  const override = String(env.TANDEM_HOME || env.TANDEM_STATE_DIR || "").trim();
  if (override) return path.resolve(override);
  if (platform === "darwin") {
    return path.join(os.homedir(), "Library", "Application Support", "tandem");
  }
  if (platform === "win32") {
    const base = String(env.APPDATA || "").trim() || path.join(os.homedir(), "AppData", "Roaming");
    return path.join(base, "tandem");
  }
  const base =
    String(env.XDG_DATA_HOME || "").trim() || path.join(os.homedir(), ".local", "share");
  return path.join(base, "tandem");
}

function resolveTandemPaths(env = process.env, platform = process.platform) {
  const home = resolveTandemHomeDir(env, platform);
  return {
    home,
    logsDir: path.join(home, "logs"),
    configPath: path.join(home, "config.json"),
    stateDir: home,
    panelPort: Number.parseInt(String(env.TANDEM_CONTROL_PANEL_PORT || DEFAULT_PANEL_PORT), 10) || DEFAULT_PANEL_PORT,
    panelHost: String(env.TANDEM_CONTROL_PANEL_HOST || DEFAULT_PANEL_HOST).trim() || DEFAULT_PANEL_HOST,
    enginePort: Number.parseInt(String(env.TANDEM_ENGINE_PORT || DEFAULT_ENGINE_PORT), 10) || DEFAULT_ENGINE_PORT,
    engineHost: String(env.TANDEM_ENGINE_HOST || DEFAULT_ENGINE_HOST).trim() || DEFAULT_ENGINE_HOST,
    panelPublicUrl: String(env.TANDEM_CONTROL_PANEL_PUBLIC_URL || "").trim(),
  };
}

function runCommand(bin, args = [], options = {}) {
  return new Promise((resolvePromise, rejectPromise) => {
    const child = spawn(bin, args, {
      env: options.env || process.env,
      cwd: options.cwd || process.cwd(),
      stdio: options.stdio || "inherit",
      shell: Boolean(options.shell),
    });
    let stdout = "";
    let stderr = "";
    let timedOut = false;
    let timer = null;
    if (Number.isFinite(options.timeoutMs) && options.timeoutMs > 0) {
      timer = setTimeout(() => {
        timedOut = true;
        try {
          child.kill("SIGKILL");
        } catch {}
      }, options.timeoutMs);
    }
    if (options.capture && child.stdout) {
      child.stdout.on("data", (chunk) => {
        stdout += chunk.toString("utf8");
      });
    }
    if (options.capture && child.stderr) {
      child.stderr.on("data", (chunk) => {
        stderr += chunk.toString("utf8");
      });
    }
    child.on("error", rejectPromise);
    child.on("close", (code) => {
      if (timer) clearTimeout(timer);
      if (timedOut) {
        const error = new Error(`${bin} ${args.join(" ")} timed out after ${options.timeoutMs}ms`);
        error.code = "ETIMEDOUT";
        error.stdout = stdout;
        error.stderr = stderr;
        rejectPromise(error);
        return;
      }
      if (code === 0) {
        resolvePromise({ code: 0, stdout, stderr });
        return;
      }
      const error = new Error(`${bin} ${args.join(" ")} exited ${code}${stderr ? `: ${stderr}` : ""}`);
      error.code = code;
      error.stdout = stdout;
      error.stderr = stderr;
      rejectPromise(error);
    });
  });
}

async function captureCommand(bin, args = [], options = {}) {
  return runCommand(bin, args, { ...options, capture: true, stdio: "pipe" });
}

function quoteShell(value) {
  const text = String(value || "");
  if (/^[A-Za-z0-9_./:@=+-]+$/.test(text)) return text;
  return `"${text.replace(/(["\\$`])/g, "\\$1")}"`;
}

function buildEngineServiceDefinition(paths, env = process.env) {
  const nodePath = process.execPath;
  const serviceUser = String(env.SUDO_USER || env.USER || os.userInfo().username || "").trim() || "root";
  const host = paths.engineHost;
  const port = String(paths.enginePort);
  const stateDir = paths.stateDir;
  const logPath = path.join(paths.logsDir, "engine.log");

  if (process.platform === "linux") {
    return {
      manager: "systemd",
      unitName: ENGINE_UNIT_NAME,
      unitPath: `/etc/systemd/system/${ENGINE_UNIT_NAME}`,
      logPath,
      content: `[Unit]
Description=Tandem Engine
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=${serviceUser}
WorkingDirectory=${stateDir}
Environment=TANDEM_STATE_DIR=${stateDir}
ExecStart=${quoteShell(nodePath)} ${quoteShell(ENGINE_ENTRYPOINT)} serve --hostname ${quoteShell(host)} --port ${quoteShell(port)} --state-dir ${quoteShell(stateDir)}
Restart=on-failure
RestartSec=5
StandardOutput=append:${logPath}
StandardError=append:${logPath}
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
`,
    };
  }

  if (process.platform === "darwin") {
    return {
      manager: "launchd",
      label: ENGINE_LAUNCHD_LABEL,
      plistPath: `/Library/LaunchDaemons/${ENGINE_LAUNCHD_LABEL}.plist`,
      logPath,
      content: `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
    <key>Label</key><string>${ENGINE_LAUNCHD_LABEL}</string>
    <key>UserName</key><string>${serviceUser}</string>
    <key>WorkingDirectory</key><string>${stateDir}</string>
    <key>EnvironmentVariables</key>
    <dict>
      <key>TANDEM_STATE_DIR</key><string>${stateDir}</string>
    </dict>
    <key>ProgramArguments</key>
    <array>
      <string>${nodePath}</string>
      <string>${ENGINE_ENTRYPOINT}</string>
      <string>serve</string>
      <string>--hostname</string>
      <string>${host}</string>
      <string>--port</string>
      <string>${port}</string>
      <string>--state-dir</string>
      <string>${stateDir}</string>
    </array>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key><true/>
    <key>ThrottleInterval</key><integer>5</integer>
    <key>StandardOutPath</key><string>${logPath}</string>
    <key>StandardErrorPath</key><string>${logPath}</string>
  </dict>
</plist>
`,
    };
  }

  if (process.platform === "win32") {
    const scriptsDir = path.join(stateDir, "scripts");
    const scriptPath = path.join(scriptsDir, "tandem-engine.ps1");
    const logPathWin = path.join(paths.logsDir, "engine.log");
    const ps1 = String.raw`$ErrorActionPreference = "Continue"
$node = "${process.execPath.replace(/\\/g, "\\\\")}"
$entry = "${ENGINE_ENTRYPOINT.replace(/\\/g, "\\\\")}"
$host = "${host}"
$port = "${port}"
$stateDir = "${stateDir.replace(/\\/g, "\\\\")}"
$logPath = "${logPathWin.replace(/\\/g, "\\\\")}"
New-Item -ItemType Directory -Force -Path (Split-Path $logPath) | Out-Null
New-Item -ItemType Directory -Force -Path $stateDir | Out-Null
while ($true) {
  Start-Process -FilePath $node -ArgumentList @($entry, "serve", "--hostname", $host, "--port", $port, "--state-dir", $stateDir) -NoNewWindow -Wait -RedirectStandardOutput $logPath -RedirectStandardError $logPath
  Start-Sleep -Seconds 5
}
`;
    return {
      manager: "scheduled-task",
      taskName: WINDOWS_TASK_NAME,
      scriptPath,
      logPath: logPathWin,
      content: ps1,
    };
  }

  throw new Error(`Unsupported platform: ${process.platform}`);
}

async function ensureEngineServiceInstalled(paths, env = process.env) {
  const def = buildEngineServiceDefinition(paths, env);
  if (process.platform === "linux") {
    if (typeof process.getuid === "function" && process.getuid() !== 0) {
      throw new Error("Installing the Tandem engine service on Linux requires root.");
    }
    fs.mkdirSync(paths.logsDir, { recursive: true });
    fs.writeFileSync(def.unitPath, def.content, "utf8");
    await runCommand("systemctl", ["daemon-reload"]);
    await runCommand("systemctl", ["enable", "--now", def.unitName]);
    return def;
  }
  if (process.platform === "darwin") {
    if (typeof process.getuid === "function" && process.getuid() !== 0) {
      throw new Error("Installing the Tandem engine service on macOS requires root.");
    }
    fs.mkdirSync(paths.logsDir, { recursive: true });
    fs.writeFileSync(def.plistPath, def.content, "utf8");
    await runCommand("launchctl", ["bootout", "system", def.plistPath]).catch(() => null);
    await runCommand("launchctl", ["bootstrap", "system", def.plistPath]);
    await runCommand("launchctl", ["kickstart", "-k", `system/${def.label}`]);
    return def;
  }
  if (process.platform === "win32") {
    fs.mkdirSync(path.dirname(def.scriptPath), { recursive: true });
    fs.mkdirSync(paths.logsDir, { recursive: true });
    fs.writeFileSync(def.scriptPath, def.content, "utf8");
    await runCommand("schtasks", [
      "/Create",
      "/TN",
      def.taskName,
      "/SC",
      "ONLOGON",
      "/RL",
      "HIGHEST",
      "/F",
      "/TR",
      `powershell -NoProfile -ExecutionPolicy Bypass -File "${def.scriptPath}"`,
    ]);
    await runCommand("schtasks", ["/Run", "/TN", def.taskName]).catch(() => null);
    return def;
  }
  throw new Error(`Unsupported platform: ${process.platform}`);
}

async function queryEngineServiceState(paths) {
  if (process.platform === "linux") {
    const result = {
      manager: "systemd",
      unitName: ENGINE_UNIT_NAME,
      installed: fs.existsSync(`/etc/systemd/system/${ENGINE_UNIT_NAME}`),
      active: false,
      enabled: false,
    };
    try {
      const active = await captureCommand("systemctl", ["is-active", ENGINE_UNIT_NAME]);
      result.active = String(active.stdout || "").trim() === "active";
    } catch {}
    try {
      const enabled = await captureCommand("systemctl", ["is-enabled", ENGINE_UNIT_NAME]);
      result.enabled = String(enabled.stdout || "").trim() === "enabled";
    } catch {}
    return result;
  }
  if (process.platform === "darwin") {
    const plistPath = `/Library/LaunchDaemons/${ENGINE_LAUNCHD_LABEL}.plist`;
    const result = {
      manager: "launchd",
      label: ENGINE_LAUNCHD_LABEL,
      installed: fs.existsSync(plistPath),
      active: false,
      enabled: fs.existsSync(plistPath),
    };
    try {
      const printed = await captureCommand("launchctl", ["print", `system/${ENGINE_LAUNCHD_LABEL}`]);
      result.active = /state\s*=\s*running/i.test(printed.stdout || "") || /pid = \d+/i.test(printed.stdout || "");
    } catch {}
    return result;
  }
  if (process.platform === "win32") {
    const result = {
      manager: "scheduled-task",
      taskName: WINDOWS_TASK_NAME,
      installed: true,
      active: false,
      enabled: true,
    };
    try {
      const printed = await captureCommand("schtasks", ["/Query", "/TN", WINDOWS_TASK_NAME, "/FO", "LIST", "/V"]);
      const statusLine = String(printed.stdout || "")
        .split(/\r?\n/)
        .find((line) => /^Status:/i.test(line));
      result.active = /Running/i.test(statusLine || "");
      result.installed = true;
    } catch {
      result.installed = false;
    }
    return result;
  }
  return {
    manager: "unknown",
    installed: false,
    active: false,
    enabled: false,
  };
}

async function probeUrl(url, timeoutMs = 1500) {
  try {
    const response = await fetch(url, { signal: AbortSignal.timeout(timeoutMs) });
    if (!response.ok) return null;
    return await response.json();
  } catch {
    return null;
  }
}

function addonInfo(name) {
  if (name !== "panel") return null;
  return {
    name: "panel",
    packageName: PANEL_PACKAGE,
    title: "Tandem Control Panel",
    commands: PANEL_COMMANDS,
    installHint: `npm i -g ${PANEL_PACKAGE}`,
  };
}

function getAddonCommand() {
  for (const command of PANEL_COMMANDS) {
    const resolved = findCommandOnPath(command);
    if (resolved) return { command, path: resolved };
  }
  return null;
}

function formatBadge(ok, textOk, textBad) {
  return ok ? textOk : textBad;
}

function printLines(lines) {
  for (const line of lines) console.log(line);
}

async function installPackage(pkgName, env = process.env) {
  const manager = detectPackageManager(env);
  const command = manager.name;
  const args = [...manager.installArgs, `${pkgName}@latest`];
  return runCommand(command, args);
}

async function updatePackage(pkgName, env = process.env) {
  const manager = detectPackageManager(env);
  const command = manager.name;
  const args = [...manager.updateArgs, `${pkgName}@latest`];
  return runCommand(command, args);
}

async function removePackage(pkgName, env = process.env) {
  const manager = detectPackageManager(env);
  const command = manager.name;
  const args = [...manager.removeArgs, pkgName];
  return runCommand(command, args);
}

async function runAddonCli(args, options = {}) {
  const addon = getAddonCommand();
  if (!addon) return null;
  return runCommand(addon.command, args, options);
}

async function runAddonDoctorJson() {
  const addon = getAddonCommand();
  if (!addon) return null;
  let result;
  try {
    result = await captureCommand(addon.command, ["doctor", "--json"], { timeoutMs: 2500 });
  } catch (error) {
    // An unhealthy runtime is a completed diagnosis, not missing output.
    // Spawn errors, signals and timeouts are not completed doctor reports.
    if (!Number.isInteger(error.code) || error.code <= 0) return null;
    result = error;
  }
  try {
    const report = JSON.parse(String(result.stdout || ""));
    if (!report || typeof report !== "object" || Array.isArray(report)) return null;
    // Accept completed unhealthy diagnoses, but not arbitrary JSON objects.
    // These are the fields consumed by status/open and the health contract.
    const health = report.engineHealth;
    if (typeof report.ok !== "boolean"
      || typeof report.panelHost !== "string" || !report.panelHost.trim()
      || !Number.isInteger(report.panelPort) || report.panelPort < 1 || report.panelPort > 65535
      || typeof report.panelPublicUrl !== "string"
      || typeof report.engineUrl !== "string"
      || (health !== null && (!health || typeof health !== "object" || Array.isArray(health)
        || typeof health.ready !== "boolean" || typeof health.healthy !== "boolean"))) return null;
    if (report.ok && (!health?.ready || !health?.healthy || !report.engineUrl.trim())) return null;
    return { ...report, doctorExitCode: result.code || (report.ok ? 0 : 1) };
  } catch {
    return null;
  }
}

async function buildDiagnostics(env = process.env) {
  const paths = resolveTandemPaths(env);
  const service = await queryEngineServiceState(paths);
  const engineHealth = await probeUrl(`http://${paths.engineHost}:${paths.enginePort}/global/health`);
  const addon = getAddonCommand();

  return {
    package: packageInfo.name,
    version: packageInfo.version,
    paths,
    service,
    engine: {
      url: `http://${paths.engineHost}:${paths.enginePort}`,
      health: engineHealth,
      reachable: Boolean(engineHealth),
    },
    addon: addon
      ? {
          installed: true,
          command: addon.command,
        }
      : {
          installed: false,
          installHint: addonInfo("panel").installHint,
        },
  };
}

function buildWorktreeCleanupPayload(cli) {
  const repoRoot = String(cli.value("repo-root") || "").trim();
  return {
    repo_root: repoRoot || undefined,
    dry_run: !cli.has("apply"),
    remove_orphan_dirs: !cli.has("keep-orphan-dirs"),
  };
}

async function requestWorktreeCleanup(cli, env = process.env) {
  const paths = resolveTandemPaths(env);
  const engineUrl = `http://${paths.engineHost}:${paths.enginePort}`;
  const response = await fetch(`${engineUrl}/worktree/cleanup`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(buildWorktreeCleanupPayload(cli)),
    signal: AbortSignal.timeout(15_000),
  });
  if (!response.ok) {
    const text = await response.text().catch(() => "");
    throw new Error(`Worktree cleanup failed (${response.status})${text ? `: ${text}` : ""}`);
  }
  return await response.json();
}

function printWorktreeCleanupReport(report, json = false) {
  if (json) {
    console.log(JSON.stringify(report, null, 2));
    return;
  }
  const staleCount = Array.isArray(report.stale_paths) ? report.stale_paths.length : 0;
  const activeCount = Array.isArray(report.active_paths) ? report.active_paths.length : 0;
  const removedCount =
    (Array.isArray(report.cleaned_worktrees) ? report.cleaned_worktrees.length : 0) +
    (Array.isArray(report.orphan_dirs_removed) ? report.orphan_dirs_removed.length : 0);
  const failureCount = Array.isArray(report.failures) ? report.failures.length : 0;
  printLines([
    `[Tandem] worktree cleanup: ${report.dry_run ? "preview" : "applied"}`,
    `[Tandem] repo root:         ${report.repo_root || "unknown"}`,
    `[Tandem] managed root:      ${report.managed_root || "unknown"}`,
    `[Tandem] active tracked:    ${activeCount}`,
    `[Tandem] stale candidates:  ${staleCount}`,
    `[Tandem] removed:           ${removedCount}`,
    `[Tandem] failures:          ${failureCount}`,
  ]);
  const logRows = [];
  for (const row of Array.isArray(report.cleaned_worktrees) ? report.cleaned_worktrees : []) {
    logRows.push(`  removed worktree: ${row.path}${row.branch ? ` (${row.branch})` : ""}`);
  }
  for (const row of Array.isArray(report.orphan_dirs_removed) ? report.orphan_dirs_removed : []) {
    logRows.push(`  removed orphan dir: ${row.path}`);
  }
  if (report.dry_run) {
    for (const row of Array.isArray(report.stale_paths) ? report.stale_paths : []) {
      logRows.push(`  stale candidate: ${row.path}${row.branch ? ` (${row.branch})` : ""}`);
    }
    for (const path of Array.isArray(report.orphan_dirs) ? report.orphan_dirs : []) {
      logRows.push(`  orphan dir: ${path}`);
    }
  }
  for (const row of Array.isArray(report.failures) ? report.failures : []) {
    logRows.push(`  failure: ${row.path || row.code || "unknown"}${row.error ? ` -> ${row.error}` : row.stderr ? ` -> ${row.stderr}` : ""}`);
  }
  if (logRows.length) printLines(logRows);
}

async function printDiagnostics(report, json = false) {
  if (json) {
    console.log(JSON.stringify(report, null, 2));
    return;
  }
  printLines([
    `[Tandem] workflow engine: ${formatBadge(report.engine.reachable, "online", "offline")}`,
    `[Tandem] engine url:      ${report.engine.url}`,
    `[Tandem] service manager: ${report.service.manager}${report.service.installed ? "" : " (not installed)"}`,
    `[Tandem] service state:   ${formatBadge(report.service.active, "running", "stopped")}`,
    report.addon.installed
      ? `[Tandem] panel add-on:    installed${report.addon.command ? ` (${report.addon.command})` : ""}`
      : `[Tandem] panel add-on:    missing. install with: ${report.addon.installHint}`,
  ]);
  if (report.engine.health) {
    printLines([
      `[Tandem] build:           ${report.engine.health.buildVersion || report.engine.health.version || "unknown"}`,
      `[Tandem] ready:           ${String(report.engine.health.ready === true)}`,
    ]);
  }
}

async function printStatus(report, json = false) {
  if (json) {
    console.log(JSON.stringify(report, null, 2));
    return;
  }
  const panelLine = report.addon.installed ? report.addon.command : `install with ${report.addon.installHint}`;
  printLines([
    `[Tandem] engine: ${formatBadge(report.engine.reachable, "online", "offline")} (${report.engine.url})`,
    `[Tandem] service: ${report.service.manager} ${formatBadge(report.service.active, "running", "stopped")}`,
    `[Tandem] panel:   ${report.addon.installed ? panelLine : `missing, ${panelLine}`}`,
  ]);
}

async function handleServiceCommand(subcommand, cli, env = process.env) {
  const paths = resolveTandemPaths(env);
  if (subcommand === "install") {
    const service = await ensureEngineServiceInstalled(paths, env);
    console.log(`[Tandem] installed engine service via ${service.manager}.`);
    console.log(`[Tandem] logs: ${service.logPath}`);
    return 0;
  }
  if (subcommand === "status") {
    const service = await queryEngineServiceState(paths);
    console.log(`[Tandem] engine service: ${service.manager} ${formatBadge(service.active, "running", "stopped")}`);
    console.log(`[Tandem] installed: ${service.installed ? "yes" : "no"}`);
    return 0;
  }
  if (subcommand === "logs") {
    const logPath = path.join(paths.logsDir, "engine.log");
    if (!fs.existsSync(logPath)) {
      console.log(`[Tandem] no log file yet at ${logPath}`);
      return 0;
    }
    const text = fs.readFileSync(logPath, "utf8");
    const lines = text.split(/\r?\n/).filter(Boolean).slice(-200);
    for (const line of lines) console.log(line);
    return 0;
  }
  if (!["start", "stop", "restart", "uninstall"].includes(subcommand)) {
    throw new Error(`Unknown service command: ${subcommand}`);
  }
  if (process.platform === "linux") {
    await runCommand("systemctl", [subcommand, ENGINE_UNIT_NAME]);
    return 0;
  }
  if (process.platform === "darwin") {
    if (subcommand === "uninstall") {
      await runCommand("launchctl", ["bootout", "system", `/Library/LaunchDaemons/${ENGINE_LAUNCHD_LABEL}.plist`]).catch(() => null);
      return 0;
    }
    if (subcommand === "restart") {
      await runCommand("launchctl", ["kickstart", "-k", `system/${ENGINE_LAUNCHD_LABEL}`]);
      return 0;
    }
    if (subcommand === "start") {
      await runCommand("launchctl", ["kickstart", `system/${ENGINE_LAUNCHD_LABEL}`]);
      return 0;
    }
    if (subcommand === "stop") {
      await runCommand("launchctl", ["bootout", "system", `/Library/LaunchDaemons/${ENGINE_LAUNCHD_LABEL}.plist`]).catch(() => null);
      return 0;
    }
  }
  if (process.platform === "win32") {
    if (subcommand === "start") {
      await runCommand("schtasks", ["/Run", "/TN", WINDOWS_TASK_NAME]);
      return 0;
    }
    if (subcommand === "stop") {
      await runCommand("schtasks", ["/End", "/TN", WINDOWS_TASK_NAME]).catch(() => null);
      return 0;
    }
    if (subcommand === "restart") {
      await runCommand("schtasks", ["/End", "/TN", WINDOWS_TASK_NAME]).catch(() => null);
      await runCommand("schtasks", ["/Run", "/TN", WINDOWS_TASK_NAME]);
      return 0;
    }
    if (subcommand === "uninstall") {
      await runCommand("schtasks", ["/Delete", "/TN", WINDOWS_TASK_NAME, "/F"]).catch(() => null);
      return 0;
    }
  }
  throw new Error(`Service command is not supported on ${process.platform}.`);
}

async function handlePanelCommand(subcommand, cli, env = process.env) {
  const addon = getAddonCommand();
  if (subcommand === "install") {
    return handleAddonCommand("install", "panel", cli, env);
  }
  if (!addon) {
    console.log(`[Tandem] panel add-on is not installed. Run: tandem install panel`);
    return 0;
  }
  if (subcommand === "open") {
    const report = await runAddonDoctorJson();
    const url = report?.panelPublicUrl
      || (report?.panelHost && report?.panelPort ? `http://${report.panelHost}:${report.panelPort}` : `http://${DEFAULT_PANEL_HOST}:${DEFAULT_PANEL_PORT}`);
    await openUrl(url);
    console.log(`[Tandem] opening ${url}`);
    return 0;
  }
  if (subcommand === "status" || subcommand === "doctor" || subcommand === "init" || subcommand === "run" || subcommand === "service") {
    const args = [subcommand, ...cli.argv.slice(1)];
    if (subcommand === "status") {
      const report = await runAddonDoctorJson();
      if (report) {
        console.log(`[Tandem] panel: http://${report.panelHost}:${report.panelPort}`);
        console.log(`[Tandem] engine: ${report.engineUrl}`);
        return report.doctorExitCode;
      }
      console.log(`[Tandem] panel add-on is installed but did not return a quick status response.`);
      console.log(`[Tandem] try: tandem panel doctor`);
      return 1;
    }
    await runCommand(addon.command, args, { stdio: "inherit" });
    return 0;
  }
  await runCommand(addon.command, [subcommand, ...cli.argv.slice(1)], { stdio: "inherit" });
  return 0;
}

async function handleAddonCommand(action, maybeName, cli, env = process.env) {
  const name = maybeName || String(cli.argv[0] || "").trim();
  const addon = addonInfo(name);
  if (!addon) {
    throw new Error(`Unknown add-on: ${name}`);
  }
  if (action === "list") {
    const installed = Boolean(getAddonCommand());
    console.log(`[Tandem] ${addon.name}: ${installed ? "installed" : "missing"}`);
    console.log(`[Tandem] package: ${addon.packageName}`);
    return 0;
  }
  if (action === "install") {
    await installPackage(addon.packageName, env);
    console.log(`[Tandem] installed ${addon.packageName}.`);
    console.log(`[Tandem] next: tandem panel init`);
    return 0;
  }
  if (action === "update") {
    await updatePackage(addon.packageName, env);
    console.log(`[Tandem] updated ${addon.packageName}.`);
    return 0;
  }
  if (action === "remove") {
    const addonCli = getAddonCommand();
    if (addonCli) {
      await runCommand(addonCli.command, ["service", "uninstall"]).catch(() => null);
    }
    await removePackage(addon.packageName, env);
    console.log(`[Tandem] removed ${addon.packageName}.`);
    return 0;
  }
  throw new Error(`Unknown add-on action: ${action}`);
}

async function openUrl(url) {
  if (process.platform === "darwin") {
    await runCommand("open", [url]);
    return;
  }
  if (process.platform === "linux") {
    await runCommand("xdg-open", [url]);
    return;
  }
  if (process.platform === "win32") {
    await runCommand("cmd", ["/c", "start", "", url]);
    return;
  }
  throw new Error(`Unsupported platform for browser open: ${process.platform}`);
}

async function handleInstallCommand(subcommand, cli, env = process.env) {
  if (subcommand === "panel") {
    return handleAddonCommand("install", "panel", cli, env);
  }
  throw new Error(`Unknown install target: ${subcommand}`);
}

async function handleUpdateCommand(cli, env = process.env) {
  await updatePackage(ENGINE_PACKAGE, env);
  console.log(`[Tandem] updated ${ENGINE_PACKAGE}.`);
  if (getAddonCommand()) {
    await updatePackage(PANEL_PACKAGE, env).catch(() => null);
    console.log(`[Tandem] updated ${PANEL_PACKAGE}.`);
  }
  console.log("[Tandem] restart the running service to pick up the new binaries.");
  return 0;
}

async function main(argv = process.argv.slice(2), env = process.env) {
  const cli = parseArgs(argv);
  const command = String(argv[0] || "").trim().toLowerCase();

  if (!command) {
    console.log(`[Tandem] ${packageInfo.name} ${packageInfo.version}`);
    console.log("[Tandem] Use: tandem doctor | tandem doctor worktrees | tandem status | tandem service install | tandem install panel");
    return 0;
  }

  if (command === "--help" || command === "-h" || command === "help") {
    console.log([
      "Tandem master CLI",
      "",
      "Commands:",
      "  tandem doctor",
      "  tandem doctor worktrees [--repo-root /abs/path] [--apply] [--json]",
      "  tandem status",
      "  tandem service install|start|stop|restart|status|logs",
      "  tandem install panel",
      "  tandem update",
      "  tandem panel status|open|init|service ...",
      "  tandem addon list|install|update|remove panel",
      "  tandem run|serve|tool|parallel|providers|browser|memory ...",
      "  tandem-engine serve --hostname 127.0.0.1 --port 39731",
    ].join("\n"));
    return 0;
  }

  if (command === "doctor") {
    const subcommand = String(argv[1] || "").trim().toLowerCase();
    if (subcommand === "worktrees" || subcommand === "worktree") {
      const report = await requestWorktreeCleanup(cli, env);
      printWorktreeCleanupReport(report, cli.has("json"));
      return Array.isArray(report.failures) && report.failures.length ? 1 : 0;
    }
    const report = await buildDiagnostics(env);
    await printDiagnostics(report, cli.has("json"));
    return report.engine.reachable ? 0 : 1;
  }

  if (command === "status") {
    const report = await buildDiagnostics(env);
    await printStatus(report, cli.has("json"));
    return 0;
  }

  if (command === "service") {
    const subcommand = String(argv[1] || "status").trim().toLowerCase();
    return handleServiceCommand(subcommand, cli, env);
  }

  if (command === "install") {
    const subcommand = String(argv[1] || "").trim().toLowerCase();
    return handleInstallCommand(subcommand, cli, env);
  }

  if (command === "update") {
    const subcommand = String(argv[1] || "").trim().toLowerCase();
    if (subcommand === "panel") {
      return handleAddonCommand("update", "panel", cli, env);
    }
    return handleUpdateCommand(cli, env);
  }

  if (command === "addon" || command === "addons") {
    const action = String(argv[1] || "list").trim().toLowerCase();
    const name = String(argv[2] || "panel").trim().toLowerCase();
    if (action === "list") return handleAddonCommand("list", name, cli, env);
    if (action === "install") return handleAddonCommand("install", name, cli, env);
    if (action === "update") return handleAddonCommand("update", name, cli, env);
    if (action === "remove") return handleAddonCommand("remove", name, cli, env);
    throw new Error(`Unknown add-on action: ${action}`);
  }

  if (command === "panel") {
    const subcommand = String(argv[1] || "status").trim().toLowerCase();
    return handlePanelCommand(subcommand, { argv: argv.slice(1) }, env);
  }

  if (command === "run") {
    await runCommand(process.execPath, [ENGINE_ENTRYPOINT, ...argv], { stdio: "inherit" });
    return 0;
  }

  if (command === "tandem-engine" || command === "engine") {
    await runCommand(process.execPath, [ENGINE_ENTRYPOINT, ...argv.slice(1)], {
      stdio: "inherit",
    });
    return 0;
  }

  await runCommand(process.execPath, [ENGINE_ENTRYPOINT, ...argv], { stdio: "inherit" });
  return 0;
}

if (require.main === module) {
  main().then((code) => {
    if (typeof code === "number") process.exit(code);
    process.exit(0);
  }).catch((error) => {
    console.error(`[Tandem] ERROR: ${error instanceof Error ? error.message : String(error)}`);
    process.exit(1);
  });
}

module.exports = {
  addonInfo,
  buildEngineServiceDefinition,
  buildDiagnostics,
  detectPackageManager,
  findCommandOnPath,
  handleAddonCommand,
  handleInstallCommand,
  handlePanelCommand,
  handleServiceCommand,
  handleUpdateCommand,
  main,
  openUrl,
  parseArgs,
  printDiagnostics,
  printStatus,
  printWorktreeCleanupReport,
  queryEngineServiceState,
  requestWorktreeCleanup,
  resolveTandemHomeDir,
  resolveTandemPaths,
  runAddonCli,
  runCommand,
  buildWorktreeCleanupPayload,
  updatePackage,
};
