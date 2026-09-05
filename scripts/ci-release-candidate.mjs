#!/usr/bin/env node

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { lstatSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const inputs = ["Cargo.lock", "scripts/linux-release-builder.Dockerfile", "scripts/build-linux-release-engine.sh"];
const profile = {
  target: "x86_64-unknown-linux-gnu",
  toolchain: "1.95.0",
  mode: "with-enterprise",
  standard_features: ["tandem-ai/browser", "tandem-ai/enterprise"],
  enterprise_features: ["tandem-ai/browser", "tandem-ai/enterprise-full"],
};

function digest(value) {
  return createHash("sha256").update(value).digest("hex");
}

function regularFile(path) {
  assert(lstatSync(path).isFile(), `Expected a regular file: ${path}`);
  return readFileSync(path);
}

function context(root, env) {
  assert(/^[0-9a-f]{40}$/.test(env.GITHUB_SHA || ""), "A full GitHub checkout SHA is required");
  for (const key of ["GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"]) {
    assert(/^[1-9][0-9]*$/.test(env[key] || ""), `${key} must be a positive integer`);
  }
  const head = execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim();
  assert.equal(head, env.GITHUB_SHA, "Checkout must match the workflow SHA");
  return {
    schema: 1,
    source_sha: head,
    run_id: env.GITHUB_RUN_ID,
    run_attempt: env.GITHUB_RUN_ATTEMPT,
    profile,
    inputs: Object.fromEntries(inputs.map((path) => [path, digest(regularFile(resolve(root, path)))])),
  };
}

export function writeCandidate(root, directory, env = process.env) {
  const manifest = {
    ...context(root, env),
    binary_sha256: digest(regularFile(resolve(directory, "tandem-engine"))),
  };
  const source = `${JSON.stringify(manifest, null, 2)}\n`;
  writeFileSync(resolve(directory, "provenance.json"), source, { flag: "wx" });
  return { sha256: manifest.binary_sha256, manifest_sha256: digest(source) };
}

export function verifyCandidate(root, directory, expectedManifest, expectedBinary, env = process.env) {
  for (const pin of [expectedManifest, expectedBinary]) {
    assert(/^[0-9a-f]{64}$/.test(pin || ""), "Producer must supply full SHA-256 digests");
  }
  const source = regularFile(resolve(directory, "provenance.json"));
  assert.equal(digest(source), expectedManifest, "Manifest differs from the producer output");
  const manifest = JSON.parse(source.toString("utf8"));
  assert.deepEqual(manifest, { ...context(root, env), binary_sha256: expectedBinary },
    "Candidate provenance does not match this checkout, run, profile and build inputs");
  assert.equal(digest(regularFile(resolve(directory, "tandem-engine"))), expectedBinary,
    "Candidate binary differs from the producer output");
  return { sha256: expectedBinary, manifest_sha256: expectedManifest };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [mode, directory, manifest, binary, ...extra] = process.argv.slice(2);
    assert(directory && extra.length === 0, "A candidate directory is required");
    let result;
    if (mode === "write" && !manifest && !binary) {
      result = writeCandidate(process.cwd(), directory);
    } else if (mode === "verify") {
      // A consumer-only rerun may reuse the successful producer's immutable
      // artifact from an earlier attempt of this same workflow run.
      const env = { ...process.env, GITHUB_RUN_ATTEMPT: process.env.CANDIDATE_BUILD_ATTEMPT };
      result = verifyCandidate(process.cwd(), directory, manifest, binary, env);
    } else {
      throw new Error("Usage: ci-release-candidate.mjs write DIR | verify DIR MANIFEST_SHA BINARY_SHA");
    }
    console.log(JSON.stringify(result));
  } catch (error) {
    console.error(`Release candidate verification failed: ${error.message}`);
    process.exitCode = 1;
  }
}
