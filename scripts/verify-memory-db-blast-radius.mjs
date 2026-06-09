#!/usr/bin/env node

/**
 * Verifies the current memory database blast-radius boundary.
 *
 * This is intentionally evidence-based:
 * - today, Tandem memory storage is SQLite/sqlite-vec, not Postgres;
 * - sensitive SQLite memory tables must keep tenant-scope columns;
 * - if a hosted deployment declares Postgres mode, the repo must contain RLS
 *   policy evidence before the check passes.
 */

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const repoRoot = path.resolve(new URL("..", import.meta.url).pathname);

const files = {
  memorySchemas: [
    "crates/tandem-memory/src/memory_database_impl_parts/part01.rs",
    "crates/tandem-memory/src/memory_database_impl_parts/part01_a.rs",
    "crates/tandem-memory/src/memory_database_impl_parts/part01_b.rs",
  ],
  memoryQueries: "crates/tandem-memory/src/memory_database_impl_parts/part02.rs",
  memoryDb: "crates/tandem-memory/src/db.rs",
  responseCache: "crates/tandem-memory/src/response_cache.rs",
  cargoLock: "Cargo.lock",
};

function readRepoFile(relativePath) {
  return fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
}

function rgLikeFiles(root, predicate) {
  const out = [];
  const stack = [root];
  while (stack.length > 0) {
    const current = stack.pop();
    const stat = fs.statSync(current);
    if (stat.isDirectory()) {
      const base = path.basename(current);
      if (base === "target" || base === ".git" || base === "node_modules") {
        continue;
      }
      for (const entry of fs.readdirSync(current)) {
        stack.push(path.join(current, entry));
      }
      continue;
    }
    if (predicate(current)) {
      out.push(current);
    }
  }
  return out.sort();
}

function packagePresent(lockfile, name) {
  return new RegExp(`name = "${name}"`, "m").test(lockfile);
}

const schemas = files.memorySchemas.map((relativePath) =>
  readRepoFile(relativePath),
);
const queries = readRepoFile(files.memoryQueries);
const db = readRepoFile(files.memoryDb);
const responseCache = readRepoFile(files.responseCache);
const cargoLock = readRepoFile(files.cargoLock);
const memorySources = [
  ...files.memorySchemas.map((file, index) => ({ file, content: schemas[index] })),
  { file: files.memoryQueries, content: queries },
  { file: files.memoryDb, content: db },
  { file: files.responseCache, content: responseCache },
];
const combinedMemorySource = memorySources.map((source) => source.content).join("\n");

const sensitiveTables = [
  "session_memory_chunks",
  "project_memory_chunks",
  "global_memory_chunks",
  "memory_records",
  "source_object_lifecycle",
  "project_file_index",
  "session_file_index",
  "global_file_index",
  "project_index_status",
  "memory_config",
  "memory_cleanup_log",
  "response_cache",
  "knowledge_spaces",
];

function escapeRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function windowsForNeedle(content, needle, windowChars = 3000) {
  const windows = [];
  let index = content.indexOf(needle);
  while (index !== -1) {
    const fallbackEnd = index + windowChars;
    const sqlStringEnd = content.indexOf(')"', index);
    const end = sqlStringEnd === -1 ? fallbackEnd : Math.min(sqlStringEnd + 2, fallbackEnd);
    windows.push(content.slice(index, end));
    index = content.indexOf(needle, index + needle.length);
  }
  return windows;
}

function windowHasTenantColumns(window) {
  return window.includes("tenant_org_id") && window.includes("tenant_workspace_id");
}

function tableScopeEvidence(table) {
  const evidence = [];
  for (const source of memorySources) {
    const directCreateWindows = [
      ...windowsForNeedle(source.content, `CREATE TABLE IF NOT EXISTS ${table} (`),
      ...windowsForNeedle(source.content, `CREATE TABLE ${table} (`),
    ];
    if (directCreateWindows.some(windowHasTenantColumns)) {
      evidence.push(`direct_ddl:${source.file}`);
    }

    const hasTenantOrgAlter = new RegExp(
      `ALTER TABLE\\s+${escapeRegex(table)}\\s+ADD COLUMN\\s+tenant_org_id\\b`,
      "m",
    ).test(source.content);
    const hasTenantWorkspaceAlter = new RegExp(
      `ALTER TABLE\\s+${escapeRegex(table)}\\s+ADD COLUMN\\s+tenant_workspace_id\\b`,
      "m",
    ).test(source.content);
    if (hasTenantOrgAlter && hasTenantWorkspaceAlter) {
      evidence.push(`alter_columns:${source.file}`);
    }

    const migrationCreateWindows = windowsForNeedle(
      source.content,
      `CREATE TABLE ${table}_new (`,
    );
    const hasMigrationRename = new RegExp(
      `ALTER TABLE\\s+${escapeRegex(table)}_new\\s+RENAME TO\\s+${escapeRegex(table)}\\b`,
      "m",
    ).test(source.content);
    if (hasMigrationRename && migrationCreateWindows.some(windowHasTenantColumns)) {
      evidence.push(`migration_table_rebuild:${source.file}`);
    }
  }
  return evidence;
}

