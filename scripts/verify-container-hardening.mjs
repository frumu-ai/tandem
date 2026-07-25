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
const EXACT_SEMVER =
  /^\d+\.\d+\.\d+(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

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

export function isDeploymentAsset(filename, source = "") {
  return (
    /(^|\/)(?:Dockerfile(?:\.[^/]+)?|[^/]+\.Dockerfile|Containerfile(?:\.[^/]+)?|[^/]+\.Containerfile)$/i.test(
      filename
    ) ||
    /(^|\/)(?:docker-)?compose[^/]*\.ya?ml$/.test(filename) ||
    /[.]tf(?:vars)?(?:[.]json)?$/i.test(filename) ||
    /(^|\/)(?:Chart|kustomization|helmfile)\.ya?ml$/i.test(filename) ||
    /\.nomad(?:\.hcl)?$/i.test(filename) ||
    (/\.ya?ml$/i.test(filename) &&
      /^\s*apiVersion\s*:\s*\S+/m.test(source) &&
      /^\s*kind\s*:\s*[A-Za-z]/m.test(source))
  );
}

export function parsePinnedEngineVersion(source) {
  const value = String(source || "").match(/^\s*TANDEM_ENGINE_VERSION=([^ \r\n]+) \\$/m)?.[1] || "";
  return EXACT_SEMVER.test(value) ? value : "";
}

export async function verifyContainerHardening(
  root = process.cwd(),
  { expectedEngineVersion } = {}
) {
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
  const enginePackage = JSON.parse(
    await readFile(path.join(root, "packages/tandem-engine/package.json"), "utf8")
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
    for (const marker of [
      "snapshot.debian.org/archive/debian/20260720T000000Z",
      "ca-certificates=20250419",
      "curl=8.14.1-2+deb13u4",
    ]) {
      if (!source.includes(marker)) errors.push(`${name} is missing immutable OS input ${marker}`);
    }
  }

  const engineVersion = parsePinnedEngineVersion(engineDockerfile);
  if (!engineVersion) errors.push("engine image must pin an exact semantic version in the image");
  if (engineVersion && engineVersion !== String(enginePackage.version || "")) {
    errors.push(
      `engine image version ${engineVersion} must match packages/tandem-engine ${enginePackage.version || "missing"}`
    );
  }
  if (expectedEngineVersion && engineVersion !== expectedEngineVersion) {
    errors.push(
      `engine image version ${engineVersion || "missing"} must be pre-pinned for release ${expectedEngineVersion}`
    );
  }
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
  if (countMatches(compose, /^\s{4}read_only:\s*true\s*$/gm) !== 3) {
    errors.push(
      "both runtime services and the migration service must use a read-only root filesystem"
    );
  }
  if (
    countMatches(compose, /^\s{4}cap_drop:\s*$/gm) !== 3 ||
    countMatches(compose, /^\s{6}- ALL\s*$/gm) !== 3
  ) {
    errors.push(
      "every Compose service must drop all Linux capabilities before any narrow add-back"
    );
  }
  if (countMatches(compose, /^\s{6}- no-new-privileges:true\s*$/gm) !== 3) {
    errors.push("every Compose service must set no-new-privileges");
  }
  if (countMatches(compose, /^\s{4}init:\s*true\s*$/gm) !== 2) {
    errors.push("both runtime services must enable an init process");
  }
  for (const marker of [
    "tandem-state-migrate:",
    'user: "0:0"',
    "- CHOWN",
    "- DAC_OVERRIDE",
    'user: "${TANDEM_DOCKER_UID:-1000}:${TANDEM_DOCKER_GID:-1000}"',
  ]) {
    if (!compose.includes(marker)) errors.push(`Compose ownership migration is missing ${marker}`);
  }
  if (countMatches(compose, /condition:\s*service_completed_successfully/g) !== 2) {
    errors.push("both runtime services must wait for state-volume ownership migration");
  }
  requireMatch(
    compose,
    /chown -R [^\n]+"\$\$\{state_dir\}"[\s\S]{0,120}touch "\$\$\{marker\}"/,
    "Compose migration marker must be written after recursive ownership succeeds",
    errors
  );
  for (const marker of ["is_non_root_id", "*[1-9]*"]) {
    if (!compose.includes(marker)) {
      errors.push(`Compose must reject zero-padded root identities using ${marker}`);
    }
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
  for (const marker of [
    "O_EXCL",
    "O_NOFOLLOW",
    "fstatSync",
    "fchmodSync",
    "fchownSync",
    "0o600",
    "0o700",
    "isSymbolicLink",
  ]) {
    if (!dockerToken.includes(marker)) errors.push(`host token provisioner is missing ${marker}`);
  }

  const discovered = new Set();
  for (const filename of await walk(root)) {
    const assetSource = /\.ya?ml$/i.test(filename)
      ? await readFile(path.join(root, filename), "utf8")
      : "";
    if (isDeploymentAsset(filename, assetSource)) discovered.add(filename);
  }
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

function selfTest() {
  const expected = [
    ["Dockerfile.production", ""],
    ["ops/Containerfile", ""],
    ["deploy/helmfile.yaml", ""],
    ["deploy/main.tf.json", ""],
    ["deploy/production.tfvars.json", ""],
    ["nomad/tandem.nomad.hcl", ""],
    ["k8s/workload.yaml", "apiVersion: apps/v1\nkind: Deployment\n"],
  ];
  if (expected.some(([filename, source]) => !isDeploymentAsset(filename, source))) {
    throw new Error("container hardening self-test missed a common deployment asset");
  }
  if (isDeploymentAsset(".github/workflows/ci.yml", "name: CI\nkindness: true\n")) {
    throw new Error("container hardening self-test classified a normal workflow as Kubernetes");
  }
  const slash = String.fromCharCode(92);
  const prerelease = `ENV A=1 ${slash}\n  TANDEM_ENGINE_VERSION=0.8.0-beta.1 ${slash}\n  B=2`;
  if (parsePinnedEngineVersion(prerelease) !== "0.8.0-beta.1") {
    throw new Error("container hardening self-test rejected a supported prerelease SemVer");
  }
  if (parsePinnedEngineVersion(`  TANDEM_ENGINE_VERSION=latest ${slash}`) !== "") {
    throw new Error("container hardening self-test accepted a floating engine version");
  }
}

function argValue(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

async function main() {
  if (process.argv.includes("--self-test")) selfTest();
  const result = await verifyContainerHardening(process.cwd(), {
    expectedEngineVersion: argValue("--expected-engine-version"),
  });
  if (result.errors.length > 0) {
    throw new Error(`container hardening policy failed:\n${result.errors.join("\n")}`);
  }
  process.stdout.write(
    `container hardening policy passed (${result.assets.length} deployment assets)\n`
  );
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main().catch((error) => {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  });
}
