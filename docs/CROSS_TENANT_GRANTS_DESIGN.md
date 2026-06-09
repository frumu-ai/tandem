# Cross-Tenant Grants Design

Status: proposed design for TAN-16 / CT-11.

This document defines the first-class cross-tenant grant that lets one tenant
issue a signed, revocable authorization to a principal in another tenant. It is
the design input for CT-13 implementation and intentionally keeps local and
single-tenant behavior unchanged.

## Goals

- Represent a grant issued by tenant A to tenant B with explicit issuer,
  audience, subject principal, resource, permissions, data classes, validity,
  revocation, and audit attribution.
- Let the audience tenant look up active inbound grants without trusting request
  headers or unsigned local state.
- Project validated inbound grants into the existing strict tenant context
  machinery so protected reads and actions can continue to use `ScopedGrant`
  evaluation.
- Fail closed when the grant is missing, expired, revoked, untrusted, outside
  its key scope, outside its resource scope, or not applicable to the requesting
  principal.
- Preserve the existing intra-tenant authority graph as an intra-tenant graph.

## Non-Goals

- This design does not implement the storage tables, HTTP routes, or verifier.
- This design does not make every existing resource cross-tenant readable. CT-13
  must opt enforcement sites into inbound grant evaluation.
- This design does not allow local development headers to create cross-tenant
  access. Local and single-tenant modes remain no-ops.

## Current Primitives

`crates/tandem-enterprise-contract/src/lib.rs` already contains the reusable
authorization vocabulary:

- `TenantContext` identifies the request tenant.
- `PrincipalRef` identifies human users, agents, service accounts, and external
  delegates.
- `ResourceRef` identifies resources by organization, workspace, project, kind,
  and path prefix.
- `ResourceScope` describes allowed and denied resource boundaries.
- `ScopedGrant` is an effective grant for one principal/resource boundary with
  permissions, data classes, expiry, source, and optional delegation id.
- `StrictTenantContext` evaluates projected grants for protected runtime paths.
- `SigningKeyPurpose` scopes key use for context assertions, approvals,
  delegation projections, peer assertions, and break-glass admin assertions.

`crates/tandem-enterprise-contract/src/authority.rs` is explicitly
intra-tenant. It denies `resource_outside_tenant` before evaluating direct or
organization-unit grants. Cross-tenant grants therefore should not be pushed
into that graph as ordinary direct grants.

`crates/tandem-server/src/http/middleware.rs` verifies hosted/enterprise tenant
context assertions and enriches strict projections with organization-unit grants.
The cross-tenant hook belongs beside that enrichment step, after the caller has
a verified tenant context and before protected routes evaluate access.

## Decisions

### D-CT-2: Introduce `CrossTenantGrant`

Introduce a first-class `CrossTenantGrant` instead of extending `ScopedGrant`.

`ScopedGrant` should remain the effective authorization object used after trust
has already been established. A cross-tenant grant needs issuer, audience,
signature, key id, revocation status, source approval, and dual-tenant audit
metadata. Putting all of that into `ScopedGrant` would make every intra-tenant
grant carry cross-tenant trust semantics and would make it too easy to treat an
unverified imported grant as an effective local grant.

Implementation should add a cross-tenant envelope in the enterprise contract,
then add a projection function that converts a validated inbound grant into one
or more `ScopedGrant` values for `StrictTenantContext`.

### D-CT-3: Issuance and Revocation Surface

Issuance and revocation should be explicit governance operations owned by the
issuer tenant.

Recommended surfaces:

- `POST /enterprise/cross-tenant-grants`: create and sign a grant.
- `GET /enterprise/cross-tenant-grants`: list grants issued by the caller's
  tenant.
- `GET /enterprise/cross-tenant-grants/inbound`: list active inbound grants for
  the caller's tenant.
- `POST /enterprise/cross-tenant-grants/{grant_id}/revoke`: append a revocation
  tombstone.
- `GET /enterprise/cross-tenant-grants/revocations`: expose issuer revocation
  state to audience lookup and cache refresh paths.

Creation, renewal, suspension, and revocation are protected actions. They must
require a verified hosted/enterprise context, a policy decision, durable audit
evidence, and an attributed human or service principal. Revocation is never a
delete: it appends a signed tombstone and updates status to `revoked`.

### D-CT-4: Use Per-Tenant Grant-Signing Keys

Use per-tenant grant-signing keys instead of reusing tenant context assertion
keys.

Implementation should add a new signing purpose, tentatively
`SigningKeyPurpose::CrossTenantGrant`, with string aliases
`cross_tenant_grant` and `cross-tenant-grant`. The keyring metadata should
continue the current pattern from context assertion keys:

