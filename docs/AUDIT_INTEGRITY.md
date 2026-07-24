# Protected audit and store integrity

Tandem schema-v3 protected audit records and encrypted protected-store heads use
HMAC-SHA256 with explicit key IDs. The last committed head is also written after
every successful keyed commit to `TANDEM_AUDIT_ANCHOR_DIR`. That directory must be
an absolute path outside the Tandem state-directory tree and must be writable only
by the engine or a dedicated anchoring service. A state-directory writer cannot
recompute a valid HMAC or roll the ledger behind the external head.

Hosted and enterprise posture fails startup configuration validation unless both
an audit-integrity key source and `TANDEM_AUDIT_ANCHOR_DIR` are configured. Local
single-tenant posture retains legacy SHA-256 compatibility when the complete
key-plus-anchor pair is absent.

## Key sources

Configure exactly one of:

- `TANDEM_AUDIT_HMAC_KEY` plus optional `TANDEM_AUDIT_HMAC_KEY_ID` (the ID defaults to `primary`).
- `TANDEM_AUDIT_HMAC_KEY_FILE` plus optional `TANDEM_AUDIT_HMAC_KEY_ID`.
- `TANDEM_AUDIT_HMAC_KEYRING_FILE` for rotation and revocation.

Unix key and keyring files must be regular, single-link files owned by the effective
user with mode `0600`; symlinks and oversized files are rejected. Hosted keys must
contain at least 32 bytes. Key bytes are never logged or returned by the manifest.

Keyring format:

```json
{
  "active_key_id": "audit-2026-07",
  "keys": [
    {
      "id": "audit-2026-06",
      "purpose": "audit_integrity",
      "status": "verify_only",
      "key": "replace-with-secret-material-from-your-secret-renderer"
    },
    {
      "id": "audit-2026-07",
      "purpose": "audit_integrity",
      "status": "active",
      "key": "replace-with-new-secret-material-from-your-secret-renderer"
    }
  ]
}
```

Only the named `active` key signs new records and heads. `verify_only` keys verify
retained segments. `revoked`, missing, duplicate, malformed, or wrong-purpose keys
fail verification. Keep the keyring file as a secret-rendered runtime file; do not
commit it.

## Rotation

1. Add the new key as `active`, change the prior active key to `verify_only`, and
   retain every key referenced by records inside the retention window.
2. Run `tandem-engine config check`, then restart or atomically replace the
   secret-rendered keyring according to the deployment procedure.
3. The first new record carries the new key ID with `segment_start: true`; subsequent
   records in that segment carry `segment_start: false`. Protected-store frames carry
   their own key ID and remain linked to the previous digest.
4. Call `GET /audit/ledger/manifest`. Confirm `schema_version: 3`, the expected
   `integrity_key_ids`, `external_anchor.verified: true`, and the expected record counts.
5. Change an old key to `revoked` only when every ledger/store segment that requires
   it has expired under the approved retention policy. Revoking early intentionally
   makes those retained segments unavailable rather than silently trusting them.

## Legacy migration

Existing schema-v1/v2 records retain their original public SHA-256 verification.
The first schema-v3 record is an explicit keyed segment boundary whose HMAC covers
the prior public root through `prev_hash`. After that boundary, an unkeyed record is
a downgrade and is rejected. The manifest reports `legacy_record_count`,
`keyed_record_count`, and all referenced key IDs so migration progress is auditable.
Protected-store collections and JSONL frames similarly move to a keyed generation on
their next encrypted write. Hosted deployments should complete and record this
migration before approving release.

## External anchors and rollback response

Every keyed audit/store commit writes an authenticated JSON anchor to the configured
external directory using create-new temporary files, file sync, atomic replacement
(Windows uses replace-existing/write-through semantics), and parent-directory sync on
Unix. The Unix directory must be a real effective-user-owned
`0700` directory; anchor reads require regular, single-link, effective-user-owned
`0600` files and reject links, oversized content, or permission drift. On Windows, the
directory, existing anchor, and create-new temporary anchor are opened with reparse-point
protection and validated from their opened-handle security descriptors. The owner must
match the process identity, and all granting DACL entries, including inherited entries,
must target that owner, LocalSystem, or built-in Administrators. Null DACLs, unprivileged
grants, and unsupported granting ACE forms fail closed. Reads require the external
generation and digest to match the local committed head exactly. Missing anchors, an older local generation,
a substituted digest, or an invalid anchor MAC fail closed.

Protect and replicate the anchor directory independently of the Tandem state volume.
For regulated retention, export it to an append-only/WORM system on the same or a
shorter interval than the audit retention objective. The local anchor adapter is a
cryptographic rollback root, not itself a managed WORM service.

On mismatch:

1. Stop writes and preserve the ledger, sidecars, anchor directory, keyring version,
   and engine logs as evidence. Do not delete or overwrite the conflicting anchor.
2. Compare the external anchor with immutable backups/SIEM exports and identify the
   last mutually verified generation.
3. Restore a matching ledger, integrity sidecars, and external anchor as one recovery
   unit. Retain all key IDs referenced by the restored data.
4. Run the manifest endpoint and the focused security regression suite before
   reopening writes. Record the recovery decision in an independent incident log.

The manifest endpoint is the supported online operator verifier and alert source.
Any `AUDIT_MANIFEST_ERROR`, `AUDIT_LEDGER_UNAVAILABLE`, anchor mismatch, key-status
failure, or decrease in keyed count/sequence should page the audit-integrity owner.
