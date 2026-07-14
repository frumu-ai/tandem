# Enterprise Connector Expansion Plan

Document status: roadmap; Google Drive is implemented, listed expansion
connectors are not.

Implementation review: 2026-07-14 against `origin/main` at `801559fd`.
An MCP connection to a provider is not evidence that its enterprise ingestion
connector, ACL classification, lifecycle tracking, or quarantine path exists.

Plan for the next ingestion connectors after Google Drive (EAA-15 / TAN-40).

Google Drive is the reference connector: it exercised source bindings,
ingestion admission, quarantine/review, the admin-labeled ACL fallback, and
per-tenant source-object lifecycle tracking end to end. This document records
**when** additional connectors may be added and **how** each one maps onto the
boundaries those mechanisms already enforce, so each new connector is a uniform
unit of work rather than a fresh design.

It builds on the two existing connector references:

- `ENTERPRISE_CONNECTOR_CONTROL_PLANE_DECISIONS.md` (EAA-13 / TAN-38) — who owns
  OAuth, credentials, telemetry, and source-object storage.
- `ENTERPRISE_CONNECTOR_ACL_POLICY.md` (EAA-14 / TAN-39) — provider ACL sync
  modes, the admin-labeled fallback, and ingestion admission.

## What Google Drive proved (entry criteria)

A new connector may be planned only after these boundaries are demonstrably
enforced by the shared path — all true today:

1. **Source binding scope.** Ingestion is keyed to an `EnterpriseSourceBinding`
   with a concrete `resource_ref: ResourceRef`, a `native_source_id`, and a
   `data_class` — not to a whole tenant. Drive binds a single folder
   (`native_source_id` = folder id) to a `DocumentCollection` resource.
2. **Fail-closed admission.** Every ingestion path routes through
   `evaluate_ingestion_admission(binding, connector, acl_mode, review_acknowledged)`
   (`tandem-enterprise-contract::source_acl`), which denies on identity/lifecycle
   mismatch, unsupported provider, or missing admin label, and quarantines on
   policy-required review or high-risk data class.
3. **ACL classification.** `provider_acl_sync_mode(provider)` classifies every
   provider; unknown providers are `Unsupported` (denied). Google Drive is
   `AdminLabeled` — it requires an admin `source_root_label` and access is
   governed by admin grants, not provider ACLs.
4. **Quarantine + review.** High-risk and `require_review` bindings index but
   immediately hold content (chunks removed, `SourceObjectLifecycleState::Quarantined`,
   an `EnterpriseIngestionQuarantine` opened) until an admin sets a disposition
   (`Release` / `Delete` / `Reindex`); reindex honors `acknowledge_review` only
   after a real release.
5. **Lifecycle tracking.** Each object becomes a `SourceObjectLifecycleRecord`
   in the per-tenant memory store (active / quarantined / tombstoned / deleted /
   rescoped); raw provider bytes are fetched to a temp dir, indexed, and removed.
6. **Credential indirection.** The runtime holds a `ConnectorCredentialRef`
   (`ReadOnly` for ingestion) and resolves a transient `ResolvedBearerToken`
   through a `SecretResolver`; secrets never persist in the runtime.

## Hard constraint: no broad workspace-level imports

Until resource/data-class authorization **and** audit posture are complete for a
provider, a connector MUST NOT offer a "import the whole workspace / drive /
account" binding. Imports must be scoped to a concrete subtree — a folder, a
repository (optionally a path subtree), a database, a channel, a label/query.
This is the intended posture for Drive (folder-scoped, never "all of Drive") and
is the policy that keeps every connector below least-privilege.

