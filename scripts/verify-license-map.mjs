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

// JS packages (published only).
const packagesDir = path.join(repoRoot, "packages");
for (const entry of fs.readdirSync(packagesDir, { withFileTypes: true })) {
  if (!entry.isDirectory()) continue;
  const pkgJson = `packages/${entry.name}/package.json`;
  if (!exists(pkgJson)) continue;
  const json = JSON.parse(read(pkgJson));
  if (json.private === true) continue; // not distributed
  discovered.push({
    path: pkgJson,
    name: json.name ?? "<unknown>",
    license: json.license ?? "<none>",
  });

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
