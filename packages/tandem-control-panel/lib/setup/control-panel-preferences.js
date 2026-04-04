import { existsSync, readFileSync, writeFileSync } from "fs";
import { mkdir } from "fs/promises";
import { dirname, resolve } from "path";
import {
  DEFAULT_WORKFLOW_SORT_MODE,
  normalizeFavoriteAutomationIds,
  normalizeWorkflowSortMode,
} from "../automations/workflow-list.js";

const DEFAULT_CONTROL_PANEL_PREFERENCES = {
  version: 1,
  principals: {},
};

function normalizePrincipalPreferences(raw = {}, principalId = "") {
  const input = raw && typeof raw === "object" ? raw : {};
  const createdAtMs = Number(
    input.created_at_ms || input.createdAtMs || input.createdAt || input.created_at || 0
  );
  const updatedAtMs = Number(
    input.updated_at_ms || input.updatedAtMs || input.updatedAt || input.updated_at || 0
  );
  return {
    principal_id: String(input.principal_id || principalId || "").trim(),
    principal_scope: String(input.principal_scope || input.principalScope || "global").trim() || "global",
    scope: input.scope && typeof input.scope === "object" ? { ...input.scope } : { kind: "global" },
    created_at_ms: Number.isFinite(createdAtMs) && createdAtMs > 0 ? createdAtMs : Date.now(),
    updated_at_ms: Number.isFinite(updatedAtMs) && updatedAtMs > 0 ? updatedAtMs : Date.now(),
    favorite_automation_ids: normalizeFavoriteAutomationIds(
      input.favorite_automation_ids || input.favoriteAutomationIds || []
    ),
    workflow_sort_mode: normalizeWorkflowSortMode(
      input.workflow_sort_mode || input.workflowSortMode || DEFAULT_WORKFLOW_SORT_MODE
    ),
  };
}

function normalizeControlPanelPreferences(raw = {}) {
  const input = raw && typeof raw === "object" ? raw : {};
  const principals = input.principals && typeof input.principals === "object" ? input.principals : {};
  const normalizedPrincipals = {};
  for (const [principalId, prefs] of Object.entries(principals)) {
    const key = String(principalId || "").trim();
    if (!key) continue;
    normalizedPrincipals[key] = normalizePrincipalPreferences(prefs, key);
  }
  return {
    version: 1,
    principals: normalizedPrincipals,
  };
}

function resolveControlPanelPreferencesPath(options = {}) {
  const env = options.env || process.env;
  const explicit = String(
    options.explicitPath ||
      env.TANDEM_CONTROL_PANEL_PREFERENCES_FILE ||
      env.TANDEM_CONTROL_PANEL_PREFERENCES_PATH ||
      ""
  ).trim();
  if (explicit) return resolve(explicit);
  const stateDir = String(options.stateDir || env.TANDEM_CONTROL_PANEL_STATE_DIR || "").trim();
  const fallbackStateDir = stateDir || resolve(process.cwd(), "tandem-data", "control-panel");
  return resolve(fallbackStateDir, "control-panel-preferences.json");
}

function readControlPanelPreferences(pathname, fallback = DEFAULT_CONTROL_PANEL_PREFERENCES) {
  const target = String(pathname || "").trim();
  if (!target || !existsSync(target)) {
    return normalizeControlPanelPreferences(fallback);
  }
  try {
    const raw = JSON.parse(readFileSync(target, "utf8"));
    return normalizeControlPanelPreferences(raw);
  } catch {
    return normalizeControlPanelPreferences(fallback);
  }
}

async function writeControlPanelPreferences(pathname, payload) {
  const target = resolve(String(pathname || "").trim());
  const data = normalizeControlPanelPreferences(payload);
  await mkdir(dirname(target), { recursive: true });
  writeFileSync(target, `${JSON.stringify(data, null, 2)}\n`, "utf8");
  return { path: target, preferences: data };
}

function getPrincipalPreferences(store, principalId) {
  const id = String(principalId || "").trim();
  if (!id) return normalizePrincipalPreferences({}, "");
  return normalizePrincipalPreferences(store?.principals?.[id] || {}, id);
}

function upsertPrincipalPreferences(store, principalId, patch = {}) {
  const normalizedStore = normalizeControlPanelPreferences(store);
  const id = String(principalId || "").trim();
  const current = getPrincipalPreferences(normalizedStore, id);
  const next = normalizePrincipalPreferences(
    {
      ...current,
      ...patch,
      principal_id: id,
      updated_at_ms: Date.now(),
    },
    id
  );
  return {
    ...normalizedStore,
    principals: {
      ...normalizedStore.principals,
      [id]: next,
    },
  };
}

export {
  DEFAULT_CONTROL_PANEL_PREFERENCES,
  getPrincipalPreferences,
  normalizeControlPanelPreferences,
  normalizePrincipalPreferences,
  readControlPanelPreferences,
  resolveControlPanelPreferencesPath,
  upsertPrincipalPreferences,
  writeControlPanelPreferences,
};
