import assert from "node:assert/strict";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { ensureBootstrapEnv } from "../lib/setup/env.js";
import { runDoctor } from "../lib/setup/doctor.js";

test("doctor reports configured panel host and env file", async () => {
  const root = await mkdtemp(join(tmpdir(), "tcp-doctor-"));
  const envFile = join(root, "control-panel.env");
  await ensureBootstrapEnv({
    cwd: root,
    envPath: envFile,
    env: { HOME: root, XDG_CONFIG_HOME: root, XDG_DATA_HOME: root },
  });
  const result = await runDoctor({
    envFile,
    env: { HOME: root, XDG_CONFIG_HOME: root, XDG_DATA_HOME: root },
    cwd: root,
  });
  assert.equal(result.envFile, envFile);
  assert.equal(result.panelHost, "127.0.0.1");
});

test("doctor treats 127.0.1.0 as loopback and skips public-url warning", async () => {
  const root = await mkdtemp(join(tmpdir(), "tcp-doctor-"));
  const envFile = join(root, "control-panel.env");
  await writeFile(envFile, "TANDEM_CONTROL_PANEL_HOST=127.0.1.0\n", "utf8");
  const result = await runDoctor({
    envFile,
    env: {
      HOME: root,
      XDG_CONFIG_HOME: root,
      XDG_DATA_HOME: root,
    },
    cwd: root,
  });
  assert.equal(result.panelHost, "127.0.1.0");
  assert.deepEqual(result.warnings, []);
});
