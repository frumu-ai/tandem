#!/usr/bin/env node

import { basename } from "path";
import { spawn } from "child_process";
import { fileURLToPath } from "url";

import { printInitSummary, initializeInstall } from "../lib/setup/bootstrap.js";
import { printDoctor, runDoctor } from "../lib/setup/doctor.js";
import { parseCliArgs, err } from "../lib/setup/common.js";
import { resolveSetupPaths } from "../lib/setup/paths.js";
import { installSystemdServices, operateSystemdServices } from "../lib/setup/services/systemd.js";
import { installLaunchdServices, operateLaunchdServices } from "../lib/setup/services/launchd.js";
import { resolveUserHome } from "../lib/setup/services/common.js";

const argv = process.argv.slice(2);
const cli = parseCliArgs(argv);
const entry = basename(process.argv[1] || "tandem-setup");
const setupLegacyPath = fileURLToPath(new URL("./setup.js", import.meta.url));

function runLegacy(args = []) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [setupLegacyPath, ...args], {
      stdio: "inherit",
      env: process.env,
    });
    child.on("error", reject);
    child.on("close", (code) => resolve(code || 0));
  });
}

async function installServicesFromEnv(envFile) {
  const paths = resolveSetupPaths();
  const serviceUser = String(cli.value("service-user") || process.env.SUDO_USER || process.env.USER || "").trim();
  const homeDir = (await resolveUserHome(serviceUser || process.env.USER, process.platform)) || paths.home;
  const options = {
    envFile,
    homeDir,
    logsDir: paths.logsDir,
    serviceUser: serviceUser || process.env.USER,
    nodePath: process.execPath,
  };
  if (process.platform === "linux") return installSystemdServices(options);
  if (process.platform === "darwin") return installLaunchdServices(options);
  throw new Error("Service install is only supported on Linux and macOS.");
}

async function operateServices(operation) {
  if (process.platform === "linux") return operateSystemdServices(operation);
  if (process.platform === "darwin") return operateLaunchdServices(operation);
  throw new Error("Service operations are only supported on Linux and macOS.");
}

async function main() {
  const first = String(argv[0] || "").trim();
  if (!first) {
    process.exit(await runLegacy([]));
  }

  if (first.startsWith("--")) {
    console.warn("[Tandem Setup] Legacy flag mode is deprecated. Use `tandem-setup init|service|doctor`.");
    process.exit(await runLegacy(argv));
  }

  if (first === "run") {
    process.exit(await runLegacy(argv.slice(1)));
  }

  if (first === "init") {
    const result = await initializeInstall({
      envPath: cli.value("env-file"),
      overwrite: cli.has("rotate-token") || cli.has("reset-token"),
      allowAmbientStateEnv: false,
      allowCwdEnvMerge: false,
    });
    if (process.platform === "linux" || process.platform === "darwin") {
      if (!cli.has("no-service") && !cli.has("foreground")) {
        await installServicesFromEnv(result.envPath);
      }
    }
    printInitSummary(result);
    return;
  }

  if (first === "doctor") {
    const result = await runDoctor({
      envFile: cli.value("env-file"),
      allowAmbientStateEnv: false,
      allowCwdEnvMerge: false,
    });
    printDoctor(result, cli.has("json"));
    process.exit(result.ok ? 0 : 1);
  }

  if (first === "service") {
    const op = String(argv[1] || "").trim().toLowerCase();
    if (!op || op === "install") {
      const result = await initializeInstall({
        envPath: cli.value("env-file"),
        overwrite: false,
        allowAmbientStateEnv: false,
        allowCwdEnvMerge: false,
      });
      await installServicesFromEnv(result.envPath);
      return;
    }
    await operateServices(op);
    return;
  }

  if (first === "pair" && String(argv[1] || "").trim().toLowerCase() === "mobile") {
    const doctor = await runDoctor({
      envFile: cli.value("env-file"),
      allowAmbientStateEnv: false,
      allowCwdEnvMerge: false,
    });
    console.log("Mobile pairing is not implemented in this build.");
    console.log(`Control panel: http://${doctor.panelHost}:${doctor.panelPort}`);
    console.log(`Public URL:    ${doctor.panelPublicUrl || "(not configured)"}`);
    console.log(`Engine URL:    ${doctor.engineUrl}`);
    return;
  }

  if (entry === "tandem-control-panel") {
    process.exit(await runLegacy(argv));
  }

  err(`Unknown command: ${first}`);
  process.exit(1);
}

main().catch((error) => {
  err(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
