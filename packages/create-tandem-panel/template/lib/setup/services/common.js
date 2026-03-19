import { createRequire } from "module";
import { existsSync } from "fs";
import { join, resolve } from "path";

import { runCmd } from "../common.js";

const require = createRequire(import.meta.url);

function resolveControlPanelRoot() {
  return resolve(join(new URL("../../", import.meta.url).pathname));
}

function resolveScriptPath(name) {
  return resolveControlPanelRoot() + `/bin/${name}`;
}

function resolveEngineEntrypoint() {
  return require.resolve("@frumu/tandem/bin/tandem-engine.js");
}

async function resolveUserHome(user, platform = process.platform) {
  const name = String(user || "").trim();
  if (!name) return "";
  if (platform === "linux") {
    try {
      const out = await runCmd("getent", ["passwd", name]);
      const row = String(out.stdout || "").trim();
      const fields = row.split(":");
      return fields[5] ? resolve(fields[5]) : "";
    } catch {}
  }
  if (platform === "darwin") {
    try {
      const out = await runCmd("dscl", [".", "-read", `/Users/${name}`, "NFSHomeDirectory"]);
      const line = String(out.stdout || "")
        .split(/\r?\n/)
        .find((row) => row.includes("NFSHomeDirectory:"));
      return line ? resolve(line.split(":").slice(1).join(":").trim()) : "";
    } catch {}
  }
  const guess = platform === "darwin" ? `/Users/${name}` : `/home/${name}`;
  return existsSync(guess) ? resolve(guess) : "";
}

export { resolveControlPanelRoot, resolveEngineEntrypoint, resolveScriptPath, resolveUserHome };
