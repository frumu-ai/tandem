# Hybrid GraphRAG Follow-Up

This tranche keeps deterministic repo graph retrieval as the default and adds
optional hooks for embedding-backed candidates. The graph layer remains useful
without provider keys; vector retrieval is additive evidence, not a replacement
for source-derived facts.

## Retrieval Shape

Repo intelligence now has three candidate layers:

- lexical graph search from files, symbols, imports, config, and docs
- chunk metadata from source symbols and doc sections, keyed by file hash plus
  line range
- optional vector candidates over those chunks when an embedding provider is
  configured

The internal hybrid merge path accepts graph candidates and vector candidates,
combines duplicate chunk ids, sums deterministic scores, records retriever names,
and sorts by score then stable path/id tie-breakers. Tests use fake candidates;
no external service or provider key is required.

## Provenance Rules

Every surfaced result must remain explainable:

- `RepoSearchResult.trace` records retriever type, matched term, edge path,
  confidence, scope, and ranking contribution.
- `RepoContextBundle.first_read_spans` points to source/doc chunks without raw
  content payloads.
- Vector hits must carry provenance that says they are semantic candidates.
- Memory-derived candidates must be marked as summaries or observations by
  adapters and must not overwrite source-derived truth.

## Governance Rules

Graph and memory retrieval share the same fail-closed envelope semantics:

- tenant/project/repo or tool mismatches return no payload
- path denials return allowed data plus denied counts/reasons
- denied paths, memory ids, hidden names, and payload bodies stay redacted
- audit records store counts, generic reasons, refs, hashes, and safe summaries

## Next Hook Points

The next implementation step is a retriever adapter that can:

1. load `RepoChunk` ids for the current snapshot
2. request embeddings only when a configured provider is available
3. return vector `RepoHybridCandidate` values
4. merge them with lexical graph candidates
5. expose merged provenance through the existing context bundle trace fields
