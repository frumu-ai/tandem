const DEFAULT_CAPABILITY_CACHE_TTL_MS = 45_000;

let _cache = {
  value: null,
  expiresAt: 0,
};

let _lastReported = {
  aca_available: null,
  engine_healthy: null,
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
    ACA_BASE_URL,
    ACA_HEALTH_PATH = "/health",
    getAcaToken,
    getInstallProfile,
    engineHealth,
    cacheTtlMs = DEFAULT_CAPABILITY_CACHE_TTL_MS,
  } = deps;

  async function probeAca() {
    const base = String(ACA_BASE_URL || "").trim();
    if (!base) {
      incrementProbeError("aca_not_configured");
      return { available: false, reason: "aca_not_configured" };
    }
    const target = `${base.replace(/\/+$/, "")}${ACA_HEALTH_PATH}`;
    const token = String(getAcaToken?.() || "").trim();
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
      if (res.ok) return { available: true, reason: "" };
      if (res.status === 404 || res.status === 405) {
        incrementProbeError("aca_endpoint_not_found");
        return { available: false, reason: "aca_endpoint_not_found" };
      }
      incrementProbeError(`aca_health_failed_${res.status}`);
      return { available: false, reason: `aca_health_failed_${res.status}` };
    } catch (err) {
      clearTimeout(timer);
      const msg = String(err?.message || "");
      if (msg.includes("abort")) {
        incrementProbeError("aca_probe_timeout");
        return { available: false, reason: "aca_probe_timeout" };
      }
      incrementProbeError("aca_probe_error");
      return { available: false, reason: "aca_probe_error" };
    }
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
    if (_cache.value && now < _cache.expiresAt) {
      deps.sendJson(res, 200, _cache.value);
      return;
    }

    const t0 = Date.now();
    const health = await engineHealth().catch(() => null);
    const engineOk = engineIsHealthy(health);
    const aca = await probeAca();
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
      hosted_release_version: installProfile?.hosted_release_version || "",
      hosted_release_channel: installProfile?.hosted_release_channel || "",
      hosted_update_policy: installProfile?.hosted_update_policy || "",
      _internal: {
        capability_detect_duration_ms: durationMs,
      },
    };

    logCapabilityTransition(result);

    _cache.value = result;
    _cache.expiresAt = now + cacheTtlMs;

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
}
