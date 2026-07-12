# PostgreSQL stateful storage backend

The stateful orchestration store (goals, handoffs, runs, durable events,
snapshots, waits, reliability records, and the tool-replay ledger) runs on a
pluggable execution backend (TAN-714). SQLite remains the default for local
and desktop installs; PostgreSQL is opt-in for deployments that want the
runtime's transactional state in a managed database (TAN-715).

## Configuration

| Variable | Purpose |
| --- | --- |
| `TANDEM_STORAGE_BACKEND` | `sqlite` (default) or `postgres`. Fail-closed: any other value is a startup error, never a silent SQLite fallback. |
| `TANDEM_STORAGE_POSTGRES_URL` | PostgreSQL connection URL. Required when the backend is `postgres`. |

Builds gate the backends with cargo features: `storage-sqlite` (default-on)
and `storage-postgres` (enabled for builds that should offer PostgreSQL).
Selecting a backend the build does not include is a startup error.

## How it maps onto PostgreSQL

- **One schema per runtime root.** Each runtime root records its schema name
  in a sticky `stateful_runtime.pg_schema` marker file (derived from the root
  path on first open; operators may pre-seed the marker to pick a name).
  Distinct roots stay isolated inside a shared database.
- **Dialect translation, not duplicated SQL.** Store statements are authored
  once in the SQLite dialect; the backend rewrites `?N` placeholders to `$N`
  and null-safe `IS ?` comparisons to `IS NOT DISTINCT FROM $N` at execution
  time. DDL is PostgreSQL-native (schema v5), with `BIGSERIAL` `rowid`
  columns standing in for SQLite's implicit rowid so durable SSE
  `Last-Event-ID` cursors stay monotonic.
- **Engine lock.** The file-based engine lock still fences engines on one
  host; on PostgreSQL an additional session-level advisory lock (keyed to the
  runtime root's schema) fences engines on other hosts sharing the database.
- **Encryption is unchanged.** Protected records (KMS-sealed payloads and the
  tool-replay ledger) are sealed above the storage layer, so rows are the
  same ciphertext on either backend.
- **Connections.** Pooled per (URL, schema) process-wide. TLS follows the
  memory backend's current convention (`NoTls`); place the database on a
  trusted network segment or a TLS-terminating proxy.

## Testing

`backend_conformance_tests` runs the same store scenarios against every
compiled backend: always on SQLite, and on PostgreSQL when
`TANDEM_TEST_POSTGRES_URL` is set (CI job `test-postgres-storage`, mirroring
`test-postgres-memory`). The scenarios cover the dialect-sensitive surface:
`ON CONFLICT` upserts, tenant scoping, rowid cursors, `INSERT .. RETURNING`,
retention's correlated subqueries, and engine-lock exclusivity.

## Scope and migration notes

- Backend selection does not copy data. New PostgreSQL deployments start at
  the current schema version directly; the SQLite v1→v5 migration chain is
  historical and SQLite-only. Moving an existing root between backends is
  future migration tooling (TAN-716).
- Two stores are not yet behind this contract: the session/questions
  repository in `tandem-core` (`session_repository.rs`) and the server's
  runtime event store (`runtime_event_store.rs`). Both still use SQLite
  directly, so fully SQLite-free binaries remain follow-up work (tracked in
  the TAN-714 scope notes).
