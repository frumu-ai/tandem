const DEFAULT_CAPABILITY_CACHE_TTL_MS = 45_000;
const DEFAULT_ACA_PROBE_GRACE_MS = 15 * 60_000;

let _cache = {
  value: null,
  expiresAt: 0,
};

let _lastReported = {
  aca_available: null,
  engine_healthy: null,
};

let _acaProbeState = {
  lastHealthyAtMs: 0,
  lastHealthyBaseUrl: "",
};

const _metrics = {
  detect_duration_ms: 0,
  detect_ok: false,
  last_detect_at_ms: 0,
  aca_probe_error_counts: {
    aca_not_configured: 0,
    aca_endpoint_not_found: 0,
    aca_probe_timeout: 0,
    aca_probe_error: 0,
    aca_health_failed_xxx: 0,
  },
};

function logCapabilityTransition(next) {
  const prev = _lastReported;
  const ts = new Date().toISOString();
  if (prev.aca_available !== next.aca_integration) {
    console.log(
      `[Capabilities] ${ts} ACA integration: ${prev.aca_available ?? "unknown"} → ${next.aca_integration} (reason: ${next.aca_reason || "n/a"})`
    );
  }
  if (prev.engine_healthy !== next.engine_healthy) {
    console.log(
      `[Capabilities] ${ts} Engine healthy: ${prev.engine_healthy ?? "unknown"} → ${next.engine_healthy}`
    );
  }
  _lastReported = { aca_available: next.aca_integration, engine_healthy: next.engine_healthy };
}

function incrementProbeError(reason) {
  const bucket =
    reason in _metrics.aca_probe_error_counts
      ? reason
      : reason.match(/^aca_health_failed_\d+$/)
        ? "aca_health_failed_xxx"
        : null;
  if (bucket) {
    _metrics.aca_probe_error_counts[bucket] += 1;
  }
}

