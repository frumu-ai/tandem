# Storage Portability & PostgreSQL Readiness — Design (TAN-659, TAN-660)

Design decision for making the memory subsystem migratable to PostgreSQL:
the backend abstraction (**TAN-659**) and the vector-store portability path
(**TAN-660**). Feeds `TAN-661` (file-based audit/policy/org-unit stores) and the
M1 schema work (`TAN-645` `owner_org_unit_id`, `TAN-648` `private`, `TAN-666`
envelope encryption).

## Current state (grounded)

- **Memory DB:** `rusqlite` 0.32 (bundled SQLite), raw SQL strings throughout
  `crates/tandem-memory/src/memory_database_impl_parts/*` and `db.rs`. Opened once
  at `crates/tandem-server/src/http.rs:496`. No storage abstraction.
- **Vectors:** `sqlite-vec` 0.1.7 — `vec0` **virtual tables**
  (`part01_a.rs:220,315,543`), a **loaded SQLite extension**. KNN via per-tenant
  top-k scans (`search_similar_for_tenant`, `part01_a.rs:1484`) that push the
  tenant/subject scope into the `WHERE` so another scope's vectors can't suppress
  candidates.
- **Audit / policy / org-units:** flat JSON/JSONL files (`audit.rs:224`,
  `config/paths.rs`) — see `TAN-661`.
- **Workspace has no SQL abstraction dependency today:** no `sqlx`,
  `tokio-postgres`, `diesel`, `sea-orm`, or `pgvector` anywhere in `Cargo.lock`.
  The memory crate already depends on `async-trait` and `tokio`.

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

### Shape (illustrative, not final)

```rust
#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn put_chunk(&self, scope: &MemoryWriteScope, chunk: &MemoryChunk) -> MemoryResult<()>;
    async fn search_similar_for_scope(
        &self, scope: &MemoryReadScope, embedding: &[f32], limit: usize,
    ) -> MemoryResult<Vec<MemoryChunkHit>>;
    async fn search_records_for_tenant(
        &self, scope: &MemoryReadScope, query: &str, limit: i64, /* tags */
    ) -> MemoryResult<Vec<GlobalMemorySearchHit>>;
    // … put_record, list, promote/demote, cleanup, knowledge spaces …
}
```

- `MemoryReadScope` / `MemoryWriteScope` carry the full scope tuple
  (tenant + `owner_org_unit_id` + `subject`/`private` + data_class/source) so the
  scope contract is uniform across backends. Enforcement stays in the query, per
  backend.
- `SqliteMemoryStore` = current rusqlite + sqlite-vec, refactored behind the trait.
- `PostgresMemoryStore` = `tokio-postgres` (+ `deadpool-postgres` pool) + `pgvector`.

## Decision 2 — vector portability (TAN-660)

- `vec0` virtual table → **`pgvector` `vector(N)` column** with an ANN index
  (HNSW default; IVFFlat as an alternative). Embedding dimension (currently 384)
  and distance metric are **parameters of the store**, not baked into DDL.
- **Scope-aware top-k is contractual on both backends:** the scope predicate
  (tenant + org_unit + subject) must be applied *in the same query* as the ANN
  search, never a global top-k followed by a filter. Postgres:
  `... WHERE <scope predicate> ORDER BY embedding <=> $1 LIMIT $k`. SQLite keeps
  today's per-tenant `vec0` scan. A shared contract test asserts no cross-scope
  candidate can suppress an in-scope one on either backend.
- Vector ops live behind the `MemoryStore` trait (`search_similar_for_scope`,
  `upsert_embedding`, `delete_embeddings_for_scope`), never as raw `vec0` SQL in
  business logic.

## Decision 3 — file-based stores (TAN-661 tie-in)

Audit / policy-decision / org-unit records move behind the same store abstraction
so a Postgres deployment gets DB-backed tables while local keeps JSONL. The audit
hash-chain (`ProtectedAuditEnvelope.seq/prev_hash/record_hash`) is preserved by
making the Postgres table append-only with a monotonic `seq` per tenant.

## Migration strategy

- **DDL in one migration module**, additive and nullable columns, a schema-version
  table. No SQLite-only column types/pragmas in new tables.
- New M1 columns (`owner_org_unit_id`, `private`/`owner_subject`) and the envelope
  metadata land as portable columns through this module.
- Greenfield Postgres needs no dual-write; document an export/import path for
  moving an existing SQLite deployment to Postgres.

## Sequencing (recommended)

1. **TAN-659** — introduce `MemoryStore` trait + `SqliteMemoryStore` (behavior-
   preserving); route `MemoryDatabase` call sites through it. Ships with the
   existing test suite green (no behavior change).
2. **TAN-645 / TAN-648** — add `owner_org_unit_id` + `private` as scope fields on
   the trait's data types and the SQLite schema (via the migration module).
3. **TAN-666** — envelope encryption + DEK cache expressed through the store.
4. **TAN-660 / TAN-661** — `PostgresMemoryStore` + `pgvector`, and the file-store
   backends, once the trait surface is stable.

Doing (1) first means every later column/crypto change is made **once** in the
trait's types and the migration module, not smeared across raw SQL call sites.
