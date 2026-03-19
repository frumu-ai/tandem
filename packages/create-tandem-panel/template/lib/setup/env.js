import { existsSync, readFileSync, writeFileSync } from "fs";
import { mkdir } from "fs/promises";
import { randomBytes } from "crypto";
import { dirname, resolve } from "path";

import { resolveSetupPaths } from "./paths.js";

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

function serializeEnv(entries) {
  return `${entries.map(([k, v]) => `${k}=${v}`).join("\n")}\n`;
}

function loadDotEnvFile(pathname, targetEnv = process.env) {
  if (!pathname || !existsSync(pathname)) return false;
  const parsed = parseDotEnv(readFileSync(pathname, "utf8"));
  for (const [key, value] of Object.entries(parsed)) {
    if (targetEnv[key] === undefined) targetEnv[key] = value;
  }
  return true;
}

function resolveEnvLoadOrder(options = {}) {
  const env = options.env || process.env;
  const cwd = resolve(options.cwd || process.cwd());
  const paths = resolveSetupPaths({
    env,
    platform: options.platform,
    home: options.home,
    allowAmbientStateEnv: options.allowAmbientStateEnv,
  });
  const explicit = String(options.explicitEnvFile || env.TANDEM_CONTROL_PANEL_ENV_FILE || "").trim();
  const order = [];
  if (explicit) order.push(resolve(explicit));
  order.push(paths.envFile);
  order.push(resolve(cwd, ".env"));
  return [...new Set(order.filter(Boolean))];
}

function bootstrapDefaults(paths) {
  return {
    TANDEM_CONTROL_PANEL_PORT: "39732",
    TANDEM_CONTROL_PANEL_HOST: "127.0.0.1",
    TANDEM_CONTROL_PANEL_PUBLIC_URL: "",
    TANDEM_ENGINE_URL: "http://127.0.0.1:39731",
    TANDEM_ENGINE_HOST: "127.0.0.1",
    TANDEM_ENGINE_PORT: "39731",
    TANDEM_CONTROL_PANEL_AUTO_START_ENGINE: "1",
    TANDEM_CONTROL_PANEL_STATE_DIR: paths.controlPanelStateDir,
    TANDEM_STATE_DIR: paths.engineStateDir,
    TANDEM_CONTROL_PANEL_ENGINE_TOKEN: "tk_change_me",
    TANDEM_DISABLE_TOOL_GUARD_BUDGETS: "1",
    TANDEM_TOOL_ROUTER_ENABLED: "0",
    TANDEM_PROMPT_CONTEXT_HOOK_TIMEOUT_MS: "5000",
    TANDEM_PROVIDER_STREAM_CONNECT_TIMEOUT_MS: "30000",
    TANDEM_PROVIDER_STREAM_IDLE_TIMEOUT_MS: "90000",
    TANDEM_PERMISSION_WAIT_TIMEOUT_MS: "15000",
    TANDEM_TOOL_EXEC_TIMEOUT_MS: "45000",
    TANDEM_BASH_TIMEOUT_MS: "30000",
    TANDEM_CONTROL_PANEL_SESSION_TTL_MINUTES: "1440",
  };
}

