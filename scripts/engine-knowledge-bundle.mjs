import { createHash } from "node:crypto";
import { promises as fs } from "node:fs";
import path from "node:path";

export const GENERATOR_VERSION = "1";
export const SCHEMA_VERSION = 1;
export const DOCS_SITE_BASE_URL = "https://tandem.docs.frumu.ai/";
export const SOURCE_ROOT = path.join("guide", "src", "content", "docs");
export const BUNDLE_PATH = path.join("engine", "resources", "default_knowledge_bundle.json");
export const MANIFEST_PATH = path.join("engine", "resources", "default_knowledge_manifest.json");

export function sha256Hex(input) {
  return createHash("sha256").update(input).digest("hex");
}

export function normalizeDocContent(content) {
  return content.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
}

export function deterministicGeneratedAt(corpusHash) {
  // Make bundle output stable across runs so CI drift checks only fail on
  // actual docs-content changes, not wall-clock timestamps.
  const seed = Number.parseInt(corpusHash.slice(0, 12), 16);
  const baseMs = Date.UTC(2020, 0, 1, 0, 0, 0, 0);
  const windowSeconds = 10 * 365 * 24 * 60 * 60;
  const offsetMs = (seed % windowSeconds) * 1000;
  return new Date(baseMs + offsetMs).toISOString();
}

export function toPosixRelative(baseDir, targetPath) {
  return path.relative(baseDir, targetPath).split(path.sep).join("/");
}

export function docsUrlForRelativePath(relativePath) {
  let slug = relativePath.replace(/\\/g, "/");
  if (slug.endsWith(".md")) {
    slug = slug.slice(0, -3);
  } else if (slug.endsWith(".mdx")) {
    slug = slug.slice(0, -4);
  }
  if (slug === "index") {
    return DOCS_SITE_BASE_URL;
  }
  if (slug.endsWith("/index")) {
    slug = slug.slice(0, -6);
  }
  return new URL(slug, DOCS_SITE_BASE_URL).toString();
}

export async function collectDocPaths(dir) {
  const out = [];
  const entries = await fs.readdir(dir, { withFileTypes: true });
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...(await collectDocPaths(fullPath)));
      continue;
    }
    if (!entry.isFile()) {
      continue;
    }
    if (!entry.name.endsWith(".md") && !entry.name.endsWith(".mdx")) {
      continue;
    }
    out.push(fullPath);
  }
  return out;
}

export async function buildKnowledgeBundle() {
  const sourceDir = path.resolve(SOURCE_ROOT);
  const allDocPaths = await collectDocPaths(sourceDir);
  allDocPaths.sort((a, b) => a.localeCompare(b));

  const docs = [];
  let totalBytes = 0;
  for (const fullPath of allDocPaths) {
    const rawContent = await fs.readFile(fullPath, "utf8");
    const content = normalizeDocContent(rawContent);
    if (!content.trim()) {
      continue;
    }
    const relativePath = toPosixRelative(sourceDir, fullPath);
    const contentHash = sha256Hex(content);
    totalBytes += Buffer.byteLength(content, "utf8");
    docs.push({
      relative_path: relativePath,
      source_url: docsUrlForRelativePath(relativePath),
      content,
      content_hash: contentHash,
    });
  }

  const corpusHasher = createHash("sha256");
  for (const doc of docs) {
    corpusHasher.update(doc.relative_path);
    corpusHasher.update("\n");
    corpusHasher.update(doc.content_hash);
    corpusHasher.update("\n");
  }
  const corpusHash = corpusHasher.digest("hex");

  const bundle = {
    schema_version: SCHEMA_VERSION,
    source_root: "guide/src/content/docs",
    docs_site_base_url: DOCS_SITE_BASE_URL,
    generated_at: deterministicGeneratedAt(corpusHash),
    docs,
  };
  const manifest = {
    schema_version: SCHEMA_VERSION,
    generator_version: GENERATOR_VERSION,
    corpus_hash: corpusHash,
    file_count: docs.length,
    total_bytes: totalBytes,
  };

  return { bundle, manifest };
}

export async function writeKnowledgeBundle() {
  const { bundle, manifest } = await buildKnowledgeBundle();
  await fs.mkdir(path.dirname(BUNDLE_PATH), { recursive: true });
  await fs.writeFile(BUNDLE_PATH, `${JSON.stringify(bundle, null, 2)}\n`);
  await fs.writeFile(MANIFEST_PATH, `${JSON.stringify(manifest, null, 2)}\n`);
  return { bundle, manifest };
}
