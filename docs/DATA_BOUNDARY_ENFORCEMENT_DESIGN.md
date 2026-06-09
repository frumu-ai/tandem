# Default DataBoundary Enforcement Design

Status: proposed design for TAN-17 / CT-12.

This document defines the default data-class boundary policy for governed reads.
It complements the cross-tenant grant design by making data classes apply even
when a caller did not receive an explicit `DataBoundary` in the strict
projection. Local single-tenant behavior remains unchanged.

## Goals

- Define the default `DataBoundary` used by verified, multi-tenant, or
  grant-backed reads.
- Define the exact trigger that turns a read from local/no-op mode into governed
  enforcement mode.
- Make governed memory and source-bound reads fail closed when strict
  authorization context or data-class evidence is missing.
- Keep existing local/single-tenant memory behavior unchanged when no verified
  or granted context is present.
- Preserve the existing `ScopedGrant` data-class semantics: grants must name the
  data classes they authorize.

## Non-Goals

- This design does not implement the helper types or update the memory filters.
- This design does not change `DataClass` taxonomy.
- This design does not make local desktop memory require enterprise assertions.
- This design does not replace cross-tenant grants; it defines the data-boundary
  behavior those grants project into.

## Current Behavior

`crates/tandem-enterprise-contract/src/lib.rs` already has:

- `DataClass` values such as `Public`, `Internal`, `Confidential`,
  `Restricted`, `CustomerData`, `SourceCode`, `FinancialRecord`, `Credential`,
  `Regulated`, and `Executive`.
- `DataBoundary`, where an empty `allowed_data_classes` list currently means
  unrestricted unless a class appears in `denied_data_classes`.
- `ScopedGrant::allows_data_class`, which is explicit: a grant allows a class
  only when that class is present in `grant.data_classes`.
- `StrictTenantContext::evaluate_access`, which denies first when
  `data_boundary` rejects a class, then requires a matching allow grant.

`crates/tandem-memory/src/types.rs` has `MemoryAccessFilter`, which wraps
`StrictTenantContext` and evaluates source-bound chunks before ranking.

`crates/tandem-memory/src/manager_parts/part01.rs` and
`crates/tandem-server/src/http/skills_memory_parts/part03.rs` /
`part04.rs` currently apply source filters only when a strict context is present.
Source-bound memory is hidden without a strict context, but ordinary chunks and
global memory rows without source-binding metadata can still be visible.

That behavior is right for local development. It is too permissive once a read
is verified, multi-tenant, or grant-backed.

## Decision

### D-CT-5: Default Boundary Policy and Trigger

Use a separate governed default boundary instead of changing
`DataBoundary::default()`.

`DataBoundary::default()` should remain `unrestricted()` for backwards
compatibility with local and legacy constructors. CT-12 should add an explicit
helper, tentatively:

```rust
impl DataBoundary {
    pub fn governed_default() -> Self {
        Self::allow(vec![DataClass::Public, DataClass::Internal])
    }
}
```

The governed default allows only `Public` and `Internal`. Higher-risk classes
require an explicit boundary projected from a verified tenant assertion,
cross-tenant grant, retrieval grant, policy decision, or equivalent enterprise
authorization source.

The trigger should be modeled as an explicit mode, tentatively:

```rust
pub enum GovernedReadMode {
    LocalNoop,
    GovernedStrict,
}
```

`GovernedStrict` applies when a protected read path is executing and any of the
following is true:

- runtime auth mode is `HostedSingleTenant` or `EnterpriseRequired`
- a `VerifiedTenantContext` is present
- a `StrictTenantContext` is present
- a cross-tenant grant, memory retrieval grant, policy token, or other
  grant-backed access artifact is present

`LocalNoop` applies only when runtime auth mode is `LocalSingleTenant`, no
verified context is present, no strict projection is present, and no grant-backed
access artifact is present.

When `GovernedStrict` is active, a governed read must have a strict projection.
If no strict projection is available, the read denies or filters out the
candidate with reason `missing_strict_projection`.

## Boundary Resolution

For a strict projection, resolve the effective boundary in this order:

1. Use the explicit `StrictTenantContext.data_boundary` when it is not
   unrestricted.
2. Use a boundary projected from the active cross-tenant grant or memory
   retrieval grant when present.
3. Use `DataBoundary::governed_default()`.

This keeps local constructors compatible while preventing hosted or grant-backed
reads from treating absent boundary metadata as unrestricted access.

Implementation should add a helper such as:

```rust
pub fn effective_data_boundary_for_governed_read(
    strict_context: &StrictTenantContext,
    mode: GovernedReadMode,
) -> DataBoundary
```

The helper should return the strict context boundary in local/no-op mode and the
governed default when strict context carries an unrestricted boundary in
governed mode.

## Target Normalization

Governed enforcement needs every candidate to become a read target with a
resource and data class.

Suggested normalized shape:

```rust
pub struct GovernedReadTarget {
    pub resource_ref: ResourceRef,
    pub data_class: DataClass,
    pub source_binding_id: Option<String>,
    pub source_object_id: Option<String>,
    pub evidence: GovernedReadEvidence,
}
```

`GovernedReadEvidence` should record where the class and resource came from:
source binding metadata, global memory classification, synthetic memory-space
resource, artifact metadata, or cross-tenant projection. Audit and debug output
should include this evidence when a consequential read is denied or allowed.

### Memory Chunk Targets

For `MemoryChunk` candidates:

- If `enterprise_source_binding.resource_ref` and
  `enterprise_source_binding.data_class` exist, use them exactly.
- If the chunk is ordinary tenant-local memory and has no enterprise source
  binding, synthesize a `ResourceRef` with `ResourceKind::MemorySpace`, the
  chunk tenant scope, project id when present, and a resource id derived from
  memory tier plus project/session id.
