import { existsSync } from "fs";
import { createRequire } from "module";

import { ensureBootstrapEnv } from "./env.js";
import { isLoopbackHost } from "./common.js";
const require = createRequire(import.meta.url);

async function runDoctor(options = {}) {
  const bootstrap = await ensureBootstrapEnv({
    envPath: options.envFile,
    overwrite: false,
    env: options.env,
    cwd: options.cwd,
    allowAmbientStateEnv: options.allowAmbientStateEnv,
    allowCwdEnvMerge: options.allowCwdEnvMerge,
  });
  const distExists = existsSync(new URL("../../dist", import.meta.url));
  let engineResolvable = false;
  try {
    require.resolve("@frumu/tandem/bin/tandem-engine.js");
    engineResolvable = true;
  } catch {}
  let engineHealth = null;
  try {
    const res = await fetch(`${bootstrap.engineUrl}/global/health`, {
      signal: AbortSignal.timeout(1500),
    });
    if (res.ok) engineHealth = await res.json();
  } catch {}
  let serviceManager = "none";
  if (process.platform === "linux") {
    serviceManager = "systemd";
  } else if (process.platform === "darwin") {
    serviceManager = "launchd";
  }
  const result = {
    ok: Boolean(distExists && engineResolvable),
    envFile: bootstrap.envPath,
    panelHost: bootstrap.panelHost,
    panelPort: bootstrap.panelPort,
    panelPublicUrl: bootstrap.env.TANDEM_CONTROL_PANEL_PUBLIC_URL || "",
    engineUrl: bootstrap.engineUrl,
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
  for (const warning of result.warnings || []) {
    console.log(`[Tandem Setup] Warning:     ${warning}`);
  }
}

export { printDoctor, runDoctor };
