param(
  [string]$Version,
  [string]$EngineBinarySha256 = $env:TANDEM_ENGINE_BINARY_SHA256
)

if (-not $Version) {
  Write-Error "Usage: scripts/bump-version.ps1 <version> -EngineBinarySha256 <extracted-linux-x64-binary-sha256>"
  exit 1
}

$rootDir = Resolve-Path (Join-Path $PSScriptRoot "..")
$env:VERSION = $Version
$env:ROOT_DIR = $rootDir.Path
$env:ENGINE_BINARY_SHA256 = $EngineBinarySha256

$script = @'
const fs = require("fs");
const path = require("path");

const version = process.env.VERSION;
const rootDir = process.env.ROOT_DIR;
const engineBinarySha256 = String(process.env.ENGINE_BINARY_SHA256 || "").trim();

if (!version || !rootDir) {
  process.stderr.write("Missing VERSION or ROOT_DIR\n");
  process.exit(1);
}

const jsonFiles = [
  "package.json",
  "apps/tandem-desktop/package.json",
  "apps/tandem-desktop/src-tauri/tauri.conf.json",
  "packages/tandem-ai/package.json",
  "packages/tandem-client-ts/package.json",
  "packages/tandem-control-panel/package.json",
  "packages/create-tandem-panel/package.json",
  "packages/tandem-engine/package.json",
  "packages/tandem-enterprise/package.json",
  "packages/tandem-tui/package.json",
];

const cargoFiles = [
  "apps/tandem-desktop/src-tauri/Cargo.toml",
  "apps/tandem-desktop/src-tauri/Cargo.lock",
  "engine/Cargo.toml",
  "Cargo.lock",
  "crates/tandem-agent-teams/Cargo.toml",
  "crates/tandem-automation/Cargo.toml",
  "crates/tandem-incident-monitor/Cargo.toml",
  "crates/tandem-browser/Cargo.toml",
  "crates/tandem-channels/Cargo.toml",
  "crates/tandem-core/Cargo.toml",
  "crates/tandem-data-boundary/Cargo.toml",
  "crates/tandem-document/Cargo.toml",
  "crates/tandem-enterprise-contract/Cargo.toml",
  "crates/tandem-enterprise-server/Cargo.toml",
  "crates/tandem-eval/Cargo.toml",
  "crates/tandem-graph-core/Cargo.toml",
  "crates/tandem-governance-engine/Cargo.toml",
  "crates/tandem-memory/Cargo.toml",
  "crates/tandem-meta-harness-eval/Cargo.toml",
  "crates/tandem-observability/Cargo.toml",
  "crates/tandem-orchestrator/Cargo.toml",
  "crates/tandem-plan-compiler/Cargo.toml",
  "crates/tandem-providers/Cargo.toml",
  "crates/tandem-repo-intelligence/Cargo.toml",
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
  const filePath = path.join(rootDir, relativePath);
  const content = fs.readFileSync(filePath, "utf8");
  const data = JSON.parse(content);
  data.version = version;
  // Leave internal npm dependency ranges pinned to published-compatible
  // versions; CI runs before the candidate release exists on npm.
  fs.writeFileSync(filePath, `${JSON.stringify(data, null, 2)}\n`);
  updatedFiles.push(relativePath);
};

const updateScaffoldTemplate = () => {
  const dependencyNames = ["@frumu/tandem", "@frumu/tandem-client"];
  const manifestRelativePath = "packages/create-tandem-panel/template/package.json";
  const manifestPath = path.join(rootDir, manifestRelativePath);
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  for (const name of dependencyNames) {
    manifest.dependencies[name] = version;
  }
  fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  updatedFiles.push(manifestRelativePath);

  const lockRelativePath = "packages/create-tandem-panel/template/package-lock.json";
  const lockPath = path.join(rootDir, lockRelativePath);
  const lock = JSON.parse(fs.readFileSync(lockPath, "utf8"));
  const rootPackage = lock.packages[""];
  for (const name of dependencyNames) {
    rootPackage.dependencies[name] = version;
    const entry = lock.packages[`node_modules/${name}`];
    entry.version = version;
    entry.resolved =
      `https://registry.npmjs.org/${name}/-/${name.split("/").pop()}-${version}.tgz`;
    delete entry.integrity;
  }
  const clientManifest = JSON.parse(
    fs.readFileSync(path.join(rootDir, "packages/tandem-client-ts/package.json"), "utf8")
  );
  lock.packages["node_modules/@frumu/tandem-client"].dependencies =
    clientManifest.dependencies;
  fs.writeFileSync(lockPath, `${JSON.stringify(lock, null, 2)}\n`);
  updatedFiles.push(lockRelativePath);
};

