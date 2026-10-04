# Versioned model account binding

A registry transport snapshot and a credential revision read independently can
refer to different token material. For example, a refresh may already be stored
while the provider still has the previous bearer loaded. The new
`versioned_runtime_binding_for_tenant_in_dir` lookup joins the existing registry
snapshot and credential lifecycle reader rather than treating any active revision
as approval for the loaded token.

The caller explicitly selects the existing credential kind and host-service or
tenant-service location. The lookup checks the actual adapter's authentication
source, reads active material and its revision under the credential file lock,
and compares the selected stored token against the actual request-header
fingerprint. Typed OAuth uses `runtime_bearer_token`, including the existing
API-key preference. Anthropic's API-key header and the other adapters' bearer
headers use the shared transport fingerprint implementation. It then checks the
registry snapshot again to detect a route/model/loaded-key change during the
filesystem lookup. Lock acquisition runs on a blocking worker, not an async
runtime worker. No network request, secret loading into the registry, or refresh
occurs.

The result contains transport fingerprints, selected credential kind/location,
and authorization/material revisions. It contains no raw token, URL, or OAuth
account details. An untracked config/environment key, stale loaded key, pending
record, disconnected account, wrong tenant store, or source mismatch cannot
produce a result. Same-account refresh changes the material revision and token
fingerprint while retaining the authorization revision. Explicit A→B→A reconnect
can restore the original fingerprint but always has a new authorization revision.

Tests cover those transitions and all four built-in header/protocol forms.
The real SQLite/PostgreSQL provider-budget test additionally changes persisted
material or reconnects the account after reservation and before dispatch. It
checks that denial makes no HTTP request and reconciles proven non-dispatch at
zero, while the permitted case sends once and settles its observed usage.

This is an optimistic current-state snapshot, not a grant. Callers must authorize
the current actor's access to the account, approved sharing, source/data-class
policy, model capabilities, prices, configuration, activation and real root/child
lineage, and repeat validation at each physical attempt. Tenant credentials remain
scoped to organization/workspace/deployment rather than individual actor. The
budget test's model/root authorization is still a synthetic fixture; it does not
claim the full production approval or activation factory exists. Logical profile
policy, fallback/escalation and production integration remain required. External
rollback anchors and clean-host recovery acceptance also remain open.
