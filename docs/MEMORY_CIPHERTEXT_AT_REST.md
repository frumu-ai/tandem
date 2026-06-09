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
- **Hosted KMS**: requires a KMS-backed DEK via the decrypt broker. Until a KMS
  provider is provisioned (BR-12), hosted mode **fails closed** on write — it
  never silently stores plaintext.

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
architecture, tracked as a separate effort (not BR-14).

## Remaining encryptable columns (follow-up within BR-14)

These hold semantic text, are retrieved by key (not full-text/vector search), and
can adopt the same provider in a follow-up: `memory_records.metadata` /
`memory_records.provenance`, and `knowledge_items.{title,summary,payload}` /
`knowledge_spaces.{title,description}`. They are currently plaintext.

## Migration / backup

- **No migration is required.** Reads transparently pass through legacy plaintext
  rows (values without the `tce1:` prefix), so existing local/dev databases keep
  working after enabling local encryption; only new writes are encrypted.
- A backfill (re-encrypt existing rows) can be added later but is not needed for
  correctness.
- **Backups:** local plaintext installs back up portable SQLite files (host/file
  security). Local-encrypted installs must back up the key file alongside the DB
  (losing the key makes encrypted rows unrecoverable). Hosted tenant memory is
  governed by KMS, so a raw DB backup is not sufficient to read it.
