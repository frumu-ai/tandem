# Stateful Runtime Enterprise Scope

Tandem stateful workflow runs must persist enough scope metadata for every
replay, resume, wait, webhook, and audit read to be evaluated under the same
tenant and governance boundary that created the run.

## Durable Scope Invariants

- Every stateful run, event, and snapshot carries a `TenantContext`.
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

Knowledge reads and writes performed during a resumed run should evaluate the
saved `resource_scope`, `data_classes`, `policy_version_id`, and
`delegation_grant_ids` from the durable run scope. This keeps replayed work from
silently widening access if organization membership, connector bindings, or
memory policy defaults change after the run was first scheduled.
