#!/usr/bin/env node

import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const EXPECTED_DEPLOYMENT_ASSETS = new Set([
  "packages/tandem-control-panel/docker-compose.yml",
  "packages/tandem-control-panel/docker/control-panel.Dockerfile",
  "packages/tandem-control-panel/docker/engine.Dockerfile",
]);
const PINNED_NODE_BASE =
  "node:24-trixie-slim@sha256:ae91dcc111a68c9d2d81ff2a17bda61be126426176fde6fe7d08ab13b7f50573";

function requireMatch(source, pattern, message, errors) {
  if (!pattern.test(source)) errors.push(message);
}

function countMatches(source, pattern) {
  return [...source.matchAll(pattern)].length;
}

async function walk(directory, root = directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    if ([".git", "node_modules", "target", "dist"].includes(entry.name)) continue;
    const pathname = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...(await walk(pathname, root)));
    else files.push(path.relative(root, pathname).replaceAll(path.sep, "/"));
  }
  return files;
}

function isDeploymentAsset(filename) {
  return (
    /(^|\/)(?:Dockerfile|[^/]+\.Dockerfile)$/.test(filename) ||
    /(^|\/)(?:docker-)?compose[^/]*\.ya?ml$/.test(filename) ||
    /\.tf(?:vars)?$/.test(filename) ||
    /(^|\/)(?:Chart|kustomization)\.ya?ml$/.test(filename)
  );
}

export async function verifyContainerHardening(root = process.cwd()) {
  const errors = [];
  const engineDockerfile = await readFile(
    path.join(root, "packages/tandem-control-panel/docker/engine.Dockerfile"),
    "utf8"
  );
  const panelDockerfile = await readFile(
    path.join(root, "packages/tandem-control-panel/docker/control-panel.Dockerfile"),
    "utf8"
  );
  const compose = await readFile(
    path.join(root, "packages/tandem-control-panel/docker-compose.yml"),
    "utf8"
  );
  const engineEntrypoint = await readFile(
    path.join(root, "packages/tandem-control-panel/docker/engine-entrypoint.sh"),
    "utf8"
  );
  const dockerToken = await readFile(
    path.join(root, "packages/tandem-control-panel/bin/docker-token.js"),
    "utf8"
  );

  for (const [name, source] of [
    ["engine Dockerfile", engineDockerfile],
    ["control-panel Dockerfile", panelDockerfile],
  ]) {
    const fromLines = source.match(/^FROM\s+\S+/gm) || [];
    if (fromLines.length === 0) errors.push(`${name} has no FROM instruction`);
    for (const line of fromLines) {
      if (!line.includes(PINNED_NODE_BASE)) {
        errors.push(`${name} uses an unapproved or non-digest-pinned base: ${line}`);
      }
    }
    requireMatch(source, /^USER node$/m, `${name} must run as USER node`, errors);
    if (/@latest\b|ENGINE_VERSION=latest\b/.test(source)) {
      errors.push(`${name} contains a floating latest dependency`);
    }
  }

  requireMatch(
    engineDockerfile,
    /^\s*TANDEM_ENGINE_VERSION=\d+\.\d+\.\d+ \\$/m,
    "engine image must pin an exact semantic version in the image",
    errors
  );
  requireMatch(
    engineDockerfile,
    /^\s*TANDEM_ENGINE_BINARY_SHA256=[0-9a-f]{64} \\$/m,
    "engine image must pin the native release binary by SHA-256",
    errors
  );
  requireMatch(
    engineDockerfile,
    /sha256sum -c -/,
    "engine image must verify the native release binary SHA-256",
    errors
  );
  for (const [name, source] of [
    ["engine Dockerfile", engineDockerfile],
    ["control-panel Dockerfile", panelDockerfile],
  ]) {
    requireMatch(
      source,
      /rm -rf \/usr\/local\/lib\/node_modules\/npm \/usr\/local\/lib\/node_modules\/corepack/,
      `${name} must remove build-only npm/corepack tooling from the runtime image`,
      errors
    );
  }
  if (countMatches(compose, /^\s{4}read_only:\s*true\s*$/gm) !== 2) {
    errors.push("both Compose services must use a read-only root filesystem");
  }
  if (countMatches(compose, /^\s{4}cap_drop:\s*$/gm) !== 2 || countMatches(compose, /^\s{6}- ALL\s*$/gm) !== 2) {
    errors.push("both Compose services must drop every Linux capability");
  }
  if (countMatches(compose, /^\s{6}- no-new-privileges:true\s*$/gm) !== 2) {
    errors.push("both Compose services must set no-new-privileges");
  }
  if (countMatches(compose, /^\s{4}init:\s*true\s*$/gm) !== 2) {
    errors.push("both Compose services must enable an init process");
  }
  requireMatch(
    compose,
    /source:\s*\.\/secrets\/tandem_api_token[\s\S]{0,160}target:\s*\/run\/secrets\/tandem_api_token[\s\S]{0,100}read_only:\s*true/,
    "engine secret must be a single read-only file mount",
    errors
  );
  if (/\.\/secrets:\s*\/run\/secrets/.test(compose)) {
    errors.push("Compose must not mount the whole secrets directory");
  }
  requireMatch(
    engineEntrypoint,
    /must be a non-empty readable file/,
    "engine entrypoint must fail closed when the secret is unavailable",
    errors
  );
  if (/tandem-engine token generate|>\s*"?\$TANDEM_API_TOKEN_FILE/.test(engineEntrypoint)) {
    errors.push("engine entrypoint must never generate or write the mounted secret");
  }
  for (const marker of ["O_EXCL", "O_NOFOLLOW", "0o600", "0o700", "isSymbolicLink"]) {
    if (!dockerToken.includes(marker)) errors.push(`host token provisioner is missing ${marker}`);
  }

  const discovered = new Set((await walk(root)).filter(isDeploymentAsset));
  for (const expected of EXPECTED_DEPLOYMENT_ASSETS) {
    if (!discovered.has(expected)) errors.push(`expected deployment asset is missing: ${expected}`);
  }
  for (const filename of discovered) {
    if (!EXPECTED_DEPLOYMENT_ASSETS.has(filename)) {
      errors.push(`unreviewed deployment asset requires scanner coverage: ${filename}`);
    }
  }
  return { assets: [...discovered].sort(), errors };
}

async function main() {
  const result = await verifyContainerHardening();
  if (result.errors.length > 0) {
    throw new Error(`container hardening policy failed:\n${result.errors.join("\n")}`);
  }
  process.stdout.write(`container hardening policy passed (${result.assets.length} deployment assets)\n`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main().catch((error) => {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  });
}
