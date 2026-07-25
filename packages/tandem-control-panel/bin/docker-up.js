#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { ensureTokenFile } from "./docker-token.js";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const hostUid = typeof process.getuid === "function" ? process.getuid() : 1000;
const hostGid = typeof process.getgid === "function" ? process.getgid() : 1000;
const requestedUid = Number.parseInt(process.env.TANDEM_DOCKER_UID || "", 10);
const requestedGid = Number.parseInt(process.env.TANDEM_DOCKER_GID || "", 10);
const runtimeUid = Number.isInteger(requestedUid) ? requestedUid : hostUid === 0 ? 1000 : hostUid;
const runtimeGid = Number.isInteger(requestedGid) ? requestedGid : hostUid === 0 ? 1000 : hostGid;
if (runtimeUid <= 0 || runtimeGid <= 0) {
  throw new Error(
    "TANDEM_DOCKER_UID and TANDEM_DOCKER_GID must select a non-root numeric identity"
  );
}
ensureTokenFile({
  cwd: packageRoot,
  ownerUid: runtimeUid,
  ownerGid: runtimeGid,
});

const result = spawnSync("docker", ["compose", "up", "--build", ...process.argv.slice(2)], {
  cwd: packageRoot,
  env: {
    ...process.env,
    TANDEM_DOCKER_UID: String(runtimeUid),
    TANDEM_DOCKER_GID: String(runtimeGid),
  },
  stdio: "inherit",
});
if (result.error) throw result.error;
process.exitCode = result.status ?? 1;
