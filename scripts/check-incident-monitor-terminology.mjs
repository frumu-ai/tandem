#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const checkedPaths = [
  ".github/",
  "CHANGELOG.md",
  "RELEASE_NOTES.md",
  "package.json",
  "apps/tandem-desktop/src/",
  "apps/tandem-desktop/src-tauri/src/",
  "crates/tandem-eval/",
  "crates/tandem-incident-monitor/",
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

const staleTerms = [
  "Bug Monitor",
  "Bug monitor",
  "bug monitor",
  "BugMonitor",
  "bugMonitor",
  "bug_monitor",
  "bug-monitor",
  "Failure Reporter",
  "Failure reporter",
  "failure reporter",
  "FailureReporter",
  "failure-reporter",
  "failure_reporter",
];

const allowedMatches = [
  {
    file: "apps/tandem-desktop/src/lib/tauri.ts",
    line: /\bbug_monitor\?: IncidentMonitorConfig\b/,
    reason: "legacy config response hydration fallback",
  },
  {
    file: "apps/tandem-desktop/src/components/settings/IncidentMonitorSettings.tsx",
    line: /\b(configPayload|response)\.bug_monitor\b/,
    reason: "legacy config response hydration fallback",
  },
  {
    file: "packages/tandem-control-panel/src/pages/SettingsPageController.ts",
    line: /\bconfigPayload(\?\.|\.)bug_monitor\b/,
    reason: "legacy config response hydration fallback",
  },
  {
    file: "packages/create-tandem-panel/template/src/pages/SettingsPage.tsx",
    line: /\bconfigPayload(\?\.|\.)bug_monitor\b/,
    reason: "legacy config response hydration fallback",
  },
  {
    file: "packages/tandem-control-panel/src/app/routes.ts",
    line: /^\s*\["(bug-monitor|failure-reporter)", "incident-monitor"\],$/,
    reason: "legacy public route redirect",
  },
  {
    file: "packages/create-tandem-panel/template/src/app/routes.ts",
    line: /^\s*\["(bug-monitor|failure-reporter)", "incident-monitor"\],$/,
    reason: "legacy public route redirect",
  },
  {
    file: "packages/tandem-client-ts/test/events.test.ts",
    line: /\/failure-reporter\//,
    reason: "negative assertion against legacy route use",
  },
  {
    file: "packages/tandem-control-panel/lib/automations/workflow-list.js",
    line: /(metadataSource|creatorId) === "bug_monitor(_approval)?"|name\.startsWith\("bug monitor triage:"\)/,
    reason: "classification fallback for already-saved automation records",
  },
  {
    file: "scripts/check-rust-panic-surface.mjs",
    line: /"crates\/tandem-server\/src\/bug_monitor\/log_watcher\.rs"/,
    reason: "server-internal module path that remains outside the public-surface rename",
  },
  {
    file: "crates/tandem-incident-monitor/src/types.rs",
    line: /^pub type BugMonitor[A-Za-z0-9_]+ = IncidentMonitor[A-Za-z0-9_]+;$/,
    reason: "temporary Rust compatibility alias",
  },
  {
    file: "crates/tandem-incident-monitor/src/log_parser.rs",
    line: /^pub type BugMonitor[A-Za-z0-9_]+ = IncidentMonitor[A-Za-z0-9_]+;$/,
    reason: "temporary Rust compatibility alias",
  },
  {
    file: "crates/tandem-eval/src/incident_monitor_regression_fixture.rs",
    line: /^pub type BugMonitor[A-Za-z0-9_]+ = IncidentMonitor[A-Za-z0-9_]+;$/,
    reason: "temporary Rust compatibility alias",
  },
  {
    file: "crates/tandem-eval/src/lib.rs",
    line: /^pub mod bug_monitor_regression_fixture \{$/,
    reason: "temporary Rust compatibility alias",
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
  return staleTerms.some((term) => line.includes(term));
}

function pathContainsStaleTerm(file) {
  return staleTerms.some((term) => file.includes(term));
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
