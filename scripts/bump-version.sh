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
  "packages/tandem-engine/package.json",
  "packages/tandem-tui/package.json",
];

const cargoFiles = [
  "src-tauri/Cargo.toml",
  "engine/Cargo.toml",
  "crates/tandem-channels/Cargo.toml",
  "crates/tandem-core/Cargo.toml",
  "crates/tandem-document/Cargo.toml",
  "crates/tandem-memory/Cargo.toml",
  "crates/tandem-observability/Cargo.toml",
  "crates/tandem-orchestrator/Cargo.toml",
  "crates/tandem-providers/Cargo.toml",
  "crates/tandem-runtime/Cargo.toml",
  "crates/tandem-server/Cargo.toml",
  "crates/tandem-skills/Cargo.toml",
  "crates/tandem-tools/Cargo.toml",
  "crates/tandem-tui/Cargo.toml",
  "crates/tandem-types/Cargo.toml",
  "crates/tandem-wire/Cargo.toml",
  "crates/tandem-agent-teams/Cargo.toml",
];

const updatedFiles = [];

const updateJson = (relativePath) => {
  const filePath = path.join(rootDir, relativePath);
  const content = fs.readFileSync(filePath, "utf8");
  const data = JSON.parse(content);
  data.version = version;
  fs.writeFileSync(filePath, `${JSON.stringify(data, null, 2)}\n`);
  updatedFiles.push(relativePath);
};

const updateCargo = (relativePath) => {
  const filePath = path.join(rootDir, relativePath);
  const content = fs.readFileSync(filePath, "utf8");
  const lines = content.split(/\r?\n/);
  let inPackage = false;
  const next = lines.map((line) => {
    if (/^\s*\[/.test(line)) {
      inPackage = /^\s*\[package\]\s*$/.test(line);
    }
    if (inPackage) {
      const match = line.match(/^(\s*)version\s*=\s*"[^"]*"\s*$/);
      if (match) {
        return `${match[1]}version = "${version}"`;
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
  fs.writeFileSync(filePath, `${next.join("\n")}\n`);
  updatedFiles.push(relativePath);
};

jsonFiles.forEach(updateJson);
cargoFiles.forEach(updateCargo);

process.stdout.write(`Updated ${updatedFiles.length} files to ${version}\n`);
NODE
