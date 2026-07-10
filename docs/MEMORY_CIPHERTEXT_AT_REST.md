# Memory ciphertext-at-rest (BR-14 / TAN-128)

This document records how Tandem memory payloads are protected at rest, which
columns are encrypted, which are intentionally left as search-required
plaintext, and the migration/backup story.

## Crypto modes (BR-13 / TAN-127)

The active mode is resolved from the decrypt-broker config
(`MemoryDecryptBrokerConfig::crypto_mode()`):

- **Local plaintext** (default single-user): no encryption; relies on host/file
  security. Backups are plain SQLite files.
- **Local encrypted**: AES-256-GCM with a 256-bit key in a `0600` key file under
  the tandem home dir (`~/.tandem/memory/local_memory.key`, or
  `TANDEM_MEMORY_LOCAL_KEY_FILE`), generated on first use.
- **Hosted KMS**: requires a KMS-backed DEK via the decrypt broker. Hosted
  provisioning sets `TANDEM_MEMORY_ENCRYPTION_REQUIRED=true`,
  `TANDEM_MEMORY_DECRYPT_PROVIDER`, and
  `TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID`. Until a KMS provider is provisioned,
  hosted mode **fails closed** on write — it never silently stores plaintext.

Stored ciphertext is self-describing: `tce1:<hex(nonce(12) || ciphertext+tag)>`.

## Encrypted columns (ciphertext-at-rest)

Encrypted on write / decrypted on authorized read via
`MemoryCryptoProvider` (`crates/tandem-memory/src/crypto.rs`):

| Payload | Table.column | Write | Read |
| --- | --- | --- | --- |
| Memory chunk text | `{session,project,global}_memory_chunks.content` | `store_chunk` | `row_to_chunk` |
| Memory chunk metadata | `{session,project,global}_memory_chunks.metadata` | `store_chunk` | `row_to_chunk` |
| Context layer text | `memory_layers.content` | `create_layer` | `get_layer` |
| Cached LLM responses | `response_cache.response` | `put` / `put_scoped` | `get` |

A raw SQLite dump of these columns shows only `tce1:…` ciphertext in
local-encrypted / hosted mode; an unauthorized key cannot decrypt them.

## Search-required plaintext (classified, NOT encrypted)

These columns cannot be encrypted at rest without breaking core search and are
governed by **authority-scoped reads** (tenant/data-class/source grants via the
retrieval gateway, BR-02) plus the documented residual below:

| Payload | Why it must stay plaintext |
| --- | --- |
| `{session,project,global}_memory_vectors.embedding` | sqlite-vec KNN computes distances over the raw vector; encryption breaks similarity search. |
| `memory_records.content` | Indexed by the `memory_records_fts` FTS5 table (`content MATCH ?`); encryption breaks full-text search. |

Residual: embeddings can leak semantic content via inversion, and FTS content is
plaintext. Both are tenant-partitioned and only returned through authority-filtered
read paths. True encryption here requires a searchable-encryption / encrypted-index
architecture, tracked as **TAN-681** (see
`docs/MEMORY_SEARCH_SURFACE_AT_REST.md` for the accepted-risk decision and
required infra mitigations until it lands).

## Remaining encryptable columns (follow-up within BR-14)

These hold semantic text, are retrieved by key (not full-text/vector search), and
can adopt the same provider in a follow-up: `memory_records.metadata` /
`memory_records.provenance`, and `knowledge_items.{title,summary,payload}` /
`knowledge_spaces.{title,description}`. They are currently plaintext.

## Migration / backup

- **No migration is required.** Local plaintext compatibility reads continue to
  accept legacy rows without the `tce1:` marker, while hosted mode continues to
  fail closed on plaintext rows; existing local/dev databases keep working after
  enabling local encryption and only new writes are encrypted.
- A backfill (re-encrypt existing rows) can be added later but is not needed for
  correctness.
- **Backups:** local plaintext installs back up portable SQLite files (host/file
  security). Local-encrypted installs must back up the key file alongside the DB
  (losing the key makes encrypted rows unrecoverable). Hosted tenant memory is
  governed by KMS, so a raw DB backup is not sufficient to read it.

## Hosted KMS provider wiring (BR-12 / TAN-116)

The runtime decrypt broker remains provider-generic. The first concrete provider
adapter is `google_cloud_kms`, implemented behind the `MemoryDekUnwrapProvider`
trait. The broker still authorizes each unwrap ticket by tenant scope, data
class, source binding, policy decision, audit id, and key lifecycle evidence
before any provider is called.

Hosted Google KMS configuration is intentionally separate from the context
assertion signing key family and connector bearer-token secret families:

| Env var | Purpose |
| --- | --- |
| `TANDEM_MEMORY_ENCRYPTION_REQUIRED=true` | Selects hosted fail-closed memory mode. |
| `TANDEM_MEMORY_DECRYPT_PROVIDER=google_cloud_kms` | Selects the concrete hosted provider adapter. |
| `TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID=...` | Names the scoped runtime decrypt principal for this deployment. |
| `TANDEM_MEMORY_GOOGLE_KMS_DECRYPT_COMMAND=...` | Optional command bridge used by the Google KMS adapter. |

When the command bridge is configured, the runtime sends a JSON decrypt request
on stdin with the Google CryptoKey id, wrapped DEK, AAD, principal, scope, and
audit evidence. The command returns the plaintext 32-byte DEK as base64 (or a
JSON object with `plaintext_base64`). If the provider, principal, key id, or
command is missing or invalid, unwrap fails closed.