const updateEngineDockerPin = () => {
  const relativePath = "packages/tandem-control-panel/docker/engine.Dockerfile";
  const filePath = path.join(rootDir, relativePath);
  const original = fs.readFileSync(filePath, "utf8");
  const currentVersion = original.match(/^\s*TANDEM_ENGINE_VERSION=([^ ]+) \\$/m)?.[1] || "";
  if (currentVersion !== version && !/^[0-9a-f]{64}$/.test(engineBinarySha256)) {
    throw new Error(
      "A new release must provide ENGINE_BINARY_SHA256 so the Docker engine pin cannot silently retain an old artifact"
    );
  }
  let content = original.replace(
    /^(\s*TANDEM_ENGINE_VERSION=)[^ ]+( \\)$/m,
    `$1${version}$2`
  );
  if (engineBinarySha256) {
    if (!/^[0-9a-f]{64}$/.test(engineBinarySha256)) {
      throw new Error("ENGINE_BINARY_SHA256 must be 64 lowercase hexadecimal characters");
    }
    content = content.replace(
      /^(\s*TANDEM_ENGINE_BINARY_SHA256=)[0-9a-f]{64}( \\)$/m,
      `$1${engineBinarySha256}$2`
    );
  }
  if (content !== original) fs.writeFileSync(filePath, content);
  updatedFiles.push(relativePath);
};

const updateCargo = (relativePath) => {
  const filePath = path.join(rootDir, relativePath);
  const content = fs.readFileSync(filePath, "utf8");
  const lines = content.split(/\r?\n/);
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
        if (
          line.match(/^version\s*=\s*"[^"]*"\s*$/) &&
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
  fs.writeFileSync(filePath, `${next.join("\n")}\n`);
  updatedFiles.push(relativePath);
};

const updatePyproject = (relativePath) => {
  const filePath = path.join(rootDir, relativePath);
  const content = fs.readFileSync(filePath, "utf8");
  const lines = content.split(/\r?\n/);
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
  fs.writeFileSync(filePath, `${next.join("\n")}\n`);
  updatedFiles.push(relativePath);
};

const stampBuslChangeDates = () => {
  // Rolling BUSL Change Date: each released version converts to the Change
  // License four years after its release date (docs/LICENSING.md, "Change
  // Date policy"). Discover the LICENSE files dynamically so newly
  // relicensed crates are covered without touching this script. Keep the
  // current-source-tree date in the licensing guide in sync as well.
  const changeDate = new Date();
  changeDate.setUTCFullYear(changeDate.getUTCFullYear() + 4);
  const stamp = changeDate.toISOString().slice(0, 10);
  const cratesDir = path.join(rootDir, "crates");
  for (const entry of fs.readdirSync(cratesDir)) {
    const relativePath = `crates/${entry}/LICENSE`;
    const filePath = path.join(rootDir, relativePath);
    if (!fs.existsSync(filePath)) continue;
    const content = fs.readFileSync(filePath, "utf8");
    if (!content.includes("Business Source License 1.1")) continue;
    const next = content.replace(
      /^Change Date: \d{4}-\d{2}-\d{2}[ \t]*$/m,
      `Change Date: ${stamp}`
    );
    if (next !== content) {
      fs.writeFileSync(filePath, next);
      updatedFiles.push(relativePath);
    }
  }

  const licensingGuidePath = path.join(rootDir, "docs/LICENSING.md");
  const licensingGuide = fs.readFileSync(licensingGuidePath, "utf8");
  const currentSourceTreeDatePattern =
    /^\*\*Current source-tree BUSL Change Date:\*\* `\d{4}-\d{2}-\d{2}`\.$/m;
  if (!currentSourceTreeDatePattern.test(licensingGuide)) {
    throw new Error("Could not find the current source-tree BUSL Change Date in docs/LICENSING.md");
  }
  const nextLicensingGuide = licensingGuide.replace(
    currentSourceTreeDatePattern,
    `**Current source-tree BUSL Change Date:** \`${stamp}\`.`
  );
  if (nextLicensingGuide !== licensingGuide) {
    fs.writeFileSync(licensingGuidePath, nextLicensingGuide);
    updatedFiles.push("docs/LICENSING.md");
  }
};

updateEngineDockerPin();
jsonFiles.forEach(updateJson);
updateScaffoldTemplate();
cargoFiles.forEach(updateCargo);
pyprojectFiles.forEach(updatePyproject);
stampBuslChangeDates();

process.stdout.write(`Updated ${updatedFiles.length} files to ${version}\n`);
'@

$script | node
