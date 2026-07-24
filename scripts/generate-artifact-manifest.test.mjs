import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  classifyRuntimeAsset,
  generateArtifactManifest,
  MANIFEST_FILENAME,
} from "./generate-artifact-manifest.mjs";

const options = (directory) => ({
  assetsDir: directory,
  outputDir: directory,
  release: "v0.7.1",
  version: "0.7.1",
  repository: "frumu-ai/tandem",
  sourceCommit: "a".repeat(40),
  workflow: ".github/workflows/release.yml",
  runId: "42",
  generatedAt: "2026-07-24T00:00:00Z",
});

test("classifies only exact supported runtime archive names", () => {
  assert.deepEqual(classifyRuntimeAsset("tandem-engine-linux-x64.tar.gz"), {
    kind: "engine",
    platform: "linux",
    architecture: "x64",
  });
  assert.deepEqual(classifyRuntimeAsset("tandem-engine-enterprise-linux-x64.tar.gz"), {
    kind: "engine_enterprise",
    platform: "linux",
    architecture: "x64",
  });
  assert.equal(classifyRuntimeAsset("tandem-engine-linux-x64.tar.gz.exe"), null);
  assert.equal(classifyRuntimeAsset("../tandem-engine-linux-x64.tar.gz"), null);
});

test("generates deterministic manifest and bound SPDX SBOM metadata", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "tandem-artifacts-"));
  const engine = "tandem-engine-linux-x64.tar.gz";
  const browser = "tandem-browser-darwin-arm64.zip";
  await writeFile(path.join(directory, engine), "verified-engine");
  await writeFile(path.join(directory, browser), "verified-browser");

  const first = await generateArtifactManifest(options(directory));
  const firstBytes = await readFile(path.join(directory, MANIFEST_FILENAME));
  const second = await generateArtifactManifest(options(directory));
  const secondBytes = await readFile(path.join(directory, MANIFEST_FILENAME));

  assert.deepEqual(first.manifest, second.manifest);
  assert.deepEqual(firstBytes, secondBytes);
  assert.deepEqual(
    first.manifest.artifacts.map((entry) => entry.filename),
    [browser, engine]
  );
  for (const entry of first.manifest.artifacts) {
    assert.equal(entry.provenance.source_commit, "a".repeat(40));
    assert.equal(entry.provenance.builder_id, "https://github.com/frumu-ai/tandem/actions/runs/42");
    const sbomBytes = await readFile(path.join(directory, entry.sbom.filename));
    assert.equal(entry.sbom.length, sbomBytes.length);
    assert.equal(entry.sbom.sha256, createHash("sha256").update(sbomBytes).digest("hex"));
    const sbom = JSON.parse(sbomBytes);
    assert.equal(sbom.spdxVersion, "SPDX-2.3");
    assert.equal(sbom.packages[0].checksums[0].checksumValue, entry.sha256);
  }
});

test("rejects duplicate logical targets", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "tandem-artifacts-"));
  await writeFile(path.join(directory, "tandem-engine-linux-x64.zip"), "one");
  await writeFile(path.join(directory, "tandem-engine-linux-x64.tar.gz"), "two");
  await assert.rejects(generateArtifactManifest(options(directory)), /duplicate runtime target/);
});

test("rejects incomplete provenance identifiers", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "tandem-artifacts-"));
  await writeFile(path.join(directory, "tandem-engine-linux-x64.tar.gz"), "engine");
  await assert.rejects(
    generateArtifactManifest({ ...options(directory), sourceCommit: "main" }),
    /full lowercase SHA/
  );
});
