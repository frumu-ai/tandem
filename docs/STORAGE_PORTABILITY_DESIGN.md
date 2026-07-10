# Storage Portability & PostgreSQL Readiness — Design (TAN-659, TAN-660)

Design decision for making the memory subsystem migratable to PostgreSQL:
the backend abstraction (**TAN-659**) and the vector-store portability path
(**TAN-660**). Feeds `TAN-661` (file-based audit/policy/org-unit stores) and the
M1 schema work (`TAN-645` `owner_org_unit_id`, `TAN-648` `private`, `TAN-666`
envelope encryption).

## Implementation status (as of 0.6.8)

This document remains the design record. The SQLite-backed implementation now
has the following completed foundation:

- **`MemoryStore` contract** (`crates/tandem-memory/src/store.rs`): an
  operation-level interface for scoped reads, queries, writes, mutations,
  batches, health, recovery, and migrations. `MemoryManager`, the importer,
  server HTTP/prompt/scheduler paths, and tools use the contract rather than
  concrete `MemoryDatabase` in production business logic (TAN-677).
- **Portable migration model:** a centralized, ordered logical registry with
  SQLite translations. Legacy schema versions 1-4 are explicit bootstrap
  baselines; version 5 migrates and backfills private owner scope transactionally.
- **Private owner scope:** `private` and `owner_subject` are queryable columns
  on memory records and chunks. Tenant, active org-unit, and shared-or-owner
  predicates run before FTS/vector ranking, pagination, and mutation (TAN-679).
- **No PostgreSQL backend.** There is no `pgvector`, `tokio-postgres`, `sqlx`,
  or other Postgres dependency in the workspace; the pgvector path below is
  unimplemented design, tracked as TAN-678.
- **Envelope encryption** (TAN-666) shipped: per-scope DEK envelopes with
  hosted-KMS key scoping (`envelope.rs`, `envelope_crypto.rs`,
  `kms_providers.rs`, `decrypt_broker.rs`, `dek_cache.rs`).
- **Governance file stores** are encrypted at rest (TAN-664) and work with
  scoped hosted-KMS encryption (TAN-675). The FTS and embedding search
  surfaces remain plaintext at rest — see
  `docs/MEMORY_SEARCH_SURFACE_AT_REST.md` and TAN-681.

## Current state (grounded)

- **Memory DB:** an object-safe, operation-level `MemoryStore` contract now owns
  scoped read, query, write, mutation, maintenance, health, and migration
  capabilities. `MemoryDatabase` is the bundled SQLite adapter; SQLite SQL and
  driver values remain in the backend implementation.
- **Vectors:** `sqlite-vec` 0.1.7 — `vec0` **virtual tables**
  (`part01_a.rs:220,315,543`), a **loaded SQLite extension**. KNN via per-tenant
  top-k scans (`search_similar_for_tenant`, `part01_a.rs:1484`) that push the
  tenant/subject scope into the `WHERE` so another scope's vectors can't suppress
  candidates.
- **Audit / policy / org-units:** flat JSON/JSONL files (`audit.rs:224`,
  `config/paths.rs`) behind the `GovernanceStoreFile` abstraction — see
  `TAN-661`/`TAN-664`.
- **Backend-neutral callers:** `MemoryManager`, the importer, and memory tools
  route storage workflows through typed contract requests, including context
  trees/layers, source lifecycle, maintenance, and scoped deletes. The concrete
  database retained by the SQLite constructor is a compatibility handle, not a
  business-logic dependency; `new_with_store` runs the same manager against any
  contract implementation.
- **Workspace dependencies:** no `sqlx`, `tokio-postgres`, `diesel`, `sea-orm`,
  or `pgvector` is used by the SQLite backend. The contract uses the existing
  `async-trait` and `tokio` dependencies.

## Decision 1 — backend abstraction (TAN-659)

### Options