- If ordinary tenant-local memory has no classification metadata, treat it as
  `DataClass::Internal` for legacy compatibility.
- If source-bound or connector-sourced memory is missing resource or data-class
  metadata under `GovernedStrict`, deny with reason `missing_data_class` or
  `missing_resource_ref`.

### Global/Governed Memory Targets

For governed/global memory rows:

- Use source-binding metadata when present.
- Otherwise map `metadata.classification` to a `DataClass`.
- Existing labels map as:
  - `public` -> `DataClass::Public`
  - `internal` or absent -> `DataClass::Internal`
  - `confidential` -> `DataClass::Confidential`
  - `restricted` -> `DataClass::Restricted`
- Add mappings during implementation for `customer_data`, `source_code`,
  `financial_record`, `credential`, `regulated`, and `executive`.
- Synthesize a `ResourceKind::MemorySpace` resource when no source resource is
  present.

The current HTTP helper defaults missing classification to `internal`; CT-12 can
keep that default for ordinary tenant-local memory, but source-bound and
connector-sourced rows must provide explicit source metadata in governed mode.

### Artifact and Export Targets

Artifact export already excludes governed artifacts when strict projection is
missing. CT-12 should align memory with that pattern:

- no resource/data-class target in local mode: include as today
- no resource/data-class target in governed mode: deny or omit with an explicit
  reason
- target present and strict evaluation denies: deny or omit with the evaluation
  reason

## Enforcement Flow

For each protected read path:

1. Resolve `GovernedReadMode` from runtime auth mode, verified context, strict
   projection, and grant-backed access artifacts.
2. If mode is `LocalNoop`, preserve current behavior.
3. If mode is `GovernedStrict`, require a strict projection.
4. Resolve the effective data boundary.
5. Normalize each candidate into a `GovernedReadTarget`.
6. Evaluate the target with `StrictTenantContext::evaluate_access`.
7. Include only `AccessDecision::Allow`.
8. Attribute denial reasons in policy/audit/debug metadata where the read is
   consequential.

The read path should filter candidates before ranking or response shaping. For
search, increase the candidate limit while filtering so authorized results are
not starved by unauthorized results.

## Memory Implementation Hooks

Primary hooks for CT-12:

- `MemoryAccessFilter` in `crates/tandem-memory/src/types.rs` should carry the
  governed read mode and effective boundary behavior.
- `memory_chunk_visible_to_access_filter` in
  `crates/tandem-memory/src/manager_parts/part01.rs` should stop returning
  `true` for missing targets when mode is `GovernedStrict`.
- `global_memory_record_visible_to_access_filter` in
  `crates/tandem-server/src/http/skills_memory_parts/part04.rs` should share the
  same target normalization and missing-metadata behavior.
- HTTP list/search surfaces should construct `GovernedReadMode` from runtime
  auth mode and verified tenant context instead of using `Option<MemoryAccessFilter>`
  as the only trigger.
- Memory retrieval gateway paths should always use `GovernedStrict`, even in a
  local process, because the presence of the gateway is a grant-backed access
  artifact.

Avoid duplicating classification parsing in each route. Prefer one helper in
`tandem-memory` or a shared server module that maps memory metadata into
`GovernedReadTarget`.

## Local Safety

Local single-tenant behavior remains unchanged when all are true:

- runtime auth mode is `LocalSingleTenant`
- no `VerifiedTenantContext` is attached
- no `StrictTenantContext` is attached
- no retrieval gateway, cross-tenant grant, policy token, or equivalent grant is
  attached

In that mode:

- ordinary local memory search/list/retrieve keeps current visibility
- legacy chunks without classification remain visible
- no enterprise assertion is required
- no source-bound data is newly exposed across tenants, because there is only
  the local implicit tenant

This local no-op rule is explicit so desktop and OSS workflows do not silently
become enterprise-only.

## Failure Modes

In `GovernedStrict` mode:

- missing strict projection: deny/filter with `missing_strict_projection`
- expired strict assertion: deny/filter with `context_expired`
- missing target resource for source-bound data: deny/filter with
  `missing_resource_ref`
- missing target data class for source-bound data: deny/filter with
  `missing_data_class`
- data class outside effective boundary: deny/filter with
  `data_class_denied_by_boundary`
- resource outside projected scope: deny/filter with
  `resource_outside_projected_scope`
- no matching allow grant: deny/filter with `no_matching_allow_grant`
- matching deny grant: deny/filter with `matching_deny_grant`

For list and search endpoints, filtering is acceptable when the caller should
not learn that a record exists. For consequential exports or internal policy
checks, return or audit the explicit denial reason.

## Audit and Attribution

Governed read audit records should include:

- tenant context
- actor or principal
- resource ref
- data class
- effective boundary source: explicit assertion, projected grant, retrieval
  grant, or governed default
- grant id or policy id when present
- decision reason
- source binding id or source object id when present

Bulk search/list responses should avoid leaking filtered record ids to the
caller, but internal protected audit may count filtered candidates by reason.

## CT-12 Implementation Checklist

- Add a governed default boundary helper in
  `crates/tandem-enterprise-contract/src/lib.rs` without changing
  `DataBoundary::default()`.
- Add a governed read mode/effective boundary helper near strict context or in a
  memory-facing contract module.
- Normalize memory chunks and global memory rows into `GovernedReadTarget`.
- Make memory filters deny missing strict projection and missing source-bound
  data-class/resource metadata in governed mode.
- Keep local/no-verified/no-grant paths no-op.
- Add tests for local no-op, hosted missing strict projection, default boundary
  allowing internal memory, default boundary denying financial/source-code data,
  explicit boundary allowing restricted data, and source-bound missing metadata
  denial.
