#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const checkedPaths = [
  ".env.example",
  ".github/",
  "CHANGELOG.md",
  "RELEASE_NOTES.md",
  "README.md",
  "README.zh-CN.md",
  "engine/",
  "eval_datasets/",
  "examples/",
  "agent-templates/",
  "contracts/",
  "manifests/",
  "specs/",
  "package.json",
  "apps/tandem-desktop/src/",
  "apps/tandem-desktop/src-tauri/src/",
  "crates/tandem-eval/",
  "crates/tandem-incident-monitor/",
  "crates/tandem-runtime/",
  "crates/tandem-server/",
  "crates/tandem-types/",
  "crates/tandem-wire/",
  "docs/",
  "guide/",
  "packages/create-tandem-panel/",
  "packages/tandem-client-py/",
  "packages/tandem-client-ts/",
  "packages/tandem-control-panel/",
  "scripts/",
];

const ignoredPathParts = new Set([".git", ".turbo", "dist", "node_modules", "target"]);

const ignoredFiles = new Set(["scripts/check-incident-monitor-terminology.mjs"]);

// Canonical, lowercased patterns. Matching is case-insensitive so mixed-case
// residuals like `Tandem-Bug-Monitor` are caught via `bug-monitor`.
const staleTerms = [
  "bug monitor",
  "bugmonitor",
  "bug_monitor",
  "bug-monitor",
  "failure reporter",
  "failurereporter",
  "failure-reporter",
  "failure_reporter",
  "tbm_",
];

const allowedMatches = [
  // `TANDEM_FAILURE_REPORTER_*` are legacy environment-variable names read only
  // as backward-compatible fallbacks (see env_value(new, legacy)); they are an
  // intentional compatibility surface, not stale terminology to rename.
  {
    file: "crates/tandem-server/src/config/env.rs",
    line: /TANDEM_FAILURE_REPORTER_/,
  },
  {
    file: "crates/tandem-server/src/config/engine.rs",
    line: /TANDEM_FAILURE_REPORTER_/,
  },
  // `TANDEM_BUG_MONITOR_*` are the oldest legacy env-var names, read only as a
  // deprecated backward-compatible fallback (TAN-542); an intentional
  // compatibility surface, not stale terminology to rename.
  {
    file: "crates/tandem-server/src/config/env.rs",
    line: /bug_monitor/i,
  },
  // Legacy on-disk state file names (`failure_reporter_*` / `bug_monitor_*`)
  // are read as migrate-on-load fallbacks so upgrades don't lose state
  // (TAN-542), not stale terminology.
  {
    file: "crates/tandem-server/src/app/state/app_state_impl_parts/part06.rs",
    line: /(failure_reporter|bug_monitor)_/,
  },
];

function normalizePath(file) {
  return file.split(path.sep).join("/");
}

function isCheckedPath(file) {
  return checkedPaths.some((checkedPath) => file === checkedPath || file.startsWith(checkedPath));
}

function hasIgnoredPart(file) {
  return file.split("/").some((part) => ignoredPathParts.has(part));
}

function readTrackedFiles() {
  const output = execFileSync("git", ["ls-files"], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  return output
    .split(/\r?\n/)
    .filter(Boolean)
    .map(normalizePath)
    .filter((file) => isCheckedPath(file) && !hasIgnoredPart(file) && !ignoredFiles.has(file));
}

function lineContainsStaleTerm(line) {
  const lowered = line.toLowerCase();
  return staleTerms.some((term) => lowered.includes(term));
}

function pathContainsStaleTerm(file) {
  const lowered = file.toLowerCase();
  return staleTerms.some((term) => lowered.includes(term));
}

function isAllowed(file, line) {
  return allowedMatches.some((entry) => entry.file === file && entry.line.test(line));
}

const failures = [];

for (const file of readTrackedFiles()) {
  if (pathContainsStaleTerm(file)) {
    failures.push({
      file,
      lineNumber: 0,
      line: file,
      message: "stale term in tracked file path",
    });
  }

  const absolute = path.join(repoRoot, file);
  const source = fs.readFileSync(absolute, "utf8");
  const lines = source.split(/\r?\n/);

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (!lineContainsStaleTerm(line) || isAllowed(file, line)) {
      continue;
    }
    failures.push({
      file,
      lineNumber: index + 1,
      line: line.trim(),
      message: "stale Incident Monitor terminology",
    });
  }
}

if (failures.length > 0) {
  console.error("Found stale Bug Monitor/Failure Reporter terminology outside the allowlist:");
  for (const failure of failures) {
    const location =
      failure.lineNumber > 0 ? `${failure.file}:${failure.lineNumber}` : failure.file;
    console.error(`- ${location}: ${failure.message}`);
    console.error(`  ${failure.line}`);
  }
  process.exit(1);
}

console.log("Incident Monitor terminology check passed.");
