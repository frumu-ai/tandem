import {
  readControlPanelConfig,
  resolveControlPanelConfigPath,
  resolveControlPanelMode,
  summarizeControlPanelConfig,
  writeControlPanelConfig,
} from "../../lib/setup/control-panel-config.js";

export function createControlPanelConfigHandler(deps) {
  const { CONTROL_PANEL_CONFIG_FILE, CONTROL_PANEL_MODE, ACA_BASE_URL, getAcaToken, sendJson } =
    deps;

  function getConfigPath() {
    return resolveControlPanelConfigPath({
      env: {
        TANDEM_CONTROL_PANEL_CONFIG_FILE: CONTROL_PANEL_CONFIG_FILE,
        TANDEM_CONTROL_PANEL_STATE_DIR: deps.TANDEM_CONTROL_PANEL_STATE_DIR,
      },
      explicitPath: CONTROL_PANEL_CONFIG_FILE,
      stateDir: deps.TANDEM_CONTROL_PANEL_STATE_DIR,
    });
  }

  async function loadInstallProfile() {
    const configPath = getConfigPath();
    const config = readControlPanelConfig(configPath);
    const baseUrl = String(ACA_BASE_URL || "").trim();
    const token = String(getAcaToken?.() || "").trim();
    let acaAvailable = false;
    let acaReason = "aca_not_configured";
    if (baseUrl) {
      try {
        const controller = new AbortController();
        const timer = setTimeout(() => controller.abort(), Number(deps.PROBE_TIMEOUT_MS || 5000));
        const res = await fetch(`${baseUrl.replace(/\/+$/, "")}/health`, {
          method: "GET",
          signal: controller.signal,
          headers: {
            Accept: "application/json",
            ...(token ? { Authorization: `Bearer ${token}` } : {}),
          },
        });
        clearTimeout(timer);
        acaAvailable = !!res.ok;
        acaReason = res.ok ? "" : `aca_health_failed_${res.status}`;
      } catch (error) {
        acaReason = String(error?.message || error || "aca_probe_error");
      }
    }
    const mode = resolveControlPanelMode({
      config,
      envMode: CONTROL_PANEL_MODE,
      acaAvailable,
    });
    const summary = summarizeControlPanelConfig(config);
    return {
      ok: true,
      control_panel_mode: mode.mode,
      control_panel_mode_source: mode.source,
      control_panel_mode_reason: mode.reason || "",
      aca_integration: acaAvailable,
      aca_reason: acaReason,
      control_panel_config_path: configPath,
      control_panel_config_ready: summary.ready,
      control_panel_config_missing: summary.missing,
      control_panel_compact_nav: !!summary.control_panel?.aca_compact_nav,
      hosted_managed: summary.hosted?.managed === true,
      hosted_provider: String(summary.hosted?.provider || "").trim(),
      hosted_deployment_id: String(summary.hosted?.deployment_id || "").trim(),
      hosted_deployment_slug: String(summary.hosted?.deployment_slug || "").trim(),
      hosted_hostname: String(summary.hosted?.hostname || "").trim(),
      hosted_public_url: String(summary.hosted?.public_url || "").trim(),
      hosted_control_plane_url: String(summary.hosted?.control_plane_url || "").trim(),
      hosted_release_version: String(summary.hosted?.release_version || "").trim(),
      hosted_release_channel: String(summary.hosted?.release_channel || "").trim(),
      hosted_update_policy: String(summary.hosted?.update_policy || "").trim(),
      config: summary,
    };
  }

  return async function handleControlPanelConfig(req, res) {
    const incoming = new URL(req.url, "http://127.0.0.1");
    if (incoming.pathname === "/api/install/profile" && req.method === "GET") {
      const payload = await loadInstallProfile();
      sendJson(res, 200, payload);
      return true;
    }

    if (incoming.pathname === "/api/control-panel/config" && req.method === "GET") {
      const configPath = getConfigPath();
      const config = readControlPanelConfig(configPath);
      sendJson(res, 200, {
        ok: true,
        path: configPath,
        config,
        summary: summarizeControlPanelConfig(config),
      });
      return true;
    }

    if (incoming.pathname === "/api/control-panel/config" && req.method === "PATCH") {
      const configPath = getConfigPath();
      const payload = await deps.readJsonBody(req);
      const saved = await writeControlPanelConfig(configPath, payload?.config || payload);
      sendJson(res, 200, {
        ok: true,
        path: saved.path,
        config: saved.config,
        summary: summarizeControlPanelConfig(saved.config),
      });
      return true;
    }

    return false;
  };
}
