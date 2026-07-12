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

## Moving a stateful runtime

Run the backend transfer only with the engine stopped. The command obtains the
source engine lock before it reads state, so a live engine is a hard failure,
not a race with the migrator.

```bash
# Existing local state to the PostgreSQL schema associated with this root.
tandem-engine storage migrate \
  --from sqlite --to postgres \
  --state-dir /srv/tandem \
  --target-postgres-url "$TANDEM_STORAGE_TARGET_POSTGRES_URL" \
  --json

# Move PostgreSQL state back into an empty local runtime root.
tandem-engine storage migrate \
  --from postgres --to sqlite \
  --state-dir /srv/tandem-postgres \
  --target-state-dir /srv/tandem-local \
  --source-postgres-url "$TANDEM_STORAGE_SOURCE_POSTGRES_URL" \
  --json
```

The transfer copies every table in the stateful orchestration contract,
including definitions, runs, goals, handoffs, waits, snapshots, reliability
records, tool-ledger rows, migration records, and durable event cursors. It
uses bounded batches, preserves sealed payloads without decrypting them, and
inserts SQLite `rowid` cursor values explicitly into PostgreSQL. PostgreSQL
sequences are then advanced before the target becomes usable. The verification
digest is logical rather than page-based, so SQLite and PostgreSQL column order
and generated internal IDs do not affect the evidence.

Before the import the target records a durable `in_progress` transfer journal.
Startup against that target fails closed. The command fingerprints the locked
source and imported target with typed, length-delimited values; it marks the
target complete only when both fingerprint and record count match. Re-running
the same command is idempotent after a completed transfer and resumes an
interrupted staged target. A different source fingerprint or a populated target
that does not match the locked source is refused rather than overwritten. The
source is not changed by the transfer.

The JSON report is the migration evidence: retain its source and target
fingerprints with the maintenance record before changing
`TANDEM_STORAGE_BACKEND`.

## Scope and maintenance notes

- The backend transfer covers the stateful orchestration contract selected by
  `TANDEM_STORAGE_BACKEND`. New PostgreSQL deployments still initialize their
  schema directly; the SQLite v1→v5 chain remains historical and SQLite-only.
- The session/questions repository in `tandem-core`
  (`session_repository.rs`) and the server's runtime event store
  (`runtime_event_store.rs`) remain SQLite implementations. When
  `--target-state-dir` selects a different runtime root, the migration takes a
  verified SQLite snapshot of both authoritative databases so sessions,
  messages, questions, and runtime events move with the deployment. When the
  source and target roots are the same, those files remain in place while only
  the stateful orchestration contract moves to PostgreSQL.
- SQLite retention uses short delete transactions followed by a passive WAL
  checkpoint. PostgreSQL retention uses deletes too; keep autovacuum enabled
  and investigate sustained table bloat before raising retention windows.
- Back up SQLite only while the engine is stopped or through a SQLite-safe
  snapshot. Back up PostgreSQL with a consistent database backup that includes
  the runtime root's schema. The `stateful_runtime.pg_schema` marker is part of
  that root and should be retained with the deployment configuration.
