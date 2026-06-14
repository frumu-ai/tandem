# Connector OAuth & Control-Plane Ownership Decisions

Design decision record for connector control-plane ownership (EAA-13 / TAN-38).
Resolves the open decisions that gate enabling additional live connectors so
subsequent connector implementation issues (EAA-15 / TAN-40 and beyond) can be
created without ownership ambiguity.

These resolutions follow the trust model already established for hosted signing
keys (EAA-04 / TAN-29) and hosted context verification (EAA-05 / TAN-30): the
hosted control plane owns secrets and authority; the runtime holds only
references and enforces what the control plane asserts.

## D1 — Where admin roles live

**Decision: the shared contract defines the types, the hosted control plane is
the source of truth, and the runtime enforces.**

- Role/grant types — `OrganizationUnit`, `OrganizationUnitMembership`,
  `OrganizationUnitAccessGrant`, `PrincipalRef`, `ScopedGrant` — live in
  `tandem-enterprise-contract` so runtime and control plane share one model.
- Membership/role assignment authority lives in the hosted control plane
  (`OrganizationUnitMembershipSource::HostedControlPlane`), which provisions
  memberships and grants into the runtime's enterprise registries.
- The runtime enforces at request time via `StrictTenantContext::evaluate_access`
  and the middleware org-unit-grant projection. It never invents admin
  authority — it only enforces what the control plane asserts through signed
  context assertions (TAN-29/TAN-30). Connector admin actions (create connector,
  create/disable source binding, review quarantine) are gated by
  `require_enterprise_admin` against that verified context.

## D2 — Who owns connector OAuth

**Decision: the hosted control plane owns the OAuth flow and the secret
material; the runtime holds only references and resolves transient, redacted
bearer tokens.**

- Mirrors the TAN-29 signing-key model: long-lived/private secrets never live in
  the runtime.
- `ConnectorCredentialRef` (contract) carries a `SecretRef` (provider +
  `secret_id`), **not** the secret. The runtime resolves it through a
  `SecretResolver` into a transient `ResolvedBearerToken` (redacted `Debug`, no
  persistence) only for the duration of a provider request.
- `env://` is the local/dev resolver. Hosted deployments use a
  KMS/secret-manager-backed resolver (the reserved `google_kms` provider): the
  control plane performs the OAuth dance, stores refresh/access tokens in
  KMS/secret manager, and the runtime fetches only short-lived access tokens by
  reference.
- The runtime never persists OAuth tokens. Rotation/expiry are tracked on
  `ConnectorCredentialRef` (`rotated_at_ms` / `expires_at_ms`) and driven by the
  control plane.

## D3 — Connector credential scope

**Decision: per org + workspace + connector (the `ConnectorCredentialRef` key),
with optional per-source-binding narrowing via `source_bound_resource`. Not per
deployment.**

- Deployments are ephemeral runtimes; binding credentials to them would break on
  redeploy and fragment rotation. Tenancy (org/workspace) is the durable
  boundary, and `ConnectorCredentialRef` is already keyed by
  `org_id + workspace_id + connector_id + credential_id` and validated per
  tenant.
- A credential MAY be narrowed to a single resource subtree via
  `source_bound_resource` so a least-privilege credential can serve exactly one
  source binding when required.
- `credential_class` (`ReadOnly` / `ReadWrite` / `Admin`) bounds capability;
  ingestion connectors default to `ReadOnly`.

## D4 — Connector telemetry

**Decision: governance audit is always recorded; operational telemetry is
opt-in, off by default, and content-free.**

- Governance audit — ingestion jobs, quarantines, protected-action and
  access-decision audit — is mandatory and tenant-scoped; it is a governance
  record, not telemetry, and is always emitted.
- Operational/product telemetry (counts, latencies, provider error rates) is
  opt-in per deployment, defaults off, and must never include tenant content,
  resource bodies, or secrets. No connector telemetry sink exists today; when
  added it follows this rule (e.g. a `TANDEM_CONNECTOR_TELEMETRY` opt-in).

## D5 — Where source objects are stored

**Decision: source-object *lifecycle metadata* lives in the runtime's per-tenant
memory store; *raw provider content* is transient and never persisted as a
second copy; derived chunks live in the memory store scoped by source binding,
resource ref, and data class.**

- `SourceObjectLifecycleRecord` (tandem-memory DB) tracks per-tenant lifecycle
  (active / quarantined / tombstoned / deleted / rescoped) keyed by tenant scope
  + binding + native object id.
- Raw bytes are fetched to a temp dir during ingestion, indexed, then removed
  (current Google Drive path), so Tandem does not become a second
  system-of-record for provider content.
- Retrieval enforcement (`MemoryAccessFilter`, TAN-39) gates chunks by resource
  ref + data class + grants; quarantined/high-risk content is held out of
  retrieval until an admin review releases it.

## Implications for future connector issues

With the above resolved, each new connector (Notion, GitHub, Slack, Gmail —
EAA-15 / TAN-40) is a uniform unit of work:

1. Control-plane OAuth integration + a KMS-backed `SecretResolver` provider.
2. A per-tenant `ConnectorCredentialRef` (least-privilege, `ReadOnly` for
   ingestion), optionally `source_bound_resource`-narrowed.
3. An ACL classification in `provider_acl_sync_mode` — `Synced` only if the
   provider exposes reliable per-object ACLs, otherwise `AdminLabeled` (admin
   label + admin grants required); see `ENTERPRISE_CONNECTOR_ACL_POLICY.md`.
4. Source bindings + the standard ingestion admission / quarantine
   (`evaluate_ingestion_admission`), with high-risk data classes held for
   review.
5. Per-tenant source-object lifecycle tracking; transient raw content.

No connector implementation should reintroduce runtime-owned OAuth secrets,
deployment-scoped credentials, mandatory non-governance telemetry, or a
persistent raw-content store.
