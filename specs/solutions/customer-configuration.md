# Customer configuration contract (TAN-827)

The existing `tandem-solutions` planner now accepts a separate versioned customer
document through `parse_customer_config` and `prepare_customer_config`. Pass the
prepared `InstallRequest` and narrowed `deployment_policy` to the existing
`resolve` function. This remains pure planning; it does not install resources,
create identities, grant data access or persist configuration.

`fixtures/company-brain-text/customer-a.yaml` and `customer-b.yaml` bind the same
text-only blueprint to two synthetic organizations. Contract tests resolve both
through the actual planner and check distinct tenant-qualified resource IDs,
configuration revisions and preferences with identical artifact digests.

## Customer-owned input

Schema version is the string `"1"`. Unknown fields, duplicate YAML/JSON keys,
unsupported memory kinds, oversized documents and malformed references are
rejected. Scope explicitly selects organization, workspace, deployment and
installation. The trusted host must independently select that installation and
supply a current verified context; matching client-provided IDs are not proof
of authorization.

Human company names, aliases and imported content belong in separately owned
profile/data stores. This initial contract references them with `profile-ref:`
and `data-ref:` identifiers. Credentials use `secret-ref:` identifiers. No raw
credential, connection URL, role, membership or provider-configuration fields
exist. A syntactically valid reference is still untrusted: the host must supply
the exact approved references for the current scope and caller. Likewise,
CapabilityResolver must approve each connector account and generation, and the
existing resolver requires host-approved model binding metadata.

Timezone and locale receive bounded syntax checks here. The runtime adapter must
validate them against its supported timezone/locale registry before activation.
This slice does not add profile/data stores, credential lookup or an HTTP import
endpoint. Never log this customer-owned document as public diagnostic output.

## Composition and revisions

All customer-owned fields participate in the canonical SHA-256 revision, including
references and memory labels. The caller supplies current and expected revisions;
`None`/`None` is an explicit new installation. A mismatch blocks preparation.
The future apply adapter must repeat authorization and compare-and-swap this
revision within its persistence transaction. A pure comparison is not atomic
installation evidence or a durable retry receipt.

Optional selections, preferences and model/connector bindings reuse the existing
planner's validation. Customer policy intersects with trusted host policy, then
with blueprint ceilings; it cannot enable forbidden egress, add a provider or
raise token, concurrency or spending limits. A customer configuration revision
is not itself a verified policy revision.

Every blueprint memory space requires a matching declaration. Private subjects,
departments and projects must already be approved by the trusted host for this
caller. These declarations are not grants. The runtime adapter must use governed
`memory_records` and its verified owner/department/tenant labels; project
partitions do not confer access. Unsupported team/curated stores are rejected.
Actual write/recall and membership-change evidence is tracked separately in
agents PR64; this pure planner test does not establish runtime privacy.

`customer_config_changes` reports upstream blueprint changes separately from
changed customer field groups. It emits no old/new values, identifiers or
secret-reference names. Review and authorization remain necessary at apply.

## Export boundaries

There are two distinct artifacts:

- Customer-owned JSON/YAML serialization preserves scoped identifiers and
  references for authorized backup/import. It contains no resolved secret values
  and must stay within that customer's storage and access controls.
- `customer_config_template(blueprint)` produces a shareable skeleton containing
  only reusable blueprint identity and declared configuration slots. It accepts
  no customer document, so customer names, source IDs, reference names, hashes,
  bindings and override values cannot be copied accidentally. The result requires
  new customer configuration and cannot be imported as a deployable document.

The blueprint itself must come from the reusable pack's existing trust/content
review. This API does not sanitize arbitrary customer content embedded in a
blueprint. Pack export and installer adapters must select the shareable skeleton
instead of serializing a customer document or resolved plan.

Remaining TAN-827 acceptance includes those runtime/export adapters, scoped
profile/data storage, transactional concurrent updates, effective lifecycle
checks, browser import/export and the real two-customer installation drill.
