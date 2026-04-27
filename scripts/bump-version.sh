#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
  echo "Usage: scripts/bump-version.sh <version>" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

VERSION="$VERSION" ROOT_DIR="$ROOT_DIR" node <<'NODE'
const fs = require("fs");
const path = require("path");

const version = process.env.VERSION;
const rootDir = process.env.ROOT_DIR;

if (!version || !rootDir) {
  process.stderr.write("Missing VERSION or ROOT_DIR\n");
  process.exit(1);
}

const jsonFiles = [
  "package.json",
  "src-tauri/tauri.conf.json",
  "packages/tandem-ai/package.json",
  "packages/tandem-client-ts/package.json",
  "packages/tandem-control-panel/package.json",
  "packages/create-tandem-panel/package.json",
  "packages/tandem-engine/package.json",
  "packages/tandem-tui/package.json",
];

const cargoFiles = [
  "src-tauri/Cargo.toml",
  "engine/Cargo.toml",
  "Cargo.lock",
  "crates/tandem-agent-teams/Cargo.toml",
  "crates/tandem-browser/Cargo.toml",
  "crates/tandem-channels/Cargo.toml",
  "crates/tandem-core/Cargo.toml",
  "crates/tandem-document/Cargo.toml",
  "crates/tandem-enterprise-contract/Cargo.toml",
  "crates/tandem-governance-engine/Cargo.toml",
  "crates/tandem-memory/Cargo.toml",
  "crates/tandem-observability/Cargo.toml",
  "crates/tandem-orchestrator/Cargo.toml",
  "crates/tandem-plan-compiler/Cargo.toml",
  "crates/tandem-providers/Cargo.toml",
  "crates/tandem-runtime/Cargo.toml",
  "crates/tandem-server/Cargo.toml",
  "crates/tandem-skills/Cargo.toml",
  "crates/tandem-tools/Cargo.toml",
  "crates/tandem-tui/Cargo.toml",
  "crates/tandem-types/Cargo.toml",
  "crates/tandem-wire/Cargo.toml",
  "crates/tandem-workflows/Cargo.toml",
];

const pyprojectFiles = [
  "packages/tandem-client-py/pyproject.toml",
];

const updatedFiles = [];

const updateJson = (relativePath) => {
  // Edit JSON files via targeted regex rather than JSON.parse + JSON.stringify
  // so we preserve prettier's existing array/object formatting (single-line
  // arrays, indentation, trailing newline). Round-tripping through
  // JSON.stringify would reformat e.g. tauri.conf.json arrays from compact
  // to multi-line on every run, generating spurious diffs.
  const filePath = path.join(rootDir, relativePath);
  const original = fs.readFileSync(filePath, "utf8");
  let content = original;

  const internalDeps = [
    ["@frumu/tandem", `^${version}`],
    ["@frumu/tandem-client", `^${version}`],
    ["@frumu/tandem-tui", `^${version}`],
    ["@frumu/tandem-panel", `^${version}`],
  ];

  // Top-level "version" only — match a "version": "..." pair preceded by
  // either the opening `{` or another field, before any nested objects'
  // version fields. JSON files in this list put `version` near the top.
  content = content.replace(
    /^(\s*)"version"(\s*):(\s*)"[^"]*"/m,
    `$1"version"$2:$3"${version}"`
  );

  for (const [name, nextVersion] of internalDeps) {
    const escapedName = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const re = new RegExp(`("${escapedName}"\\s*:\\s*)"[^"]*"`, "g");
    content = content.replace(re, `$1"${nextVersion}"`);
  }

  if (content !== original) {
    fs.writeFileSync(filePath, content);
  }
  updatedFiles.push(relativePath);
};

const updateCargo = (relativePath) => {
  const filePath = path.join(rootDir, relativePath);
  const content = fs.readFileSync(filePath, "utf8");
  // Preserve the existing trailing-newline state so re-runs of the script
  // are idempotent. cargo writes Cargo.lock without a trailing newline,
  // while Cargo.toml files end with one — we mirror whichever the file has.
  const trailingNewline = content.endsWith("\n") ? "\n" : "";
  const lines = content.split(/\r?\n/);
  if (trailingNewline === "\n" && lines.length > 0 && lines[lines.length - 1] === "") {
    lines.pop();
  }
  const isLockfile = path.basename(relativePath) === "Cargo.lock";
  let inPackage = false;
  let currentPackageName = "";
  const next = lines.map((line) => {
    if (isLockfile) {
      if (/^\[\[package\]\]\s*$/.test(line)) {
        inPackage = true;
        currentPackageName = "";
      } else if (/^\s*\[/.test(line)) {
        inPackage = false;
        currentPackageName = "";
      }
      if (inPackage) {
        const nameMatch = line.match(/^name\s*=\s*"([^"]+)"\s*$/);
        if (nameMatch) {
          currentPackageName = nameMatch[1];
        }
        const match = line.match(/^version\s*=\s*"[^"]*"\s*$/);
        if (
          match &&
          currentPackageName &&
          (currentPackageName === "tandem" || currentPackageName.startsWith("tandem-"))
        ) {
          return `version = "${version}"`;
        }
      }
    } else {
      if (/^\s*\[/.test(line)) {
        inPackage = /^\s*\[package\]\s*$/.test(line);
      }
      if (inPackage) {
        const match = line.match(/^(\s*)version\s*=\s*"[^"]*"\s*$/);
        if (match) {
          return `${match[1]}version = "${version}"`;
        }
      }
    }
    const depMatch = line.match(
      /^(\s*tandem-[^=]*=\s*\{[^}]*\bversion\s*=\s*")([^"]*)(".*)$/
    );
    if (depMatch) {
      return `${depMatch[1]}${version}${depMatch[3]}`;
    }
    return line;
  });
  fs.writeFileSync(filePath, `${next.join("\n")}${trailingNewline}`);
  updatedFiles.push(relativePath);
};

const updatePyproject = (relativePath) => {
  const filePath = path.join(rootDir, relativePath);
  const content = fs.readFileSync(filePath, "utf8");
  const trailingNewline = content.endsWith("\n") ? "\n" : "";
  const lines = content.split(/\r?\n/);
  if (trailingNewline === "\n" && lines.length > 0 && lines[lines.length - 1] === "") {
    lines.pop();
  }
  let inProject = false;
  const next = lines.map((line) => {
    if (/^\s*\[/.test(line)) {
      inProject = /^\s*\[project\]\s*$/.test(line);
    }
    if (inProject) {
      const match = line.match(/^(\s*)version\s*=\s*"[^"]*"\s*$/);
      if (match) {
        return `${match[1]}version = "${version}"`;
      }
    }
    return line;
  });
  fs.writeFileSync(filePath, `${next.join("\n")}${trailingNewline}`);
  updatedFiles.push(relativePath);
};

jsonFiles.forEach(updateJson);
cargoFiles.forEach(updateCargo);
pyprojectFiles.forEach(updatePyproject);

process.stdout.write(`Updated ${updatedFiles.length} files to ${version}\n`);
NODE
