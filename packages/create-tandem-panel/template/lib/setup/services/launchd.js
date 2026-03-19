import { mkdir, writeFile } from "fs/promises";
import { resolve } from "path";

import { log, runCmd } from "../common.js";
import { resolveScriptPath } from "./common.js";

function buildLaunchdPlists(options = {}) {
  const nodePath = options.nodePath || process.execPath;
  const serviceRunner = resolveScriptPath("service-runner.js");
  const envFile = options.envFile;
  const logsDir = resolve(options.logsDir);
  const homeDir = resolve(options.homeDir);
  const userName = options.serviceUser;
  const engineLabel = "ai.frumu.tandem.engine";
  const panelLabel = "ai.frumu.tandem.control-panel";
  const enginePath = `/Library/LaunchDaemons/${engineLabel}.plist`;
  const panelPath = `/Library/LaunchDaemons/${panelLabel}.plist`;
  const plistFor = (label, mode) => `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
    <key>Label</key><string>${label}</string>
    <key>UserName</key><string>${userName}</string>
    <key>WorkingDirectory</key><string>${homeDir}</string>
    <key>ProgramArguments</key>
    <array>
      <string>${nodePath}</string>
      <string>${serviceRunner}</string>
      <string>${mode}</string>
      <string>--env-file</string>
      <string>${envFile}</string>
    </array>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key><true/>
    <key>ThrottleInterval</key><integer>5</integer>
    <key>StandardOutPath</key><string>${logsDir}/${mode}.log</string>
    <key>StandardErrorPath</key><string>${logsDir}/${mode}.log</string>
  </dict>
</plist>
`;
  return {
    engineLabel,
    panelLabel,
    enginePath,
    panelPath,
    enginePlist: plistFor(engineLabel, "engine"),
    panelPlist: plistFor(panelLabel, "panel"),
  };
}

async function installLaunchdServices(options = {}) {
  if (process.platform !== "darwin") {
    throw new Error("launchd install is only supported on macOS.");
  }
  if (typeof process.getuid === "function" && process.getuid() !== 0) {
    throw new Error("launchd install requires root.");
  }
  const plists = buildLaunchdPlists(options);
  await mkdir(options.logsDir, { recursive: true });
  await writeFile(plists.enginePath, plists.enginePlist, "utf8");
  await writeFile(plists.panelPath, plists.panelPlist, "utf8");
  await runCmd("launchctl", ["bootout", "system", plists.enginePath]).catch(() => null);
  await runCmd("launchctl", ["bootout", "system", plists.panelPath]).catch(() => null);
  await runCmd("launchctl", ["bootstrap", "system", plists.enginePath], { stdio: "inherit" });
  await runCmd("launchctl", ["bootstrap", "system", plists.panelPath], { stdio: "inherit" });
  await runCmd("launchctl", ["kickstart", "-k", `system/${plists.engineLabel}`], {
    stdio: "inherit",
  });
  await runCmd("launchctl", ["kickstart", "-k", `system/${plists.panelLabel}`], {
    stdio: "inherit",
  });
  log(`Installed ${plists.engineLabel} and ${plists.panelLabel}`);
  return plists;
}

async function operateLaunchdServices(operation) {
  const labels = ["ai.frumu.tandem.engine", "ai.frumu.tandem.control-panel"];
  if (operation === "status") {
    for (const label of labels) {
      await runCmd("launchctl", ["print", `system/${label}`], { stdio: "inherit" });
    }
    return;
  }
  if (operation === "logs") {
    throw new Error("launchd logs are file-based; inspect the configured log paths.");
  }
  if (operation === "uninstall") {
    for (const label of labels) {
      await runCmd("launchctl", ["bootout", `system/${label}`], { stdio: "inherit" }).catch(() => null);
    }
    return;
  }
  if (!["start", "stop", "restart"].includes(operation)) {
    throw new Error(`Unsupported launchd operation: ${operation}`);
  }
  for (const label of labels) {
    if (operation === "stop") {
      await runCmd("launchctl", ["kill", "TERM", `system/${label}`], { stdio: "inherit" }).catch(
        () => null
      );
      continue;
    }
    await runCmd("launchctl", ["kickstart", operation === "restart" ? "-k" : "", `system/${label}`].filter(Boolean), {
      stdio: "inherit",
    });
  }
}

export { buildLaunchdPlists, installLaunchdServices, operateLaunchdServices };
