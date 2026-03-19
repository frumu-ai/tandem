#!/usr/bin/env node

import { spawn } from "child_process";
import { createRequire } from "module";
import path from "path";
import process from "process";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(__dirname, "..");
const require = createRequire(import.meta.url);
const viteEntrypoint = require.resolve("vite/bin/vite.js");

const frontendPort = String(process.env.TANDEM_CONTROL_PANEL_DEV_PORT || "39732").trim();
const backendPort = String(process.env.TANDEM_CONTROL_PANEL_DEV_BACKEND_PORT || "39733").trim();
const backendUrl = `http://127.0.0.1:${backendPort}`;

let shuttingDown = false;
const children = new Set();

function spawnChild(command, args, options = {}) {
  const child = spawn(command, args, {
    stdio: "inherit",
    cwd: projectRoot,
    env: options.env || process.env,
  });
  children.add(child);
  child.on("close", (code) => {
    children.delete(child);
    if (!shuttingDown && code && code !== 0) {
      shuttingDown = true;
      for (const proc of children) {
        try {
          proc.kill("SIGTERM");
        } catch {}
      }
      process.exit(code);
    }
  });
  return child;
}

function shutdown(signal) {
  if (shuttingDown) return;
  shuttingDown = true;
  for (const child of children) {
    try {
      child.kill(signal);
    } catch {}
  }
  process.exit(0);
}

process.on("SIGINT", () => shutdown("SIGINT"));
process.on("SIGTERM", () => shutdown("SIGTERM"));

spawnChild(process.execPath, ["bin/setup.js"], {
  env: {
    ...process.env,
    TANDEM_CONTROL_PANEL_PORT: backendPort,
    TANDEM_CONTROL_PANEL_DISABLE_STATIC: "1",
  },
});

spawnChild(
  process.execPath,
  [viteEntrypoint, "--host", "127.0.0.1", "--port", frontendPort],
  {
    env: {
      ...process.env,
      TANDEM_CONTROL_PANEL_DEV_BACKEND_URL: backendUrl,
      TANDEM_CONTROL_PANEL_DEV_PORT: frontendPort,
      TANDEM_CONTROL_PANEL_DEV_BACKEND_PORT: backendPort,
    },
  }
);
