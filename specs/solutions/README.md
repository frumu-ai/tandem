# Solution planning contract (TAN-824)

`tandem-solutions` contains the first versioned, pure Company Brain planning
contract. It parses JSON/YAML, rejects invalid input, verifies selected artifact
digests, and produces the same resolved plan for every caller. It has no network,
filesystem, credential lookup, account provisioning or installation side effects.

This is the foundation for the installer, **not a working Company Brain
installation yet**. The fixture is an offline planning example; it neither logs
in a user nor runs a model. Keep TAN-824 open until the runtime adapter and the
agreed contract review are complete. TAN-840 separately owns real private user
enrollment and the second-user lifecycle.

## Run it

From the repository root:

```sh
cargo test -p tandem-solutions --locked
cargo run -p tandem-solutions --example plan_fixture --locked
cargo run -p tandem-solutions --example plan_fixture --locked -- --schema
```

The example prints a resolved plan and its SHA-256. It uses fixed, explicitly
synthetic identities and a placeholder local model, so it needs no server,
provider key or customer data. `--schema` emits the schema checked into
`solution-blueprint.schema.json`; a test detects schema drift. Parsing also checks
semantic constraints that JSON Schema cannot express, including reference
compatibility, graph cycles, preference bounds and artifact paths.

The dedicated `Solution Contract` workflow runs this test suite for crate,
lockfile and `specs/solutions/**` edits, including schema-only changes. It does
not require a schema edit to trigger the full engine OS matrix.

## Current code seams

Inventory against main `c628546a480dbdd94356fcbb94ab1a9fd2cabed4`:

| Existing code | Integration boundary |
| --- | --- |
| `tandem-server/src/pack_manager.rs` | Continues to own archives, signature/trust inspection, verified entry bytes and pack lifecycle. A solution remains content inside an existing pack whose outer `type` is `solution`. |
| `tandem-server/src/preset_registry.rs` | Already indexes installed/builtin/override presets. TAN-825 adds typed composition and tracked override resolution to this existing registry. |
| `tandem-server/src/preset_composer.rs` | Continues composing prompts; its prompt hash is not a deployment lock. |
| `tandem-server/src/capability_resolver.rs` | Continues account/capability discovery and resolution; callers supply approved connection IDs/generations, never raw credentials. |
| `tandem-orchestrator/src/agent_team.rs` | Fixture agent deserializes into the actual `AgentTemplate` in a contract test. |
| `tandem-server/src/routines/types.rs` | Optional fixture is a paused routine template using the existing pack-builder `mission.default` entrypoint. The installer must stamp tenant, creator and instance-owned IDs before constructing `RoutineSpec`. |
| `tandem-enterprise-contract` | Existing verified context and authority projection remain authoritative. |
| Current enterprise onboarding preview/apply APIs | TAN-834 adapts the resolved plan to these APIs and rechecks authorization before mutations. |

No second pack manager, capability resolver, preset index or authorization engine
is introduced. The crate is unpublished while this contract is being integrated.

## Manifest v1

`schema_version` must be the string `"1"`. Unknown versions and fields fail closed;
there is no implicit upgrade or permissive fallback. `solution.id` and exact
semver `solution.version` identify the reusable product. `engine_version` is a
semver requirement checked against the target engine.

`components` is keyed by stable logical component IDs. Each declares a typed
kind, whether it is required, and an exact pack entry reference
(`pack_id`, `version`, SHA-256 of its bytes, portable relative `path`). Workers,
presets, routines, workflows, goals, policies, ontology, connector recipes,
model profiles, onboarding, UI and eval assets share this reference contract.
Their content validation remains with the owning compiler; digest validation is
not semantic approval or publisher trust.

Dependencies map component IDs to compatible semver requirements. Selecting an
optional component selects its prerequisites. Missing references, incompatible
pins, cycles anywhere in the manifest, active conflicts and empty selections
fail with a stable code and field path. No optional worker, voice module or
connector is needed by the default text fixture. Required capability use always
dominates optional use across selected components.

All mappings and set-like fields canonicalize deterministically. The manifest
hash includes the complete normalized manifest, including optional definitions.
Artifact content is byte-pinned; changing line endings intentionally requires a
new digest. The schema is bounded to 1 MiB/256 components by the parser and
validator; selected artifacts are bounded to 8 MiB each. PackManager must also
enforce its archive and expansion limits before supplying any bytes.

## Configuration, constraints and lock identity

