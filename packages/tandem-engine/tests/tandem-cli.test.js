const test = require("node:test");
const assert = require("node:assert/strict");

const {
  parseVersion,
  resolveArtifactInfo,
  shouldDownloadBinary,
} = require("../scripts/install.js");

const enterpriseInstaller = require("../../tandem-enterprise/scripts/install.js");

const {
  buildWorktreeCleanupPayload,
  buildEngineServiceDefinition,
  detectPackageManager,
  findCommandOnPath,
  parseArgs,
  resolveTandemHomeDir,
  resolveTandemPaths,
} = require("../bin/tandem.js");

test("parseArgs handles flags and values", () => {
  const cli = parseArgs(["doctor", "--json", "--env-file", "/tmp/tandem.env", "--name=value"]);
  assert.equal(cli.has("json"), true);
  assert.equal(cli.value("env-file"), "/tmp/tandem.env");
  assert.equal(cli.value("name"), "value");
});

test("detectPackageManager defaults to npm", () => {
  const pm = detectPackageManager({ npm_config_user_agent: "" });
  assert.equal(pm.name, "npm");
  assert.deepEqual(pm.installArgs, ["install", "-g"]);
});

test("resolveTandemHomeDir respects state overrides", () => {
  assert.equal(
    resolveTandemHomeDir({ TANDEM_STATE_DIR: "/tmp/tandem-state" }, "linux"),
    "/tmp/tandem-state"
  );
});

test("resolveTandemPaths fills expected defaults", () => {
  const paths = resolveTandemPaths({ TANDEM_ENGINE_PORT: "39731" }, "linux");
  assert.equal(paths.enginePort, 39731);
  assert.equal(paths.panelPort, 39732);
  assert.match(paths.logsDir, /tandem[\\/]+logs$/);
});

test("buildEngineServiceDefinition emits platform-specific artifacts", () => {
  const linux = buildEngineServiceDefinition(
    resolveTandemPaths({ TANDEM_STATE_DIR: "/tmp/tandem" }, "linux"),
    { USER: "tandem" }
  );
  assert.equal(linux.manager, "systemd");
  assert.equal(linux.unitName, "tandem-engine.service");
  assert.match(linux.content, /--state-dir/);
});

test("findCommandOnPath ignores missing commands", () => {
  assert.equal(findCommandOnPath("definitely-not-a-real-command"), "");
});

test("buildWorktreeCleanupPayload defaults to dry-run cleanup", () => {
  const payload = buildWorktreeCleanupPayload(parseArgs(["worktrees", "--repo-root", "/tmp/repo"]));
  assert.deepEqual(payload, {
    repo_root: "/tmp/repo",
    dry_run: true,
    remove_orphan_dirs: true,
  });
});

test("buildWorktreeCleanupPayload honors apply and orphan retention flags", () => {
  const payload = buildWorktreeCleanupPayload(
    parseArgs(["worktrees", "--repo-root=/tmp/repo", "--apply", "--keep-orphan-dirs"])
  );
  assert.deepEqual(payload, {
    repo_root: "/tmp/repo",
    dry_run: false,
    remove_orphan_dirs: false,
  });
});

test("installer parses tandem-engine version output", () => {
  assert.equal(parseVersion("tandem-engine 0.4.44\n"), "0.4.44");
  assert.equal(parseVersion("0.4.44"), "0.4.44");
  assert.equal(parseVersion("tandem-engine 0.4.44-beta.1"), "0.4.44-beta.1");
});

test("installer replaces existing binary when version mismatches package", () => {
  const temp = require("node:fs").mkdtempSync(require("node:path").join(require("node:os").tmpdir(), "tandem-install-"));
  const binary = require("node:path").join(temp, "tandem-engine");
  require("node:fs").writeFileSync(binary, Buffer.alloc(1024 * 1024 + 1));

  assert.deepEqual(shouldDownloadBinary(binary, "0.4.44", () => "0.4.39"), {
    download: true,
    reason: "version mismatch (0.4.39 != 0.4.44)",
  });
  assert.deepEqual(shouldDownloadBinary(binary, "0.4.44", () => "0.4.44"), {
    download: false,
    reason: "version 0.4.44 already installed",
  });

  require("node:fs").rmSync(temp, { recursive: true, force: true });
});