> **Not yet enforced by the runtime.** The source-binding creation path
> (`routes_enterprise.rs` ~line 292) only validates IDs, tenant match, and
> connector policy shape; the Google Drive ingestion validator
> (`google_drive_ingestion.rs` ~line 284) checks tenant/connector/source
> type/state. Neither rejects a binding whose `resource_ref.resource_kind` is a
> tenant-wide root (`Organization`, `Workspace`, `OrganizationUnit`, `Department`)
> or whose `native_source_id` denotes a provider-wide root. **Enforcing this gate
> is required work before each new connector ships** — add a validation step at
> binding creation and at ingestion admission that denies these cases with a stable
> error code. Until that guard is implemented and tested, do not document the
> constraint as fail-closed enforcement.

## Per-connector plans

Each plan specifies the one-time work from the TAN-38 uniform checklist plus the
connector-specific `ResourceRef` mapping and data-class posture. None is started
until the entry criteria above hold and the connector's own OAuth + KMS-backed
`SecretResolver` provider exists (the `google_kms` resolver is still future work
per TAN-38).

### 1. Notion (first after Google Drive)

- **ACL sync mode:** `AdminLabeled`. Notion exposes per-page/teamspace sharing,
  but it is not reliable or stable enough to sync as authoritative ACLs;
  treat it like Drive — require an admin `source_root_label` and govern access
  through admin grants on the binding's `resource_ref`.
- **ResourceRef mapping:**
  - Teamspace / knowledge base → `ResourceKind::KnowledgeSpace`.
  - Database → `ResourceKind::DocumentCollection`.
  - Page → `ResourceKind::Document` (page id in `resource_id`; parent
    teamspace/database in `parent_path`).
  - `native_source_id` = Notion page or database id (never the workspace root).
- **Data class:** default `Confidential`; pages an admin tags as `Credential`,
  `Regulated`, `FinancialRecord`, or `Restricted` go high-risk → quarantined for
  review on every import until released.
- **Scope guard (to implement):** reject bindings whose `native_source_id` is the
  Notion workspace root; require a teamspace, database, or page subtree.
- **Credential:** `ReadOnly` Notion integration token via `ConnectorCredentialRef`,
  optionally `source_bound_resource`-narrowed to the bound teamspace/database.

### 2. GitHub (repository / path `ResourceRef` scoping)

- **ACL sync mode:** `AdminLabeled`. Repo collaborator/team access is effectively
  binary at the repo level and has no per-path ACL; do not treat GitHub
  membership as syncable per-object authority. Require an admin label and govern
  via admin grants.
- **ResourceRef mapping (repository + path aware):**
  - Repository → `ResourceKind::Repository`, `resource_id` = `owner/repo`.
  - Branch pinned via `ResourceRef.branch_id`.
  - Path subtree → `ResourceKind::Directory` with `ResourceRef.path_prefix` set
    to the subtree (e.g. `docs/`), so a binding can ingest exactly one directory
    of a repo rather than the whole tree.
  - Single file → `ResourceKind::File`.
  - `native_source_id` = repository node id (plus branch + path_prefix carried on
    the binding) — never an org-wide "all repositories" id.
- **Data class:** default `SourceCode`. Note `SourceCode` is **not** auto
  high-risk, so a code repo admits by default; but bindings that include secrets
  scanning hits, `.env`-style files, or admin-tagged `Credential`/`Restricted`
  paths must set `data_class` accordingly and are then quarantined for review.
- **Scope guard (to implement):** reject org-level ("all repos") bindings; require
  a single repository, optionally narrowed by `branch_id` and `path_prefix`.
- **Credential:** `ReadOnly` GitHub App installation / fine-grained PAT, scoped
  to the single repository; `source_bound_resource` narrows to the repo subtree.

### 3. Slack (review/quarantine + down-scoped posture)

- **ACL sync mode:** `AdminLabeled`. Channel membership exists but is fluid and
  message/DM visibility is not safely syncable; private channels and DMs are
  high-sensitivity. Require an admin label and admin grants.