async function ensureBootstrapEnv(options = {}) {
  const cwd = resolve(options.cwd || process.cwd());
  const paths = resolveSetupPaths({
    env: options.env || process.env,
    platform: options.platform,
    allowAmbientStateEnv: options.allowAmbientStateEnv,
  });
  const envPath = resolve(options.envPath || paths.envFile);
  const overwrite = options.overwrite === true;
  const cwdEnvPath = resolve(cwd, ".env");
  const sourceExamplePath = resolve(cwd, ".env.example");
  const existing = existsSync(envPath) ? parseDotEnv(readFileSync(envPath, "utf8")) : {};
  const cwdEnv =
    options.allowCwdEnvMerge !== false && envPath !== cwdEnvPath && existsSync(cwdEnvPath)
      ? parseDotEnv(readFileSync(cwdEnvPath, "utf8"))
      : {};
  const example = options.allowCwdEnvMerge !== false && existsSync(sourceExamplePath)
    ? parseDotEnv(readFileSync(sourceExamplePath, "utf8"))
    : {};
  const defaults = { ...bootstrapDefaults(paths), ...example };
  const merged = { ...defaults, ...cwdEnv, ...existing };

  if (
    overwrite ||
    !merged.TANDEM_CONTROL_PANEL_ENGINE_TOKEN ||
    merged.TANDEM_CONTROL_PANEL_ENGINE_TOKEN === "tk_change_me"
  ) {
    merged.TANDEM_CONTROL_PANEL_ENGINE_TOKEN = `tk_${randomBytes(16).toString("hex")}`;
  }

  merged.TANDEM_CONTROL_PANEL_STATE_DIR =
    merged.TANDEM_CONTROL_PANEL_STATE_DIR || paths.controlPanelStateDir;
  merged.TANDEM_STATE_DIR = merged.TANDEM_STATE_DIR || paths.engineStateDir;
  merged.TANDEM_CONTROL_PANEL_HOST = merged.TANDEM_CONTROL_PANEL_HOST || "127.0.0.1";

  const preferredOrder = [
    "TANDEM_CONTROL_PANEL_PORT",
    "TANDEM_CONTROL_PANEL_HOST",
    "TANDEM_CONTROL_PANEL_PUBLIC_URL",
    "TANDEM_ENGINE_URL",
    "TANDEM_ENGINE_HOST",
    "TANDEM_ENGINE_PORT",
    "TANDEM_STATE_DIR",
    "TANDEM_CONTROL_PANEL_STATE_DIR",
    "TANDEM_CONTROL_PANEL_AUTO_START_ENGINE",
    "TANDEM_CONTROL_PANEL_ENGINE_TOKEN",
    "TANDEM_CONTROL_PANEL_SESSION_TTL_MINUTES",
  ];

  const ordered = [];
  for (const key of preferredOrder) {
    if (merged[key] !== undefined) ordered.push([key, merged[key]]);
  }
  for (const [key, value] of Object.entries(merged)) {
    if (!preferredOrder.includes(key)) ordered.push([key, value]);
  }

  await mkdir(dirname(envPath), { recursive: true });
  await mkdir(paths.logsDir, { recursive: true });
  await mkdir(paths.engineStateDir, { recursive: true });
  await mkdir(paths.controlPanelStateDir, { recursive: true });
  writeFileSync(envPath, serializeEnv(ordered), "utf8");

  return {
    envPath,
    token: merged.TANDEM_CONTROL_PANEL_ENGINE_TOKEN,
    engineUrl:
      merged.TANDEM_ENGINE_URL ||
      `http://${merged.TANDEM_ENGINE_HOST || "127.0.0.1"}:${merged.TANDEM_ENGINE_PORT || "39731"}`,
    panelHost: merged.TANDEM_CONTROL_PANEL_HOST || "127.0.0.1",
    panelPort: merged.TANDEM_CONTROL_PANEL_PORT || "39732",
    paths,
    env: merged,
  };
}

async function bootstrapEngineConfig(options = {}) {
  const env = options.env || {};
  const stateDir = resolve(String(env.TANDEM_STATE_DIR || options.stateDir || "").trim() || ".");
  const configPath = resolve(stateDir, "config.json");
  if (existsSync(configPath)) return { configPath, created: false };
  await mkdir(dirname(configPath), { recursive: true });
  writeFileSync(
    configPath,
    JSON.stringify(
      {
        default_provider: "openrouter",
        providers: {
          openrouter: { default_model: "google/gemini-2.5-pro-preview" },
          openai: { default_model: "gpt-4o-mini" },
          anthropic: { default_model: "claude-sonnet-4-5-latest" },
        },
      },
      null,
      2
    ),
    "utf8"
  );
  return { configPath, created: true };
}

export {
  bootstrapEngineConfig,
  ensureBootstrapEnv,
  loadDotEnvFile,
  parseDotEnv,
  resolveEnvLoadOrder,
  serializeEnv,
};