const tenantScopedTables = sensitiveTables.map((table) => {
  const tableMentions = new RegExp(`\\b${escapeRegex(table)}\\b`, "g").test(combinedMemorySource);
  const evidence = tableScopeEvidence(table);
  return { table, tableMentions, scopePresent: evidence.length > 0, evidence };
});

const postgresPackages = [
  "sqlx",
  "tokio-postgres",
  "postgres",
  "deadpool-postgres",
  "diesel",
  "sea-orm",
]
  .filter((name) => packagePresent(cargoLock, name));

const hostedDbMode = (process.env.TANDEM_HOSTED_DB_MODE || "").toLowerCase();
const requireHostedBoundary =
  process.env.TANDEM_HOSTED_REQUIRE_DB_BOUNDARY === "1" || hostedDbMode.includes("postgres");
const scanPostgresEvidence =
  requireHostedBoundary ||
  postgresPackages.length > 0 ||
  process.env.TANDEM_SCAN_POSTGRES_EVIDENCE === "1";

let postgresEvidence = "";
if (scanPostgresEvidence) {
  const rootsToScan = ["crates", "scripts", ".github", "deploy", "migrations"]
    .map((relative) => path.join(repoRoot, relative))
    .filter((absolute) => fs.existsSync(absolute));
  const sqlAndRustFiles = rootsToScan.flatMap((root) =>
    rgLikeFiles(root, (file) => /\.(rs|sql|toml|ya?ml)$/.test(file)),
  );
  for (const file of sqlAndRustFiles) {
    const content = fs.readFileSync(file, "utf8");
    if (/ENABLE ROW LEVEL SECURITY|FORCE ROW LEVEL SECURITY|CREATE POLICY|pgcrypto/i.test(content)) {
      postgresEvidence += `\n--- ${path.relative(repoRoot, file)} ---\n${content}`;
    }
  }
}

const rlsEvidence = {
  enableRls: /ENABLE ROW LEVEL SECURITY/i.test(postgresEvidence),
  forceRls: /FORCE ROW LEVEL SECURITY/i.test(postgresEvidence),
  createPolicy: /CREATE POLICY/i.test(postgresEvidence),
  pgcrypto: /pgcrypto/i.test(postgresEvidence),
};

const failures = [];

for (const row of tenantScopedTables) {
  if (!row.tableMentions) {
    failures.push(`sensitive table ${row.table} is missing from memory DB source inventory`);
  } else if (!row.scopePresent) {
    failures.push(`sensitive table ${row.table} does not show tenant scope columns`);
  }
}

if (!db.includes("SQLite + sqlite-vec")) {
  failures.push("memory DB backend comment no longer confirms SQLite/sqlite-vec storage");
}

if (requireHostedBoundary) {
  if (postgresPackages.length === 0) {
    failures.push(
      "hosted/Postgres boundary was required, but no Postgres Rust client dependency is present",
    );
  }
  if (!rlsEvidence.enableRls || !rlsEvidence.forceRls || !rlsEvidence.createPolicy) {
    failures.push(
      "hosted/Postgres boundary was required, but RLS evidence is incomplete: expected ENABLE RLS, FORCE RLS, and CREATE POLICY",
    );
  }
}

const report = {
  storage_backend: postgresPackages.length > 0 ? "postgres_candidate_detected" : "sqlite_vec_current",
  postgres_packages: postgresPackages,
  postgres_rls_evidence: rlsEvidence,
  hosted_boundary_required: requireHostedBoundary,
  sensitive_tables: tenantScopedTables,
  inherited_tenant_scope: [
    {
      table: "knowledge_items",
      via: "space_id -> knowledge_spaces",
      note: "rows inherit tenant boundary from tenant-scoped knowledge_spaces; query paths must preserve *_for_tenant accessors",
    },
    {
      table: "knowledge_coverage",
      via: "space_id -> knowledge_spaces",
      note: "rows inherit tenant boundary from tenant-scoped knowledge_spaces; query paths must preserve *_for_tenant accessors",
    },
  ],
  plaintext_boundary: {
    memory_content_plaintext_in_sqlite: true,
    pgcrypto_present: rlsEvidence.pgcrypto,
    buyer_claim: "Do not claim database-compromise containment for hosted memory until Postgres RLS, role separation, and key-scope evidence exist.",
  },
  failures,
};

console.log(JSON.stringify(report, null, 2));

if (failures.length > 0) {
  process.exitCode = 1;
}