test("installer resolves standard engine platform artifacts", () => {
  assert.deepEqual(
    resolveArtifactInfo({ binaryBaseName: "tandem-engine" }, { platform: "linux", arch: "x64" }),
    {
      artifactName: "tandem-engine-linux-x64.tar.gz",
      binaryName: "tandem-engine",
      isWindows: false,
    }
  );
  assert.deepEqual(
    resolveArtifactInfo({ binaryBaseName: "tandem-engine" }, { platform: "win32", arch: "x64" }),
    {
      artifactName: "tandem-engine-windows-x64.zip",
      binaryName: "tandem-engine.exe",
      isWindows: true,
    }
  );
});

test("enterprise installer resolves Linux x64 enterprise artifact", () => {
  assert.deepEqual(
    enterpriseInstaller.resolveArtifactInfo(enterpriseInstaller.config, { platform: "linux", arch: "x64" }),
    {
      artifactName: "tandem-engine-enterprise-linux-x64.tar.gz",
      binaryName: "tandem-engine",
      isWindows: false,
    }
  );
});

test("enterprise installer rejects unsupported platforms", () => {
  assert.throws(
    () => enterpriseInstaller.resolveArtifactInfo(enterpriseInstaller.config, { platform: "darwin", arch: "x64" }),
    /Unsupported platform for @frumu\/tandem-enterprise: darwin-x64/
  );
  assert.throws(
    () => enterpriseInstaller.resolveArtifactInfo(enterpriseInstaller.config, { platform: "linux", arch: "arm64" }),
    /Unsupported platform for @frumu\/tandem-enterprise: linux-arm64/
  );
});

test("enterprise installer only accepts the exact package release asset", () => {
  const artifactName = "tandem-engine-enterprise-linux-x64.tar.gz";
  const wrongRelease = {
    tag_name: "v0.6.7",
    assets: [{ name: artifactName, browser_download_url: "https://example.invalid/old" }],
  };
  const exactRelease = {
    tag_name: "v0.6.8",
    assets: [{ name: artifactName, browser_download_url: "https://example.invalid/exact" }],
  };

  assert.deepEqual(
    enterpriseInstaller.requireReleaseAsset([wrongRelease, exactRelease], "0.6.8", artifactName),
    {
      release: exactRelease,
      asset: exactRelease.assets[0],
    }
  );
  assert.throws(
    () => enterpriseInstaller.requireReleaseAsset([wrongRelease], "0.6.8", artifactName),
    /Release v0\.6\.8 was not found; refusing to install an asset from another release/
  );
  assert.throws(
    () => enterpriseInstaller.requireReleaseAsset([{ tag_name: "v0.6.8", assets: [] }], "0.6.8", artifactName),
    /Release v0\.6\.8 does not contain tandem-engine-enterprise-linux-x64\.tar\.gz/
  );
});

test("enterprise installer verifies the extracted binary version", () => {
  const binaryPath = "/tmp/tandem-engine";
  let checkedPath = "";
  const readVersion = (candidate) => {
    checkedPath = candidate;
    return "0.6.8";
  };

  assert.equal(enterpriseInstaller.verifyInstalledBinary(binaryPath, "0.6.8", readVersion), "0.6.8");
  assert.equal(checkedPath, binaryPath);
  assert.throws(
    () => enterpriseInstaller.verifyInstalledBinary(binaryPath, "0.6.8", () => "0.6.7"),
    /Installed binary version 0\.6\.7 does not match package version 0\.6\.8/
  );
  assert.throws(
    () => enterpriseInstaller.verifyInstalledBinary(binaryPath, "0.6.8", () => ""),
    /failed --version verification/
  );
});
