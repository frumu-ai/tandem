#!/usr/bin/env node

import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const EXPECTED_DEPLOYMENT_ASSETS = new Set([
  "packages/tandem-control-panel/docker-compose.yml",
  "packages/tandem-control-panel/docker/control-panel.Dockerfile",
  "packages/tandem-control-panel/docker/engine.Dockerfile",
  "scripts/linux-release-builder.Dockerfile",
]);
const PINNED_NODE_BASE =
  "node:24.20.0-trixie-slim@sha256:50c3b2f6988dfc307b86e5301d69611af31f4789bdf232863b07d3b02fe55ae0";
const SEMVER_NUMERIC_IDENTIFIER = "(?:0|[1-9][0-9]*)";
const SEMVER_PRERELEASE_IDENTIFIER =
  "(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)";
const SEMVER_BUILD_IDENTIFIER = "[0-9A-Za-z-]+";
const EXACT_SEMVER = new RegExp(
  `^${SEMVER_NUMERIC_IDENTIFIER}\\.${SEMVER_NUMERIC_IDENTIFIER}\\.${SEMVER_NUMERIC_IDENTIFIER}` +
    `(?:-${SEMVER_PRERELEASE_IDENTIFIER}(?:\\.${SEMVER_PRERELEASE_IDENTIFIER})*)?` +
    `(?:\\+${SEMVER_BUILD_IDENTIFIER}(?:\\.${SEMVER_BUILD_IDENTIFIER})*)?$`
);

function requireMatch(source, pattern, message, errors) {
  if (!pattern.test(source)) errors.push(message);
}

function countMatches(source, pattern) {
  return [...source.matchAll(pattern)].length;
}

