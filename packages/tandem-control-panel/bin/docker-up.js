#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { ensureTokenFile } from "./docker-token.js";

function parseIdentityOverride(value, name) {
  const raw = String(value || "").trim();
  if (!raw) return undefined;
  if (!/^\d+$/.test(raw)) throw new Error(`${name} must be a non-root numeric identity`);
  const parsed = Number(raw);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) {
    throw new Error(`${name} must be a non-root numeric identity`);
  }
  return parsed;
}

export function resolveDockerRuntimeIdentity({
  hostUid = typeof process.getuid === "function" ? process.getuid() : 1000,
  hostGid = typeof process.getgid === "function" ? process.getgid() : 1000,
  hostIdentityAvailable = typeof process.getuid === "function",
  requestedUid = process.env.TANDEM_DOCKER_UID,
  requestedGid = process.env.TANDEM_DOCKER_GID,
} = {}) {
  const uidOverride = parseIdentityOverride(requestedUid, "TANDEM_DOCKER_UID");
  const gidOverride = parseIdentityOverride(requestedGid, "TANDEM_DOCKER_GID");
  const runtimeUid = uidOverride ?? (hostUid === 0 ? 1000 : hostUid);
  const runtimeGid = gidOverride ?? (hostUid === 0 ? 1000 : hostGid);
  if (!Number.isSafeInteger(runtimeUid) || !Number.isSafeInteger(runtimeGid)) {
    throw new Error("Docker runtime UID/GID must be safe integers");
  }
  if (
    hostIdentityAvailable &&
    hostUid !== 0 &&
    (runtimeUid !== hostUid || runtimeGid !== hostGid)
  ) {
    throw new Error(
      "Non-root launchers cannot override TANDEM_DOCKER_UID or TANDEM_DOCKER_GID away from their host identity"
    );
  }
  return { runtimeUid, runtimeGid };
}

function main() {
  const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const { runtimeUid, runtimeGid } = resolveDockerRuntimeIdentity();
  ensureTokenFile({ cwd: packageRoot, ownerUid: runtimeUid, ownerGid: runtimeGid });
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
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) main();
