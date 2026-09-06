# Current model account authority

`AppState::authorize_solution_model_account` connects a model binding's existing
stored credential revision to the current hosted user's native credential-use
grant. Tenant credential scope alone does not grant account access.

The existing operator `solution_installation.models` entry may now include an
optional `account` object with `credential_kind` (`api_key` or `credential`),
`credential_location` (`host_service` or `tenant_service`), the reviewed
`authorization_revision`, and an existing credential `resource` reference. The
resource must be `secret_provider_credential`, belong to the selected workspace
and organization, and identify the model entry's existing `credential_ref`.
Only managed/startup CLI policy supplies this binding; ordinary configuration
overlays and customer documents do not approve it. An absent account object
continues to support installation metadata but cannot authorize credential use.

The service reprojects current hosted identity and memberships, requires
`HostedUse`, and incorporates existing native organization-unit resource grants.
The current human must have `Execute` on the exact credential resource for
`Credential` data. An installer's old admin role and a peer's department grant
are insufficient. This uses the existing resource-grant system; it does not add
another provider-account store, user directory or permission registry.

The existing provider matcher verifies that the actual loaded material matches
the persisted credential and selected host/tenant location. The stored
authorization revision must match operator approval. Same-account refresh may
change material while retaining authorization; reconnect requires a new reviewed
authorization revision even when its material is unchanged. Pending, disconnected,
untracked and stale loaded credentials fail closed.

After the credential read, the service rechecks operator configuration and
reprojects current resource grants. The result includes the current verified
identity, versioned runtime binding and a digest of the authority snapshot. It
contains no credential values and performs no provider request or refresh.
Callers must repeat authorization for every physical attempt and reject changes
across reservation; the snapshot is not a durable dispatch capability.

Installation preview and every staging transition now apply the same current
account checks to operator model entries with an account binding. A denied or
stale account is omitted from approved models, so selecting it fails resolution.
An unrelated denied model does not prevent review of an allowed selection.
Authorization receives the exact operator model snapshot used for the plan and
rejects a changed binding. Existing account-free diagnostic metadata retains its
behavior. Saving customer intent remains separate from model readiness; it does
not grant authority to stage or activate it.

## Remaining integration

The new service authorizes use of an existing service credential through current
native grants. It does not implement personal provider-account storage or change
which adapters support tenant bearer credentials. Only the existing Codex path
currently supports that tenant authentication form. Unauthenticated local model
routes continue to require their separate runtime/policy checks; they do not need
fabricated credentials to fit this credential-use service.

Current model availability, modality, tool support, data handling, price and
customer constraints still belong to model-profile resolution and its trusted
fact adapter. Installation-to-goal binding, durable profile history, activation,
worker/routine/session integration and complete customer acceptance remain open.
No runtime resources are activated by account authorization.

## Tests

`solution_service_model_account_` tests reuse the installed signed-pack service
fixture and actual hosted-policy projection, native grants, credential store and
provider registry. Two members in different departments demonstrate default deny,
explicit sharing, native grant revocation and hosted-user removal. A second test
checks reconnect with unchanged material, operator revision review and a mismatch
between loaded and persisted material. The endpoint is never called; availability
and provider billing are not claimed by these tests.

A deterministic race test observes the initial successful grant check, holds the
actual credential mutation lock, revokes the grant and releases the lock. The
pending lookup must reject the user after its credential read completes. Its
observation hook exists only in test builds and does not alter production checks.

The preview/staging integration test signs a separate synthetic HTTP-provider
pack in a temporary directory. It checks default denial, an unrelated denied
account, grant removal after review, reconnect invalidation and disabled staging
after explicit revision review. It does not alter the shipped local-only pack,
send a provider request or activate the staged agent.
