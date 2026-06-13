# Context Assertion Security

Signed tenant context assertions are the trust primitive for hosted and
enterprise runtime modes. In `hosted_single_tenant` and `enterprise_required`
modes the runtime rejects raw tenant headers and requires an EdDSA-signed
assertion (JWS, `header.claims.signature`) on one of:

- `x-tandem-context-assertion`
- `x-tandem-context-jws`
- `x-tandem-tenant-context-jws`

Verification is fail-closed and implemented in
`crates/tandem-server/src/http/middleware.rs`
(`TenantContextAssertionVerifier`):

1. Ed25519 signature over `header.claims`, key selected by `kid`.
2. Claims validation: version `v1`, issuer/audience match, expiry and
   issued-at skew, non-empty `assertion_id`/actor/org/workspace, explicit
   tenant source with deployment scope, actor consistency across
   `tenant_context`, `human_actor`, and `authority_chain.initiated_by`.
3. Key metadata validation: key status, purpose, lifetime window, allowed
   audiences, organization/deployment restrictions, resource scope prefixes.
4. Replay policy (below).

## Key configuration

| Variable | Meaning |
| --- | --- |
| `TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS` / `..._FILE` | JSON keyset keyed by `kid`, each entry carrying the public key plus optional `purpose`, `organization_id`, `deployment_id`, `allowed_audiences`, `allowed_resource_scope_prefixes`, `not_before_ms`, `not_after_ms`, `status`. |
| `TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY` / `..._FILE` | Legacy single key (no `kid` binding). Prefer the keyset. |
| `TANDEM_CONTEXT_ASSERTION_ISSUER` | Expected `issuer` claim. Default `tandem-web`. |
| `TANDEM_CONTEXT_ASSERTION_AUDIENCE` | Expected `audience` claim. Default `tandem-runtime`. |
| `TANDEM_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS` | Maximum accepted future `issued_at_ms` skew. Default `10000`; values are clamped to `10000..60000`. |

If no key is configured in hosted/enterprise mode, all assertion-bearing
requests are rejected (`context_assertion_key_not_configured`).

At verifier initialization, the runtime logs a safe summary of configured keys:
`kid`, status, purpose, lifetime, and tenant/deployment constraints. Public key
bytes are never logged. Verified assertions retain the selected `kid` as
assertion metadata so downstream strict projections and audit evidence can show
which configured verifier key accepted the request.

Example keyset:

```json
{
  "ctx-2026-06-primary": {
    "publicKey": "base64url-ed25519-public-key",
    "purpose": "context_assertion",
    "organizationId": "org-a",
    "deploymentId": "prod-eu",
    "allowedAudiences": ["tandem-runtime"],
    "allowedResourceScopePrefixes": ["org/org-a/workspace/workspace-a"],
    "notBeforeMs": 1781300000000,
    "notAfterMs": 1783892000000,
    "status": "active"
  }
}
```

Key lifecycle:

1. Generate Ed25519 key material in the hosted control plane or customer key
   management boundary. The private key must never be configured on the
   runtime.
2. Publish the public key in the runtime keyset with `purpose:
   context_assertion`, a bounded lifetime, expected audience, and tenant or
   deployment restrictions.
3. Introduce the next key as active before rotating issuers. Keep the old key
   active until all assertions signed by it have expired plus replay retention.
4. Retire or remove old keys after the overlap window. Do not reuse `kid`
   values for new key material.
5. Treat malformed, expired, untrusted, replayed, or missing assertions as
   protected audit evidence. The runtime records these as
   `context_assertion.rejected` when an audit path is available.

## Replay protection

`TANDEM_CONTEXT_ASSERTION_REPLAY_MODE` controls how the runtime treats reuse
of an `assertion_id`. Replays are rejected with reason
`context_assertion_replayed` (HTTP 403).

| Mode | Behavior | Use when |
| --- | --- | --- |
| `bound` (default) | First use binds the `assertion_id` to the SHA-256 of the exact assertion bytes. Re-presenting the identical assertion is allowed until expiry. A different assertion carrying the same `assertion_id` is rejected. | The control plane mints one assertion per client/session and clients reuse it across requests (this is how `tandem-channels` behaves). Protects against assertion substitution and forged-ID collisions; pure capture-replay of the identical bytes is bounded by the expiry window. |
| `one_shot` | Each `assertion_id` is accepted exactly once. | The control plane mints a fresh assertion per request. Strongest replay protection. |
| `off` | No replay tracking. | Migration escape hatch only. Do not run hosted deployments in this mode. |

Operational notes:

- The replay cache is in-process. Multi-replica deployments behind a load
  balancer should pin clients per replica or front assertions with a shared
  verifier until a shared cache backend exists.
- Entries are retained until assertion expiry plus a 60s grace window and are
  swept opportunistically once the cache exceeds 1024 entries; memory use is
  bounded by the number of live assertions.
- Replay rejections are logged with `assertion_id`, `org_id`, and the active
  mode, and are written to protected audit as `context_assertion.rejected`
  when the runtime can attribute them.

## Choosing assertion lifetimes

Because `bound` mode allows identical-bytes reuse, the assertion expiry is
the effective replay window for a fully captured request (an attacker who can
read the assertion header can read the transport token too). Issuers should
keep `expires_at_ms - issued_at_ms` short — minutes, not hours — and rotate
`assertion_id` on every re-issue. Issuers must never re-sign new claims under
an existing `assertion_id`; in `bound` mode the runtime will reject the
refreshed assertion as a substitution.

The runtime allows only a small future-issued window for clock drift. Keep
control-plane and runtime clocks synchronized with NTP. If an environment
needs a wider window during migration, `TANDEM_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS`
can temporarily raise it up to 60 seconds, but hosted deployments should use
the 10 second default.
