import assert from "node:assert/strict";
import { lstatSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { mkdtemp, mkdir, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { ensureTokenFile, readTokenFile } from "../bin/docker-token.js";

test("docker token provisioning creates one stable private token", async (t) => {
  const cwd = await mkdtemp(path.join(os.tmpdir(), "tandem-docker-token-"));
  t.after(() => rm(cwd, { recursive: true, force: true }));

  const first = ensureTokenFile({ cwd });
  const second = ensureTokenFile({ cwd });
  assert.equal(first.created, true);
  assert.equal(second.created, false);
  assert.equal(second.token, first.token);
  assert.match(first.token, /^tk_[0-9a-f]{32}$/);
  assert.equal(readFileSync(first.tokenPath, "utf8"), `${first.token}\n`);
  if (process.platform !== "win32") {
    assert.equal(lstatSync(path.dirname(first.tokenPath)).mode & 0o777, 0o700);
    assert.equal(lstatSync(first.tokenPath).mode & 0o777, 0o600);
  }
});

test("docker token provisioning rejects empty files and symbolic links", async (t) => {
  const cwd = await mkdtemp(path.join(os.tmpdir(), "tandem-docker-token-invalid-"));
  t.after(() => rm(cwd, { recursive: true, force: true }));
  const secretDirectory = path.join(cwd, "secrets");
  await mkdir(secretDirectory, { mode: 0o700 });
  const tokenPath = path.join(secretDirectory, "tandem_api_token");
  writeFileSync(tokenPath, "", { mode: 0o600 });
  assert.throws(() => readTokenFile(tokenPath), /invalid format/);

  await rm(tokenPath);
  const targetPath = path.join(cwd, "elsewhere");
  const syntheticToken = `tk_${"0123456789abcdef".repeat(2)}\n`;
  writeFileSync(targetPath, syntheticToken, { mode: 0o600 });
  symlinkSync(targetPath, tokenPath);
  assert.throws(() => readTokenFile(tokenPath), /regular file/);
});

test("docker token provisioning rejects a symbolic-link secrets directory", async (t) => {
  const cwd = await mkdtemp(path.join(os.tmpdir(), "tandem-docker-token-dir-link-"));
  t.after(() => rm(cwd, { recursive: true, force: true }));
  const targetDirectory = path.join(cwd, "elsewhere");
  await mkdir(targetDirectory, { mode: 0o700 });
  symlinkSync(targetDirectory, path.join(cwd, "secrets"), "dir");
  assert.throws(() => ensureTokenFile({ cwd }), /real directory/);
});
