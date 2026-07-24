#!/usr/bin/env node

import { createHash } from "node:crypto";
import { lstat, mkdir, readdir, readFile, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const MANIFEST_SCHEMA = "tandem-artifact-manifest/v1";
export const MANIFEST_FILENAME = "tandem-artifacts-v1.json";
const MAX_ARTIFACT_BYTES = 2 * 1024 * 1024 * 1024;
const RUNTIME_ASSET =
  /^tandem-(engine(?:-enterprise)?|tui|browser)-(linux|darwin|windows)-(x64|arm64)\.(zip|tar\.gz)$/;

function parseArgs(argv) {
  const values = new Map();
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value === undefined) {
      throw new Error(`invalid argument near ${key ?? "end of arguments"}`);
    }
    values.set(key.slice(2), value);
  }
  return Object.fromEntries(values);
}

function requireSafeIdentifier(label, value, maxLength = 128) {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > maxLength ||
    !/^[A-Za-z0-9._+-]+$/.test(value)
  ) {
    throw new Error(`${label} is invalid`);
  }
  return value;
}

function requireRepository(value) {
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(value ?? "")) {
    throw new Error("repository is invalid");
  }
  return value;
}

function requireCommit(value) {
  if (!/^[0-9a-f]{40}$/.test(value ?? "")) {
    throw new Error("commit must be a full lowercase SHA");
  }
  return value;
}

function requireWorkflow(value) {
  if (!/^\.github\/workflows\/[A-Za-z0-9._-]+\.ya?ml$/.test(value ?? "")) {
    throw new Error("workflow path is invalid");
  }
  return value;
}

function requireRunId(value) {
  if (!/^[1-9][0-9]*$/.test(String(value ?? ""))) {
    throw new Error("run id is invalid");
  }
  const runId = Number(value);
  if (!Number.isSafeInteger(runId)) {
    throw new Error("run id exceeds the safe integer range");
  }
  return runId;
}

function requireGeneratedAt(value) {
  if (typeof value !== "string" || Number.isNaN(Date.parse(value))) {
    throw new Error("generated-at must be an RFC3339 timestamp");
  }
  return value;
}

export function classifyRuntimeAsset(filename) {
  const match = RUNTIME_ASSET.exec(filename);
  if (!match) return null;
  const [, product, platform, architecture] = match;
  const kind = product === "engine-enterprise" ? "engine_enterprise" : product;
  return { kind, platform, architecture };
}

async function digestFile(filename) {
  const bytes = await readFile(filename);
  return createHash("sha256").update(bytes).digest("hex");
}

function stableJson(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function buildSbom({ filename, length, sha256, generatedAt, repository, release }) {
  const packageId = `SPDXRef-Package-${sha256.slice(0, 16)}`;
  return {
    spdxVersion: "SPDX-2.3",
    dataLicense: "CC0-1.0",
    SPDXID: "SPDXRef-DOCUMENT",
    name: `${filename} SBOM`,
    documentNamespace: `https://github.com/${repository}/releases/download/${release}/sbom/${sha256}`,
    creationInfo: {
      created: generatedAt,
      creators: ["Tool: tandem-artifact-manifest-generator/1"],
    },
    packages: [
      {
        name: filename,
        SPDXID: packageId,
        downloadLocation: "NOASSERTION",
        filesAnalyzed: false,
        packageFileName: filename,
        checksums: [{ algorithm: "SHA256", checksumValue: sha256 }],
        licenseConcluded: "NOASSERTION",
        licenseDeclared: "NOASSERTION",
        copyrightText: "NOASSERTION",
        comment: `Packaged runtime artifact length: ${length} bytes`,
      },
    ],
    relationships: [
      {
        spdxElementId: "SPDXRef-DOCUMENT",
        relationshipType: "DESCRIBES",
        relatedSpdxElement: packageId,
      },
    ],
  };
}

export async function generateArtifactManifest(options) {
  const assetsDir = path.resolve(options.assetsDir);
  const outputDir = path.resolve(options.outputDir ?? options.assetsDir);
  const release = requireSafeIdentifier("release", options.release);
  const version = requireSafeIdentifier("version", options.version, 96);
  const repository = requireRepository(options.repository);
  const sourceCommit = requireCommit(options.sourceCommit);
  const workflow = requireWorkflow(options.workflow);
  const runId = requireRunId(options.runId);
  const generatedAt = requireGeneratedAt(options.generatedAt);
  const builderId = `https://github.com/${repository}/actions/runs/${runId}`;

  const directory = await lstat(assetsDir);
  if (!directory.isDirectory() || directory.isSymbolicLink()) {
    throw new Error("assets directory must be a real directory");
  }
  await mkdir(outputDir, { recursive: true });

  const candidates = (await readdir(assetsDir))
    .map((filename) => ({ filename, target: classifyRuntimeAsset(filename) }))
    .filter(({ target }) => target !== null)
    .sort((left, right) => left.filename.localeCompare(right.filename));
  if (candidates.length === 0 || candidates.length > 64) {
    throw new Error("runtime artifact count is invalid");
  }

  const targetKeys = new Set();
  const artifacts = [];
  for (const { filename, target } of candidates) {
    const fullPath = path.join(assetsDir, filename);
    const metadata = await lstat(fullPath);
    if (
      !metadata.isFile() ||
      metadata.isSymbolicLink() ||
      metadata.size <= 0 ||
      metadata.size > MAX_ARTIFACT_BYTES
    ) {
      throw new Error(`runtime artifact ${filename} is not a bounded regular file`);
    }
    const targetKey = `${target.kind}:${target.platform}:${target.architecture}`;
    if (targetKeys.has(targetKey)) {
      throw new Error(`duplicate runtime target ${targetKey}`);
    }
    targetKeys.add(targetKey);

    const sha256 = await digestFile(fullPath);
    const sbomFilename = `${filename}.sbom.spdx.json`;
    const sbomPath = path.join(outputDir, sbomFilename);
    const sbom = buildSbom({
      filename,
      length: metadata.size,
      sha256,
      generatedAt,
      repository,
      release,
    });
    await writeFile(sbomPath, stableJson(sbom), { mode: 0o644 });
    const sbomMetadata = await stat(sbomPath);
    const sbomSha256 = await digestFile(sbomPath);

    artifacts.push({
      kind: target.kind,
      version,
      platform: target.platform,
      architecture: target.architecture,
      filename,
      length: metadata.size,
      sha256,
      sbom: {
        filename: sbomFilename,
        length: sbomMetadata.size,
        sha256: sbomSha256,
      },
      provenance: {
        source_repository: repository,
        source_commit: sourceCommit,
        workflow,
        run_id: runId,
        builder_id: builderId,
      },
    });
  }

  const manifest = {
    schema: MANIFEST_SCHEMA,
    release,
    version,
    generated_at: generatedAt,
    source_repository: repository,
    source_commit: sourceCommit,
    artifacts,
  };
  const outputPath = path.join(outputDir, MANIFEST_FILENAME);
  await writeFile(outputPath, stableJson(manifest), { mode: 0o644 });
  return { manifest, outputPath };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const result = await generateArtifactManifest({
    assetsDir: args["assets-dir"],
    outputDir: args["output-dir"],
    release: args.release,
    version: args.version,
    repository: args.repository,
    sourceCommit: args.commit,
    workflow: args.workflow,
    runId: args["run-id"],
    generatedAt: args["generated-at"],
  });
  process.stdout.write(`${result.outputPath}\n`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main().catch((error) => {
    process.stderr.write(`artifact manifest generation failed: ${error.message}\n`);
    process.exitCode = 1;
  });
}
