import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
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

test("ensureBootstrapEnv ignores a poisoned TANDEM_STATE_DIR in .env.example", async () => {
  // Regression: a committed `.env.example` once carried a developer's personal
  // benchmark path (`TANDEM_STATE_DIR=%HOME%\...\.bench-state`). Merging it
  // silently redirected the engine's state dir, so history/automations looked
  // wiped on every restart. The example must never pin the state dirs.
  const root = await mkdtemp(join(tmpdir(), "tcp-init-poison-"));
  const envPath = join(root, "config", "control-panel.env");
  await writeFile(
    join(root, ".env.example"),
    [
      "TANDEM_DEFAULT_PROVIDER=openrouter",
      "TANDEM_STATE_DIR=%HOME%\\work\\tandem-engine\\tandem\\scripts\\bench-js\\.bench-state",
      "TANDEM_CONTROL_PANEL_STATE_DIR=",
      "",
    ].join("\n"),
    "utf8",
  );
  const paths = resolveSetupPaths({
    platform: "linux",
    home: root,
    env: { HOME: root, XDG_CONFIG_HOME: join(root, "config-base"), XDG_DATA_HOME: join(root, "data-base") },
  });
  await ensureBootstrapEnv({
    cwd: root,
    envPath,
    env: {
      HOME: root,
      XDG_CONFIG_HOME: join(root, "config-base"),
      XDG_DATA_HOME: join(root, "data-base"),
    },
  });
  const content = await readFile(envPath, "utf8");
  assert.doesNotMatch(content, /bench-state/);
  assert.doesNotMatch(content, /%HOME%/);
  // A harmless example key still flows through; only the state dirs are pinned
  // to the computed platform defaults.
  assert.match(content, /^TANDEM_DEFAULT_PROVIDER=openrouter/m);
  assert.match(content, new RegExp(`^TANDEM_STATE_DIR=${paths.engineStateDir.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`, "m"));
});
