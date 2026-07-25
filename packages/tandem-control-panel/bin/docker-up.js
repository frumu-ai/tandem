#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { ensureTokenFile } from "./docker-token.js";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
ensureTokenFile({ cwd: packageRoot });

const result = spawnSync("docker", ["compose", "up", "--build", ...process.argv.slice(2)], {
  cwd: packageRoot,
  stdio: "inherit",
});
if (result.error) throw result.error;
process.exitCode = result.status ?? 1;
