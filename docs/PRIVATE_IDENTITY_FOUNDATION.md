# Private identity foundation (TAN-840)

This checkpoint hardens the existing panel identity boundary before adding
private multi-user enrollment. It does **not** complete owner enrollment,
invitations, recovery or a supported private identity provider. TAN-840 remains
In Progress. A shared engine token still represents the existing local operator
mode, not several individually verified Company Brain users.

## Reused contract

The current hosted adapter exchanges a one-time login code with the trusted
control plane and receives a server-held user session and context assertion.
The engine's existing `tandem-enterprise-contract` Ed25519 verifier, replay
protection and governed authorization remain authoritative. The panel validates
the response envelope; it does not replace signature verification or implement
an account database. A private adapter must use this same verified runtime seam.

The response must identify the expected deployment and a stable user and include
nonempty `panel_session_token` and `context_assertion`, plus valid future
`session_expires_at` and `context_assertion_expires_at` timestamps. These fields
come only from the trusted control plane. Browser-submitted user/role fields are
ignored. A malformed response cannot fall back to a root-token-only session.

## Implemented behavior

- The panel and scaffold remove browser-supplied context assertions, all three
  runtime assertion aliases, raw actor/tenant aliases, and agent identity/source
  headers from engine, OAuth callback and webhook proxies. Only server-held
  session identity is attached to an authenticated engine request.
- Internal engine helpers cannot override that identity or transport credential
  through extra headers. Hosted browser sessions cannot opt into local agent
  test mode.
- Every authenticated panel route passes through hosted refresh before using
  the session, including `/api/auth/me`. Absolute session expiry also applies
  during pruning; activity alone cannot preserve an expired session.
- Concurrent requests share one refresh. Successful refresh can rotate
  credentials and update memberships/capabilities but cannot change the user or
  deployment. Logout during refresh cannot recreate the removed session.
- Failed refresh removes the affected session and returns 401. Other users'
  panel sessions remain independent. Configuring another deployment invalidates
  a session even while its previous assertion remains unexpired.
- Cookies use `Secure` when the configured public URL is HTTPS, including
  mixed-case URL schemes. HttpOnly and
  SameSite protection remain in place; configure the actual HTTPS public URL.
- Hosted members/viewers cannot reach deployment-wide settings, raw shared
  file/workspace routes, knowledgebase administration or ACA/swarm/orchestrator
  sidecar handlers. Those paths currently use global resources or administrative
  credentials. Existing owner/admin roles or hosted administrative capabilities
  are required until a verified per-user implementation exists. Personal
  preferences and engine-governed routes retain their existing access paths.
- Hosted identity settings, credential-file paths and control-plane endpoints
  are server-operator configuration. Browser-authenticated workspace admins
  cannot change them or remove them by saving a partial configuration.
- Login and refresh validate their destination against the configured control
  plane, require HTTPS outside literal loopback, reject URL credentials/query
  strings/fragments, use an eight-second timeout and refuse redirects. The
  host token is read only after destination validation. This is an intentional
  credential-file-to-control-plane authentication flow, not a general file
  forwarding operation. The server operator remains a trusted actor.

This last restriction is intentional: authenticating a user does not authorize
deployment-wide configuration edits or global files. Existing local operator
behavior remains supported. Runtime routes retain their own policy/grant checks.

## Verification

After installing locked panel dependencies and building its existing frontend:

```sh
cd packages/tandem-control-panel
pnpm install --frozen-lockfile
pnpm build
node --test --test-concurrency=1 tests/hosted-auth-endpoint.test.mjs tests/hosted-session.test.mjs tests/hosted-session-integration.test.mjs tests/engine-proxy-header-strip.test.mjs tests/smoke.test.mjs
```

The 33 tests cover the envelope and expiry boundary, identity/header spoofing,
refresh concurrency, changed-user rejection, removal of one affected session,
admin-route denial, scaffold parity, public callback/webhook behavior and existing
local auth/proxy/swarm behavior, mixed-case HTTPS cookies, off-origin endpoint
rejection, redirect rejection and immutable hosted authentication settings.
The integration tests start the real panel with
synthetic control-plane and engine services. They test the panel seam; they do
not prove cryptographic verification or private memory isolation end to end.

