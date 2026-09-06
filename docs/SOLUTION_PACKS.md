# Validated solution packs

`type: solution` extends the existing PackManager. It uses the same ZIP archive,
trusted Ed25519 signature, extraction limits, staging/index/current lifecycle and
operator-only ingestion APIs. It does not install runtime resources or activate
routines. Existing workflow pack behavior is preserved.

The initial format requires `entrypoints.solution` to name a version 1
SolutionBlueprint. Its identity/version must match the pack. Every component's
artifact is pinned to this pack ID/version and its exact SHA-256 bytes. Required
and optional artifacts both belong to the self-contained closure. Unsupported
external pack references fail closed; they are not fetched during installation.

The allowlist contains only `tandempack.yaml`, `tandempack.sig`, the blueprint and
its declared artifacts. Extra files, symlinks, unsafe paths, missing files or
digest mismatches reject the solution. Existing secret scan patterns apply to
all allowlisted payload bytes without file-size or `.example` exemptions. These
patterns are a heuristic, not a classification of every possible private value.
Publishers must package reusable product content, not customer data.

`PackManager::solution_artifacts` supplies verified bytes keyed by component ID
for the existing resolver. The host must separately authorize access and provide
current identities, model/connector bindings, customer configuration and policy.
Inspection exposes the blueprint's components, constraints, memory declarations
and required/optional dependencies. It never claims runtime materialization.

## Integrity and export

Installed solution records bind a canonical content digest as well as the
existing archive digest. Loading, inspection and export verify current publisher
trust and this installed-content binding. A new valid signature on changed
same-version content cannot silently replace the original install receipt.

Verification operates on the exact in-memory file snapshot returned to the
planner or written to the exported archive. Export never does an unchecked second
tree walk after verifying a snapshot. Adding a customer file beside installed
artifacts rejects export; it is not included or silently removed while retaining
an invalid signature. Legacy records that merely used the free-form `solution`
type lack the new validated digest and must be reinstalled from a verified archive.

## Text fixture and remaining work

The current `company-brain-text` fixture supplies the manifest, solution.json,
agents/central-brain.json and routines/review-notes.json. Package exactly those
four files plus the normal signature; exclude the neighboring customer YAML
fixtures. Regression tests sign this allowlisted content using the existing
synthetic publisher helper, install it, feed its bytes into the actual resolver,
and export/reinstall it with the same content. Synthetic signing keys are not
production publisher trust configuration.

This is validated ingestion and artifact loading. Runtime installation-reference
guards/shared dependency ownership on uninstall, materialization, activation,
external-effect compensation and upgrade/rollback acceptance remain installer
work. The existing uninstall operation changes pack files/indexes only; it does
not claim rollback of installed runtime resources or customer data. Keep TAN-826
and the complete solution acceptance open until those integrations are proven.
