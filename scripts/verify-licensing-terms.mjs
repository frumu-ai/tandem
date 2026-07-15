#!/usr/bin/env node

// Verifies the prospective BUSL commercial boundary. This is intentionally a
// documentation/license check: it neither implements nor validates technical
// license enforcement.

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const protectedBuslPackages = [
  "tandem-plan-compiler",
  "tandem-governance-engine",
  "tandem-enterprise-server",
  "tandem-incident-monitor",
];
const expectedChangeLicense = "GPL-2.0-or-later OR MIT OR Apache-2.0";
const requiredGrantFragments = [
  "evaluation, source inspection, security review, development, testing",
  "personal self-hosting",
  "not performed\non behalf of an employer or client",
  "does not process employer, client, or\ncustomer production data",
  "does not govern commercial production agents or\nsystems",
  "Agencies, consultants, and systems integrators",
  "This permission does not authorize production deployment for a client.",
  "Any Production Use of the Licensed Work requires a separate commercial",
  "internal Production Use by a commercial organization is not",
  "embedded, OEM, reseller, or other commercial product or service.",
  "not Production Use solely\nbecause it is persistent, scheduled, or unattended",
  "separate written partner agreement with\nFrumu LTD.",
];
const legacyPhraseTokens = [
  ["internal", "production use is free"],
  ["internal use", "at no cost"],
  ["regardless", "of revenue"],
  ["self-hosted", "in production"],
  ["internal use", "is permitted"],
  ["production use", "is free"],
  ["own", "internal use"],
  ["free internal", "production"],
  ["hosted-service", "restriction"],
  ["commercial license", "required only"],
  ["free for local development", "and internal deployment"],
  ["cannot be wrapped and sold", "as a managed"],
  ["managed competitive", "SaaS"],
  ["本地开发和内部部署"],
  ["竞争性 managed", "SaaS 销售"],
];

function read(relativePath) {
  return fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
}

function exists(relativePath) {
  return fs.existsSync(path.join(repoRoot, relativePath));
}

function normalize(value) {
  return value.replace(/\r\n/g, "\n").trim();
}

function packageLicense(manifest) {
  return read(manifest).match(/^\s*license\s*=\s*"([^"]+)"/m)?.[1];
}

function extractGrant(licensePath) {
  const match = read(licensePath).match(
    /^Additional Use Grant:\n([\s\S]*?)^Change Date:/m
  );
  return match ? normalize(match[1]) : null;
}

function validIsoDate(value) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const date = new Date(`${value}T00:00:00Z`);
  return !Number.isNaN(date.valueOf()) && date.toISOString().slice(0, 10) === value;
}