- active/inactive status
- issuer organization id
- deployment id
- allowed audiences
- allowed resource scope prefixes
- not-before and not-after windows

Separate keys give operators independent rotation, revocation, and blast-radius
control. They also make audit records clearer: a context assertion proves who is
calling Tandem now, while a cross-tenant grant proves tenant A intentionally
leased a scoped capability to tenant B.

## Representation

The contract should add a signed grant envelope and a stable payload. Field
names below are design names; CT-13 can refine Rust naming while preserving the
semantics.

```rust
pub struct CrossTenantGrant {
    pub header: CrossTenantGrantHeader,
    pub claims: CrossTenantGrantClaims,
    pub signature: String,
}

pub struct CrossTenantGrantRecord {
    pub grant: CrossTenantGrant,
    pub state: CrossTenantGrantState,
    pub revocation: Option<CrossTenantGrantRevocation>,
}

pub struct CrossTenantGrantHeader {
    pub alg: String, // "EdDSA"
    pub typ: String, // "tandem-cross-tenant-grant+jws"
    pub kid: String,
}

pub struct CrossTenantGrantClaims {
    pub version: String, // "v1"
    pub grant_id: String,
    pub issuer: TenantGrantParty,
    pub audience: TenantGrantParty,
    pub subject: PrincipalRef,
    pub resource_scope: ResourceScope,
    pub permissions: Vec<AccessPermission>,
    pub data_classes: Vec<DataClass>,
    pub tool_patterns: Vec<String>,
    pub issued_at_ms: u64,
    pub not_before_ms: u64,
    pub expires_at_ms: u64,
    pub issued_by: PrincipalRef,
    pub source_policy_decision_id: String,
    pub source_audit_event_id: String,
    pub approval_id: Option<String>,
}
```

`TenantGrantParty` should include at least organization id, workspace id, and
deployment id. `subject` is always a principal in the audience tenant.
`resource_scope` is always owned by the issuer tenant. `CrossTenantGrantRecord`
is the mutable storage/read model around the immutable signed grant. Its state
is derived from the signed grant plus revocation state and should support
`active`, `suspended`, `revoked`, and `expired`.

Cross-tenant grants are allow leases. Deny behavior comes from revocation,
suspension, audience-local policy, resource scope denial, and explicit deny
grants in the projected strict context. This keeps the cross-tenant format
focused on what tenant A intentionally shared.

The canonical signed form should match the existing context assertion shape:
base64url JWS header, base64url canonical JSON claims, and Ed25519 signature.
The verifier should expose the grant id, key id, issuer, audience, and canonical
claims hash for audit records.

## Projection

Validated inbound grants project into `StrictTenantContext` as effective
`ScopedGrant` values only after all trust checks pass.

Projection rules:

- `principal` is the audience principal from `claims.subject`.
- `resource` is the issuer-owned root resource or each explicitly allowed
  resource in `claims.resource_scope`.
- `permissions`, `data_classes`, and `tool_patterns` copy from the claims.
- `expires_at_ms` is the grant expiry.
- `grant_source` is a new `GrantSource::CrossTenantGrant`.
- `delegation_id` is the cross-tenant grant id.
- `source_principal` is `claims.issued_by`.

The projected strict resource scope must include the issuer-owned resource
scope. If the existing strict projection has only the audience tenant root,
implementation must merge in inbound cross-tenant allowed resources before
calling `StrictTenantContext::evaluate_access`; otherwise the current
`resource_outside_projected_scope` check will correctly deny.

Do not add inbound grants to `IntraTenantAuthorityGraph::direct_grants`. That
graph intentionally denies cross-tenant resources. A future helper can compose
two decisions instead:

- local authority graph for audience-local resources
- verified inbound grant projection for issuer-owned resources

## Issuance Flow

1. The issuer tenant calls the create route with a verified hosted/enterprise
   context.
2. The server checks that the issuer principal can delegate the requested
   resource, permissions, data classes, and tool patterns.
3. High-risk grants require approval before signing.
4. The server writes a source-tenant policy decision and protected audit event.
5. The server signs the canonical grant with the issuer tenant's
   cross-tenant-grant key.
6. The grant is stored in issuer state and published to the inbound lookup index
   keyed by audience tenant and subject principal.

The issuer may grant at organization, workspace, project, collection, document,
connector, MCP server/tool, artifact, run, or audit export scope only when the
issuer policy allows delegation at that scope.

## Revocation Flow

Revocation must be immediate and fail closed for new enforcement decisions.

1. The issuer tenant calls the revoke route with a verified hosted/enterprise
   context.
