#!/usr/bin/env node

import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");

const DEFAULT_OUT_DIR = path.join(repoRoot, "crates", "tandem-server", "resources", "mcp-catalog");
const DEFAULT_RUST_MODULE_PATH = path.join(
  repoRoot,
  "crates",
  "tandem-server",
  "src",
  "mcp_catalog_generated.rs"
);
const REGISTRY_BASE = "https://api.anthropic.com/mcp-registry/v0/servers";
const CURATED_SOURCE = "curated-mcp-overrides";

const CURATED_ENTRIES = [
  {
    slug: "github",
    name: "GitHub (Official)",
    description:
      "Official GitHub MCP server (remote). Access repositories, issues, pull requests, actions, and workflows.",
    documentationUrl: "https://github.com/github/github-mcp-server",
    directoryUrl: "https://github.com/github/github-mcp-server",
    primaryTransportUrl: "https://api.githubcopilot.com/mcp/",
    remotes: [{ type: "streamable-http", url: "https://api.githubcopilot.com/mcp/" }],
    toolNames: [],
    useCases: ["devtools", "github", "repositories"],
    worksWith: ["claude", "claude-api", "claude-code", "codex"],
    visibility: ["curated"],
    requiresAuth: true,
    requiresSetup: false,
    serverName: "github/github-mcp-server",
    serverVersion: "",
    displayName: "GitHub (Official)",
    uuid: "curated:github-remote",
    rank: 0,
    authorName: "GitHub",
    authorUrl: "https://github.com/github",
    publishedOn: "",
    updatedOn: "",
    serverConfigName: "github",
  },
  {
    slug: "jira",
    name: "Jira (Atlassian Official)",
    description:
      "Official Atlassian remote MCP server for Jira and Confluence access.",
    documentationUrl:
      "https://support.atlassian.com/atlassian-rovo-mcp-server/docs/setting-up-ides/",
    directoryUrl: "https://github.com/atlassian/atlassian-mcp-server",
    primaryTransportUrl: "https://mcp.atlassian.com/v1/mcp",
    remotes: [{ type: "streamable-http", url: "https://mcp.atlassian.com/v1/mcp" }],
    toolNames: [],
    useCases: ["devtools", "jira", "atlassian", "project-management"],
    worksWith: ["claude", "claude-api", "claude-code", "codex"],
    visibility: ["curated"],
    requiresAuth: true,
    requiresSetup: false,
    serverName: "atlassian/atlassian-mcp-server",
    serverVersion: "",
    displayName: "Jira (Atlassian Official)",
    uuid: "curated:jira-remote",
    rank: 0,
    authorName: "Atlassian",
    authorUrl: "https://www.atlassian.com",
    publishedOn: "",
    updatedOn: "",
    serverConfigName: "jira",
  },
  {
    slug: "notion",
    name: "Notion (Official)",
    description:
      "Official Notion remote MCP server for pages, databases, and workspace content.",
    documentationUrl: "https://developers.notion.com/docs/mcp",
    directoryUrl: "https://notion.com",
    primaryTransportUrl: "https://mcp.notion.com/mcp",
    remotes: [{ type: "streamable-http", url: "https://mcp.notion.com/mcp" }],
    toolNames: [],
    useCases: ["productivity", "notes", "knowledge-base"],
    worksWith: ["claude", "claude-api", "claude-code", "codex"],
    visibility: ["curated"],
    requiresAuth: true,
    requiresSetup: false,
    authKind: "oauth",
    serverName: "com.notion/mcp",
    serverVersion: "",
    displayName: "Notion (Official)",
    uuid: "curated:notion-remote",
    rank: 0,
    authorName: "Notion",
    authorUrl: "https://notion.com",
    publishedOn: "",
    updatedOn: "",
    serverConfigName: "notion",
  },
];

