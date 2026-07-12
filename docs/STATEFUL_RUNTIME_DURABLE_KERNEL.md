# Stateful Runtime Durable Kernel

Tandem's stateful runtime currently uses JSON and JSONL files as the durable
kernel for workflow events, lifecycle projections, snapshots, waits, and
reliability records. That keeps local development and support inspection simple,
but it also means concurrency and retention must be designed explicitly.

This note records the storage direction behind the TAN-507 through TAN-511
hardening work.

## Current Constraints

- Definition snapshot hashes are derived metadata. Recovery must prefer the
  persisted workflow snapshot when it exists, because a serializer or schema
  migration can change the derived hash without changing the user's intended
  run definition.
- Lifecycle projection must be append-oriented. Replaying the same automation
  history should not rewrite historical stateful snapshots with the run's latest
  status or checkpoint.
- Wait completion must honor active claims. A process that did not claim a wait
  cannot complete that wait while another claimant's lease is still active.
- List endpoints must avoid reparsing the full event log once per run row.
- JSON files do not provide cross-process compare-and-swap or advisory locking
  by themselves.

## Tactical Kernel

The JSON kernel remains acceptable for the current local/runtime profile when
the server process owns writes:

- event appends are idempotent by stable event id;
- lifecycle snapshots are written once per event id and are not overwritten on
  later projection passes;
- direct wait completion refuses active claims, while the claimed begin/finish
  path validates the claimant and lease metadata;
- list responses summarize the event log in one pass before rendering run rows;
- corrupt JSON stores are sidelined before mutation instead of being overwritten.

These constraints are intended as compatibility rails while the runtime remains
file-backed.

## Compatibility Sidecars

The first startup with the transactional orchestration store imports
`stateful_events.jsonl`, `stateful_snapshots/`, `stateful_waits.json`, and
`stateful_reliability.json` as read-only legacy sources. Once that import is
complete, SQLite is authoritative for all stateful reads, including when a
sidecar is stale or malformed.

New JSON/JSONL sidecar writes are off by default after import. Set
`TANDEM_STATEFUL_RUNTIME_COMPATIBILITY_MIRRORS_ENABLED=true` only when a legacy
diagnostic or integration needs dual writes temporarily. The setting does not
change reader authority, and it does not disable the pre-import legacy path.

## Embedded Store Evaluation

For multi-process or hosted deployments, Tandem should move the stateful runtime
kernel to an embedded transactional store. The leading candidate is SQLite with
WAL enabled because Tandem already ships SQLite for memory, it supports
cross-process readers and writers, and it gives us transactional uniqueness for
event ids, claim compare-and-swap, retention, and indexed list queries.

Recommended shape:

- `stateful_run_events(run_id, tenant_scope, seq, event_id unique, payload_json)`
- `stateful_run_snapshots(run_id, snapshot_id unique, seq, payload_json)`
- `stateful_waits(run_id, wait_id, tenant_scope, status, claim columns, payload_json)`
- `stateful_reliability(kind, id, tenant_scope, status, payload_json)`

Migration should be dual-read and single-write at first:

1. Open SQLite alongside the existing JSON files.
2. Backfill events, snapshots, waits, and reliability rows from JSON.
3. Write new stateful records to SQLite, keeping JSON export as a debug artifact
   until the hosted profile no longer needs it.
4. Add retention jobs that compact terminal waits and old event ranges only
   after a later snapshot is durable.

Until that migration lands, do not run multiple independent server processes
against the same file-backed runtime root.