- **ResourceRef mapping:**
  - Public/private channel → `ResourceKind::Group`, `resource_id` = channel id.
  - Message / thread → `ResourceKind::Document` under the channel in
    `parent_path`.
  - `native_source_id` = channel id (never the workspace/team root).
- **Down-scoped, review-first posture:**
  - **Public channels only** in the first iteration; **private channels and DMs
    require review/quarantine** and are off by default. A DM/private binding is
    created with `ingestion_policy.require_review = true` so it quarantines on
    every import until an admin releases it.
  - Default `data_class` `Confidential`; DMs and HR/finance channels are
    admin-tagged `Restricted`/`Regulated`/`FinancialRecord` → high-risk →
    always quarantined.
- **Scope guard (to implement):** reject "all channels" bindings; require a single channel.
- **Credential:** `ReadOnly` Slack app token scoped to the bound channel(s).

### 4. Gmail (review/quarantine + down-scoped posture)

- **ACL sync mode:** `AdminLabeled`, and the most conservative of the four. A
  mailbox has no per-object ACL beyond ownership, and message bodies routinely
  contain credentials, regulated data, and financial records.
- **ResourceRef mapping:**
  - Mailbox scope → `ResourceKind::DataStore`, `resource_id` = mailbox/label id.
  - Message → `ResourceKind::Document` under the mailbox/label in `parent_path`.
  - `native_source_id` = a **label or saved query**, never the whole mailbox.
- **Review-first posture:**
  - Every Gmail binding is created with `ingestion_policy.require_review = true`
    (review-first), and the default `data_class` is treated as high-risk so
    content quarantines until an admin reviews it. Releasing requires the
    explicit reviewed-release + `acknowledge_review` reindex path.
  - No prompt-context use until reviewed (`allow_prompt_context = false` until
    release).
- **Scope guard (to implement):** reject whole-mailbox bindings; require a label or query subtree.
- **Credential:** `ReadOnly`, least-privilege Gmail scope (read-only),
  `source_bound_resource`-narrowed to the label/query.

## Sequencing

1. **Notion** first — closest to the proven Drive model (document-centric,
   admin-labeled, folder/teamspace subtree scoping).
2. **GitHub** — adds repository + branch + path-prefix `ResourceRef` scoping and
   the `SourceCode` data class; proves path-level narrowing.
3. **Slack** then **Gmail** — highest sensitivity; both are review-first and
   down-scoped, and depend on the quarantine/review posture being routine.

Broad workspace-level imports stay disabled across all four until that provider's
resource/data-class authorization and audit posture are complete; lifting the
gate is a deliberate per-provider follow-up, never a default.

## Per-connector implementation checklist

For each connector, the uniform unit of work (from TAN-38 §"Implications"):

1. Control-plane OAuth integration + a KMS-backed `SecretResolver` provider
   (the reserved `google_kms` provider; still future work).
2. A per-tenant `ConnectorCredentialRef` (`ReadOnly` for ingestion), optionally
   `source_bound_resource`-narrowed to the bound subtree.
3. An ACL classification added to `provider_acl_sync_mode` — `Synced` only with
   proven reliable per-object ACLs (none qualify today), otherwise `AdminLabeled`.
4. Source bindings mapped to the `ResourceRef`/`ResourceKind` above, routed
   through `evaluate_ingestion_admission`. **The broad-root scope guard (rejecting
   tenant-wide resource kinds and provider-wide native source IDs) must be
   implemented at binding creation and at ingestion** — it is not yet enforced by
   the current runtime (see §"Hard constraint" above).
5. Provider fetch that writes transient bytes, indexes, and removes them; per
   object a `SourceObjectLifecycleRecord` in the per-tenant memory store.
6. Review/quarantine wired for high-risk and `require_review` bindings, with the
   reviewed-release + `acknowledge_review` reindex path.

No connector may reintroduce runtime-owned OAuth secrets, deployment-scoped
credentials, broad workspace-level imports, mandatory non-governance telemetry,
or a persistent raw-content store.