| Option | Pros | Cons |
|---|---|---|
| **A. Operation-level trait, two concrete backends** (rusqlite+sqlite-vec now, tokio-postgres+pgvector later) | No heavy new dep (reuses `async-trait`); each backend expresses its own ANN idiom; scope-predicate contract shared; incremental | More code than a single driver; two SQL dialects to maintain |
| **B. `sqlx` for both SQLite and Postgres** | One async API + compile-time-checked queries; connection pooling for PG | **`sqlx`-sqlite cannot easily load the `sqlite-vec` extension** the vector path depends on; large dep; would still need dialect branches for `vec0` vs `pgvector` |
| **C. ORM (`diesel` / `sea-orm`)** | Schema modeling, migrations | Heaviest dep; poor fit for `vec0` virtual tables + custom ANN SQL; large rewrite |

### Decision: **Option A — an operation-level `MemoryStore` trait with two concrete backends.**

Rationale:
- The vector layer is the deciding factor. `sqlite-vec` is a **loaded extension**
  exposing `vec0` virtual tables; `sqlx`-sqlite does not host it cleanly, so
  Option B would not actually unify the vector path — the hardest part — and would
  add a large dependency for little gain. `pgvector` (Postgres) and `vec0`
  (SQLite) are different enough that a raw-SQL passthrough abstraction would leak
  the dialect anyway.
- An **operation-level** trait (methods like `put_chunk`, `search_similar_for_scope`,
  `put_record`, `search_records_for_tenant`, `delete_scope`, …) — rather than a
  `query(sql)` passthrough — lets each backend own its ANN idiom while the caller
  depends only on the operation + the scope contract. This is also what makes the
  new M1 columns and the DEK-envelope work land once, in the trait's data types,
  not per-call-site.
- Reuses the existing `async-trait`; no new heavyweight dependency; the SQLite
  impl is a behavior-preserving wrap of today's code.

### Implemented shape

```rust
#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn read(&self, request: MemoryStoreReadRequest) -> MemoryStoreResult<MemoryStoreReadResult>;
    async fn query(&self, request: MemoryStoreQueryRequest) -> MemoryStoreResult<MemoryStoreQueryResult>;
    async fn write(&self, request: MemoryStoreWriteRequest) -> MemoryStoreResult<MemoryStoreWriteResult>;
    async fn mutate(&self, request: MemoryStoreMutationRequest) -> MemoryStoreResult<MemoryStoreMutationResult>;
    async fn batch(&self, request: MemoryStoreBatchRequest) -> MemoryStoreResult<MemoryStoreBatchResult>;
    async fn backend_health(&self, request: MemoryBackendHealthRequest) -> MemoryStoreResult<MemoryBackendHealthResult>;
    async fn migration_capabilities(&self, request: MemoryMigrationCapabilityRequest) -> MemoryStoreResult<MemoryMigrationCapabilityResult>;
}
```

- `MemoryReadScope` / `MemoryWriteScope` carry the storage scope tuple (tenant +
  active `owner_org_unit_id` + owner subject). Data-class and source grants stay
  in the governed retrieval layer, while every predicate represented by the
  storage scope is enforced before ranking and limits in each backend.
- `MemoryDatabase` = current rusqlite + sqlite-vec compatibility adapter behind
  the trait.
- `PostgresMemoryStore` is the next concrete adapter (TAN-678), using
  `tokio-postgres` (+ `deadpool-postgres`) and `pgvector` against this completed
  operation contract and migration registry.

## Decision 2 — vector portability (TAN-660)

- `vec0` virtual table → **`pgvector` `vector(N)` column**. Embedding dimension
  (currently 384) and distance metric are **parameters of the store**, not baked
  into DDL.
