#!/usr/bin/env node

import { spawn } from "child_process";
import { createRequire } from "module";
import { fileURLToPath } from "url";

import { loadDotEnvFile, resolveEnvLoadOrder } from "../lib/setup/env.js";
import { parseCliArgs } from "../lib/setup/common.js";

const require = createRequire(import.meta.url);
const argv = process.argv.slice(2);
const mode = String(argv[0] || "").trim().toLowerCase();
const cli = parseCliArgs(argv.slice(1));
const explicitEnvFile = String(cli.value("env-file") || "").trim();

for (const envPath of resolveEnvLoadOrder({ explicitEnvFile })) {
  loadDotEnvFile(envPath);
}

async function runPanel() {
  const runtimePath = fileURLToPath(new URL("./setup.js", import.meta.url));
  const child = spawn(process.execPath, [runtimePath, "--env-file", explicitEnvFile].filter(Boolean), {
    stdio: "inherit",
    env: process.env,
  });
  child.on("close", (code) => process.exit(code || 0));
}

async function runEngine() {
  const engineEntrypoint = require.resolve("@frumu/tandem/bin/tandem-engine.js");
  const host = String(process.env.TANDEM_ENGINE_HOST || "127.0.0.1").trim();
  const port = String(process.env.TANDEM_ENGINE_PORT || "39731").trim();
  const child = spawn(
    process.execPath,
    [engineEntrypoint, "serve", "--hostname", host, "--port", port],
    { stdio: "inherit", env: process.env }
  );
  child.on("close", (code) => process.exit(code || 0));
}

if (mode === "engine") {
  runEngine();
} else if (mode === "panel") {
  runPanel();
} else {
  console.error("Usage: service-runner.js <engine|panel> [--env-file PATH]");
  process.exit(1);
}