function parseArgs(argv) {
  const out = {
    outDir: DEFAULT_OUT_DIR,
    rustModulePath: DEFAULT_RUST_MODULE_PATH,
    keep: false,
    version: "latest",
    visibility: "commercial",
    limit: 100,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = String(argv[i] || "").trim();
    if (!arg) continue;
    if (arg === "--keep") {
      out.keep = true;
      continue;
    }
    if (arg.startsWith("--out-dir=")) {
      out.outDir = path.resolve(arg.slice("--out-dir=".length));
      continue;
    }
    if (arg === "--out-dir") {
      out.outDir = path.resolve(String(argv[i + 1] || "").trim());
      i += 1;
      continue;
    }
    if (arg.startsWith("--rust-module=")) {
      out.rustModulePath = path.resolve(arg.slice("--rust-module=".length));
      continue;
    }
    if (arg === "--rust-module") {
      out.rustModulePath = path.resolve(String(argv[i + 1] || "").trim());
      i += 1;
      continue;
    }
    if (arg.startsWith("--version=")) {
      out.version = arg.slice("--version=".length).trim() || out.version;
      continue;
    }
    if (arg.startsWith("--visibility=")) {
      out.visibility = arg.slice("--visibility=".length).trim() || out.visibility;
      continue;
    }
    if (arg.startsWith("--limit=")) {
      const parsed = Number.parseInt(arg.slice("--limit=".length), 10);
      if (Number.isFinite(parsed) && parsed > 0) out.limit = parsed;
      continue;
    }
  }
  return out;
}