`InstallRequest` contains an instance ID, SHA-256 customer configuration revision,
optional selections, declared preference overrides and connector/model references.
It contains no identity claims, roles, policies or account-provisioning requests.
`models` maps each requested class to an opaque host-approved binding ID, for
example `{"economy":"local.fixture"}`. Model/provider/credential/network fields
are rejected in customer request JSON. The separate `ResolutionInput.approved_models`
registry must come from current host model/account readiness checks scoped to
the verified caller. Missing or revoked binding IDs fail closed; there is no
fallback to client-supplied metadata. `host-models.json` is a synthetic registry
used only by the offline fixture, not part of customer configuration.
Preference types are bounded integers, booleans and declared choices. Unknown
preferences, disallowed overrides and invalid values are rejected. There is no
arbitrary customer-content or raw-secret payload field. This structural boundary
does not replace secret scanning: a publisher can still paste confidential text
into an artifact, identifier or declared choice. TAN-827 owns export sanitation.

Policy ceilings are separate from preferences. Effective providers are the
intersection of solution and host allowlists; network permission requires both;
token/concurrency/cost ceilings take the lower limit. Empty provider sets allow
no providers. Cost uses integer micro-US dollars. The trusted host must obtain
these limits and model/account metadata itself; browser-supplied
`uses_network: false` is never evidence that a provider is local.
The resolver reads those properties exclusively from the approved host registry
and locks both the selected binding ID and its resolved metadata. Resolver
version 1.0.1 identifies this stricter contract. This crate remains unpublished;
the previous inline model-binding request shape is deliberately rejected.

The lock includes exact entry versions/digests, schema/resolver/engine versions,
manifest hash, instance and configuration revision, tenant/workspace/deployment,
principal and effective-authority hash, selected components and installation
order, bindings, preferences, memory labels, constraints and readiness/UI
requirements. A tenant-qualified namespace prevents resource-ID collisions
between instances. Every generated resource records its owning instance.
TAN-834/TAN-837 must reject overwriting resources owned by another installation.

`ResolvedPlan::composition_hash()` hashes the whole canonical lock, excluding a
self-referential hash field. Assertion IDs, signing keys and issuance/expiry
timestamps do not alter a plan solely because a session refreshed. Roles,
memberships, capabilities, authority chain, policy version and strict grant/data
boundary projection do alter it. Strict projection array order is retained
conservatively; callers should use their canonical projection.

## Identity boundary with TAN-840

`resolve()` accepts a `VerifiedTenantContext` from the trusted host. That host
must use the existing context assertion verifier and replay protection first;
the Rust type alone is not proof of verification and must never be deserialized
from a client request. Expired, inconsistent or local implicit identities are
rejected. `now_ms` is supplied by the host to keep planning deterministic.

Hosted v1 contexts without an optional strict projection remain representable;
their signed roles/memberships are included. The deployment's existing
authorization mode decides what such a context can do. The resolver never
grants installation or memory access. An authenticated person with insufficient
authority may view a permitted preview, but cannot apply it.

Accounts, initial owner, membership, revocation and recovery remain owned by the
identity adapter/control plane. Solution content and models cannot create an
account or self-grant permissions. A private shared engine token and local
single-human mode do not qualify as Company Brain multi-user authentication.

## Supported memory mapping

Every v1 label below maps to the existing governed `memory_records` store.

| Label | Existing storage/enforcement contract |
| --- | --- |
| `private_user` | Verified subject stamped as `user_id`; never inferred from text or a customer-selected username. |
| `department_shared` | Explicit verified `owner_org_unit_id`; membership checked at write and retrieval. |
| `tenant_shared` | Explicit `tenant_shared` metadata inside the verified tenant scope. |
| `project` | Server-resolved `project_tag`, plus the applicable subject/department/shared boundary. A project tag is not an access grant. |

No `team` or `curated` backing store is invented. No local model-tool chunk-write
shortcut is allowed. See `docs/MEMORY_SCOPE_MODEL.md` for the enforced behavior.

## Remaining integration gates

1. TAN-824/TAN-825: review the contract with its runtime consumers; adapt selected
   entries to validated preset/agent/routine definitions with runtime-owned IDs.
2. TAN-840: deliver real owner and second-user enrollment, stable identity,
   revocation/recovery, and verified context across memory/tools/approvals.
3. TAN-826/TAN-827: package ingestion/export sanitation, customer config revision,
   preference application and safe secret-reference handling.
4. TAN-831/TAN-832: supply live, approved model/account bindings and enforce limits
   at execution, including price or credential-generation changes.
5. TAN-834: preview/apply adapter, authorization, ownership, stale-plan rejection,
   durable operation state, retries and rollback. A hash is not authorization.
6. TAN-841/TAN-839: run the early user workflow and complete second-customer
   installation gates. The offline fixture does not substitute for either.

UI, CLI, install and upgrade must consume this one validated plan. They must not
reimplement resolution independently or add customer-specific runtime branches.
