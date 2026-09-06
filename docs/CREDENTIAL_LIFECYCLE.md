# Durable credential lifecycle revisions

Credential revisions live beside `providers` in the existing API-key and typed
credential index files. Secrets continue to use the existing file/keychain
backends. The existing cross-process credential file lock now also covers legacy
API-key writers. Index updates preserve other bindings and deletion tombstones.

Every explicit set, disconnect, reconnect, and ordinary compare-and-set creates
fresh authorization and material UUIDs, including replacement with identical
bytes and compensation that restores earlier bytes. Before changing a secret,
the writer persists a pending record. It marks the record active only after the
credential and index writes succeed and the selected stored material matches.
Deletion leaves an absent tombstone. An interrupted or failed mutation cannot
provide an active revision. JSON writes use a private temporary file, sync the
contents, atomically replace the destination, and sync the parent directory on
Unix. Durability ultimately depends on the filesystem and keychain backend.

The dedicated OAuth refresh CAS can retain the authorization revision only when
the prior record is active, its material matches, and both credentials identify
the same nonempty account and management source. It always advances the material
revision. Changed, unknown, or untracked accounts require a new authorization
revision. The server uses this operation for successful automatic refresh;
audit-failure compensation continues to use ordinary CAS and invalidates earlier
approvals. Existing stale-refresh CAS behavior remains intact.

The revision lookup APIs read under the same file lock and reject missing
metadata, unsupported schemas, pending/absent records, corrupt files, missing
index entries, and material mismatches. They do not initialize metadata on read.
Legacy credentials remain readable through the compatibility APIs but cannot
satisfy versioned solution approval until an explicit write or refresh establishes
a tracked revision. A keychain-backed revision cannot be satisfied by a file
fallback merely because the keychain is unavailable.

Regression coverage exercises A→B→A replacement, identical reconnect,
disconnect/re-add, restart reads, same-account refresh, account changes, stale
refresh, compensation, interrupted writes, corrupt/missing metadata, and actual
cross-process API-key and OAuth writers. Existing server tests check successful
refresh and protected-audit failure against persisted revisions. These are
synthetic credential tests; no live provider account or billing is involved.

## Remaining integration

These revisions are local persistence facts, not grants. Tenant scope remains
organization/workspace/deployment scope; it does not prove personal ownership or
permission to share an account. Production solution admission must combine the
current revision with the actual registry transport binding, current user and
sharing authority, model/data-class policy, installation generation, root lineage,
and an approved price schedule. It must revalidate before every physical attempt
and propagate authority to child tasks. That production factory and activation
path remain open.

An attacker restoring an entire old credential store and its index can restore
old revisions. External audit anchors, signing-key lifecycle and clean-host
recovery acceptance remain separate requirements. Untracked environment/config
keys and changes made by older binaries fail versioned lookup; this change does
not claim to intercept external credential administration.
