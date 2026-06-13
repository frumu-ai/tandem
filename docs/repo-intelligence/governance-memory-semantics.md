# Repo Graph And Governance Memory Semantics

Repo intelligence, workflow memory, policy scopes, and run observations share
one graph contract: all context must carry scope, provenance, freshness, and
visibility before an agent can use it.

## Fact Classes

- Source-derived repo facts are `Extracted` and can point agents to files,
  symbols, imports, docs headings, and chunks.
- Impact and relationship facts are `Inferred` unless they are direct extracted
  edges.
- Memory-derived facts are never current source truth. They must be represented
  as summaries or observations by adapters that bridge memory into repo context.
- Run observations are scoped to the run that produced them unless explicitly
  promoted.

## Scope Contract

Every query uses `GraphQueryEnvelope` with tenant/project/repo or run scope,
readable paths, allowed tools, allowed memory tiers, budgets, approvals, and an
actor id. Missing or mismatched base scope fails closed. Candidate-level path or
memory denials may still return allowed results, but only with redacted denied
counts and generic reasons.

## Display-Safe Storage

Graph payloads may store:

- display-safe refs and artifact ids
- file hashes and chunk ids
- source line ranges
- safe summaries
- confidence, freshness, and provenance
- policy scope ids and visibility classes

Graph payloads must not store:

- raw secrets
- hidden path names from denied scopes
- full memory bodies
- private prompt/tool payloads
- unredacted denied memory ids

## Agent Usage

Agents can use repo graph and memory evidence to choose first reads, narrow
searches, or explain why a result was selected. They must still read source
files before editing or making final behavioral claims. Memory summaries can
explain history, but source-derived chunks are the authority for current code.
