import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, symlinkSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { ensureBootstrapEnv } from "../lib/setup/env.js";

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "tandem-env-race-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return {
    root,
    options: {
      cwd: root, envPath: join(root, "control-panel.env"),
      env: { HOME: root, XDG_CONFIG_HOME: join(root, "config"), XDG_DATA_HOME: join(root, "data") },
    },
  };
}

test("bootstrap never follows a replacement env symlink during directory setup", { skip: process.platform === "win32" }, async (t) => {
  const { root, options } = fixture(t);
  writeFileSync(options.envPath, "EXISTING=original\n");
  const target = join(root, "sentinel");
  writeFileSync(target, "must remain unchanged\n");
  const pending = ensureBootstrapEnv(options);
  unlinkSync(options.envPath);
  symlinkSync(target, options.envPath);
  await pending.catch(() => {});
  assert.equal(readFileSync(target, "utf8"), "must remain unchanged\n");
});

test("bootstrap merges a replacement regular env after directory setup", async (t) => {
  const { options } = fixture(t);
  writeFileSync(options.envPath, "EXISTING=original\n");
  const pending = ensureBootstrapEnv(options);
  unlinkSync(options.envPath);
  writeFileSync(options.envPath, "REPLACEMENT=preserve-me\n");
  const result = await pending;
  assert.equal(result.env.REPLACEMENT, "preserve-me");
  assert.match(readFileSync(options.envPath, "utf8"), /^REPLACEMENT=preserve-me$/m);
});

test("concurrent first initialization converges on the persisted token", async (t) => {
  const { options } = fixture(t);
  const results = await Promise.all([ensureBootstrapEnv(options), ensureBootstrapEnv(options)]);
  assert.equal(results[0].token, results[1].token);
  assert.ok(readFileSync(options.envPath, "utf8").includes(results[0].token));
});

test("normal setup preserves keys and token; explicit overwrite rotates only token", async (t) => {
  const { options } = fixture(t);
  const first = await ensureBootstrapEnv(options);
  writeFileSync(options.envPath, readFileSync(options.envPath, "utf8") + "CUSTOM=keep\n");
  const second = await ensureBootstrapEnv(options);
  assert.equal(second.token, first.token);
  assert.equal(second.env.CUSTOM, "keep");
  const rotated = await ensureBootstrapEnv({ ...options, overwrite: true });
  assert.notEqual(rotated.token, first.token);
  assert.equal(rotated.env.CUSTOM, "keep");
  if (process.platform !== "win32") assert.equal(statSync(options.envPath).mode & 0o777, 0o600);
});

test("read-only diagnosis creates no paths and preserves existing contents and mode", async (t) => {
  const { root, options } = fixture(t);
  await ensureBootstrapEnv({ ...options, readOnly: true });
  assert.equal(existsSync(options.envPath), false);
  assert.equal(existsSync(join(root, "config")), false);
  assert.equal(existsSync(join(root, "data")), false);
  writeFileSync(options.envPath, "CUSTOM=unchanged\n", { mode: 0o640 });
  const before = statSync(options.envPath);
  await ensureBootstrapEnv({ ...options, readOnly: true, overwrite: true });
  assert.equal(readFileSync(options.envPath, "utf8"), "CUSTOM=unchanged\n");
  assert.equal(statSync(options.envPath).mode, before.mode);
});
