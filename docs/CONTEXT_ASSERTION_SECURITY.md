# Context Assertion Security

Signed tenant context assertions are the trust primitive for hosted and
enterprise runtime modes. In `hosted_single_tenant` and `enterprise_required`
modes the runtime rejects raw tenant headers and requires an EdDSA-signed
assertion (JWS, `header.claims.signature`) on one of:

- `x-tandem-context-assertion`
- `x-tandem-context-jws`
- `x-tandem-tenant-context-jws`

Verification is fail-closed in the shared
`tandem-enterprise-contract::context_assertion_security` module. Runtime and
ACA both use that verifier; the runtime publishes one immutable snapshot at
startup and request processing never rereads environment variables or keyring
files.

1. Ed25519 signature over `header.claims`, key selected by `kid`.
2. Claims validation: version `v1`, issuer/audience match, expiry, issued-at
   skew, maximum lifetime, non-empty `assertion_id`/actor/org/workspace,
   explicit tenant source with deployment scope, and actor consistency across
   `tenant_context`, `human_actor`, and `authority_chain.initiated_by`.
3. Key metadata validation: key status, purpose, lifetime window, allowed
   audiences, organization/deployment restrictions, resource scope prefixes.
4. Atomic shared replay policy (below).
5. Canonical `VerifiedTenantContext` projection returned to consumers.

## Key configuration

| Variable | Meaning |
| --- | --- |
| `TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS` / `..._FILE` | JSON keyset keyed by `kid`. Hosted/enterprise entries require explicit `purpose` and `status`; org, deployment, audience, resource, and validity metadata are enforced when present. |
| `TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY` / `..._FILE` | Legacy metadata-free single key. Local migration compatibility only; hosted/enterprise startup rejects it. |
| `TANDEM_CONTEXT_ASSERTION_ISSUER` | Expected `issuer` claim. Default `tandem-web`. |
| `TANDEM_CONTEXT_ASSERTION_AUDIENCE` | Expected `audience` claim. Default `tandem-runtime`. |
| `TANDEM_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS` | Maximum accepted future `issued_at_ms` skew. Default `10000`; valid range `10000..=60000`. |
| `TANDEM_CONTEXT_ASSERTION_MAX_LIFETIME_MS` | Maximum `expires_at_ms - issued_at_ms`. Default 15 minutes (`900000`); valid range `1..=3600000` (one-hour hard ceiling). |
| `TANDEM_CONTEXT_ASSERTION_REPLAY_MODE` | `bound` (default), `one_shot`, or `off`. Hosted/enterprise startup rejects `off`. |
| `TANDEM_CONTEXT_ASSERTION_REPLAY_STORE_FILE` | Shared durable transactional SQLite replay ledger. Required and opened before bind in hosted/enterprise mode. All replicas must point at the same supported shared-filesystem path. |

If no key is configured in hosted/enterprise mode, all assertion-bearing
requests are rejected (`context_assertion_key_not_configured`).
On Unix, hosted runtime and ACA keyring files plus configured replay database files must be
regular, non-symlink files owned by the runtime user with no group/world permissions
(`0600` or stricter).

At verifier initialization and reload, the runtime logs only the SHA-256 keyring
fingerprint, key count, replay mode, and maximum lifetime. Public key bytes are
never logged. Verified assertions retain the selected `kid` as assertion metadata
so downstream strict projections and audit evidence can show which configured
verifier key accepted the request.

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

- Hosted startup creates or opens an owner-only SQLite replay database. Immediate
  transactions and rollback journaling provide cross-process serialization and crash
  atomicity; backend unavailability, corruption, replacement, or capacity exhaustion
  fails closed.
- Every replica must use the same replay-store path on storage that provides reliable SQLite cross-process file locking. A per-pod local path does not satisfy the
  multi-replica guarantee.
- Entries are retained until assertion expiry plus a 60-second grace window.
  Storage is bounded to 100,000 live entries globally and 10,000 per
  issuer/audience namespace.
- Durable keys are SHA-256 hashes of length-delimited issuer, audience, and
  assertion ID. Stored namespace and assertion identifiers are hashed; exact
  assertion fingerprints are stored, never assertion or token bytes.
- Rejections increment `tandem_context_assertion_rejections_total{reason=...}`.
  Lifetime/overflow anomalies also emit a warning without assertion bytes.

## Choosing assertion lifetimes

Because `bound` mode allows identical-bytes reuse, the assertion expiry is the
effective replay window for a fully captured request. Issuers should keep
`expires_at_ms - issued_at_ms` within the 15-minute default and rotate
`assertion_id` on every re-issue. The verifier rejects any configured maximum
above one hour and uses checked arithmetic for lifetime and skew calculations.
Issuers must never re-sign new claims under an existing `assertion_id`; in
`bound` mode the runtime rejects the refreshed assertion as a substitution.

The runtime allows only a small future-issued window for clock drift. Keep
control-plane and runtime clocks synchronized with NTP. If an environment
needs a wider window during migration, `TANDEM_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS`
can temporarily raise it up to 60 seconds, but hosted deployments should use
the 10 second default.

## Explicit reload and rotation

Startup loads and validates the complete keyring, policy, and replay backend
before the server binds. Rotation uses the permission-checked existing
`POST /admin/reload-config` operation:

1. Replace the keyring file atomically with a complete `0600` file.
2. Call the admin reload endpoint through the normal host-effect authorization
   boundary.
3. The runtime parses and validates the complete next generation, verifies
   replay readiness, then swaps one `Arc` snapshot under a write lock.
4. Invalid or partial replacement returns `400` and leaves the last-known-good
   generation live.
5. A protected `context_assertion.verifier_reloaded` event records previous and
   current SHA-256 keyring fingerprints, whether they changed, key count,
   replay mode, and lifetime policy. No key bytes are logged.

ACA uses the same verifier with `ACA_CONTEXT_ASSERTION_ISSUER`,
`ACA_CONTEXT_ASSERTION_AUDIENCE`, `ACA_CONTEXT_ASSERTION_MAX_LIFETIME_MS`,
`ACA_CONTEXT_ASSERTION_REPLAY_MODE`, and a required shared
`ACA_CONTEXT_ASSERTION_REPLAY_STORE_FILE` (or Tandem replay-store fallback). ACA
rejects replay mode `off`, requires explicit key purpose/status metadata, and applies
the same owner-only regular-file/no-symlink checks to file-backed keyrings.
