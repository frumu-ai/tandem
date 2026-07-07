# Memory At-Rest Key Scoping — Spike (TAN-665)

Feeds the decision in **TAN-662** (add `org_unit` / department to the memory
encryption key scope). Timeboxed investigation; **no production code changes**.

## Question

For the department-scoped agent work, should department become a *cryptographic*
key dimension so a raw DB leak cannot decrypt another department's memory —
and if so, at what cost?

- **Option 1** — add `org_unit` to `MemoryKeyScope`: a distinct wrapped DEK per
  `(tenant × department × data_class × source)`.
- **Option 2** — map departments onto existing `data_class` / `source_binding`
  values (which already key separately), adding no new key dimension.

## TL;DR recommendation

1. **Option 1 (org_unit as a key-scope dimension) is the right target**, and its
   steady-state cost is modest **provided a envelope-keyed DEK cache exists**.
   Reject Option 2 — it overloads `data_class`, which is also a DLP / decrypt-grant
   axis, and would entangle department membership with data-loss policy.
2. **But Option 1 is currently premature.** The per-scope, KMS-backed envelope
   layer that `MemoryKeyScope` belongs to **is not wired into the read/write path**
   — it is designed and unit-tested only. Today's *active* encryption is a single
   process-wide key with **no tenant/data-class/department scoping at all**.
3. **New prerequisite discovered:** wire the envelope + decrypt-broker + KMS
   provider into the actual encrypt/decrypt path, with a envelope-keyed DEK cache,
   **before** TAN-662. Filed as a dependency (see "Impact" below). Adding
   `org_unit` to a key scope that nothing reads/writes would be dead code.

## Findings

### 1. There are two encryption layers, at very different maturity

**Active layer — `MemoryCryptoProvider` (`crates/tandem-memory/src/crypto.rs`).**
This is what actually encrypts memory today. The `MemoryDatabase` constructs
**one** provider `from_env()` (`memory_database_impl_parts/part01_a.rs:69`) and
uses it for every row (`encrypt_field` at `part01_a.rs:1348,1357`; `db.rs:513`).
Modes (`crypto.rs:95-117`):

- `LocalPlaintext` (default): no-op.
- `LocalEncrypted`: AES-256-GCM with a **single host key file** —
  `load_or_create_local_key(&local_key_path())`, and `local_key_path()` takes no
  tenant argument (`crypto.rs:99,225`). One key for the whole database.
- `HostedKms`: returns `HostedPending` → `encrypt_field` **fails closed**
  (`crypto.rs:113-115,130-134`). The comment is explicit: "Hosted KMS-backed
  encryption requires a provisioned DEK provider (BR-12). Until then, fail closed."

So the active layer has **no per-tenant, per-data-class, or per-department key
separation**. In `LocalEncrypted` mode a DB leak is protected by exactly one key;
if that key file is captured with the DB, everything decrypts and there is zero
cross-tenant/department cryptographic isolation. In `HostedKms` mode, writes fail
closed (nothing is stored) because the provider is unimplemented in this path.

**Per-scope layer — envelope + broker + KMS
(`envelope.rs`, `decrypt_broker.rs`, `kms_providers.rs`).** This is the
sophisticated design: `MemoryEnvelopeMetadata { key_scope, kek_id, wrapped_dek, … }`
with `MemoryKeyScope = (org × workspace × deployment × data_class × source_binding)`
(`envelope.rs:10`), a `MemoryDecryptBroker` that authorizes per-scope DEK unwrap
against a scoped principal (`decrypt_broker.rs:361`), and a Google Cloud KMS
provider (`kms_providers.rs:80`). It fails closed on wildcard scopes, tenant
mismatch, missing data-class/source grants, and revoked/disabled keys.

**This layer is not wired to anything but tests.** Every non-test caller of
`authorize_unwrap` / `unwrap_dek` is in `#[cfg(test)]`; `MemoryEnvelopeMetadata`
is only constructed in tests; `MemoryCryptoProvider::from_mode(HostedKms)` does
**not** call the broker — it returns `HostedPending`. The two layers are not
connected.

