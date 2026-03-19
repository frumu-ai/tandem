import { existsSync, readFileSync } from "fs";
import { mkdir } from "fs/promises";
import { createRequire } from "module";
import { join } from "path";

import { bootstrapEngineConfig, ensureBootstrapEnv, parseDotEnv } from "./env.js";
import { log } from "./common.js";

const require = createRequire(import.meta.url);

function resolvePackagePaths() {
  const controlPanelRoot = join(new URL("../../", import.meta.url).pathname);
  let engineEntrypoint = "";
  try {
    engineEntrypoint = require.resolve("@frumu/tandem/bin/tandem-engine.js");
  } catch {}
  return {
    controlPanelRoot,
    engineEntrypoint,
  };
}

async function initializeInstall(options = {}) {
  const result = await ensureBootstrapEnv(options);
  const pkg = resolvePackagePaths();
  if (!pkg.engineEntrypoint) {
    throw new Error("Could not resolve @frumu/tandem engine entrypoint. Run `npm install` in this app.");
  }
  await mkdir(result.paths.configDir, { recursive: true });
  await mkdir(result.paths.dataDir, { recursive: true });
  const config = await bootstrapEngineConfig({ env: result.env });
  return { ...result, engineEntrypoint: pkg.engineEntrypoint, configPath: config.configPath };
}

async function readManagedEnv(options = {}) {
  const result = await ensureBootstrapEnv({ ...options, overwrite: false });
  const env = existsSync(result.envPath) ? parseDotEnv(readFileSync(result.envPath, "utf8")) : {};
  return { ...result, env };
}

function printInitSummary(result) {
  log("Environment initialized.");
  log(`Env file:    ${result.envPath}`);
  log(`Engine URL:  ${result.engineUrl}`);
  log(`Panel URL:   http://${result.panelHost}:${result.panelPort}`);
  log(`Token:       ${result.token}`);
  if (result.configPath) log(`Engine cfg:  ${result.configPath}`);
}

export { initializeInstall, printInitSummary, readManagedEnv, resolvePackagePaths };
