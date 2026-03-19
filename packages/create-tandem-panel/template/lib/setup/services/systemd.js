import { writeFile } from "fs/promises";

import { log, runCmd } from "../common.js";
import { resolveEngineEntrypoint, resolveScriptPath } from "./common.js";

function buildSystemdUnits(options = {}) {
  const nodePath = options.nodePath || process.execPath;
  const serviceRunner = resolveScriptPath("service-runner.js");
  const envFile = options.envFile;
  const serviceUser = options.serviceUser;
  const serviceGroup = options.serviceGroup || serviceUser;
  const engineLabel = "tandem-engine";
  const panelLabel = "tandem-control-panel";
  const engineUnit = `[Unit]
Description=Tandem Engine
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=${serviceUser}
Group=${serviceGroup}
WorkingDirectory=${options.homeDir}
ExecStart=${nodePath} ${serviceRunner} engine --env-file ${envFile}
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
`;
  const panelUnit = `[Unit]
Description=Tandem Control Panel
After=network-online.target ${engineLabel}.service
Wants=network-online.target

[Service]
Type=simple
User=${serviceUser}
Group=${serviceGroup}
WorkingDirectory=${options.homeDir}
ExecStart=${nodePath} ${serviceRunner} panel --env-file ${envFile}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
`;
  return {
    engineLabel,
    panelLabel,
    enginePath: `/etc/systemd/system/${engineLabel}.service`,
    panelPath: `/etc/systemd/system/${panelLabel}.service`,
    engineUnit,
    panelUnit,
    engineEntrypoint: resolveEngineEntrypoint(),
  };
}

async function installSystemdServices(options = {}) {
  if (process.platform !== "linux") {
    throw new Error("systemd install is only supported on Linux.");
  }
  if (typeof process.getuid === "function" && process.getuid() !== 0) {
    throw new Error("systemd install requires root.");
  }
  const units = buildSystemdUnits(options);
  await writeFile(units.enginePath, units.engineUnit, "utf8");
  await writeFile(units.panelPath, units.panelUnit, "utf8");
  await runCmd("systemctl", ["daemon-reload"], { stdio: "inherit" });
  await runCmd("systemctl", ["enable", "--now", `${units.engineLabel}.service`], {
    stdio: "inherit",
  });
  await runCmd("systemctl", ["enable", "--now", `${units.panelLabel}.service`], {
    stdio: "inherit",
  });
  log(`Installed ${units.engineLabel}.service and ${units.panelLabel}.service`);
  return units;
}

async function operateSystemdServices(operation) {
  const units = ["tandem-engine.service", "tandem-control-panel.service"];
  if (operation === "logs") {
    const args = units.flatMap((unit) => ["-u", unit]).concat(["-n", "120", "-f", "-o", "short-iso"]);
    await runCmd("journalctl", args, { stdio: "inherit" });
    return;
  }
  if (operation === "status") {
    await runCmd("systemctl", ["--no-pager", "--full", "status", ...units], { stdio: "inherit" });
    return;
  }
  if (operation === "uninstall") {
    for (const unit of units) {
      await runCmd("systemctl", ["disable", "--now", unit], { stdio: "inherit" }).catch(() => null);
    }
    return;
  }
  for (const unit of units) {
    await runCmd("systemctl", [operation, unit], { stdio: "inherit" });
  }
}

export { buildSystemdUnits, installSystemdServices, operateSystemdServices };
