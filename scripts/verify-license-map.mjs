#!/usr/bin/env node

// Keeps docs/LICENSING.md honest: every workspace package's declared license
// must appear in the canonical map with a matching value, and every mapped path
// must still exist. Runs in the "Validate Docs" CI job (TAN-629).
//
// Discovery mirrors how the toolchains resolve packages:
//   - Rust: the [workspace] members list in the root Cargo.toml.
//   - JS:   packages/*/package.json, excluding private packages (not published).
//   - Py:   packages/*/pyproject.toml.
// The scaffold template under packages/*/template/ is intentionally not a
// package (its license is filled in at generation time), so it is not globbed.

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const mapPath = "docs/LICENSING.md";

function read(rel) {
  return fs.readFileSync(path.join(repoRoot, rel), "utf8");
}

function exists(rel) {
  return fs.existsSync(path.join(repoRoot, rel));
}

// --- Discover declared licenses from manifests -----------------------------

/** @type {{path: string, name: string, license: string}[]} */
const discovered = [];

// Rust workspace members.
const rootCargo = read("Cargo.toml");
const membersBlock = rootCargo.match(/members\s*=\s*\[([\s\S]*?)\]/);
if (!membersBlock) {
  console.error("Could not parse [workspace] members from Cargo.toml");
  process.exit(2);
}
const members = [...membersBlock[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
for (const member of members) {
  const manifest = `${member}/Cargo.toml`;
  const text = read(manifest);
  const name = text.match(/^\s*name\s*=\s*"([^"]+)"/m)?.[1] ?? "<unknown>";
  const license = text.match(/^\s*license\s*=\s*"([^"]+)"/m)?.[1] ?? "<none>";
  discovered.push({ path: manifest, name, license });
}

// JS packages (published only) and Python packages. A package directory may
// have either, both, or neither manifest, so each is checked independently
// (packages/tandem-client-py, for example, has a pyproject.toml but no
// package.json).
const packagesDir = path.join(repoRoot, "packages");
for (const entry of fs.readdirSync(packagesDir, { withFileTypes: true })) {
  if (!entry.isDirectory()) continue;

  const pkgJson = `packages/${entry.name}/package.json`;
  if (exists(pkgJson)) {
    const json = JSON.parse(read(pkgJson));
    if (json.private !== true) {
      // private packages are not distributed
      discovered.push({
        path: pkgJson,
        name: json.name ?? "<unknown>",
        license: json.license ?? "<none>",
      });
    }
  }

  const pyProject = `packages/${entry.name}/pyproject.toml`;
  if (exists(pyProject)) {
    const text = read(pyProject);
    const name = text.match(/^\s*name\s*=\s*"([^"]+)"/m)?.[1] ?? "<unknown>";
    const license =
      text.match(/license\s*=\s*\{\s*text\s*=\s*"([^"]+)"/)?.[1] ??
      text.match(/^\s*license\s*=\s*"([^"]+)"/m)?.[1] ??
      "<none>";
    discovered.push({ path: pyProject, name, license });
  }
}

// --- Parse the canonical map -----------------------------------------------

/** @type {Map<string, string>} path -> license from the map tables */
const mapped = new Map();
const manifestCell = /`([^`]+\/(?:Cargo\.toml|package\.json|pyproject\.toml))`/;
const licenseCell = /`([^`]+)`\s*\|\s*$/;
for (const line of read(mapPath).split("\n")) {
  if (!line.trimStart().startsWith("|")) continue;
  const pathMatch = line.match(manifestCell);
  if (!pathMatch) continue;
  const licMatch = line.match(licenseCell);
  mapped.set(pathMatch[1], licMatch ? licMatch[1] : "<unparsed>");
}

// --- Compare ---------------------------------------------------------------

const problems = [];

for (const pkg of discovered) {
  if (!mapped.has(pkg.path)) {
    problems.push(
      `MISSING from ${mapPath}: ${pkg.name} (${pkg.path}) declares ${pkg.license}`
    );
    continue;
  }
  const mappedLicense = mapped.get(pkg.path);
  if (mappedLicense !== pkg.license) {
    problems.push(
      `MISMATCH for ${pkg.name} (${pkg.path}): manifest says ${pkg.license}, map says ${mappedLicense}`
    );
  }
}

for (const mappedPath of mapped.keys()) {
  if (!exists(mappedPath)) {
    problems.push(`STALE entry in ${mapPath}: ${mappedPath} no longer exists`);
  }
}

// --- Release-fact consistency (TAN-683) -------------------------------------
// docs/LICENSING.md states release facts (BUSL Change Date, released version)
// that live authoritatively elsewhere; keep the prose from drifting again.

const licensingDoc = read(mapPath);

// Every BUSL crate LICENSE must agree on one Change Date. Historical release
// dates are also documented, so validate only the explicitly labeled current
// source-tree date rather than every date in the licensing guide.
const buslChangeDates = new Set();
for (const member of members) {
  const licenseFile = `${member}/LICENSE`;
  if (!exists(licenseFile)) continue;
  const changeDate = read(licenseFile).match(/^Change Date:\s*(\S+)/m)?.[1];
  if (changeDate) buslChangeDates.add(changeDate);
}
if (buslChangeDates.size > 1) {
  problems.push(
    `BUSL LICENSE files disagree on Change Date: ${[...buslChangeDates].sort().join(", ")}`
  );
} else if (buslChangeDates.size === 1) {
  const [actual] = buslChangeDates;
  const currentSourceTreeChangeDate = licensingDoc.match(
    /\*\*Current source-tree BUSL Change Date:\*\* `(\d{4}-\d{2}-\d{2})`/
  )?.[1];
  if (!currentSourceTreeChangeDate) {
    problems.push(
      `${mapPath} must state the current source-tree BUSL Change Date explicitly`
    );
  } else if (currentSourceTreeChangeDate !== actual) {
    problems.push(
      `${mapPath} states current source-tree BUSL Change Date ${currentSourceTreeChangeDate}, but the BUSL LICENSE files say ${actual}`
    );
  }
}

// A version RELEASE_NOTES.md already lists as released must not be described
// as "the next release".
const nextRelease = licensingDoc.match(/\*\*(\d+\.\d+\.\d+)\*\*\s*\(the next release\)/);
if (nextRelease && read("RELEASE_NOTES.md").includes(`## v${nextRelease[1]} (`)) {
  problems.push(
    `${mapPath} calls ${nextRelease[1]} "the next release", but RELEASE_NOTES.md already lists it as released`
  );
}

if (problems.length > 0) {
  console.error(`License map is out of sync with package manifests:\n`);
  for (const p of problems.sort()) console.error(`  - ${p}`);
  console.error(
    `\nFix docs/LICENSING.md (or the manifest) so every workspace package is listed with its declared license.`
  );
  process.exit(1);
}

console.log(
  `License map OK: ${discovered.length} packages match docs/LICENSING.md.`
);