function workspaceMembers() {
  const membersBlock = read("Cargo.toml").match(/members\s*=\s*\[([\s\S]*?)\]/);
  if (!membersBlock) throw new Error("Could not parse workspace members from Cargo.toml");
  return [...membersBlock[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
}

const problems = [];
const buslMembers = [];

for (const member of workspaceMembers()) {
  const manifest = `${member}/Cargo.toml`;
  if (packageLicense(manifest) === "BUSL-1.1") {
    buslMembers.push(path.basename(member));
  }
}

if (buslMembers.sort().join("\n") !== protectedBuslPackages.slice().sort().join("\n")) {
  problems.push(
    `BUSL manifest set differs from protected package set: found ${buslMembers.join(", ")}`
  );
}

const grants = [];
const changeDates = new Set();
for (const packageName of protectedBuslPackages) {
  const manifest = `crates/${packageName}/Cargo.toml`;
  const licensePath = `crates/${packageName}/LICENSE`;
  if (packageLicense(manifest) !== "BUSL-1.1") {
    problems.push(`${manifest} must declare BUSL-1.1`);
  }
  if (!exists(licensePath)) {
    problems.push(`${licensePath} is missing`);
    continue;
  }

  const license = read(licensePath);
  if (!license.includes("Business Source License 1.1")) {
    problems.push(`${licensePath} does not preserve the standard BUSL heading`);
  }
  if (!license.includes(`Change License: ${expectedChangeLicense}`)) {
    problems.push(`${licensePath} changes the approved Change License`);
  }
  if (/Enterprise Pilot/i.test(extractGrant(licensePath) ?? "")) {
    problems.push(`${licensePath} makes a pilot program part of the public grant`);
  }

  const grant = extractGrant(licensePath);
  if (!grant) {
    problems.push(`${licensePath} has no parsable Additional Use Grant`);
  } else {
    grants.push({ licensePath, grant });
    for (const fragment of requiredGrantFragments) {
      if (!grant.includes(fragment)) {
        problems.push(`${licensePath} is missing required grant language: ${fragment}`);
      }
    }
  }

  const changeDate = license.match(/^Change Date:\s*(\S+)$/m)?.[1];
  if (!changeDate || !validIsoDate(changeDate)) {
    problems.push(`${licensePath} has an invalid Change Date`);
  } else {
    changeDates.add(changeDate);
  }
}

// Every Rust source file in a protected package must carry the Frumu LTD
// copyright and BUSL header so file-level provenance is unambiguous.
function rustFiles(dir) {
  const absolute = path.join(repoRoot, dir);
  if (!fs.existsSync(absolute)) return [];
  return fs.readdirSync(absolute, { withFileTypes: true }).flatMap((entry) => {
    const child = `${dir}/${entry.name}`;
    if (entry.isDirectory()) {
      return entry.name === "target" ? [] : rustFiles(child);
    }
    return entry.name.endsWith(".rs") ? [child] : [];
  });
}

for (const packageName of protectedBuslPackages) {
  for (const file of rustFiles(`crates/${packageName}`)) {
    const head = read(file).slice(0, 400);
    if (
      !head.includes("Copyright (c) 2026 Frumu LTD") ||
      !head.includes("Licensed under the Business Source License 1.1")
    ) {
      problems.push(`${file} is missing the Frumu LTD BUSL copyright header`);
    }
  }
}

if (new Set(grants.map(({ grant }) => grant)).size !== 1) {
  problems.push("Protected BUSL packages do not use identical Additional Use Grants");
}
if (changeDates.size !== 1) {
  problems.push(`Protected BUSL Change Dates disagree: ${[...changeDates].join(", ")}`);
}

const rootLicense = read("LICENSE");
if (!rootLicense.includes("repository-level notice") || !rootLicense.includes("not a blanket")) {
  problems.push("Root LICENSE must remain a non-blanket repository notice");
}
if (!rootLicense.includes("package-local manifest and package-local license file control")) {
  problems.push("Root LICENSE must preserve package-local license authority");
}
if (!rootLicense.includes("Commercial production use, including")) {
  problems.push("Root LICENSE does not state the commercial production boundary");
}

const licensingDoc = read("docs/LICENSING.md");
const requiredDocFragments = [
  "the intended policy for **0.7.0 and later**",
  "0.6.9 remains governed by its original",
  "it does not revoke, replace, or modify",
  "Personal or hobbyist use cannot be performed for an employer or client",
  "do not authorize a\nclient production deployment",
  "not a condition of\nthe public BUSL grant",
  "may charge for their own consulting",
  "no automatic production exemption based on organization\nsize",
  "qualified software-licensing lawyer",
];
for (const fragment of requiredDocFragments) {
  if (!licensingDoc.includes(fragment)) {
    problems.push(`docs/LICENSING.md is missing required prospective notice: ${fragment}`);
  }
}

const releaseTools = [
  "scripts/bump-version.sh",
  "scripts/bump-version.ps1",
  ".github/workflows/release.yml",
];
for (const tool of releaseTools) {
  const source = read(tool);
  if (!source.includes("Change Date") || !source.includes("Business Source License 1.1")) {
    problems.push(`${tool} does not visibly stamp BUSL Change Dates`);
  }
  if (!source.includes("Current source-tree BUSL Change Date")) {
    problems.push(`${tool} does not update the documented current BUSL Change Date`);
  }
}
if (!read("scripts/bump-version.sh").includes("crates/${entry}/LICENSE")) {
  problems.push("scripts/bump-version.sh does not dynamically discover BUSL licenses");
}
if (!read("scripts/bump-version.ps1").includes("crates/${entry}/LICENSE")) {
  problems.push("scripts/bump-version.ps1 does not dynamically discover BUSL licenses");
}
if (!read(".github/workflows/release.yml").includes('root.glob("crates/*/LICENSE")')) {
  problems.push("release workflow does not iterate package-local license files");
}

for (const tokens of legacyPhraseTokens) {
  const phrase = tokens.join(" ");
  const result = spawnSync(
    "rg",
    [
      "--files-with-matches",
      "--hidden",
      "--ignore-case",
      "--fixed-strings",
      "--glob",
      "!.git/**",
      "--glob",
      "!node_modules/**",
      "--glob",
      "!target/**",
      "--glob",
      "!**/playwright-report/**",
      "--glob",
      "!**/test-results/**",
      "--glob",
      "!scripts/verify-licensing-terms.mjs",
      phrase,
      ".",
    ],
    { cwd: repoRoot, encoding: "utf8" }
  );
  if (result.error || (result.status !== 0 && result.status !== 1)) {
    problems.push(`Could not search repository for obsolete licensing claim: ${phrase}`);
    continue;
  }
  if (result.status === 0) {
    for (const match of result.stdout.split("\n").filter(Boolean)) {
      problems.push(`${match} contains obsolete licensing claim: ${phrase}`);
    }
  }
}

if (problems.length > 0) {
  console.error("Licensing terms verification failed:\n");
  for (const problem of problems.sort()) console.error(`  - ${problem}`);
  process.exit(1);
}

console.log(
  `Licensing terms OK: ${protectedBuslPackages.length} protected BUSL packages share one grant and Change Date ${[...changeDates][0]}.`
);
