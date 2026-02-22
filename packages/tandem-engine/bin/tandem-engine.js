#!/usr/bin/env node

const path = require("path");
const https = require("https");
const { spawnSync } = require("child_process");

const binaryName = process.platform === "win32" ? "tandem-engine.exe" : "tandem-engine";
const binaryPath = path.join(__dirname, "native", binaryName);

const packageInfo = require("../package.json");
const UPDATE_CHECK_TIMEOUT_MS = 1200;

function parseVersion(version) {
  const core = String(version || "").split("-")[0];
  return core.split(".").map((part) => {
    const value = Number.parseInt(part, 10);
    return Number.isFinite(value) ? value : 0;
  });
}

function isNewerVersion(latest, current) {
  const a = parseVersion(latest);
  const b = parseVersion(current);
  const length = Math.max(a.length, b.length);

  for (let i = 0; i < length; i += 1) {
    const left = a[i] || 0;
    const right = b[i] || 0;
    if (left > right) return true;
    if (left < right) return false;
  }

  return false;
}

function fetchLatestVersion(packageName, timeoutMs) {
  return new Promise((resolve) => {
    const encodedName = encodeURIComponent(packageName);
    const url = `https://registry.npmjs.org/${encodedName}/latest`;
    const request = https.get(url, { headers: { Accept: "application/json" } }, (response) => {
      if (response.statusCode !== 200) {
        response.resume();
        resolve(null);
        return;
      }

      let body = "";
      response.setEncoding("utf8");
      response.on("data", (chunk) => {
        body += chunk;
      });
      response.on("end", () => {
        try {
          const parsed = JSON.parse(body);
          resolve(typeof parsed.version === "string" ? parsed.version : null);
        } catch {
          resolve(null);
        }
      });
    });

    request.on("error", () => resolve(null));
    request.setTimeout(timeoutMs, () => {
      request.destroy();
      resolve(null);
    });
  });
}

async function notifyIfUpdateAvailable() {
  const latestVersion = await fetchLatestVersion(packageInfo.name, UPDATE_CHECK_TIMEOUT_MS);
  if (!latestVersion || !isNewerVersion(latestVersion, packageInfo.version)) {
    return;
  }

  console.error(`[${packageInfo.name}] Update available: ${packageInfo.version} -> ${latestVersion}`);
  console.error(`Run: npm i -g ${packageInfo.name}`);
}

async function main() {
  await notifyIfUpdateAvailable();

  const child = spawnSync(binaryPath, process.argv.slice(2), { stdio: "inherit" });

  if (child.error) {
    console.error("tandem-engine binary is missing. Reinstall with: npm i -g @frumu/tandem");
    console.error(child.error.message);
    process.exit(1);
  }

  process.exit(child.status ?? 1);
}

main().catch(() => {
  const child = spawnSync(binaryPath, process.argv.slice(2), { stdio: "inherit" });
  if (child.error) {
    console.error("tandem-engine binary is missing. Reinstall with: npm i -g @frumu/tandem");
    console.error(child.error.message);
    process.exit(1);
  }
  process.exit(child.status ?? 1);
});
