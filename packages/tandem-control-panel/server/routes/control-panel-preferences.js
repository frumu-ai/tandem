import {
  getPrincipalPreferences,
  readControlPanelPreferences,
  resolveControlPanelPreferencesPath,
  upsertPrincipalPreferences,
  writeControlPanelPreferences,
} from "../../lib/setup/control-panel-preferences.js";

function resolvePrincipalIdentity(deps, session) {
  const resolver = deps.resolvePrincipalIdentity;
  if (typeof resolver === "function") return resolver(session);
  return { principal_id: "", principal_source: "unknown", principal_scope: "global" };
}

export function createControlPanelPreferencesHandler(deps) {
  const {
    CONTROL_PANEL_PREFERENCES_FILE,
    TANDEM_CONTROL_PANEL_STATE_DIR,
    sendJson,
    readJsonBody,
  } = deps;

  function getPreferencesPath() {
    return resolveControlPanelPreferencesPath({
      env: {
        TANDEM_CONTROL_PANEL_PREFERENCES_FILE: CONTROL_PANEL_PREFERENCES_FILE,
        TANDEM_CONTROL_PANEL_STATE_DIR,
      },
      explicitPath: CONTROL_PANEL_PREFERENCES_FILE,
      stateDir: TANDEM_CONTROL_PANEL_STATE_DIR,
    });
  }

  function loadPreferences() {
    return readControlPanelPreferences(getPreferencesPath());
  }

  function persistPreferences(preferences) {
    return writeControlPanelPreferences(getPreferencesPath(), preferences);
  }

  return async function handleControlPanelPreferences(req, res, session) {
    const url = new URL(req.url, "http://127.0.0.1");
    const principal = resolvePrincipalIdentity(deps, session);
    const principalId = String(principal?.principal_id || "").trim();
    if (!principalId) {
      sendJson(res, 401, { ok: false, error: "Session principal could not be resolved." });
      return true;
    }

    if (url.pathname === "/api/control-panel/preferences" && req.method === "GET") {
      const store = loadPreferences();
      const preferences = getPrincipalPreferences(store, principalId);
      sendJson(res, 200, {
        ok: true,
        principal_id: principalId,
        principal_source: String(principal?.principal_source || "unknown"),
        principal_scope: String(principal?.principal_scope || preferences.principal_scope || "global"),
        preferences,
      });
      return true;
    }

    if (url.pathname === "/api/control-panel/preferences" && req.method === "PATCH") {
      try {
        const payload = await readJsonBody(req);
        const incoming = payload?.preferences && typeof payload.preferences === "object" ? payload.preferences : payload || {};
        const store = loadPreferences();
        const current = getPrincipalPreferences(store, principalId);
        const nextPreferences = upsertPrincipalPreferences(store, principalId, {
          ...current,
          ...incoming,
          principal_id: principalId,
          principal_scope:
            String(incoming.principal_scope || incoming.principalScope || current.principal_scope || "global")
              .trim() || "global",
        });
        const saved = await persistPreferences(nextPreferences);
        sendJson(res, 200, {
          ok: true,
          principal_id: principalId,
          principal_source: String(principal?.principal_source || "unknown"),
          principal_scope: String(principal?.principal_scope || "global"),
          preferences: getPrincipalPreferences(saved.preferences, principalId),
        });
      } catch (error) {
        sendJson(res, 400, {
          ok: false,
          error: error instanceof Error ? error.message : String(error),
        });
      }
      return true;
    }

    return false;
  };
}
