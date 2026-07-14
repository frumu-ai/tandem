# Stateful Runtime Enterprise Scope

Document status: implemented stateful-runtime scope and storage contract.

Implementation review: 2026-07-14 against `origin/main` at `801559fd`.
This document describes repository behavior, not proof that a particular
deployment uses PostgreSQL, has completed migration verification, or has made
the runtime a non-bypassable system of record.

Tandem stateful workflow runs must persist enough scope metadata for every
replay, resume, wait, webhook, and audit read to be evaluated under the same
tenant and governance boundary that created the run.

## Durable Scope Invariants

- Every stateful run, event, and snapshot carries a `TenantContext`.
- Snapshot-backed runs also carry the workflow definition version and snapshot
  hash used to start the run. Explicit definition metadata wins; otherwise the
  runtime derives a stable version from plan ID/revision or the definition hash.
- Enterprise deployments must also preserve the owning organization unit,
  owner principal, resource scope, data classes, risk tier, policy version, and
  delegation grants whenever the caller or trigger provides them.
- Local implicit runs remain readable by the local implicit tenant for
  developer compatibility, but explicit tenant reads are filtered by
  organization, workspace, and deployment.
- Snapshots are stored under sanitized run directories so run identifiers cannot
  escape the stateful runtime root.

## Automation And Knowledge Boundaries

Automation and workflow run adapters preserve existing `TenantContext` values
instead of deriving scope from process-global state. Current automation,
webhook, orchestration, and goal-runtime paths reuse or project the stateful
scope rather than treating process-global state as authority. Memory, knowledge,
and connector subsystems have their own tenant/resource contracts, and the
stateful enterprise summary resolves relevant grants and source bindings. Any
integration that invokes those subsystems during resume or replay must project
the saved `StatefulRuntimeScope` rather than derive fresh, wider authority or add
parallel ad hoc scope fields.

The canonical stateful runtime run list and detail endpoints expose a top-level
`enterprise_scope` summary beside each `run`. The summary keeps the durable scope
fields visible and resolves matching organization units, active org-unit grants,
and enabled knowledge source bindings within the same tenant/resource boundary.
List callers can filter by organization unit, owner principal, root resource,
policy version, data class, risk tier, delegation grant, and source binding.
Delegation grant filters and summaries resolve against active org-unit grants in
the same tenant/resource scope; stale stored grant IDs remain visible as scope
metadata but are not presented as active authority.

Knowledge reads and writes performed during a resumed run should evaluate the
saved `resource_scope`, `data_classes`, `policy_version_id`, and
`delegation_grant_ids` from the durable run scope. This keeps replayed work from
silently widening access if organization membership, connector bindings, or
memory policy defaults change after the run was first scheduled.

## Definition Identity

Stateful automation adapters derive a `sha256:` snapshot hash from the persisted
`automation_snapshot` and preserve a matching definition version on the
canonical run record. Published orchestration references require pinned
definition hashes. Governed handoff validation compares the immutable downstream
automation snapshot with the pinned hash and fails closed when a target workflow
is stale, requiring revalidation and republication. This is current enforcement,
not only metadata reserved for a future replay path.

Automation V2 lifecycle boundaries are projected into the authoritative
stateful runtime event log. The projection uses deterministic event IDs based
on the run and lifecycle index, so repeated writes are idempotent while per-run
sequences remain monotonic. Each projected boundary also writes a redacted
summary snapshot with checkpoint node IDs, attempts, gate summary, execution
claim metadata, a stable checkpoint digest, and the workflow definition
version/hash. Raw node outputs stay out of these snapshots; consumers that need
full payloads should follow the referenced Automation V2 run or future
payload-pointer APIs under the same tenant boundary.

## Authoritative Storage Backends

The transactional orchestration store is authoritative for stateful runs,
events, snapshots, waits, goals, handoffs, outbox effects, dead letters,
compensations, and tool effects after initialization. SQLite is the default
local backend. Builds with PostgreSQL support may select it with
`TANDEM_STORAGE_BACKEND=postgres` and `TANDEM_STORAGE_POSTGRES_URL`; invalid
selection or a missing URL fails startup rather than falling back silently.

The PostgreSQL backend uses a backend-specific schema and advisory engine lock.
SQLite and PostgreSQL share backend-conformance scenarios for the core store
contract. An offline, verified storage-transfer path supports backend migration
and records transfer state; operators must complete and verify the transfer
before starting against the target backend. Backend support does not by itself
prove backup, restore, replication, failover, or hosted operational maturity.

The memory store has its own independent SQLite/PostgreSQL selection and
migration procedure. Stateful storage configuration must not be assumed to move
memory records automatically.

## Reliability And Effect Records

External actions are projected into durable outbox and tool-effect records with
stable identities. Failures can produce tenant-scoped dead-letter records and
compensation records; retry and compensation execution preserve the governing
scope. Retention and cleanup are snapshot-aware so required recovery state is
not treated as disposable event history.

These records provide runtime recovery and evidence primitives. They are not a
claim that every external provider offers an idempotent API, that every action
has a valid compensation, or that all business effects are automatically
reversible.

## Durable Waits

Durable waits use the same `StatefulRuntimeScope` as runs, events, and
snapshots. Timer, webhook, approval, external-condition, and retry-backoff waits
must persist the run ID, wait ID, wait kind, phase, wake time, timeout policy,
event sequence, and wake idempotency key before execution is released. Wake
claiming is tenant-filtered and lease-bound so startup recovery can find missed
timer wakeups without allowing another tenant or concurrent scheduler worker to
resume the same wait twice. Wait identity is scoped to the tenant boundary, so
duplicate wait IDs in another organization, workspace, or deployment cannot
overwrite or shadow the visible wait. Claim and wake-completion operations
address waits by run ID and wait ID inside that tenant boundary.
