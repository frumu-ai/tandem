# Stateful Runtime Enterprise Scope

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
instead of deriving scope from process-global state. Future memory, knowledge,
connector, and webhook integrations should enrich `StatefulRuntimeScope` rather
than adding parallel ad hoc fields to each subsystem.

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
canonical run record. This lets future resume and replay paths compare the
definition that originally started a run against the current mutable workflow
definition before reclaiming leases, applying migrations, or executing effects.

Automation V2 lifecycle boundaries are projected into the authoritative
stateful runtime event log. The projection uses deterministic event IDs based
on the run and lifecycle index, so repeated writes are idempotent while per-run
sequences remain monotonic. Each projected boundary also writes a redacted
summary snapshot with checkpoint node IDs, attempts, gate summary, execution
claim metadata, a stable checkpoint digest, and the workflow definition
version/hash. Raw node outputs stay out of these snapshots; consumers that need
full payloads should follow the referenced Automation V2 run or future
payload-pointer APIs under the same tenant boundary.

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
