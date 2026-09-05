import { existsSync } from "fs";
import { createRequire } from "module";

import { ensureBootstrapEnv } from "./env.js";
import { isLoopbackHost } from "./common.js";
const require = createRequire(import.meta.url);

function diagnosticUrl(value) {
  try {
    const url = new URL(value);
    if (!["http:", "https:"].includes(url.protocol)) return "invalid";
    url.username = "";
    url.password = "";
    url.search = "";
    url.hash = "";
    return url.toString().replace(/\/$/, "");
  } catch {
    return "invalid";
  }
}

async function probeEngine(engineUrl) {
  let running = false;
  try {
    const url = new URL(engineUrl);
    if (!["http:", "https:"].includes(url.protocol) || url.username || url.password || url.search || url.hash) {
      return { running, engineHealth: null, code: "engine_url_invalid" };
    }
    const res = await fetch(`${url.toString().replace(/\/$/, "")}/global/health`, {
      signal: AbortSignal.timeout(1500),
      redirect: "error",
    });
    running = true;
    if (!res.ok) return { running, engineHealth: null, code: "engine_health_http_error" };
    const body = await res.json();
    if (!body || typeof body.ready !== "boolean" || typeof body.healthy !== "boolean") {
      return { running, engineHealth: null, code: "engine_health_invalid" };
    }
    // Keep only the public booleans, never arbitrary response/error content.
    const engineHealth = { ready: body.ready, healthy: body.healthy };
    return { running, engineHealth, code: body.ready && body.healthy ? "engine_ready" : "engine_not_ready" };
  } catch {
    return { running, engineHealth: null, code: running ? "engine_health_invalid" : "engine_unreachable" };
  }
}

async function runDoctor(options = {}) {
  const bootstrap = await ensureBootstrapEnv({
    envPath: options.envFile,
    overwrite: false,
    env: options.env,
    cwd: options.cwd,
    allowAmbientStateEnv: options.allowAmbientStateEnv,
    allowCwdEnvMerge: options.allowCwdEnvMerge,
    readOnly: true,
  });
  const distExists = existsSync(new URL("../../dist", import.meta.url));
  let engineResolvable = false;
  try {
    require.resolve("@frumu/tandem/bin/tandem-engine.js");
    engineResolvable = true;
  } catch {}
  const probe = await probeEngine(bootstrap.engineUrl);
  const { engineHealth, running } = probe;
  const runtimeReady = probe.code === "engine_ready";
  const checks = [
    { id: "panel_bundle", status: distExists ? "passed" : "failed", code: distExists ? "panel_installed" : "panel_bundle_missing",
      remediation: distExists ? null : "Install or build the control panel distribution." },
    { id: "engine_package", status: engineResolvable ? "passed" : "failed", code: engineResolvable ? "engine_installed" : "engine_package_missing",
      remediation: engineResolvable ? null : "Install the supported engine package." },
    { id: "engine_readiness", status: runtimeReady ? "passed" : "failed", code: probe.code,
      remediation: runtimeReady ? null : "Check the configured engine URL and service, then inspect its readiness diagnostics and run doctor again." },
  ];
  let serviceManager = "none";
  if (process.platform === "linux") {
    serviceManager = "systemd";
  } else if (process.platform === "darwin") {
    serviceManager = "launchd";
  }
  const result = {
    scope: "runtime-health",
    ok: checks.every((check) => check.status === "passed"),
    installed: Boolean(distExists && engineResolvable),
    running,
    runtimeReady,
    authenticated: null,
    policyCurrent: null,
    solutionReady: false,
    solutionReadinessCode: "authenticated_solution_checks_not_run",
    checkedAt: new Date().toISOString(),
    checks,
    envFile: bootstrap.envPath,
    panelHost: bootstrap.panelHost,
    panelPort: bootstrap.panelPort,
    panelPublicUrl: bootstrap.env.TANDEM_CONTROL_PANEL_PUBLIC_URL ? diagnosticUrl(bootstrap.env.TANDEM_CONTROL_PANEL_PUBLIC_URL) : "",
    engineUrl: diagnosticUrl(bootstrap.engineUrl),
    distExists,
    engineResolvable,
    serviceManager,
    engineHealth,
    warnings: [],
  };
  if (!isLoopbackHost(bootstrap.panelHost) && !result.panelPublicUrl) {
    result.warnings.push("Panel binds non-loopback without TANDEM_CONTROL_PANEL_PUBLIC_URL.");
  }
  return result;
}

function printDoctor(result, json = false) {
  if (json) {
    console.log(JSON.stringify(result, null, 2));
    return;
  }
  console.log(`[Tandem Setup] Env file:     ${result.envFile}`);
  console.log(`[Tandem Setup] Panel:        http://${result.panelHost}:${result.panelPort}`);
  console.log(`[Tandem Setup] Engine URL:   ${result.engineUrl}`);
  console.log(`[Tandem Setup] Dist exists:  ${result.distExists ? "yes" : "no"}`);
  console.log(`[Tandem Setup] Engine pkg:   ${result.engineResolvable ? "yes" : "no"}`);
  console.log(`[Tandem Setup] Service mgr:  ${result.serviceManager}`);
  console.log(
    `[Tandem Setup] Engine health:${result.engineHealth ? ` ready=${result.engineHealth.ready === true}` : " unavailable"}`
  );
  for (const check of result.checks || []) {
    console.log(`[Tandem Setup] ${check.id}: ${check.status} (${check.code})`);
    if (check.remediation) console.log(`[Tandem Setup] Action: ${check.remediation}`);
  }
  console.log("[Tandem Setup] Solution readiness: unverified; authenticated identity, policy, storage, provider and recovery checks are still required.");
  for (const warning of result.warnings || []) {
    console.log(`[Tandem Setup] Warning:     ${warning}`);
  }
}

export { printDoctor, runDoctor };