**Consequence:** the "per-scope DEK isolates tenants at rest" property that
TAN-662 (and the project's DB-leak narrative) builds on **does not exist in a
running deployment yet.** It is a schema + authorization design awaiting
integration.

### 2. KMS unwrap cost: subprocess-per-call, no DEK cache

The Google Cloud KMS provider unwraps a DEK by **spawning an external subprocess**
per call — `Command::new(command_path)` with a JSON request on stdin
(`kms_providers.rs:167-201`), driven by `TANDEM_MEMORY_GOOGLE_KMS_DECRYPT_COMMAND`.
There is **no DEK cache** anywhere in `tandem-memory` (the only `lru` reference is
the unrelated response cache, `response_cache.rs:498`).

So *as written*, each DEK unwrap = one process spawn + one KMS API round-trip.
On a hot retrieval path that decrypts N rows spanning K distinct key scopes, an
un-cached design would issue up to **K KMS calls per query** (or N, if unwrap were
done per row). This is the real cost driver — **not** the number of key
dimensions. Any hosted deployment needs a envelope-keyed DEK cache before encryption
is viable at all, independent of the department question.

### 3. `data_class` is already an authorization axis (why Option 2 is bad)

The decrypt broker enforces **per-data-class and per-source-binding decrypt
grants**: a `MemoryDecryptPrincipal` carries `allowed_data_classes` +
`allowed_source_binding_ids`, and unwrap is denied without the matching grant
(`decrypt_broker.rs:510-530`). `data_class` also drives DLP / redaction / data
boundary decisions elsewhere. Mapping departments onto `data_class` (Option 2)
would therefore overload one field with two orthogonal meanings —
"how sensitive is this" and "which department owns this" — and couple department
membership to DLP policy. Rejected.

## Cost analysis — adding `org_unit` (Option 1)

Assume the envelope/KMS path is wired **with** an envelope-keyed DEK cache
keyed by `(canonical_id, kek_version, rotation_epoch)` — **not** `canonical_id`
alone. During rotation/backfill the same scope can hold rows under different
`wrapped_dek` / `kek_version`, so a scope-only cache would return the first
unwrapped DEK for later rows encrypted under a newer key version and cause GCM
decrypt failures. Keying by the envelope's key identity lets old and new rows
coexist; entries are LRU-evicted and invalidated on revocation. (Alternative:
require every rotation to rewrite all rows before mixed versions are readable —
rejected as operationally heavier than a versioned cache key.)

- **Key / DEK count.** Distinct scopes today = `tenant × data_class × source`.
  Adding `org_unit` multiplies by department cardinality. Departments are
  **low-cardinality** (typically 5–50 per tenant), so this is a small constant
  factor, not a blow-up. Wrapped DEKs are stored per row in envelope metadata; the
  KEK count in KMS grows with `(tenant × org_unit)` at most, still small.
- **KMS call volume.** With the cache, steady-state KMS calls are
  **O(distinct scopes touched), amortized** — a query that reads one department's
  rows unwraps one DEK once and reuses it. Adding `org_unit` increases the number
  of *distinct* cache entries / cold-miss unwraps, **not** steady-state per-query
  calls. Without the cache, cost is dominated by the subprocess-per-unwrap issue
  regardless of department — i.e. the cache is the real prerequisite, org_unit is
  a second-order multiplier on cache size.
- **Rotation.** Rotation/revocation is per key scope (`key_lifecycle.rs`
  `revoked_scopes`, `MemoryKeyScopeRevocation`), so rotation work scales with the
  scope count — `org_unit` multiplies it by department cardinality. In exchange
  you gain **per-department crypto-shred** (revoke one department's scope without
  touching others), which is a real security benefit and aligns with the M1
  access-control model.

**Verdict:** Option 1's marginal cost over the (already-required) cached
envelope/KMS wiring is a low-cardinality constant factor on DEK/cache/rotation —
acceptable, and it buys per-department crypto-shred.

## Impact on the project

1. **New prerequisite issue (P0 for the at-rest story):** *Wire the per-scope
   envelope + decrypt-broker + KMS provider into the memory encrypt/decrypt path,
   with a envelope-keyed DEK cache.* Until this lands, encrypted mode is single-key
   and TAN-662 has nothing to extend. This is the true blocker, not department
   cost.
2. **TAN-662 depends on (1)** and should add `org_unit` to `MemoryKeyScope`
   **as part of / immediately after** the wiring, so the key shape is right before
   any demo data is encrypted (same "get the shape right before data lands"
   argument as pulling TAN-659 forward).
3. **TAN-663** (plaintext search-surface decision) is unaffected — embeddings /
   FTS remain plaintext regardless of key scoping.
4. **Demo caveat:** the demo should run either `LocalPlaintext` (and rely on
   access control + the honest "encryption not enabled" note) or wait for the
   wiring — it must **not** claim per-tenant/department encryption-at-rest on the
   current single-key `LocalEncrypted` path.

## Recommendation

- Adopt **Option 1** (org_unit as a `MemoryKeyScope` dimension) + a scope-keyed
  DEK cache.
- **Sequence:** envelope/KMS wiring + DEK cache **first** (new prerequisite),
  then TAN-662 folds `org_unit` into the key scope, then the M1 department column
  work (TAN-645/646) shares the same org-unit value for row column and key scope.
- Reject **Option 2** (data_class overload).

*Timebox: complete. No production code changed by this spike.*
