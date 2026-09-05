import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { verifyCandidate, writeCandidate } from "./ci-release-candidate.mjs";

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "tandem-candidate-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, "scripts"));
  const directory = join(root, "candidate");
  mkdirSync(directory);
  for (const path of ["Cargo.lock", "scripts/linux-release-builder.Dockerfile", "scripts/build-linux-release-engine.sh"]) {
    writeFileSync(join(root, path), `fixture ${path}\n`);
  }
  writeFileSync(join(directory, "tandem-engine"), "fixture binary\n");
  const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  git("init", "-q");
  git("add", ".");
  git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false",
    "commit", "-qsm", "fixture");
  const env = { GITHUB_SHA: git("rev-parse", "HEAD"), GITHUB_RUN_ID: "123", GITHUB_RUN_ATTEMPT: "1" };
  const pins = writeCandidate(root, directory, env);
  const verify = (overrides = {}) => verifyCandidate(root, directory, pins.manifest_sha256, pins.sha256, { ...env, ...overrides });
  return { root, directory, env, pins, verify };
}

test("accepts the exact candidate and producer attempt, including a consumer-only rerun", (t) => {
  const f = fixture(t);
  assert.deepEqual(f.verify(), f.pins);
  const output = execFileSync(process.execPath, [
    join(import.meta.dirname, "ci-release-candidate.mjs"), "verify", f.directory, f.pins.manifest_sha256, f.pins.sha256,
  ], { cwd: f.root, encoding: "utf8", env: { ...process.env, ...f.env, GITHUB_RUN_ATTEMPT: "2", CANDIDATE_BUILD_ATTEMPT: "1" } });
  assert.deepEqual(JSON.parse(output), f.pins);
});

test("rejects a different checkout, workflow run or producer attempt", (t) => {
  const f = fixture(t);
  for (const overrides of [
    { GITHUB_SHA: "a".repeat(40) }, { GITHUB_RUN_ID: "124" }, { GITHUB_RUN_ATTEMPT: "2" }, { GITHUB_RUN_ID: "" },
  ]) assert.throws(() => f.verify(overrides));
});

test("rejects altered or missing binary bytes and missing producer pins", (t) => {
  const f = fixture(t);
  writeFileSync(join(f.directory, "tandem-engine"), "different binary");
  assert.throws(f.verify, /binary differs/);
  rmSync(join(f.directory, "tandem-engine"));
  assert.throws(f.verify, /ENOENT/);
  assert.throws(() => verifyCandidate(f.root, f.directory, "", f.pins.sha256, f.env), /full SHA-256/);
});

test("rejects manifest tampering even if the manifest's own binary digest is changed", (t) => {
  const f = fixture(t);
  const path = join(f.directory, "provenance.json");
  const manifest = JSON.parse(readFileSync(path, "utf8"));
  manifest.binary_sha256 = "a".repeat(64);
  writeFileSync(path, JSON.stringify(manifest));
  assert.throws(f.verify, /Manifest differs/);
});

test("rejects a producer manifest declaring the wrong feature composition", (t) => {
  const f = fixture(t);
  const path = join(f.directory, "provenance.json");
  const manifest = JSON.parse(readFileSync(path, "utf8"));
  manifest.profile.mode = "standard";
  const source = JSON.stringify(manifest);
  writeFileSync(path, source);
  const manifestPin = createHash("sha256").update(source).digest("hex");
  assert.throws(() => verifyCandidate(f.root, f.directory, manifestPin, f.pins.sha256, f.env), /provenance/);
});

test("rejects changed lockfile or builder inputs on the consumer", (t) => {
  const f = fixture(t);
  for (const path of ["Cargo.lock", "scripts/linux-release-builder.Dockerfile", "scripts/build-linux-release-engine.sh"]) {
    const full = join(f.root, path);
    const before = readFileSync(full);
    writeFileSync(full, "changed build input");
    assert.throws(f.verify, /provenance/);
    writeFileSync(full, before);
  }
});

test("rejects symlinked artifact files before the executable can be used", (t) => {
  const f = fixture(t);
  const file = join(f.directory, "tandem-engine");
  rmSync(file);
  symlinkSync(join(f.root, "Cargo.lock"), file);
  assert.throws(f.verify, /ELOOP|regular file/);
});