2. The server checks that the caller can revoke the grant.
3. The server appends a signed revocation tombstone with reason, revoked by,
   revoked at, source policy decision id, and source audit event id.
4. Audience lookup treats the grant as revoked once the tombstone is visible.
5. Audience-side caches must expire quickly and must revalidate revocation state
   before consequential protected actions.

If revocation lookup is unavailable, hosted/enterprise enforcement denies the
grant. Local mode does not attempt lookup.

## Trust Root

The audience tenant trusts a grant only when all checks pass:

- Header uses `alg = "EdDSA"` and `typ = "tandem-cross-tenant-grant+jws"`.
- Key id resolves in the trusted issuer keyring.
- Key purpose is `CrossTenantGrant`.
- Key status is active and within validity windows.
- Key issuer organization and deployment match the claims issuer.
- The audience tenant matches the current verified tenant context.
- The audience is allowed by key metadata.
- Every resource prefix in the claims is allowed by key metadata.
- The grant is not expired, not before its start time, not revoked, and not
  suspended.
- The subject principal matches the current strict principal exactly.

Context assertion keys must not verify cross-tenant grants. Cross-tenant grant
keys must not verify request context assertions.

## Inbound Lookup

Inbound lookup is keyed by audience tenant first, then subject principal. A
grant must not be discoverable by unrelated tenants or principals.

Recommended lookup key:

```text
audience_org_id / audience_workspace_id / audience_deployment_id / subject_kind / subject_id
```

The lookup result should return signed grants plus revocation summaries, not
pre-trusted `ScopedGrant` values. The audience side verifier performs the trust
checks locally and only then projects grants into `StrictTenantContext`.

The lookup path should support pagination, freshness metadata, and a
fail-closed cache policy:

- reads may use a short-lived positive cache
- revoked and suspended tombstones should invalidate positive cache entries
- verifier errors produce deny/not-applicable decisions, not partial access

## Enforcement Points

Primary middleware hook:

1. `resolve_enterprise_request_context_for_mode` verifies the caller's tenant
   context assertion in hosted/enterprise mode.
2. `enrich_verified_context_with_org_unit_grants` keeps adding intra-tenant
   organization-unit grants.
3. CT-13 adds `enrich_verified_context_with_inbound_cross_tenant_grants` after
   the org-unit step.
4. The enrichment hook looks up inbound grants for the verified tenant and
   strict principal, verifies them, merges issuer-owned allowed resources into
   strict scope, and appends projected `ScopedGrant`s.

Protected runtime paths should keep using `StrictTenantContext::evaluate_access`
where possible. Routes that currently call the intra-tenant authority graph must
continue to use it for local tenant resources and must call the inbound grant
projection for issuer-owned resources.

Initial enforcement targets for CT-13:

- memory retrieval and source binding reads
- MCP inventory/tool discovery and execution policy
- workflow artifact reads and governance evidence exports
- audit export reads when tenant A explicitly shares evidence with tenant B
- connector/source object reads that carry customer, regulated, credential, or
  source-code data classes

Every consequential allow or deny must include grant id, issuer tenant, audience
tenant, subject principal, resource, permission, data class, decision reason, and
assertion/grant key id in policy and audit records.

## Failure Modes

- Missing strict projection: deny.
- Missing inbound grant: deny or not applicable.
- Unknown key id: deny.
- Wrong key purpose: deny.
- Expired grant or assertion: deny.
- Revoked or suspended grant: deny.
- Audience mismatch: deny.
- Subject mismatch: deny.
- Resource outside projected scope: deny/not applicable.
- Revocation lookup unavailable in hosted/enterprise mode: deny.
- Local/single-tenant mode: no inbound lookup and no cross-tenant projection.

## CT-13 Implementation Checklist

- Add `CrossTenantGrant` contract types and `SigningKeyPurpose::CrossTenantGrant`
  to `crates/tandem-enterprise-contract/src/lib.rs`.
- Add `GrantSource::CrossTenantGrant` for projected effective grants.
- Add grant header validation, verifier, key metadata checks, and tests modeled
  on the tenant context assertion verifier.
- Add issuer-owned create/list/revoke routes and inbound audience lookup routes.
- Store signed grants and signed revocation tombstones with durable audit ids.
- Add middleware enrichment after verified context resolution.
- Merge inbound resource scopes into strict projections before access
  evaluation.
- Keep `IntraTenantAuthorityGraph` intra-tenant; do not weaken its
  `resource_outside_tenant` denial.
- Add negative tests for wrong audience, wrong subject, wrong key purpose,
  expired grant, revoked grant, unavailable revocation lookup, and local-mode
  no-op.
