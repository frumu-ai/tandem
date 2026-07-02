# Stateful Runtime Enterprise Threat Model

This document captures the enterprise threat model for Tandem's stateful agent runtime. It focuses on durable runs, automation webhooks, MCP/tool effects, knowledge scope, delegation grants, and replay/resume boundaries.

## Security Objectives

- Tenant, workspace, deployment, organization-unit, resource, data-class, policy, and delegation scope must persist with the run state that uses it.
- A resumed or replayed run must not gain broader authority because current defaults, memberships, connectors, or webhook metadata changed after scheduling.
- Runtime-owned policy, approvals, receipts, and protected audit evidence must describe who or what acted, under which tenant, and with which run-as principal.
- External effects must be observable as receipts, including MCP-only effects that do not pass through the legacy external-action bridge.
- Legacy unscoped records must be deterministically stamped or treated as unauthoritative when a scoped operation arrives.

## Trusted Boundaries

| Boundary | Trusted input | Fail-closed condition |
| --- | --- | --- |
| Tenant context | Signed context assertion in hosted/enterprise modes, local implicit tenant in local mode | Missing or invalid assertion where strict tenant policy is active |
| Automation definition | Persisted Automation V2 spec and canonical enterprise-scope metadata | Declared delegation grants are missing, expired, deny-effect, or outside scope |
| Runtime run record | Stored `StatefulRuntimeScope`, definition version, and snapshot hash | Scope metadata cannot be projected for scoped operations |
| Webhook trigger | Verified signature, tenant-bound trigger record, trigger enterprise scope | Scoped trigger targets an automation without matching enterprise scope |
| MCP execution | Runtime-selected connection, effective tenant context, run-as principal, policy preflight | Context assertion, phase authority, secret scope, or run-as validation fails |
| Receipts and audit | Protected audit JSONL plus stateful reliability receipts | Effect is executed without a receipt or acting principal context |

## Threats And Controls

| Threat | Control | Residual work |
| --- | --- | --- |
| Scoped webhook wakes legacy unscoped automation | Webhook delivery rejects scoped triggers when the automation has no canonical enterprise scope. | Existing customers may need migration notes for intentionally unscoped legacy webhooks. |
| Stale or forged delegation grant ID widens authority | Automation writes and run creation validate declared grant IDs against active allow grants in the same tenant, org unit, resource scope, data classes, and execute permission. Runtime explorer filters by active grants rather than stored labels. | Provider ACL sync remains control-plane work. |
| Legacy automation/run records lack canonical enterprise scope | Startup/load paths stamp definitions and run snapshots from existing resource/webhook metadata and persist upgraded hot state. | Cold archived run shards are normalized when read; a separate offline compaction command would make that fully eager. |
| MCP-only tool calls leave no stateful receipt | MCP run-as execution now writes `StatefulToolEffectRecord` receipts with run-as, connection, effective tenant, preflight decision, and redacted result metadata. | Dedicated receipt detail UI can make these easier to inspect. |
| Replay executes with a changed workflow definition | Stateful records carry workflow definition version and snapshot hash for comparison during resume/replay. | Stronger replay gates should block execution when hashes drift unless an operator accepts migration. |
| Knowledge retrieval crosses source or tenant boundary | Stateful scope stores resource scope, data classes, policy version, and delegation grant IDs; source-bound retrieval filters use tenant/resource/source metadata. | Continue expanding regression coverage for newly added retrieval surfaces. |
| Protected audit exists but cannot be correlated to effects | Tool-effect receipts include policy decision/context assertion IDs where available, and protected audit carries matching run-as context. | Evidence export should add first-class receipt bundles. |

## Engineering Invariants

- Do not treat `delegation_grant_ids` as mere display metadata. A declared grant ID is authority only when it resolves to an active allow grant that caps the runtime scope.
- Do not accept a scoped webhook delivery for an automation whose enterprise scope is absent.
- Do not emit an external side effect without one of: stateful tool-effect receipt, protected audit event, or both when the surface supports both.
- Do not derive replay authority from mutable current automation definitions alone; use the snapshot and durable scope saved with the run.
- Do not add new enterprise scope fields to subsystem-specific metadata without also projecting them into the canonical enterprise scope.

## Related Documents

- [Runtime trust boundaries](RUNTIME_TRUST_BOUNDARIES.md)
- [Stateful runtime enterprise scope](stateful-runtime-enterprise-scope.md)
- [Enterprise MCP identity and delegation](ENTERPRISE_MCP_IDENTITY_AND_DELEGATION.md)
- [Context assertion security](CONTEXT_ASSERTION_SECURITY.md)
- [Cross-tenant grants design](CROSS_TENANT_GRANTS_DESIGN.md)