- **Scope-aware top-k is contractual on both backends:** cross-scope vectors must
  never suppress in-scope results. This is subtle on Postgres: an **approximate**
  ANN index (HNSW/IVFFlat) applies a `WHERE` scope filter *after* the index scan,
  so a naive `WHERE <scope> ORDER BY embedding <=> $1 LIMIT $k` can return an ANN
  candidate set dominated by out-of-scope rows and **miss closer in-scope hits**
  for selective tenant/org/subject scopes (pgvector post-filtering behaviour).
  The Postgres backend MUST therefore use one of:
  1. **exact search for scoped queries** (no ANN index / disable index scan) —
     simplest, correct, acceptable while per-scope row counts are small;
  2. **per-scope partial or partitioned indexes** (e.g. partition by tenant) so
     the ANN index is already scope-bounded; or
  3. **iterative scan** (`hnsw.iterative_scan` / `ivfflat.iterative_scan`, pgvector
     ≥ 0.8) with a bounded max, which re-scans until enough in-scope rows are found.

  SQLite keeps today's per-tenant `vec0` scan. A shared contract test asserts, on
  **both** backends, that adding many closer out-of-scope vectors does not drop or
  reorder the in-scope top-k — this test gates the guarantee (it is not assumed).
- Vector ops live behind the `MemoryStore` trait (`search_similar_for_scope`,
  `upsert_embedding`, `delete_embeddings_for_scope`), never as raw `vec0` SQL in
  business logic.

## Decision 3 — file-based stores (TAN-661 tie-in)

Audit / policy-decision / org-unit records move behind the same store abstraction
so a Postgres deployment gets DB-backed tables while local keeps JSONL. The audit
hash-chain (`ProtectedAuditEnvelope.seq/prev_hash/record_hash`) is preserved by
making the Postgres table append-only with a monotonic `seq` per tenant.

The compatibility backend remains file-backed and preserves the existing on-disk
formats:

- protected audit: JSONL at `audit/protected_events.log.jsonl`
- memory audit: JSONL at `memory/audit.log.jsonl`
- policy decisions: JSON object at `governance/policy_decisions.json`
- enterprise org-unit registries: JSON objects at the existing `enterprise/*.json`
  paths

Existing deployments keep these files in place and the file backend reads them
without conversion. A future PostgreSQL backend should implement the same logical
operations from `GovernanceStoreFile` and migrate by importing the files once,
preserving audit row order and `ProtectedAuditEnvelope.seq/prev_hash/record_hash`
values exactly. Protected audit remains append-only; any DB implementation must
allocate the next sequence under the same append lock or transaction boundary and
verify the previous record hash before committing a new row.

## Migration strategy

- **Logical schema in one migration module:** ordered migrations describe
  portable tables, columns, defaults, backfills, and constraints. The SQLite
  ledger validates version/name identity and records executable migrations in
  the same transaction that applies them.
- SQLite versions 1-4 are explicitly marked as legacy bootstrap baselines while
  their existing DDL remains in the compatibility initializer. Version 5
  (`private_owner_subject_scope`) is translated and applied by the migration
  coordinator. Later migrations must be executable translations and may not be
  stamped without a translator.
- New M1 columns (`owner_org_unit_id`, `private`/`owner_subject`) and the envelope
  metadata land as portable columns through this module.
- Greenfield Postgres needs no dual-write; document an export/import path for
  moving an existing SQLite deployment to Postgres.

## Sequencing (recommended)

1. **TAN-659 / TAN-677** — complete the `MemoryStore` vocabulary and route
   memory business workflows through the SQLite adapter.
2. **TAN-645 / TAN-648 / TAN-679** — persist `owner_org_unit_id`, `private`, and
   `owner_subject`; enforce shared-or-owner visibility before ranking and limit.
3. **TAN-666** — envelope encryption + DEK cache expressed through the store.
4. **TAN-660 / TAN-661** — `PostgresMemoryStore` + `pgvector`, and the file-store
   backends, once the trait surface is stable.

Doing (1) first means every later column/crypto change is made **once** in the
trait's types and the migration module, not smeared across raw SQL call sites.
