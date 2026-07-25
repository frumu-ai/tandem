#!/usr/bin/env node

import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const repositoryRoot = resolve(import.meta.dirname, "..");
const ignoredDirectories = new Set([".git", "node_modules", "target"]);
const expected = [
  { lockfile: "pnpm-lock.yaml", manager: "pnpm", manifests: ["package.json"] },
  {
    lockfile: "apps/tandem-desktop/pnpm-lock.yaml",
    manager: "pnpm",
    manifests: ["apps/tandem-desktop/package.json"],
  },
  { lockfile: "guide/pnpm-lock.yaml", manager: "pnpm", manifests: ["guide/package.json"] },
  {
    lockfile: "packages/create-tandem-panel/template/package-lock.json",
    manager: "npm",
    manifests: ["packages/create-tandem-panel/template/package.json"],
  },
  {
    lockfile: "packages/tandem-client-ts/pnpm-lock.yaml",
    manager: "pnpm",
    manifests: ["packages/tandem-client-ts/package.json"],
  },
  {
    lockfile: "packages/tandem-control-panel/pnpm-lock.yaml",
    manager: "pnpm",
    manifests: ["packages/tandem-control-panel/package.json"],
  },
  {
    lockfile: "scripts/bench-js/package-lock.json",
    manager: "npm",
    manifests: ["scripts/bench-js/package.json"],
  },
];

function discoverLockfiles(directory) {
  const found = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (!ignoredDirectories.has(entry.name))
        found.push(...discoverLockfiles(join(directory, entry.name)));
      continue;
    }
    if (!["pnpm-lock.yaml", "package-lock.json", "yarn.lock"].includes(entry.name)) continue;
    found.push(relative(repositoryRoot, join(directory, entry.name)).replaceAll("\\", "/"));
  }
  return found;
}

function discoverDependencyManifests(directory) {
  const found = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (!ignoredDirectories.has(entry.name)) {
        found.push(...discoverDependencyManifests(join(directory, entry.name)));
      }
      continue;
    }
    if (entry.name !== "package.json") continue;
    const pathname = join(directory, entry.name);
    const manifest = JSON.parse(readFileSync(pathname, "utf8"));
    const dependencyCount = [
      manifest.dependencies,
      manifest.devDependencies,
      manifest.optionalDependencies,
      manifest.peerDependencies,
    ].reduce((total, group) => total + Object.keys(group || {}).length, 0);
    if (dependencyCount > 0) {
      found.push(relative(repositoryRoot, pathname).replaceAll("\\", "/"));
    }
  }
  return found;
}

function vulnerabilityTotal(report) {
  const counts = report?.metadata?.vulnerabilities;
  if (!counts || typeof counts !== "object") return null;
  if (Number.isFinite(Number(counts.total))) return Number(counts.total);
  return Object.entries(counts)
    .filter(([key]) => key !== "total")
    .reduce((sum, [, value]) => sum + (Number(value) || 0), 0);
}

const discovered = discoverLockfiles(repositoryRoot).sort();
const required = expected.map(({ lockfile }) => lockfile).sort();
if (JSON.stringify(discovered) !== JSON.stringify(required)) {
  console.error("JavaScript lockfile inventory changed without security-gate coverage.");
  console.error(JSON.stringify({ required, discovered }, null, 2));
  process.exit(1);
}
const discoveredManifests = discoverDependencyManifests(repositoryRoot).sort();
const requiredManifests = expected.flatMap(({ manifests }) => manifests).sort();
if (JSON.stringify(discoveredManifests) !== JSON.stringify(requiredManifests)) {
  console.error(
    "Dependency-bearing JavaScript manifest inventory changed without lockfile audit coverage."
  );
  console.error(JSON.stringify({ requiredManifests, discoveredManifests }, null, 2));
  process.exit(1);
}

const results = [];
let failed = false;
for (const workspace of expected) {
  const cwd = dirname(join(repositoryRoot, workspace.lockfile));
  const command = workspace.manager;
  const args = ["audit", "--audit-level=low", "--json"];
  const audit = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    env: { ...process.env, NO_COLOR: "1" },
    maxBuffer: 16 * 1024 * 1024,
  });
  let report;
  try {
    report = JSON.parse(audit.stdout || "{}");
  } catch {
    report = null;
  }
  const vulnerabilities = vulnerabilityTotal(report);
  const ok = audit.status === 0 && vulnerabilities === 0;
  results.push({
    workspace: relative(repositoryRoot, cwd).replaceAll("\\", "/") || ".",
    manager: workspace.manager,
    vulnerabilities,
    ok,
  });
  if (!ok) {
    failed = true;
    console.error(`Audit failed for ${workspace.lockfile}.`);
    if (vulnerabilities === null) {
      console.error((audit.stderr || audit.stdout || "No audit output").trim());
    } else {
      console.error(JSON.stringify(report, null, 2));
    }
  }
}

console.log(
  JSON.stringify(
    { lockfiles: required.length, dependencyManifests: requiredManifests.length, results },
    null,
    2
  )
);
if (failed) process.exit(1);
