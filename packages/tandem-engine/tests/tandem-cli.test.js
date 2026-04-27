const test = require("node:test");
const assert = require("node:assert/strict");

const {
  parseVersion,
  shouldDownloadBinary,
} = require("../scripts/install.js");

const {
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