export function createCapabilitiesHandler(deps) {
  const {
    PROBE_TIMEOUT_MS = 5_000,
    ACA_PROBE_GRACE_MS = DEFAULT_ACA_PROBE_GRACE_MS,
    ACA_BASE_URL,
    ACA_HEALTH_PATH = "/ready",
    getAcaToken,
    getInstallProfile,
    engineHealth,
    cacheTtlMs = DEFAULT_CAPABILITY_CACHE_TTL_MS,
  } = deps;

  async function probeAcaPath(target, token) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);
    try {
      const res = await fetch(target, {
        method: "GET",
        signal: controller.signal,
        headers: {
          Accept: "application/json",
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
      });
      clearTimeout(timer);
      return { res };
    } catch (err) {
      clearTimeout(timer);
      return { err };
    }
  }

  async function probeAca() {
    const base = String(ACA_BASE_URL || "").trim();
    if (!base) {
      incrementProbeError("aca_not_configured");
      return { available: false, reason: "aca_not_configured" };
    }
    const token = String(getAcaToken?.() || "").trim();
    const normalizedBase = base.replace(/\/+$/, "");
    const configuredPath = String(ACA_HEALTH_PATH || "/ready").startsWith("/")
      ? String(ACA_HEALTH_PATH || "/ready")
      : `/${ACA_HEALTH_PATH}`;
    const probePaths = [...new Set([configuredPath, "/ready", "/health"])];
    let lastFailure = { reason: "aca_probe_error" };
    for (const path of probePaths) {
      const target = `${normalizedBase}${path}`;
      const { res, err } = await probeAcaPath(target, token);
      if (res) {
        if (res.ok) {
          _acaProbeState.lastHealthyAtMs = Date.now();
          _acaProbeState.lastHealthyBaseUrl = base;
          return { available: true, reason: "", degraded: false, path };
        }
        if (res.status === 404 || res.status === 405) {
          lastFailure = { reason: "aca_endpoint_not_found", path };
          continue;
        }
        lastFailure = { reason: `aca_health_failed_${res.status}`, path };
        break;
      }
      const msg = String(err?.message || "");
      if (msg.includes("abort")) {
        lastFailure = { reason: "aca_probe_timeout", path };
        break;
      }
      lastFailure = { reason: "aca_probe_error", path };
      break;
    }
    incrementProbeError(lastFailure.reason);
    return { available: false, ...lastFailure };
  }

  function smoothAcaProbeResult(probe, { engineOk = false } = {}) {
    if (probe?.available) return probe;
    const reason = String(probe?.reason || "");
    if (!["aca_probe_timeout", "aca_probe_error"].includes(reason)) return probe;
    const base = String(ACA_BASE_URL || "").trim();
    const graceMs = Number.isFinite(Number(ACA_PROBE_GRACE_MS))
      ? Math.max(0, Number(ACA_PROBE_GRACE_MS))
      : DEFAULT_ACA_PROBE_GRACE_MS;
    const lastHealthyAtMs = Number(_acaProbeState.lastHealthyAtMs || 0);
    const recentlyHealthy =
      lastHealthyAtMs > 0 &&
      _acaProbeState.lastHealthyBaseUrl === base &&
      Date.now() - lastHealthyAtMs <= graceMs;
    const configuredTimeoutWithHealthyEngine = reason === "aca_probe_timeout" && !!base && engineOk;
    if (!recentlyHealthy && !configuredTimeoutWithHealthyEngine) return probe;
    return {
      available: true,
      reason,
      degraded: true,
    };
  }

  async function probeEngineFeatures(engineOk, acaOk) {
    if (!engineOk && !acaOk) {
      return { coding_workflows: false, missions: false, agent_teams: false, coder: false };
    }
    return {
      coding_workflows: engineOk || acaOk,
      missions: true,
      agent_teams: true,
      coder: engineOk,
    };
  }

  function engineIsHealthy(health) {
    const engine = health?.engine && typeof health.engine === "object" ? health.engine : health;
    return !!(engine?.ready || engine?.healthy);
  }

  return async function handleCapabilities(req, res) {
    const now = Date.now();
    const incoming = new URL(req?.url || "/", "http://127.0.0.1");
    const refresh = ["1", "true", "yes"].includes(
      String(incoming.searchParams.get("refresh") || "").trim().toLowerCase()
    );
    if (!refresh && _cache.value && now < _cache.expiresAt) {
      deps.sendJson(res, 200, _cache.value);
      return;
    }

    const t0 = Date.now();
    const health = await engineHealth().catch(() => null);
    const engineOk = engineIsHealthy(health);
    const aca = smoothAcaProbeResult(await probeAca(), { engineOk });
    const features = await probeEngineFeatures(engineOk, aca.available);
    const durationMs = Date.now() - t0;
    let installProfile = null;
    if (typeof getInstallProfile === "function") {
      try {
        installProfile = await getInstallProfile({
          acaAvailable: aca.available,
          engineHealthy: engineOk,
          acaReason: aca.reason,
        });
      } catch {
        installProfile = null;
      }
    }

    _metrics.detect_duration_ms = durationMs;
    _metrics.detect_ok = true;
    _metrics.last_detect_at_ms = now;

    const result = {
      aca_integration: aca.available,
      aca_reason: aca.reason,
      aca_probe_degraded: aca.degraded === true,
      coding_workflows: features.coding_workflows,
      missions: features.missions,
      agent_teams: features.agent_teams,
      coder: features.coder,
      engine_healthy: engineOk,
      cached_at_ms: now,
      control_panel_mode: installProfile?.control_panel_mode || (aca.available ? "aca" : "standalone"),
      control_panel_mode_source: installProfile?.control_panel_mode_source || "detected",
      control_panel_mode_reason: installProfile?.control_panel_mode_reason || "",
      control_panel_config_path: installProfile?.control_panel_config_path || "",
      control_panel_config_ready: !!installProfile?.control_panel_config_ready,
      control_panel_config_missing: Array.isArray(installProfile?.control_panel_config_missing)
        ? installProfile.control_panel_config_missing
        : [],
      control_panel_compact_nav: !!installProfile?.control_panel_compact_nav,
      hosted_managed: installProfile?.hosted_managed === true,
      hosted_provider: installProfile?.hosted_provider || "",
      hosted_deployment_id: installProfile?.hosted_deployment_id || "",
      hosted_deployment_slug: installProfile?.hosted_deployment_slug || "",
      hosted_hostname: installProfile?.hosted_hostname || "",
      hosted_public_url: installProfile?.hosted_public_url || "",
      hosted_control_plane_url: installProfile?.hosted_control_plane_url || "",
      hosted_auth_mode: installProfile?.hosted_auth_mode || "",
      hosted_auth_available: installProfile?.hosted_auth_available === true,
      hosted_panel_login_url: installProfile?.hosted_panel_login_url || "",
      hosted_release_version: installProfile?.hosted_release_version || "",
      hosted_release_channel: installProfile?.hosted_release_channel || "",
      hosted_update_policy: installProfile?.hosted_update_policy || "",
      workspace_files_root: installProfile?.workspace_files_root || "",
      workspace_files_available: !!installProfile?.workspace_files_available,
      workspace_files_api_available: installProfile?.workspace_files_api_available === true,
      _internal: {
        capability_detect_duration_ms: durationMs,
      },
    };

    logCapabilityTransition(result);

    const transientAcaMiss = !aca.available && ["aca_probe_timeout", "aca_probe_error"].includes(aca.reason);
    _cache.value = result;
    _cache.expiresAt = now + (transientAcaMiss ? Math.min(cacheTtlMs, 5_000) : cacheTtlMs);

    deps.sendJson(res, 200, result);
  };
}

export function getCapabilitiesMetrics() {
  return {
    ..._metrics,
    aca_probe_error_counts: { ..._metrics.aca_probe_error_counts },
  };
}

export function resetCapabilitiesCache() {
  _cache.value = null;
  _cache.expiresAt = 0;
}

export function resetCapabilitiesState() {
  _lastReported.aca_available = null;
  _lastReported.engine_healthy = null;
  _acaProbeState.lastHealthyAtMs = 0;
  _acaProbeState.lastHealthyBaseUrl = "";
}
