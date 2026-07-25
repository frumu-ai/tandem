#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const matrixPath = path.join(repoRoot, "docs", "SECURITY_BOUNDARY_RETEST_MATRIX.json");
const requiredIds = Array.from({ length: 17 }, (_, index) => `TA-${String(index + 1).padStart(2, "0")}`);
const requiredFixtureValues = [
  "org-a/workspace-a",
  "org-b/workspace-b",
  "ordinary_member",
  "deployment_admin",
  "independent_reviewer",
  "temporary_workspace",
  "temporary_git_repository",
  "temporary_pack_root",
  "marker_provider_credential",
  "marker_channel_credential",
  "marker_audit_value",
  "fake_private_dns_answer",
  "fake_cloud_metadata_address",
  "fake_redirect_service",
  "chunked_over_budget_service",
  "protected_audit_unwritable",
  "state_persistence_failure",
  "two_replay_store_instances",
];

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function loadMatrix() {
  return JSON.parse(fs.readFileSync(matrixPath, "utf8"));
}

function allControls(matrix) {
  return matrix.findings.flatMap((finding) => [
    ...finding.negative_controls.map((control) => ({ ...control, finding: finding.id, polarity: "negative" })),
    ...finding.positive_controls.map((control) => ({ ...control, finding: finding.id, polarity: "positive" })),
  ]);
}

export function matrixSymbols(matrix) {
  return [...new Set(allControls(matrix).map((control) => control.symbol))].sort();
}

export function validateMatrix(
  matrix,
  readSource = (sourcePath) => fs.readFileSync(path.join(repoRoot, sourcePath), "utf8"),
) {
  const errors = [];
  if (matrix.schema_version !== 1) errors.push("schema_version must be 1");
  if (!Array.isArray(matrix.findings)) errors.push("findings must be an array");
  if (errors.length > 0) throw new Error(`security retest matrix invalid:\n${errors.join("\n")}`);

  const ids = matrix.findings.map((finding) => finding.id);
  if (JSON.stringify(ids) !== JSON.stringify(requiredIds)) {
    errors.push(`finding ids must be exactly ${requiredIds.join(", ")} in order`);
  }
  const fixtureText = JSON.stringify(matrix.fixture_catalog ?? {});
  for (const required of requiredFixtureValues) {
    if (!fixtureText.includes(`"${required}"`)) errors.push(`fixture catalog missing ${required}`);
  }

  for (const finding of matrix.findings) {
    if (typeof finding.title !== "string" || finding.title.trim().length < 12) {
      errors.push(`${finding.id} must include its authoritative title`);
    }
    if (!Array.isArray(finding.fixture_tags) || finding.fixture_tags.length === 0) {
      errors.push(`${finding.id} must name its dynamic fixtures`);
    }
    for (const polarity of ["negative_controls", "positive_controls"]) {
      if (!Array.isArray(finding[polarity]) || finding[polarity].length === 0) {
        errors.push(`${finding.id} must include at least one ${polarity}`);
        continue;
      }
      for (const control of finding[polarity]) {
        if (!/^[a-z0-9-]+$/.test(control.package ?? "")) {
          errors.push(`${finding.id} has invalid package ${control.package ?? "<missing>"}`);
        }
        if (
          typeof control.source !== "string" ||
          path.isAbsolute(control.source) ||
          control.source.split(/[\\/]/).includes("..")
        ) {
          errors.push(`${finding.id} has unsafe source path ${control.source ?? "<missing>"}`);
          continue;
        }
        if (!/^[A-Za-z0-9_]+$/.test(control.symbol ?? "")) {
          errors.push(`${finding.id} has invalid test symbol ${control.symbol ?? "<missing>"}`);
          continue;
        }
        let source;
        try {
          source = readSource(control.source);
        } catch {
          errors.push(`${finding.id} source does not exist: ${control.source}`);
          continue;
        }
        const declaration = new RegExp(
          `(?:async\\s+)?fn\\s+${escapeRegExp(control.symbol)}\\s*\\(`,
        );
        if (!declaration.test(source)) {
          errors.push(`${finding.id} test symbol not found in ${control.source}: ${control.symbol}`);
        }
      }
    }
  }
  if (errors.length > 0) throw new Error(`security retest matrix invalid:\n${errors.join("\n")}`);
  return { finding_count: matrix.findings.length, test_count: matrixSymbols(matrix).length };
}

export function verifyNextestEvidence(matrix, discoveredText, junitText) {
  const errors = [];
  const executedTestcases = [...junitText.matchAll(/<testcase\b[^>]*(?:\/>|>[\s\S]*?<\/testcase>)/g)]
    .map((match) => match[0])
    .filter((testcase) => !/<skipped\b/.test(testcase))
    .join("\n");
  for (const symbol of matrixSymbols(matrix)) {
    const exactIdentifier = new RegExp(
      `(?:^|[^A-Za-z0-9_])${escapeRegExp(symbol)}(?:$|[^A-Za-z0-9_])`,
      "m",
    );
    if (!exactIdentifier.test(discoveredText)) errors.push(`not discovered: ${symbol}`);
    if (!exactIdentifier.test(executedTestcases)) errors.push(`not executed: ${symbol}`);
  }
  if (errors.length > 0) {
    throw new Error(`security retest execution evidence incomplete:\n${errors.join("\n")}`);
  }
}

function optionValue(name) {
  const index = process.argv.indexOf(name);
  return index === -1 ? null : process.argv[index + 1] ?? null;
}

function runSelfTest() {
  const matrix = loadMatrix();
  validateMatrix(matrix);
  const invalid = structuredClone(matrix);
  invalid.findings[0].positive_controls = [];
  let rejected = false;
  try {
    validateMatrix(invalid);
  } catch {
    rejected = true;
  }
  if (!rejected) throw new Error("matrix self-test failed to reject missing positive controls");
  process.stdout.write("security retest matrix self-test passed\n");
}

function main() {
  if (process.argv.includes("--self-test")) {
    runSelfTest();
    return;
  }
  const matrix = loadMatrix();
  const summary = validateMatrix(matrix);
  if (process.argv.includes("--print-nextest-filter")) {
    process.stdout.write(`${matrixSymbols(matrix).map((symbol) => `test(${symbol})`).join(" | ")}\n`);
    return;
  }
  const discoveredPath = optionValue("--nextest-list");
  const junitPath = optionValue("--junit");
  if ((discoveredPath && !junitPath) || (!discoveredPath && junitPath)) {
    throw new Error("--nextest-list and --junit must be provided together");
  }
  if (discoveredPath && junitPath) {
    verifyNextestEvidence(
      matrix,
      fs.readFileSync(discoveredPath, "utf8"),
      fs.readFileSync(junitPath, "utf8"),
    );
    process.stdout.write(`verified execution evidence for ${summary.test_count} security tests\n`);
    return;
  }
  process.stdout.write(
    `security retest matrix verified: ${summary.finding_count} findings, ${summary.test_count} unique tests\n`,
  );
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}
