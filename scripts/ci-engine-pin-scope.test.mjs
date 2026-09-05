import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { changedBetweenCommits, engineReleasePinChanged } from "./ci-engine-pin-scope.mjs";

const original = readFileSync(new URL("../packages/tandem-control-panel/docker/engine.Dockerfile", import.meta.url), "utf8");

test("OS, base-image and comment changes keep the published engine pin unchanged", () => {
  const changed = original
    .replace(/^FROM .+$/m, "FROM node:24-example@sha256:" + "a".repeat(64))
    .replaceAll("20260904T000000Z", "20261001T000000Z")
    .replaceAll("3.5.7-1~deb13u2", "3.5.8-1~deb13u1") + "\n# Reviewed OS maintenance\n";
  assert.equal(engineReleasePinChanged(original, changed), false);
});

test("a version or binary digest change still requires release-pin verification", () => {
  assert.equal(engineReleasePinChanged(original, original.replace(/TANDEM_ENGINE_VERSION=\S+/, "TANDEM_ENGINE_VERSION=99.0.0")), true);
  assert.equal(engineReleasePinChanged(original, original.replace(/TANDEM_ENGINE_BINARY_SHA256=[0-9a-f]+/, "TANDEM_ENGINE_BINARY_SHA256=" + "a".repeat(64))), true);
  assert.equal(engineReleasePinChanged(original, original), false);
});

test("missing, invalid or duplicate release pins fail classification", () => {
  const invalid = [
    original.replace(/^\s*TANDEM_ENGINE_VERSION=.*$/m, ""),
    original.replace(/TANDEM_ENGINE_VERSION=\S+/, "TANDEM_ENGINE_VERSION=latest"),
    original.replace(/^\s*TANDEM_ENGINE_BINARY_SHA256=.*$/m, ""),
    original.replace(/TANDEM_ENGINE_BINARY_SHA256=[0-9a-f]+/, "TANDEM_ENGINE_BINARY_SHA256=invalid"),
    original + "\nTANDEM_ENGINE_VERSION=1.0.0 \\\n",
    original + "\nTANDEM_ENGINE_BINARY_SHA256=" + "a".repeat(64) + " \\\n",
  ];
  for (const source of invalid) {
    assert.throws(() => engineReleasePinChanged(original, source), /one valid release/);
    assert.throws(() => engineReleasePinChanged(source, original), /one valid release/);
  }
});

test("commit inputs cannot become git options or arbitrary revision expressions", () => {
  for (const value of [undefined, "", "main", "HEAD~1", "--output=/tmp/pin", "0".repeat(40)]) {
    assert.throws(() => changedBetweenCommits("a".repeat(40), value), /full commit SHAs/);
  }
});
