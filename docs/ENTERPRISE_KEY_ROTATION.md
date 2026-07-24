# Enterprise Signing Key Rotation

Document status: verifier contract plus target hosted operating procedure.

Implementation review: 2026-07-14 against `origin/main` at `801559fd`.

Operator reference for the intended hosted Tandem signing-key model and the
public verifier keyrings that runtime and ACA load (EAA-04 / TAN-29). The
keyring verification contract is implemented. The repository does not contain a
general hosted KMS signing service or an automated key-distribution/rotation
control plane, so the KMS custody and rotation steps below are required target
operations rather than a repository-proven deployed guarantee.

## Trust model

- **Target hosted boundary: private key material never leaves the hosted control
  plane / KMS.** Hosted signing of context assertions, approval receipts,
  delegation projections, and cross-tenant grants should use private keys held
  in Google KMS or an equivalent customer-managed signer.
- **Runtime and ACA receive only public verifier keyrings.** A keyring is a
  `kid -> public key` map where every entry is scoped by purpose, organization,
  deployment, audience, resource scope, status, and validity window.
- Verification routes through a single fail-closed lookup
  (`VerifierKeyring::resolve_verifying_key`, in `tandem-enterprise-contract`):
  a key resolves only when it is registered, **active**, **in window**, and its
  declared **purpose** and **scope** match the token being verified. A key minted
  for one lane (e.g. `context_assertion`) can never verify another lane (e.g.
  `approval_receipt`), and a key scoped to one org/deployment/audience/resource
  prefix cannot verify outside that scope.

Current repository behavior is narrower:

- The shared contract implements purpose- and scope-bound Ed25519 verifier
  keyrings, active/retired/revoked status, validity windows, and fail-closed key
  lookup. Runtime and ACA context assertions route through the same verifier.
- Hosted assertion state is loaded before bind into an immutable snapshot.
  Legacy metadata-free keys are rejected; Unix keyring files must be owned by
  the runtime user and `0600` or stricter.
- Rotation is explicit through permission-checked `POST /admin/reload-config`.
  A complete next keyring and replay backend are validated before one atomic
  snapshot swap. Invalid reloads retain the last-known-good generation.
- Protected reload audit records previous/current SHA-256 keyring fingerprints,
  change status, key count, replay mode, and lifetime policy without key bytes.
- The enterprise cross-tenant grant issuance route currently loads its private
  Ed25519 signing key from an environment variable or file. That compatibility
  path is not KMS custody and must not be presented as the target hosted design.
- Approval-receipt and other signing-lane types/verifiers exist, but source
  presence alone does not prove that a production control plane issues every
  token type through KMS.

## Keyring entry schema

Each `kid` maps to:

| Field                             | Required              | Meaning                                                                                                                                                                |
| --------------------------------- | --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `purpose`                         | yes                   | Lane the key may verify (`context_assertion`, `approval_receipt`, `delegation_projection`, `cross_tenant_grant`, `a2a_peer_assertion`, `break_glass_admin_assertion`). |
| `public_key`                      | yes                   | Base64 (url-safe or standard) of the 32-byte Ed25519 public key.                                                                                                       |
| `organization_id`                 | no                    | Restricts the key to one org. Omit for a global key.                                                                                                                   |
| `deployment_id`                   | no                    | Restricts the key to one deployment.                                                                                                                                   |
| `allowed_audiences`               | no                    | Allowlist of token audiences. Empty = unrestricted.                                                                                                                    |
| `allowed_resource_scope_prefixes` | no                    | Token resource scope must start with one prefix. Empty = unrestricted.                                                                                                 |
| `status`                          | no (default `active`) | `active` verifies; `retired` and `revoked` never verify.                                                                                                               |
| `not_before_ms` / `not_after_ms`  | no                    | Validity window (epoch ms).                                                                                                                                            |
| `kms_key_reference`               | no                    | Control-plane reference to the KMS key/version holding the **private** key. Metadata only; runtime/ACA never use it.                                                   |

### Distribution form (JSON)

The `kid` is the map key; runtime/ACA load this via `VerifierKeyring::from_json`:

```json
{
  "ctx-2026-06-a": {
    "purpose": "context_assertion",
    "public_key": "BASE64_ED25519_PUBLIC_KEY",
    "organization_id": "org-a",
    "deployment_id": "deploy-1",
    "allowed_audiences": ["tandem-runtime"],
    "allowed_resource_scope_prefixes": ["org-a/workspace-a"],
    "status": "active",
    "not_before_ms": 1735689600000,
    "not_after_ms": 1767225600000,
    "kms_key_reference": "projects/p/locations/l/keyRings/r/cryptoKeys/ctx/cryptoKeyVersions/3"
  }
}
```

## Key statuses

- **active** — in service; may verify tokens within its window and scope.
- **retired** — rotated out; kept in the keyring for audit/lookup but rejected
  for verification (`verifier_key_retired`). Use during overlap windows so
  in-flight tokens signed by the previous key fail closed once it is retired.
- **revoked** — compromised or explicitly killed; always rejected
  (`verifier_key_revoked`).

## Target Hosted Rotation Procedure

This procedure must be implemented by the hosted control plane or deployment
operator. The reviewed repository does not automatically mint KMS versions,
publish keyrings, reload all verifiers, cut over signers, or attest completion.

1. **Mint** a new private key/version in KMS for the target `purpose` and scope.
2. **Publish** the new public key into the keyring as a second `active` entry
   with a fresh `kid` and a `not_before_ms` at/after the cutover. Keep the old
   entry `active` during the overlap window so both verify.
   Publish the complete file atomically with owner-only permissions, then call
   `POST /admin/reload-config` and verify the protected old/new fingerprint
   event before signer cutover.
3. **Cut over** signing in the control plane to the new `kid`.
4. **Retire** the old entry (set `status: "retired"`) once no valid tokens can
   still bear the old `kid` (i.e. after the old key's tokens have all expired).
5. **Revoke** instead of retire if a key is believed compromised — set
   `status: "revoked"` immediately and rotate as above; revoked keys never
   verify regardless of window.

Before claiming managed rotation, the deployment should additionally prove:

1. Atomic or versioned keyring publication to every runtime/ACA verifier.
2. Readiness checks that identify stale keyring versions before signer cutover.
3. Audit events for mint, publish, cutover, retire, revoke, and rollback.
4. Emergency revocation propagation with a measured maximum delay.
5. A rollback procedure that does not re-enable compromised keys.
6. Rotation evidence retained outside the runtime being verified.

## Scoping guidance

- Prefer org- and deployment-scoped keys over global keys; reserve unscoped
  (no `organization_id`/`deployment_id`) keys for genuinely platform-wide lanes.
- Set `allowed_audiences` so a runtime key cannot verify tokens minted for a
  different service, and `allowed_resource_scope_prefixes` to bound a key to the
  resource subtree it is meant to cover.
- Always set a validity window; unbounded keys are harder to retire safely.
