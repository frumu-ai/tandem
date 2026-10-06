import assert from "node:assert/strict";
import test from "node:test";
import { assertAllowedChanges, assertPatchedLock, repairWorkspaces } from "./regenerate-javascript-lockfiles.mjs";

test("repair rejects omitted lockfiles, untracked files and edits outside its closed inventory", () => {
  const locks = repairWorkspaces.map(({ lockfile }) => lockfile);
  assert.doesNotThrow(() => assertAllowedChanges(locks));
  assert.throws(() => assertAllowedChanges(locks.slice(1)), /all six/);
  assert.throws(() => assertAllowedChanges([...locks, "package.json"]), /outside/);
  assert.throws(() => assertAllowedChanges(locks, ["unexpected-lock.yaml"]), /outside/);
});

test("repair checks every nested npm occurrence rather than just the hoisted version", () => {
  const workspace = repairWorkspaces.find(({ manager }) => manager === "npm");
  const lock = {
    lockfileVersion: 3,
    packages: {
      "node_modules/source-map-js": { version: "1.2.2" },
      "node_modules/css-tree/node_modules/source-map-js": { version: "1.2.1" },
    },
  };
  assert.throws(() => assertPatchedLock(workspace, JSON.stringify(lock)), /not resolved/);
  lock.packages["node_modules/css-tree/node_modules/source-map-js"].version = "1.2.2";
  assert.deepEqual(assertPatchedLock(workspace, JSON.stringify(lock)), { "source-map-js": ["1.2.2"] });
});

test("guide repair rejects missing, vulnerable and prerelease contributing packages", () => {
  const workspace = repairWorkspaces.find(({ lockfile }) => lockfile === "guide/pnpm-lock.yaml");
  const lock = "packages:\n  source-map-js@1.2.2:\n  smol-toml@1.9.0:\n  katex@0.18.2:\n  postcss-selector-parser@7.1.6:\n";
  assert.doesNotThrow(() => assertPatchedLock(workspace, lock));
  assert.throws(() => assertPatchedLock(workspace, lock.replace("smol-toml@1.9.0", "smol-toml@1.7.1")), /not resolved/);
  assert.throws(() => assertPatchedLock(workspace, lock.replace("katex@0.18.2:\n", "")), /not resolved/);
  assert.throws(() => assertPatchedLock(workspace, lock.replace("katex@0.18.2", "katex@0.18.2-alpha")), /not resolved/);
});

test("pnpm override declarations cannot masquerade as resolved package versions", () => {
  const workspace = repairWorkspaces.find(({ lockfile }) => lockfile === "guide/pnpm-lock.yaml");
  const lock = [
    "lockfileVersion: '9.0'",
    "overrides:",
    "  smol-toml@<=1.8.0: 1.9.0",
    "  postcss-selector-parser@<7.1.6: 7.1.6",
    "packages:",
    "  source-map-js@1.2.2:",
    "  'smol-toml@1.9.0':",
    "  katex@0.18.2:",
    "  postcss-selector-parser@7.1.6:",
    "snapshots:",
    '  "smol-toml@1.9.0": {}',
    "",
  ].join("\n");
  assert.deepEqual(assertPatchedLock(workspace, lock), {
    "source-map-js": ["1.2.2"],
    "smol-toml": ["1.9.0"],
    katex: ["0.18.2"],
    "postcss-selector-parser": ["7.1.6"],
  });
  assert.throws(() => assertPatchedLock(workspace,
    lock.replace('"smol-toml@1.9.0": {}', '"smol-toml@1.8.0": {}')), /not resolved/);
  assert.throws(() => assertPatchedLock(workspace,
    lock.replace("  'smol-toml@1.9.0':\n", "").replace('  "smol-toml@1.9.0": {}\n', "")), /not resolved/);
  assert.throws(() => assertPatchedLock(workspace,
    lock + "  smol-toml@<=1.8.0: {}\n"), /not resolved/);
});
