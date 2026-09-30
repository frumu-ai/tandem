# Current runtime model bindings

`ProviderRegistry::runtime_binding_for_tenant` reads the actual configured adapter
and current authentication selection for an explicit tenant/provider/model. It
performs no network request, credential refresh or mutation. The adapter lock
remains held while tenant authentication is inspected, preventing a reload from
mixing two adapter snapshots.

The result identifies the model, protocol, tenant and credential source:
`host_service`, `tenant_bearer` or `unauthenticated`. Endpoint and final credential
headers use the same fingerprints as physical-attempt accounting. Raw keys and
endpoint URLs are absent. This describes concrete transport state; neither a
customer credential-reference string nor a caller-provided hash supplies it.

Codex keeps the existing tenant override rules. An explicit tenant with no loaded
bearer token cannot inherit a global credential. Other adapters report their
configured host service credential or unauthenticated state; that classification
does not approve sharing a service account. Unknown adapters, ambiguous provider
IDs and unavailable/ambiguous models fail the lookup. Legacy dispatch remains
available outside the solution policy path.

The lookup deliberately does not use configured URL overrides to infer Codex's
endpoint: its concrete adapter owns the route. It also does not equate advertised
context windows with verified input, output, tool, modality or billing limits.

## Verification

Provider tests inspect model availability, endpoint/key replacement, absent
credentials, concurrent tenant token isolation and clearing one tenant without
affecting another. Synthetic `.invalid` endpoints establish that snapshot lookup
does not resolve or contact a provider. Compatible, Responses and Anthropic tests
compare snapshots with actual complete/stream request descriptors and stop at
admission before networking; Anthropic additionally uses a local proxy. Cohere
has read-only binding coverage; its hardened transport does not permit the local
HTTP fixture, so it is not claimed as an executed Cohere integration.

The existing SQLite/PostgreSQL provider-ledger tests now obtain transport hashes
independently from this registry API. Their higher-level model/root approval
remains synthetic. Replacing the actual registry credential after reservation
changes the second authorization result, prevents the HTTP send and reconciles
the proven non-dispatch at zero.

## Remaining authority and lifecycle work

A snapshot is not a verified user identity, account authorization or durable
account generation. Replacing a key and later restoring the same bytes can
restore its fingerprint. Durable account lifecycle metadata belongs in the
existing credential subsystem; OAuth token refresh must remain distinct from
authorized account replacement. Registry reload and persisted credential
changes also require current reconciliation at runtime.

The production solution factory must independently authorize current users,
account ownership/service sharing, model capabilities and data-class destination
policy, approved prices/input ceilings and actual root lineage. It must compare
the final attempt to current facts again after reservation, while preserving
hosted-policy and exact-payload egress checks. Activation and user source/private
memory authorization remain separate from the installer's admin preview. This
slice does not complete TAN-831 or full Company Brain acceptance.
