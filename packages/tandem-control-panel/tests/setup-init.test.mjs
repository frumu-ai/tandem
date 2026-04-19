import assert from "node:assert/strict";
import { mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { ensureBootstrapEnv } from "../lib/setup/env.js";
import { resolveSetupPaths } from "../lib/setup/paths.js";

test("resolveSetupPaths uses canonical linux roots", () => {
  const paths = resolveSetupPaths({
    platform: "linux",
    home: "/home/tester",
    env: {},
  });
  assert.equal(paths.configDir, "/home/tester/.config/tandem");
  assert.equal(paths.dataDir, "/home/tester/.local/share/tandem");
  assert.equal(paths.envFile, "/home/tester/.config/tandem/control-panel.env");
});

test("resolveSetupPaths uses AppData roots on windows", () => {
  const paths = resolveSetupPaths({
    platform: "win32",
    home: "/Users/tester",
    env: {},
  });
  assert.equal(paths.configDir, "/Users/tester/AppData/Roaming/tandem");
  assert.equal(paths.dataDir, "/Users/tester/AppData/Local/tandem");
  assert.equal(paths.envFile, "/Users/tester/AppData/Roaming/tandem/control-panel.env");
});

test("resolveSetupPaths uses Application Support on macOS", () => {
  const paths = resolveSetupPaths({
    platform: "darwin",
    home: "/Users/tester",
    env: {},
  });
  assert.equal(paths.configDir, "/Users/tester/Library/Application Support/tandem");
  assert.equal(paths.dataDir, "/Users/tester/Library/Application Support/tandem");
});

test("ensureBootstrapEnv writes canonical env file with host and state dirs", async () => {
  const root = await mkdtemp(join(tmpdir(), "tcp-init-"));
  const envPath = join(root, "config", "control-panel.env");
  const result = await ensureBootstrapEnv({
    cwd: root,
    envPath,
    env: {
      HOME: root,
      XDG_CONFIG_HOME: join(root, "config-base"),
      XDG_DATA_HOME: join(root, "data-base"),
    },
  });
  const content = await readFile(envPath, "utf8");
  assert.equal(result.panelHost, "127.0.0.1");
  assert.match(content, /^TANDEM_CONTROL_PANEL_HOST=127\.0\.0\.1/m);
  assert.match(content, /^TANDEM_STATE_DIR=/m);
  assert.match(content, /^TANDEM_CONTROL_PANEL_STATE_DIR=/m);
  assert.match(content, /^TANDEM_CONTROL_PANEL_ENGINE_TOKEN=tk_/m);
});
