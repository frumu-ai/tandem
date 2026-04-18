import { existsSync, readFileSync, writeFileSync } from "fs";
import { mkdir } from "fs/promises";
import { dirname, resolve } from "path";

const DEFAULT_CONTROL_PANEL_CONFIG = {
  version: 1,
  control_panel: {
    mode: "auto",
    aca_compact_nav: true,
  },
  agent: {
    name: "ACA",
    dry_run: false,
  },
  tandem: {
    base_url: "http://127.0.0.1:39733",
    token_env: "TANDEM_API_TOKEN",
    token_file: "secrets/tandem_api_token",
    required_version: "",
    startup_mode: "reuse_or_start",
    update_policy: "notify",
    engine_command: "scripts/tandem-engine-serve.sh",
  },
  hosted: {
    managed: false,
    provider: "",
    deployment_id: "",
    deployment_slug: "",
    hostname: "",
    public_url: "",
    control_plane_url: "",
    release_version: "",
    release_channel: "",
    engine_image: "",
    aca_image: "",
    control_panel_image: "",
    proxy_image: "",
    update_policy: "manual",
  },
  task_source: {
    type: "kanban_board",
    owner: "",
    repo: "",
    project: "",
    item: "",
    url: "",
    path: "config/board.yaml",
    prompt: "",
    source_name: "",
    card_id: "",
    payload: {},
  },
  repository: {
    path: "",
    slug: "",
    clone_url: "",
    default_branch: "main",
    worktree_root: "",
    remote_name: "origin",
  },
  provider: {
    id: "openai",
    model: "gpt-4.1-mini",
    base_url: "",
    fallback_provider: "",
    fallback_model: "",
  },
  execution: {
    backend: "auto",
  },
  swarm: {
    enabled: false,
    shared_model: false,
    max_workers: 3,
    max_retries: 1,
    manager: { provider: "", model: "" },
    worker: { provider: "", model: "" },
    reviewer: { provider: "", model: "" },
    tester: { provider: "", model: "" },
  },
  output: {
    root: "runs",
  },
  github_mcp: {
    enabled: true,
    url: "https://api.githubcopilot.com/mcp/",
    toolsets: "default,projects",
    scope: "intake_finalize",
    remote_sync: "status_comment",
  },
};

function deepMerge(base, overlay) {
  if (Array.isArray(base) && Array.isArray(overlay)) {
    return overlay.slice();
  }
  if (
    base &&
    overlay &&
    typeof base === "object" &&
    typeof overlay === "object" &&
    !Array.isArray(base) &&
    !Array.isArray(overlay)
  ) {
    const out = { ...base };
    for (const [key, value] of Object.entries(overlay)) {
      out[key] = key in out ? deepMerge(out[key], value) : value;
    }
    return out;
  }
  return overlay === undefined ? base : overlay;
}

function normalizeControlPanelConfig(raw = {}) {
  const input = raw && typeof raw === "object" ? raw : {};
  return deepMerge(DEFAULT_CONTROL_PANEL_CONFIG, input);
}

function resolveControlPanelConfigPath(options = {}) {
  const env = options.env || process.env;
  const explicit = String(
    options.explicitPath || env.TANDEM_CONTROL_PANEL_CONFIG_FILE || ""
  ).trim();
  if (explicit) return resolve(explicit);
  const stateDir = String(
    options.stateDir || env.TANDEM_CONTROL_PANEL_STATE_DIR || ""
  ).trim();
  const fallbackStateDir = stateDir || resolve(process.cwd(), "tandem-data", "control-panel");
  return resolve(fallbackStateDir, "control-panel-config.json");
}

function readControlPanelConfig(pathname, fallback = DEFAULT_CONTROL_PANEL_CONFIG) {
  const target = String(pathname || "").trim();
  if (!target || !existsSync(target)) {
    return normalizeControlPanelConfig(fallback);
  }
  try {
    const raw = JSON.parse(readFileSync(target, "utf8"));
    return normalizeControlPanelConfig(raw);
  } catch {
    return normalizeControlPanelConfig(fallback);
  }
}

async function writeControlPanelConfig(pathname, payload) {
  const target = resolve(String(pathname || "").trim());
  const data = normalizeControlPanelConfig(payload);
  await mkdir(dirname(target), { recursive: true });
  writeFileSync(target, `${JSON.stringify(data, null, 2)}\n`, "utf8");
  return { path: target, config: data };
}

function resolveControlPanelMode({
  config,
  envMode,
  acaAvailable,
} = {}) {
  const normalizedEnvMode = String(envMode || "").trim().toLowerCase();
  const configMode = String(config?.control_panel?.mode || "").trim().toLowerCase();
  const explicitMode = ["aca", "standalone"].includes(normalizedEnvMode)
    ? normalizedEnvMode
    : "";
  const requestedMode = explicitMode || configMode;
  if (requestedMode === "aca" || requestedMode === "standalone") {
    return {
      mode: requestedMode,
      source: explicitMode ? "env" : "config",
      reason: explicitMode ? `forced via TANDEM_CONTROL_PANEL_MODE=${requestedMode}` : "",
    };
  }
  return {
    mode: acaAvailable ? "aca" : "standalone",
    source: "detected",
    reason: acaAvailable
      ? "ACA integration detected on startup."
      : "ACA integration not detected; using the standalone setup profile.",
  };
}

function summarizeControlPanelConfig(config) {
  const normalized = normalizeControlPanelConfig(config);
  const missing = [];
  if (
    !String(normalized.repository.path || "").trim() &&
    !String(normalized.repository.slug || "").trim() &&
    !String(normalized.repository.clone_url || "").trim()
  ) {
    missing.push("repository");
  }
  if (!String(normalized.task_source.type || "").trim()) {
    missing.push("task_source");
  }
  if (!String(normalized.provider.id || "").trim() || !String(normalized.provider.model || "").trim()) {
    missing.push("provider");
  }
  return {
    ...normalized,
    hosted: normalized.hosted || {},
    missing,
    ready: missing.length === 0,
  };
}

export {
  DEFAULT_CONTROL_PANEL_CONFIG,
  deepMerge,
  normalizeControlPanelConfig,
  readControlPanelConfig,
  resolveControlPanelConfigPath,
  resolveControlPanelMode,
  summarizeControlPanelConfig,
  writeControlPanelConfig,
};