// Require the pinned command as a RUN instruction or a continued RUN command,
// not merely as prose, an ENV value, or an argument to echo/printf.
function hasPinnedOsUpgrade(source) {
  const runtimeStage = source.split(/^[ \t]*FROM[ \t]+[^\r\n]+$/im).at(-1);
  // The pinned base's default shell is part of the execution contract. A
  // stage-local override can report success without running any RUN payload.
  if (/^[ \t]*SHELL[ \t]+/im.test(runtimeStage)) return false;
  const runs = runtimeStage.match(/^[ \t]*RUN[ \t]+(?:[^\n]*\\\r?\n)*[^\n]*/gim) || [];
  return runs.some((run) => {
    // Mask shell strings/escapes before inspecting command boundaries. Keep a
    // placeholder for quoted arguments so they cannot disappear into a command.
    const shell = run.replace(/\\\r?\n/g, " ");
    let quote = null;
    let commands = "";
    for (let i = 0; i < shell.length; i++) {
      const char = shell[i];
      if (char === "\\" && quote !== "'") {
        commands += "__";
        i++;
      } else if (quote) {
        if (char === quote) quote = null;
        commands += "_";
      } else if (char === "'" || char === '"' || char === "`") {
        quote = char;
        commands += "_";
      } else if (char === "#") {
        break;
      } else {
        commands += char;
      }
    }
    // Accept only the repository's straight-line AND chain. OR, pipelines,
    // groups, substitutions and early-exit builtins can make a skipped upgrade
    // look like a successful image build, so do not try to interpret them.
    if (quote || /[|;(){}$&]/.test(commands.replaceAll("&&", "")) || commands.includes("<<")) return false;
    const steps = commands.replace(/^[ \t]*RUN\s+/i, "").split(/\s*&&\s*/).map((step) => step.trim());
    const upgrade = steps.indexOf("apt-get -y --no-install-recommends upgrade");
    if (upgrade < 0) return false;
    return steps.slice(0, upgrade).every((step) =>
      step === "rm -f /etc/apt/sources.list.d/debian.sources" ||
      /^printf _+(?:\s+_+)+\s+>\s+\/etc\/apt\/sources\.list$/.test(step) ||
      /^apt-get(?: -o Acquire::Check-Valid-Until=false)? update$/.test(step)
    );
  });
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
    const fromLines = source.match(/^[ \t]*FROM[ \t]+[^\r\n]+/gim) || [];
    if (fromLines.length === 0) errors.push(`${name} has no FROM instruction`);
    for (const line of fromLines) {
      const base = line.match(/^[ \t]*FROM[ \t]+(\S+)(?:[ \t]+AS[ \t]+\S+)?[ \t]*$/i)?.[1];
      if (base !== PINNED_NODE_BASE) {
        errors.push(`${name} uses an unapproved or non-digest-pinned base: ${line}`);
      }
    }
    const runtimeStage = source.split(/^[ \t]*FROM[ \t]+[^\r\n]+$/im).at(-1);
    const users = [...runtimeStage.matchAll(/^[ \t]*USER[ \t]+([^\r\n]+)$/gim)];
    if (users.at(-1)?.[1].trim() !== "node") errors.push(`${name} must run as USER node`);
    if (/@latest\b|ENGINE_VERSION=latest\b/.test(source)) {
      errors.push(`${name} contains a floating latest dependency`);
    }
    for (const marker of [
      "snapshot.debian.org/archive/debian/20260923T120000Z",
      "snapshot.debian.org/archive/debian-security/20260923T120000Z",
      "ca-certificates=20250419",
      "curl=8.14.1-2+deb13u5",
      "libssl3t64=3.5.7-1~deb13u2",
      "openssl=3.5.7-1~deb13u2",
      "openssl-provider-legacy=3.5.7-1~deb13u2",
    ]) {
      if (!source.includes(marker)) errors.push(`${name} is missing immutable OS input ${marker}`);
    }
    if (!hasPinnedOsUpgrade(source)) errors.push(`${name} must execute the pinned OS upgrade`);
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
  for (const source of [
    "RUN apt-get -y --no-install-recommends upgrade",
    `RUN apt-get update ${slash}\n  && apt-get -y --no-install-recommends upgrade ${slash}\n  && true`,
  ]) {
    if (!hasPinnedOsUpgrade(source)) throw new Error("missing real OS upgrade instruction");
  }
  for (const source of [
    "# RUN apt-get -y --no-install-recommends upgrade",
    'RUN echo "apt-get -y --no-install-recommends upgrade"',
    'ENV NOTE="apt-get -y --no-install-recommends upgrade"',
    "RUN true\n# && apt-get -y --no-install-recommends upgrade",
    `RUN printf '%s' ' ${slash}\n  && apt-get -y --no-install-recommends upgrade ${slash}\n  '`,
    `RUN echo " ${slash}\n  && apt-get -y --no-install-recommends upgrade ${slash}\n  "`,
    "RUN true # && apt-get -y --no-install-recommends upgrade",
    "FROM base AS build\nRUN apt-get -y --no-install-recommends upgrade\nFROM base\nRUN true",
    "RUN false && apt-get -y --no-install-recommends upgrade && true || true",
    "RUN false && (true && apt-get -y --no-install-recommends upgrade) || true",
    "RUN unused() { true && apt-get -y --no-install-recommends upgrade; }; true",
    "RUN exit 0 && apt-get -y --no-install-recommends upgrade",
    "RUN exec true && apt-get -y --no-install-recommends upgrade",
    'FROM base\nSHELL ["/bin/sh", "-c", "exit 0"]\nRUN apt-get -y --no-install-recommends upgrade',
    'FROM base\nshell ["/bin/echo"]\nRUN apt-get -y --no-install-recommends upgrade',
    "RUN apt-get update && apt-get -y --no-install-recommends upgrade && true & exit 0",
    "FROM base\nRUN apt-get -y --no-install-recommends upgrade\nfrom alpine:3.20\nRUN true",
    "FROM base\nRUN apt-get -y --no-install-recommends upgrade\n  from alpine:3.20\nRUN true",
  ]) {
    if (hasPinnedOsUpgrade(source)) throw new Error("accepted inert OS upgrade text");
  }
  const prerelease =
    `ENV A=1 ${slash}\n` +
    `  TANDEM_ENGINE_VERSION=0.8.0-beta.1+build.01 ${slash}\n  B=2`;
  if (parsePinnedEngineVersion(prerelease) !== "0.8.0-beta.1+build.01") {
    throw new Error("container hardening self-test rejected a supported prerelease SemVer");
  }
  for (const invalid of ["latest", "01.2.3", "1.02.3", "1.2.03", "1.2.3-01"]) {
    if (parsePinnedEngineVersion(`  TANDEM_ENGINE_VERSION=${invalid} ${slash}`) !== "") {
      throw new Error(`container hardening self-test accepted invalid SemVer ${invalid}`);
    }
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