The broader `capabilities-integration.test.mjs` suite's
“Hosted install profile enables search and scheduler settings without a local
engine URL” test failed here with engine-unavailable/502 when contacting
`engine.localtest.me`. The same test and unchanged main `setup.js` reproduced
that failure at `c628546a480dbdd94356fcbb94ab1a9fd2cabed4`. Its four other cases
passed. This environment result is not a passing remote-host deployment gate.

## Remaining TAN-840 acceptance

1. Select and integrate the supported private identity adapter. Account and
   session libraries must be maintained; reuse existing hosted interfaces where
   compatible. Do not invent public-runtime invite/SCIM APIs or a second grant
   engine. Models and solution manifests cannot provision their own authority.
2. Deliver fresh instance -> first owner -> tenant/workspace -> second user ->
   individual login through a documented UI/bootstrap path. First-owner setup
   must be one-time, with operator-authorized recovery that cannot be reused for
   account takeover.
3. Verify stable principals across browser sessions, restart and credential
   rotation. Define and test membership/revocation propagation and in-flight
   execution behavior. Current assertions refresh near expiry; this patch does
   not claim immediate revocation of an otherwise valid assertion.
4. Complete per-user authorization and verified context propagation for every
   enabled product surface. The temporary admin restrictions above must not be
   removed until those handlers have that contract.
5. Run real two-user and same-tenant two-department tests across governed memory,
   tools, approvals, private retrieval and permitted sharing. Missing or spoofed
   context must fail closed. Existing memory isolation and assertion-verifier
   suites provide reusable fixtures.
6. Expose readiness to TAN-827/TAN-834/TAN-836 that distinguishes uninitialized
   identity, unauthenticated access, verified user identity and completion of
   the supported multi-user deployment checks. Process health is insufficient.

TAN-824's pure solution planner receives the verified context from this trusted
host boundary. Planning never provisions accounts; apply must reauthorize the
current user and recheck memberships, grants and bindings before every mutation.

## Hosted policy synchronization

The opt-in runtime security v2 profile consumes the existing control plane's
authenticated policy bundle. One complete, validated snapshot owns hosted human
memberships and deployment-operation grants. The engine rejects mismatched
organization/deployment scope, rollback, conflicting content at the same
revision, and snapshots at least 120 seconds old. The timestamp check permits
at most five seconds of future clock skew. Failed fetches do not extend trust.
Every process restart requires a fetch generated after startup; the durable
high-water record prevents rollback but cannot restore live authority itself.

An accepted revision invalidates assertions from older revisions, including
those belonging to unaffected users; those users must obtain fresh assertions.
Current authority is checked at HTTP ingress and again at governed execution
boundaries after approval, provider credential recovery and MCP readiness waits.
This bounds future dispatch; it cannot undo an external effect already sent.

Hosted permissions use a distinct deployment resource and typed operations.
For example, `hosted.use` permits runtime execution without granting generic
document Read or Admin. Automation operations additionally retain their existing
resource-owner/audience checks. Administrative routes require their existing
independent authorization until individually integrated.

The existing enterprise registry APIs expose imported units under the reserved
`hosted-control-plane` taxonomy. Their principal IDs are
`hosted-control-plane/{unit_id}`. Imported rows are ephemeral, and local API
writes cannot claim that namespace or membership source. Local data grants can
target these units, but apply only through current hosted memberships. Retained
local human memberships cannot restore removed hosted authority. The readiness
view uses one hosted revision and does not count deployment-operation grants as
proof of governed data access.

Focused tests live in the contract's `hosted_policy` module and the server,
core, provider and MCP hosted-policy tests. Cross-repository process acceptance
uses the source-pinned enterprise engine and copied authenticated policy agent
in `tandem-agents`. Release-image verification, clean-host encrypted recovery
and full two-user/two-department governed-memory acceptance remain required.
