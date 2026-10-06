#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const repositoryRoot = resolve(import.meta.dirname, "..");

// This fixed inventory preserves the managers used by the ordinary audit gate.
export const repairWorkspaces = [
  { lockfile: "apps/tandem-desktop/pnpm-lock.yaml", manager: "pnpm" },
  { lockfile: "guide/pnpm-lock.yaml", manager: "pnpm" },
  { lockfile: "packages/create-tandem-panel/template/package-lock.json", manager: "npm" },
  { lockfile: "packages/tandem-client-ts/pnpm-lock.yaml", manager: "pnpm" },
  { lockfile: "packages/tandem-control-panel/pnpm-lock.yaml", manager: "pnpm" },
  { lockfile: "scripts/bench-js/package-lock.json", manager: "npm" },
];

const patchedVersions = {
  "source-map-js": "1.2.2",
  "smol-toml": "1.9.0",
  katex: "0.18.2",
  "postcss-selector-parser": "7.1.6",
};

function atLeast(actual, minimum) {
  if (!/^\d+\.\d+\.\d+$/.test(actual)) return false;
  const parts = actual.split(".").map(Number);
  const floor = minimum.split(".").map(Number);
  for (let index = 0; index < parts.length; index += 1) {
    if (parts[index] !== floor[index]) return parts[index] > floor[index];
  }
  return true;
}

