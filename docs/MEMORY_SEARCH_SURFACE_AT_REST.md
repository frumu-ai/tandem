# Memory Search Surface At Rest — Decision (TAN-663)

Decision record for the one residual DB-leak exposure in memory-at-rest: the
**FTS search index and the sqlite-vec embeddings are stored plaintext**.

## Context

`MemoryCryptoProvider` encrypts the semantic memory *payload* columns (chunk
content + metadata, memory layers, response cache). But the embeddings
(`sqlite-vec` `vec0` virtual tables) and the FTS-indexed content **cannot be
encrypted without breaking similarity / full-text search** — they are classified
as "search-required plaintext" (`crates/tandem-memory/src/crypto.rs:21-24`,
BR-14). A raw database dump therefore still exposes:

- the searchable text in the FTS index, and
- the embedding vectors — which are invertible enough (embedding-inversion
  attacks) to approximately reconstruct the source text.

These are protected by authority-scoped reads (the M1 tenant/department/subject
access filters), **not** by cryptography.

> Note: the per-scope envelope/KMS path from the TAN-665 spike
> (`docs/MEMORY_KEY_SCOPING_SPIKE.md`) has since landed (TAN-666): payload
> encryption uses per-scope DEK envelopes with hosted-KMS key scoping rather
> than a single process-wide key. This decision concerns the search surface
> specifically, which stays plaintext regardless of key scoping.

## Options considered

1. **Accept + mitigate (infra-layer at rest).** Treat the memory DB and its
   index as a sensitive asset: require full-disk / encrypted-volume at rest on
   the DB host, forbid plaintext backups, restrict who can obtain a dump, and
   document the residual risk. Cheapest; honest; no search regression.
2. **Deterministic / tokenized FTS.** Index tokenized or deterministically
   encrypted terms so exact-match search still works over ciphertext. Loses
   semantic ranking, and deterministic encryption **itself leaks** equality and
   frequency patterns (a term appearing in many rows is visible), so it trades
   one leak for a subtler one.
3. **Encrypted embeddings + decrypt-side rerank.** Store encrypted vectors,
   decrypt a scoped candidate window in-process for KNN/rerank. Preserves
   semantics but is expensive on the hot path and needs the crypto path to hang
   on — which does not exist until TAN-666, and interacts with the M4 pgvector
   move (TAN-660).

## Decision

**Adopt Option 1 (accept + mitigate) for now**, with Option 3 as a documented
future revisit.

Rationale:

- Option 2's deterministic-encryption leak (equality/frequency) is a poor trade
  and breaks the semantic ranking the product relies on.
- Option 3 is the principled long-term answer but has no foundation to build on
  until the per-scope envelope/KMS path is wired (**TAN-666**) and the vector
  store portability decision lands (**TAN-660**, pgvector). Building it now would
  be premature and duplicated work.
- The primary control for cross-tenant/department separation is already the M1
  access filter (structural, fail-closed), not at-rest crypto. At-rest crypto is
  defense-in-depth; the search surface is the one column it cannot cover, and
  infra-layer at rest + dump controls close most of the practical exposure.

## Required mitigations (Option 1)

- **Full-disk / encrypted-volume at rest** on any host storing `memory.sqlite`
  (and, post-M4, the Postgres data directory / pgvector store).
- **No plaintext backups** of the memory DB or FTS index; backups inherit the
  same at-rest encryption and access controls.
- **Restrict dump/query access** to the DB to the runtime principal; treat a raw
  dump as a sensitive credential-equivalent artifact.
- **Honest product narrative:** "encrypted at rest" covers the payload columns,
  **not** the search index or embeddings — state this plainly in any security
  claim and in the demo. Do not imply the FTS/vector surface is encrypted.

## Residual risk (accepted)

An actor with a raw dump *and* the ability to bypass infra-layer at-rest (e.g. a
live-host memory read, or a compromised DB host) can read the FTS content and
invert embeddings to approximate memory text. This is accepted for now and
bounded by: access control as the primary gate, infra-layer FDE, and dump
restriction.

## Follow-up

- **TAN-681** tracks the implementation follow-up for protecting the FTS and
  embedding surface (Option 3: encrypted embeddings + decrypt-side rerank).
  The envelope/KMS prerequisite (TAN-666) has landed; the vector-store
  portability prerequisite is still open — the pgvector backend is design-only
  and implemented under TAN-678. Until TAN-681 lands, the search surface is
  plaintext at rest and the Option 1 mitigations above are required.
