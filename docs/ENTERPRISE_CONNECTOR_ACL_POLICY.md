# Enterprise Connector ACL Policy

Operator reference for how Tandem decides whether connector-sourced data may be
ingested and indexed (EAA-14 / TAN-39).

## Trust model

Connectors pull data from external providers whose native access-control lists
(ACLs) vary in fidelity. Tandem classifies each provider into one of three ACL
sync modes (`provider_acl_sync_mode`, in `tandem-enterprise-contract`):

| Mode | Meaning | Ingestion requirement |
| --- | --- | --- |
| `synced` | Provider exposes reliable per-object ACLs that Tandem syncs and enforces. | Indexing may proceed on provider ACLs (still subject to review/data-class gating). |
| `admin_labeled` | Provider ACLs are absent/incomplete/unsafe to rely on. | The source binding must carry an explicit admin label, and access is governed by admin-created access grants. |
| `unsupported` | Provider is unknown / not yet classified. | Ingestion is denied (fail closed). |

Only providers with proven, reliable ACL fidelity are classified `synced`. No
provider has that classification today. **Google Drive is `admin_labeled`** (its
ACLs are not synced — `not_synced_v1`), so its bindings require an admin label.

## Admin-labeled fallback

For `admin_labeled` providers, the source binding must set a non-empty
`source_root_label` (the human label an admin applies to the source root). A
binding with no admin label is denied ingestion (`ingestion_admin_label_required`).

Access to admin-labeled sources is then governed by the access grants the
retrieval layer already enforces: a request only sees a source-bound memory
chunk when its verified context grants `Read` on the binding's `resource_ref`
for the chunk's `data_class` (org-unit membership grants, scoped grants, or
cross-tenant grants). Where provider ACLs cannot be trusted, an admin grants
access explicitly rather than Tandem inferring it from the provider.

## High-risk data classes require review

Ingestion of high-risk data classes is held for human review (quarantine)
before the data becomes retrievable, regardless of the per-binding
`require_review` flag. `DataClass::requires_ingestion_review` flags:

- `credential` — secrets, API keys, tokens
- `regulated` — HIPAA / GDPR / PCI-DSS and similar regulated data
- `financial_record` — sensitive financial data
- `restricted` — the most sensitive internal tier

A binding may also set `ingestion_policy.require_review = true` to force review
for any data class.

## Admission decision

`evaluate_ingestion_admission(binding, connector, acl_mode)` is the single
fail-closed decision every connector ingestion path routes through. In order:

1. **Deny** if the binding/connector identity or tenant mismatch, the connector
   is not active, the binding is disabled/quarantined, or indexing is disabled.
2. **Deny** if the provider is `unsupported`, or `admin_labeled` without an
   admin label.
3. **Quarantine** if the binding's policy requires review, or its data class is
   high-risk.
4. **Admit** otherwise.

`Deny` aborts ingestion with a stable error code. `Quarantine` indexes the data
but immediately holds it (chunks removed, source objects marked `quarantined`,
an `IngestionQuarantine` opened) until an admin reviews it via
`PATCH /enterprise/ingestion-quarantines/{id}/review` and sets a disposition
(`release`, `delete`, or `reindex`). `Admit` indexes and keeps the data.

## Adding a new connector provider

1. Implement the connector and prove its ACL behavior.
2. Classify it in `provider_acl_sync_mode`: `synced` only if its per-object
   ACLs are reliable and enforced; otherwise `admin_labeled`.
3. Leaving a provider unclassified keeps it `unsupported` (ingestion denied),
   so new providers fail closed until explicitly reviewed.