function lockGuardDiagnostics(workspace, content, name, found) {
  // Keep failure output closed to known package/version keys. Never include
  // registry resolutions, arbitrary lock values or the full generated lock.
  const escapedName = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const safeVersion = /^['"]?\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?['"]?$/;
  const versions = [...new Set(found.map((version) =>
    typeof version === "string" && version.length <= 96 && safeVersion.test(version)
      ? version : "<invalid-version>"))].sort();
  const versionToken = "\\d+\\.\\d+\\.\\d+(?:-[0-9A-Za-z.-]+)?(?:\\+[0-9A-Za-z.-]+)?";
  const keyPattern = workspace.manager === "npm"
    ? new RegExp(`^ {4}"(?:node_modules/(?:@[A-Za-z0-9._-]+/)?[A-Za-z0-9._-]+/)*node_modules/${escapedName}": \\{ *$`)
    : new RegExp(
      `^  ['"]?${escapedName}(?:@(?:${versionToken}|[-0-9.*+<>=~^| ()]+))?['"]?: *(?:['"]?${versionToken}['"]?|\\{\\})? *$`,
    );
  const keyLines = content.split(/\r?\n/).filter((line) =>
    line.length <= 256 && keyPattern.test(line));
  return {
    lockfile: workspace.lockfile,
    package: name,
    expectedFloor: patchedVersions[name],
    generatedLockSha256: createHash("sha256").update(content).digest("hex"),
    matchedVersionCount: found.length,
    matchedVersions: versions.slice(0, 8),
    omittedUniqueVersionCount: Math.max(0, versions.length - 8),
    matchingKeyLines: keyLines.slice(0, 8),
    omittedKeyLineCount: Math.max(0, keyLines.length - 8),
  };
}

function resolvedPnpmVersions(content, escapedName) {
  // pnpm uses the same indentation for override selectors and resolved keys.
  // Only packages and snapshots prove a resolved package version.
  const entry = new RegExp(
    "^  (?:" + escapedName + "@([^\\s:]+)|'" + escapedName +
      "@([^']+)'|\"" + escapedName + "@([^\"]+)\"):",
  );
  let section = "";
  const versions = [];
  for (const line of content.split(/\r?\n/)) {
    if (/^\S/.test(line)) {
      section = /^([A-Za-z][A-Za-z0-9_-]*):\s*$/.exec(line)?.[1] || "";
      continue;
    }
    if (section !== "packages" && section !== "snapshots") continue;
    const match = entry.exec(line);
    if (match) versions.push((match[1] ?? match[2] ?? match[3]).split("(")[0]);
  }
  return versions;
}

export function assertPatchedLock(workspace, content) {
  const required = workspace.lockfile === "guide/pnpm-lock.yaml"
    ? Object.keys(patchedVersions)
    : ["source-map-js"];
  const versions = {};
  for (const name of required) {
    let found;
    if (workspace.manager === "npm") {
      const lock = JSON.parse(content);
      if (lock.lockfileVersion !== 3 || !lock.packages) {
        throw new Error(`Unsupported npm lock format: ${workspace.lockfile}`);
      }
      found = Object.entries(lock.packages)
        .filter(([pathname]) => pathname === `node_modules/${name}`
          || pathname.endsWith(`/node_modules/${name}`))
        .map(([, pkg]) => pkg.version);
    } else {
      const escapedName = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      found = resolvedPnpmVersions(content, escapedName);
    }
    if (!found.length || found.some((version) => !atLeast(version, patchedVersions[name]))) {
      const error = new Error(`Patched ${name} not resolved in ${workspace.lockfile}`);
      error.lockGuardDiagnostics = lockGuardDiagnostics(workspace, content, name, found);
      throw error;
    }
    versions[name] = [...new Set(found)].sort();
  }
  return versions;
}

export function assertAllowedChanges(changedPaths, untrackedPaths = []) {
  const allowed = new Set(repairWorkspaces.map(({ lockfile }) => lockfile));
  if (untrackedPaths.length || changedPaths.some((pathname) => !allowed.has(pathname))) {
    throw new Error("Lock regeneration changed a path outside its fixed lockfile inventory");
  }
  if (new Set(changedPaths).size !== allowed.size) {
    throw new Error("Lock regeneration did not update all six affected lockfiles");
  }
}

function command(program, args, cwd = repositoryRoot, showOutput = false) {
  const result = spawnSync(program, args, {
    cwd,
    encoding: "utf8",
    env: { ...process.env, NO_COLOR: "1" },
    maxBuffer: 8 * 1024 * 1024,
  });
  if (showOutput) {
    process.stdout.write(result.stdout || "");
    process.stderr.write(result.stderr || "");
  }
  if (result.error || result.status !== 0) {
    throw new Error(`${program} failed during fixed lock regeneration (exit ${result.status})`);
  }
  return result.stdout;
}

function pathsFromNullList(value) {
  return value.split("\0").filter(Boolean);
}

function main() {
  if (process.env.GITHUB_ACTIONS !== "true"
    || process.env.RUNNER_ENVIRONMENT !== "github-hosted"
    || process.env.GITHUB_EVENT_NAME !== "workflow_dispatch"
    || !process.env.RUNNER_TEMP) {
    throw new Error("Lock regeneration is restricted to the explicit GitHub-hosted repair job");
  }
  const artifactDirectory = join(resolve(process.env.RUNNER_TEMP), "javascript-lockfile-repair");
  mkdirSync(artifactDirectory, { recursive: true });
  const reportPath = join(artifactDirectory, "report.json");
  const sourceCommit = command("git", ["rev-parse", "HEAD"]).trim();
  if (!/^[0-9a-f]{40}$/.test(sourceCommit) || sourceCommit !== process.env.GITHUB_SHA) {
    throw new Error("Repair checkout does not match the dispatched GitHub commit");
  }
  const report = { phase: "lock_regeneration", sourceCommit, ok: false, completed: [], lockfiles: [] };
  const saveReport = () => writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  saveReport();
  try {
    if (command("git", ["status", "--porcelain", "--untracked-files=all"]).trim()) {
      throw new Error("Repair must begin with a clean checkout");
    }
    report.pnpmVersion = command("pnpm", ["--version"]).trim();
    report.npmVersion = command("npm", ["--version"]).trim();
    if (report.pnpmVersion !== "10.34.5") throw new Error("Unexpected repair pnpm version");
    for (const workspace of repairWorkspaces) {
      const cwd = dirname(join(repositoryRoot, workspace.lockfile));
      const args = workspace.manager === "pnpm"
        ? ["install", "--lockfile-only", "--ignore-scripts", "--no-frozen-lockfile"]
        : ["install", "--package-lock-only", "--ignore-scripts", "--no-audit", "--no-fund"];
      command(workspace.manager, args, cwd, true);
      report.completed.push(workspace.lockfile);
      saveReport();
    }
    const changedPaths = pathsFromNullList(command("git", ["diff", "--name-only", "-z"]));
    const untrackedPaths = pathsFromNullList(command("git", ["ls-files", "--others", "--exclude-standard", "-z"]));
    assertAllowedChanges(changedPaths, untrackedPaths);
    for (const workspace of repairWorkspaces) {
      const content = readFileSync(join(repositoryRoot, workspace.lockfile), "utf8");
      report.lockfiles.push({
        ...workspace,
        sha256: createHash("sha256").update(content).digest("hex"),
        resolved: assertPatchedLock(workspace, content),
      });
    }
    const patch = command("git", ["diff", "--binary", "--no-ext-diff", "--",
      ...repairWorkspaces.map(({ lockfile }) => lockfile)]);
    if (!patch.trim()) throw new Error("Repair produced an empty patch");
    writeFileSync(join(artifactDirectory, "lockfile-repair.patch"), patch);
    report.patchSha256 = createHash("sha256").update(patch).digest("hex");
    report.ok = true;
    saveReport();
  } catch (error) {
    report.failure = error.lockGuardDiagnostics
      ? { message: error.message, lockGuard: error.lockGuardDiagnostics }
      : error.message;
    saveReport();
    if (error.lockGuardDiagnostics) process.stdout.write(`${JSON.stringify(report.failure)}\n`);
    throw error;
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  try {
    main();
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
