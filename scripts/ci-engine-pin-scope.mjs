#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parsePinnedEngineVersion } from "./verify-container-hardening.mjs";

const dockerfile = "packages/tandem-control-panel/docker/engine.Dockerfile";

function releasePin(source) {
  const version = parsePinnedEngineVersion(source);
  const versions = [...source.matchAll(/^\s*TANDEM_ENGINE_VERSION=/gm)];
  const hashes = [...source.matchAll(/^\s*TANDEM_ENGINE_BINARY_SHA256=([^\r\n]*)/gm)];
  const hash = hashes[0]?.[1].match(/^([0-9a-f]{64}) \\$/)?.[1];
  if (!version || versions.length !== 1 || hashes.length !== 1 || !hash) {
    throw new Error("Engine Dockerfile must contain one valid release version and SHA-256 pin.");
  }
  return `${version}:${hash}`;
}

// A runtime OS/base-image update does not publish a new engine binary. Only
// a version or binary digest edit should compare the source-built candidate
// with the checked-in release pin. Candidate builds still verify their own
// computed digest, and published-release verification remains unconditional.
export function engineReleasePinChanged(before, after) {
  return releasePin(before) !== releasePin(after);
}

export function changedBetweenCommits(base, head) {
  const validRef = (value) => /^[0-9a-f]{40}(?:[0-9a-f]{24})?$/.test(value || "");
  if (!validRef(base) || !validRef(head) || /^0+$/.test(head)) {
    throw new Error("Engine pin scope requires full commit SHAs.");
  }
  const read = (ref) => execFileSync("git", ["show", `${ref}:${dockerfile}`], { encoding: "utf8" });
  const after = read(head);
  releasePin(after);
  // A new branch with no previous commit cannot prove the pin is unchanged.
  if (/^0+$/.test(base)) return true;
  return engineReleasePinChanged(read(base), after);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    console.log(changedBetweenCommits(process.argv[2], process.argv[3]));
  } catch (error) {
    console.error(`Engine release pin classification failed: ${error.message}`);
    process.exitCode = 1;
  }
}