function normalizeSlug(raw) {
  const slug = String(raw || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return slug || "mcp-server";
}

function normalizeServerName(raw) {
  const text = String(raw || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return text || "mcp-server";
}

function toTomlString(value) {
  const text = String(value ?? "");
  return `"${text
    .replace(/\\/g, "\\\\")
    .replace(/\"/g, '\\"')
    .replace(/\n/g, "\\n")
    .replace(/\r/g, "")}"`;
}

function toTomlArray(values) {
  if (!Array.isArray(values) || values.length === 0) return "[]";
  return `[${values.map((entry) => toTomlString(String(entry || ""))).join(", ")}]`;
}

function truthy(value) {
  return value ? "true" : "false";
}

function buildToml(entry, generatedAt) {
  const lines = [];
  lines.push("schema_version = 1");
  lines.push(`generated_at = ${toTomlString(generatedAt)}`);
  lines.push(`source = ${toTomlString(entry.source || "anthropic-mcp-registry")}`);
  lines.push(`catalog_version = ${toTomlString(entry.catalogVersion)}`);
  lines.push(`catalog_visibility = ${toTomlString(entry.catalogVisibility)}`);
  lines.push(`pack_id = ${toTomlString(entry.packId)}`);
  lines.push(`slug = ${toTomlString(entry.slug)}`);
  lines.push(`name = ${toTomlString(entry.name)}`);
  lines.push(`description = ${toTomlString(entry.description)}`);
  lines.push(`documentation_url = ${toTomlString(entry.documentationUrl)}`);
  lines.push(`directory_url = ${toTomlString(entry.directoryUrl)}`);
  lines.push(`requires_auth = ${truthy(entry.requiresAuth)}`);
  lines.push(`requires_setup = ${truthy(entry.requiresSetup)}`);
  if (entry.authKind) {
    lines.push(`auth_kind = ${toTomlString(entry.authKind)}`);
  }
  lines.push(`works_with = ${toTomlArray(entry.worksWith)}`);
  lines.push(`use_cases = ${toTomlArray(entry.useCases)}`);
  lines.push(`tool_names = ${toTomlArray(entry.toolNames)}`);
  lines.push(`visibility = ${toTomlArray(entry.visibility)}`);
  lines.push("");
  lines.push("[server]");
  lines.push(`name = ${toTomlString(entry.serverName)}`);
  lines.push(`version = ${toTomlString(entry.serverVersion)}`);
  lines.push(`display_name = ${toTomlString(entry.displayName)}`);
  lines.push(`transport_url = ${toTomlString(entry.primaryTransportUrl)}`);
  lines.push("");
  lines.push("[registry]");
  lines.push(`uuid = ${toTomlString(entry.uuid)}`);
  lines.push(`rank = ${Number.isFinite(entry.rank) ? String(entry.rank) : "0"}`);
  lines.push(`author_name = ${toTomlString(entry.authorName)}`);
  lines.push(`author_url = ${toTomlString(entry.authorUrl)}`);
  lines.push(`published_on = ${toTomlString(entry.publishedOn)}`);
  lines.push(`updated_on = ${toTomlString(entry.updatedOn)}`);

  for (const remote of entry.remotes) {
    lines.push("");
    lines.push("[[remote]]");
    lines.push(`type = ${toTomlString(remote.type)}`);
    lines.push(`url = ${toTomlString(remote.url)}`);
  }

  return `${lines.join("\n")}\n`;
}

function inferRequiresSetup(remotes) {
  const joined = remotes.map((row) => String(row.url || "")).join("\n").toLowerCase();
  if (!joined) return false;
  if (/[{<][^}>]+[}>]/.test(joined)) return true;
  if (/your[_-]?|replace[_-]?|example|workspace[_-]?id|team[_-]?id|tenant[_-]?id/.test(joined)) return true;
  return false;
}

async function fetchRegistryPage({ version, visibility, limit, cursor }) {
  const params = new URLSearchParams();
  params.set("version", version);
  params.set("visibility", visibility);
  params.set("limit", String(limit));
  if (cursor) params.set("cursor", cursor);
  const response = await fetch(`${REGISTRY_BASE}?${params.toString()}`);
  if (!response.ok) {
    throw new Error(`Registry request failed (${response.status} ${response.statusText})`);
  }
  return response.json();
}

async function fetchAllRegistryRows({ version, visibility, limit }) {
  const rows = [];
  let cursor = "";
  for (;;) {
    const payload = await fetchRegistryPage({ version, visibility, limit, cursor });
    const pageRows = Array.isArray(payload?.servers) ? payload.servers : [];
    rows.push(...pageRows);
    const nextCursor = String(payload?.metadata?.nextCursor || "").trim();
    if (!nextCursor) break;
    cursor = nextCursor;
  }
  return rows;
}

function toCatalogEntry(row, dedupeState, catalogVersion, catalogVisibility) {
  const server = row?.server && typeof row.server === "object" ? row.server : {};
  const meta =
    row?._meta && row._meta["com.anthropic.api/mcp-registry"] && typeof row._meta["com.anthropic.api/mcp-registry"] === "object"
      ? row._meta["com.anthropic.api/mcp-registry"]
      : {};

  const rawSlug = String(meta.slug || server.title || meta.displayName || server.name || "").trim();
  let slug = normalizeSlug(rawSlug);
  const count = dedupeState.get(slug) || 0;
  if (count > 0) {
    slug = `${slug}-${count + 1}`;
  }
  dedupeState.set(normalizeSlug(rawSlug), count + 1);

  const displayName = String(meta.displayName || server.title || server.name || slug).trim() || slug;
  const serverName = String(server.name || slug).trim() || slug;
  const remotes = Array.isArray(server.remotes)
    ? server.remotes
        .map((remote) => ({ type: String(remote?.type || "").trim(), url: String(remote?.url || "").trim() }))
        .filter((remote) => remote.url)
    : [];
  const primaryRemote = remotes.find((remote) => remote.type === "streamable-http") || remotes[0] || { type: "", url: "" };
  const visibility = Array.isArray(meta.visibility) ? meta.visibility.map((row) => String(row || "").trim()).filter(Boolean) : [];
  const toolNames = Array.isArray(meta.toolNames) ? meta.toolNames.map((row) => String(row || "").trim()).filter(Boolean) : [];
  const useCases = Array.isArray(meta.useCases) ? meta.useCases.map((row) => String(row || "").trim()).filter(Boolean) : [];
  const worksWith = Array.isArray(meta.worksWith) ? meta.worksWith.map((row) => String(row || "").trim()).filter(Boolean) : [];
  const description = String(meta.oneLiner || server.description || "").trim();
  const requiresAuth = !meta.isAuthless;
  const authKind = String(meta.authKind || meta.auth_kind || "").trim() || undefined;

  return {
    source: "anthropic-mcp-registry",
    catalogVersion,
    catalogVisibility,
    slug,
    packId: `mcp.remote.${slug}`,
    name: displayName,
    displayName,
    serverName,
    serverVersion: String(server.version || "").trim(),
    description,
    documentationUrl: String(meta.documentation || "").trim(),
    directoryUrl: String(meta.directoryUrl || "").trim(),
    primaryTransportUrl: String(primaryRemote.url || meta.url || "").trim(),
    remotes,
    toolNames,
    useCases,
    worksWith,
    visibility,
    requiresAuth,
    requiresSetup: inferRequiresSetup(remotes),
    authKind,
    uuid: String(meta.uuid || "").trim(),
    rank: Number.isFinite(Number(meta.rank)) ? Number(meta.rank) : 0,
    authorName: String(meta.author?.name || "").trim(),
    authorUrl: String(meta.author?.url || "").trim(),
    publishedOn: String(meta.publishedOn || "").trim(),
    updatedOn: String(meta.updatedOn || "").trim(),
    serverConfigName: normalizeServerName(meta.slug || displayName),
  };
}

function toCuratedEntry(def, catalogVersion, catalogVisibility) {
  const slug = normalizeSlug(def.slug || def.name || "curated-mcp");

  return {
    source: CURATED_SOURCE,
    catalogVersion,
    catalogVisibility,
    slug,
    packId: `mcp.remote.${slug}`,
    name: String(def.name || slug).trim(),
    displayName: String(def.displayName || def.name || slug).trim(),
    serverName: String(def.serverName || slug).trim(),
    serverVersion: String(def.serverVersion || "").trim(),
    description: String(def.description || "").trim(),
    documentationUrl: String(def.documentationUrl || "").trim(),
    directoryUrl: String(def.directoryUrl || "").trim(),
    primaryTransportUrl: String(def.primaryTransportUrl || "").trim(),
    remotes: Array.isArray(def.remotes)
      ? def.remotes
          .map((remote) => ({ type: String(remote?.type || "").trim(), url: String(remote?.url || "").trim() }))
          .filter((remote) => remote.url)
      : [],
    toolNames: Array.isArray(def.toolNames) ? def.toolNames.map((row) => String(row || "").trim()).filter(Boolean) : [],
    useCases: Array.isArray(def.useCases) ? def.useCases.map((row) => String(row || "").trim()).filter(Boolean) : [],
    worksWith: Array.isArray(def.worksWith) ? def.worksWith.map((row) => String(row || "").trim()).filter(Boolean) : [],
    visibility: Array.isArray(def.visibility) ? def.visibility.map((row) => String(row || "").trim()).filter(Boolean) : ["curated"],
    requiresAuth: def.requiresAuth !== false,
    requiresSetup: !!def.requiresSetup,
    authKind: String(def.authKind || def.auth_kind || "").trim() || undefined,
    uuid: String(def.uuid || `curated:${slug}`).trim(),
    rank: Number.isFinite(Number(def.rank)) ? Number(def.rank) : 0,
    authorName: String(def.authorName || "").trim(),
    authorUrl: String(def.authorUrl || "").trim(),
    publishedOn: String(def.publishedOn || "").trim(),
    updatedOn: String(def.updatedOn || "").trim(),
    serverConfigName: normalizeServerName(def.serverConfigName || def.slug || def.name || slug),
  };
}

async function ensureCleanDir(dir, keep) {
  if (!keep) {
    await rm(dir, { recursive: true, force: true });
  }
  await mkdir(dir, { recursive: true });
}

function buildRustModuleSource() {
  const lines = [];
  lines.push("// @generated by scripts/generate-mcp-catalog.mjs");
  lines.push("// Do not edit manually.");
  lines.push("");
  lines.push('pub static INDEX_JSON: &str = include_str!("../resources/mcp-catalog/index.json");');
  lines.push("");
  lines.push("pub static SERVERS: &[(&str, &str)] = &[");
  return lines;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const generatedAt = new Date().toISOString();
  const outDir = args.outDir;
  const serversDir = path.join(outDir, "servers");

  await ensureCleanDir(outDir, args.keep);
  await mkdir(serversDir, { recursive: true });

  const rows = await fetchAllRegistryRows({
    version: args.version,
    visibility: args.visibility,
    limit: args.limit,
  });

  const dedupe = new Map();
  const entries = rows.map((row) => toCatalogEntry(row, dedupe, args.version, args.visibility));
  const curatedEntries = CURATED_ENTRIES.map((entry) =>
    toCuratedEntry(entry, args.version, args.visibility)
  );
  const mergedBySlug = new Map(entries.map((entry) => [entry.slug, entry]));
  for (const curated of curatedEntries) {
    mergedBySlug.set(curated.slug, curated);
  }
  const mergedEntries = Array.from(mergedBySlug.values());
  mergedEntries.sort((a, b) => a.name.localeCompare(b.name));

  for (const entry of mergedEntries) {
    const toml = buildToml(entry, generatedAt);
    await writeFile(path.join(serversDir, `${entry.slug}.toml`), toml, "utf8");
  }

  const index = {
    schema_version: 1,
    source: "anthropic-mcp-registry",
    generated_at: generatedAt,
    version: args.version,
    visibility: args.visibility,
    count: mergedEntries.length,
    servers: mergedEntries.map((entry) => ({
      slug: entry.slug,
      pack_id: entry.packId,
      name: entry.name,
      description: entry.description,
      transport_url: entry.primaryTransportUrl,
      server_name: entry.serverName,
      server_config_name: entry.serverConfigName,
      documentation_url: entry.documentationUrl,
      directory_url: entry.directoryUrl,
      tool_count: entry.toolNames.length,
      tool_names: entry.toolNames,
      requires_auth: entry.requiresAuth,
      requires_setup: entry.requiresSetup,
      ...(entry.authKind ? { auth_kind: entry.authKind } : {}),
      visibility: entry.visibility,
      works_with: entry.worksWith,
      use_cases: entry.useCases,
      toml_path: `servers/${entry.slug}.toml`,
    })),
  };

  await writeFile(path.join(outDir, "index.json"), `${JSON.stringify(index, null, 2)}\n`, "utf8");

  const readme = `# MCP Catalog (Generated)\n\n- Sources:\n  - Anthropic MCP registry (${REGISTRY_BASE})\n  - Curated additions (${CURATED_SOURCE})\n- Generated at: ${generatedAt}\n- Version: ${args.version}\n- Visibility: ${args.visibility}\n- Servers: ${mergedEntries.length}\n\nRegenerate:\n\n\`node scripts/generate-mcp-catalog.mjs\`\n`;
  await writeFile(path.join(outDir, "README.md"), readme, "utf8");

  const rustLines = buildRustModuleSource();
  for (const entry of mergedEntries) {
    rustLines.push(
      `    (${JSON.stringify(entry.slug)}, include_str!("../resources/mcp-catalog/servers/${entry.slug}.toml")),`
    );
  }
  rustLines.push("];\n");
  await writeFile(args.rustModulePath, `${rustLines.join("\n")}`, "utf8");

  const packageJsonPath = path.join(repoRoot, "packages", "tandem-control-panel", "package.json");
  const packageJsonRaw = await readFile(packageJsonPath, "utf8");
  const packageJson = JSON.parse(packageJsonRaw);
  if (!packageJson.scripts) packageJson.scripts = {};
  if (!packageJson.scripts["mcp:catalog:refresh"]) {
    packageJson.scripts["mcp:catalog:refresh"] = "node ../../scripts/generate-mcp-catalog.mjs";
    await writeFile(packageJsonPath, `${JSON.stringify(packageJson, null, 2)}\n`, "utf8");
  }

  process.stdout.write(
    `Generated ${mergedEntries.length} MCP TOML manifests in ${outDir} and ${path.relative(repoRoot, args.rustModulePath)}\n`
  );
}

main().catch((error) => {
  const message = error instanceof Error ? error.stack || error.message : String(error);
  process.stderr.write(`${message}\n`);
  process.exit(1);
});
