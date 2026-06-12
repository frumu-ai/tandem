# Tandem Repo Intelligence Architecture

Tandem repo intelligence is the source-derived graph layer that coding agents,
workflow tools, and runtime diagnostics use before broad file discovery. The
core implementation lives in the main Tandem repo so desktop, engine, tools, and
ACA can share one deterministic source of repo facts.

## Why It Exists

Coding agents need fast orientation before they spend context on broad file
reads, grep sweeps, or memory retrieval. Repo intelligence gives them a
deterministic map of what the repository currently contains:

- files that are in scope for indexing
- source symbols and imports
- config references
- documentation headings
- graph edges that explain why a file or symbol may matter
- likely first reads, impact hints, and likely test targets for a task

The layer is intentionally source-derived. It is not a private knowledge store,
not an agent diary, and not a substitute for reading the files that will be
edited. Its job is to make the first minutes of a coding run less blind while
keeping provenance, confidence, and fallback behavior visible.

## Ownership Boundary

The shared core is `crates/tandem-repo-intelligence`.

It owns:

- gitignore-aware file discovery
- file manifest entries with size, mtime, and content hash
- deterministic incremental change detection
- repo graph schema primitives for source-derived facts
- query APIs such as `repo_file`, `repo_search`, `repo_symbol`,
  `repo_neighbors`, `repo_impact`, and `repo_context_bundle`
- JSON snapshot persistence through `JsonRepoIndexStore`

It does not own:

- semantic memory ranking
- LLM-generated summaries
- ACA prompt construction
- tenant-specific policy decisions
- UI rendering

ACA should consume the shared core through a tool, CLI, or API boundary. During
rollout, ACA keeps `repo_truth.py` as a fallback when the shared index is
unavailable, stale, or policy-denied.

## Crate, API, and Tool Boundaries

The crate boundary is the deterministic library surface:

- `scanner` walks the repo with ignore rules and file-size/type exclusions.
- `manifest` and `model` define stable source-derived records.
- `extractors` turn files into symbols, imports, config references, and doc
  headings.
- `query` exposes deterministic lookup and search over a loaded snapshot.
- `context` builds graph neighbors, impact summaries, and bounded context
  bundles.
- `store` persists and loads `.tandem/repo-index.json`.

The tool boundary lives in `tandem-tools` and is the supported agent-facing
surface:

- `repo.index` builds and persists the deterministic index for a repo.
- `repo.update_changed_files` refreshes the index after edits; the current MVP
  performs a full rescan and records `refresh_mode: full_rescan`.
- `repo.search` searches indexed files, symbols, imports, config, and docs.
- `repo.symbol` finds indexed symbols by name and optional kind.
- `repo.neighbors` traverses graph neighbors from a file, symbol, or graph node.
- `repo.impact` summarizes changed-file fallout.
- `repo.context_bundle` builds a deterministic, budgeted task bundle.
- `repo.test_targets` returns likely test targets from impact analysis.

Tool results include metadata with the tool name, repo root, store path,
`index_source`, and the structured payload. Agents should treat that metadata
as part of the evidence trail, especially when debugging fallback behavior.

The prompt boundary remains outside repo intelligence. A context bundle is a set
of pointers, reasons, graph edges, and gaps; it is not the final provider prompt
and does not by itself authorize claims about file contents.

## Graph Model

The shared context graph taxonomy is defined in
[`context-graph-taxonomy.md`](./context-graph-taxonomy.md) and implemented by
`crates/tandem-graph-core`. Repo intelligence remains the first adapter into
that shared model.

Repo graph nodes should start with these source-derived entities:

- `repository`
- `file`
- `symbol`
- `module_or_package`
- `config_entry`
- `test_target`
- `workflow_reference`
- `tool_reference`
- `policy_reference`

Edges should start with:

- `contains`
- `imports`
- `defines`
- `references`
- `configures`
- `tests`
- `likely_related`
- `changed_with`

Facts carry provenance:

- `Extracted`: directly parsed from source or config
- `Inferred`: deterministic but indirect
- `Summary`: compressed or memory-derived
- `Ambiguous`: useful hint that requires confirmation

Agents may use graph output for discovery and planning, but exact files named in
a task remain mandatory source evidence. Before editing or making final claims,
agents must read concrete files from the repo.

## How It Differs From Memory Search and Grep

Repo intelligence, memory search, and grep answer different questions:

| Source            | Best For                                                      | Limits                                                                  |
| ----------------- | ------------------------------------------------------------- | ----------------------------------------------------------------------- |
| Repo intelligence | "What files, symbols, configs, docs, and graph edges matter?" | Index-based; can be stale; only sees supported extracted fact types.    |
| Memory search     | "What did a prior run, user preference, or durable note say?" | May be semantic, summarized, or cross-session; not source truth.        |
| Grep/read         | "What exact bytes are in this repository right now?"          | Accurate but broad searches can be noisy and expensive in context/time. |

Expected agent behavior:

1. Use repo intelligence for orientation and scoped first reads.
2. Use grep when exact text, unsupported language features, or missing graph
   facts matter.
3. Use memory only for preferences, prior decisions, and historical context.
4. Use direct file reads as the final source of truth before edits, reviews, or
   user-facing claims.

## Governance Contract

Every index and query request must include a governance envelope before this
layer is exposed through hosted tools:

- tenant id
- workspace or project id
- actor id
- automation or run id when available
- repo id and worktree path
- readable path scope
- writable path scope
- allowed tools and memory tiers
- approval and budget context
- context assertion metadata when applicable

Missing scope fails closed. Query results must be filtered by readable paths and
tenant/project visibility before they reach agents. Context bundles should
include policy-denied counts or reasons without leaking hidden file names or
payloads.

`crates/tandem-graph-core` defines `GraphQueryEnvelope` and `GraphQueryAudit`;
repo intelligence applies that envelope in governed query wrappers before the
`repo.*` tool surface returns graph-derived results.

The same crate also defines typed `ContextNodePayload` variants for tool,
memory, policy, approval, and artifact nodes. These payloads are the shared
contract for non-repo context graph adapters. They are display-safe by
construction: credentials are represented by opaque refs and status metadata,
schemas/artifacts by hashes and summaries, and policy/runtime details by scoped
IDs. Raw tokens, credential material, artifact contents, and sensitive hidden
payloads remain in their owning stores.

Trust semantics are centralized in graph-core:

- `Provenance::Extracted`, `Configured`, and `Observed` are source-truth capable.
- `Inferred`, `Summarized`, and `Ambiguous` are planning hints and require source
  confirmation before final claims or edits.
- `Freshness` can carry revision, checked-at, and stale-after metadata so stale
  graph facts trigger refresh or fallback.
- `Visibility` binds facts to tenant/project/run/readable-path scope and records
  redaction.

## Reuse Points

Existing code that informed this slice:

- desktop memory indexing in `apps/tandem-desktop/src-tauri/src/memory/indexer.rs`
- `tandem-memory` file indexing and memory storage
- `tandem-tools` grep/codesearch/LSP discovery tools
- ACA `repo_truth.py` heuristic discovery and manager prompt injection

The new crate reuses the deterministic file-walking shape from desktop indexing
without depending on Tauri, memory storage, or ACA internals.

## Agent Workflow

For autonomous coding work, the intended repo-intelligence flow is:

1. Confirm the active repo root, readable path scope, writable path scope, task
   source, and execution backend before indexing.
2. Run `repo.index` at the start of a run when no trustworthy snapshot exists.
3. Call `repo.context_bundle` with the task, any user-required files, changed
   files if known, path scope, budget, and result limit.
4. Read the `suggested_first_reads` and any explicit task files before
   planning edits.
5. Use `repo.search`, `repo.symbol`, `repo.neighbors`, and `repo.impact` for
   follow-up questions instead of repeatedly broadening discovery.
6. After edits, call `repo.update_changed_files` with changed paths before
   relying on graph impact or likely test targets.
7. Use `repo.test_targets` or `repo.impact` to choose focused checks, then
   report which files, tools, index source, and gaps informed the answer.

If repo intelligence is unavailable, stale, empty, or denied by policy, the
agent should fall back to direct file reads, grep/codesearch, and the current
ACA fallback discovery path. The final answer should make that fallback visible
when it affects confidence.

## Current Extraction MVP

The first extraction pass is intentionally conservative and deterministic:

- Rust: `use`, `fn`, `struct`, `enum`, `trait`, `impl`, and `mod`
- TypeScript/JavaScript: `import`, exported/local functions, classes,
  interfaces, type aliases, and constants
- Python: `import`, `from ... import`, `def`, `async def`, and `class`
- Config/docs: TOML/YAML/JSON-style key/value lines and Markdown headings with
  short excerpts

This MVP favors low false-positive rates and stable tests over deep language
coverage. Tree-sitter or LSP-backed extraction can replace individual
language extractors later without changing the public fact types.

## Storage and Query MVP

The first durable store is a JSON snapshot written by `JsonRepoIndexStore`.
It persists:

- manifest entries
- extracted facts
- root label
- index timestamp

This is intentionally simpler than SQLite while the graph schema is still
settling. The query API is deterministic and testable without Tauri:

- `repo_file` returns manifest metadata for a relative path
- `repo_symbol` finds symbols by name and optional kind
- `repo_search` searches files, symbols, imports, config references, and docs
- `edges_by_relation` exposes graph-like edges for defines/imports/config/docs
- `repo_neighbors` traverses graph edges from a file, symbol, or graph node
- `repo_impact` summarizes changed-file fallout, including import neighbors,
  config/docs evidence, and likely test targets
- `repo_context_bundle` turns task intent plus optional required/changed files
  into a deterministic, budgeted set of first reads, relevant symbols, graph
  evidence, test targets, and known gaps

SQLite/FTS can replace the storage backend later once query volume and schema
stability justify it. Callers should depend on the public query functions and
snapshot model rather than the JSON file layout.

Shared graph storage contracts live in `crates/tandem-graph-core` even though
the first repo store is JSON-only. Durable repo snapshots map to a
`repo_canonical` partition keyed by tenant, project, workspace, repo, and index
revision. Worktree scans map to `repo_worktree` partitions and run diagnostics
map to `run_ephemeral` partitions; both require explicit promotion before they
can affect canonical repo context.

Retention is part of the graph contract rather than an implementation detail:
project and workspace deletion must remove associated partitions, run-scoped
partitions can expire by TTL, and audit-retained partitions may compact detailed
history while keeping safe aggregate evidence. This gives future hosted storage
the same deletion and compaction semantics as local snapshots.

## Migration Path to the Shared Context Graph

Repo intelligence remains the first production adapter, but the storage and
audit contracts are intentionally repo-agnostic:

1. Keep `.tandem/repo-index.json` as the local repo snapshot while graph-core
   stabilizes.
2. Attach `GraphStoragePartition` metadata to every persisted repo snapshot and
   governed query response.
3. Emit `GraphAuditEvent` records for indexing, context bundles, policy
   filtering, stale-index fallback, and dirty-node invalidation.
4. Add compatibility readers that can load existing repo snapshots without
   partition metadata and assign them a `repo_canonical` partition from the
   active `GraphScope`.
5. Reuse the same partition, retention, provenance, and audit types for
   workflow-version and run graph adapters instead of creating per-domain
   storage schemas.

The stable agent-facing API remains `repo.context_bundle` for the repo
intelligence rollout. A future `context.bundle` alias can combine repo,
workflow, run, memory, and policy graph facts after those adapters share the
same scope, retention, and audit semantics.

## Stale Index and Fallback Behavior

The persisted snapshot lives at `.tandem/repo-index.json` under the repo root.
`repo.index` writes a snapshot with `indexed_unix_ms`, manifest entries, and
extracted facts. Query tools first try to load that stored snapshot.

Current MVP behavior:

- If the stored snapshot loads, query tools use it and report
  `index_source: stored`.
- If loading fails, query tools perform an ephemeral scan and report an
  `index_source` value beginning with `ephemeral_scan_after_load_error:`.
- `repo.update_changed_files` currently performs a safe full rescan, persists a
  new snapshot, and reports `refresh_mode: full_rescan`.
- A readable but old snapshot is not automatically rejected yet. Callers that
  know files changed must refresh the index before trusting graph impact,
  neighbors, or test-target hints.
- Empty snapshots and tasks without searchable terms surface bundle `gaps`.

Fallback rules:

- Required task files and files about to be edited must be read directly even
  when the index looks fresh.
- If a result is missing or surprising, use direct reads and grep before
  concluding the file, symbol, or dependency does not exist.
- If governance or path scope denies access, results must omit hidden paths and
  may include denied counts or safe reasons without leaking names or payloads.

## Confidence and Evidence Rules

Every search result, graph edge, and impact item carries a `confidence` value.
Agents must preserve the distinction between evidence types:

- `Extracted` can support navigation and first-pass claims, but still needs a
  direct file read before edits or final behavioral assertions.
- `Inferred` can guide follow-up searches and impact analysis, but should be
  described as likely or possible until confirmed.
- `Summary` can only support historical or compressed context. It must not be
  treated as current source truth.
- `Ambiguous` is a pointer to investigate, not a conclusion.

When reporting work that used repo intelligence, include enough evidence for a
human to audit the path:

- exact files read
- tool names used
- index source (`stored` or ephemeral fallback)
- relevant confidence classes when they affected the conclusion
- known gaps, stale-index concerns, or fallback searches

## Debugging Bad Context Bundles

Use this checklist when `repo.context_bundle` returns the wrong files, misses an
obvious file, exceeds expectations, or gives weak evidence:

1. Inspect the tool metadata: `repo_root`, `store_path`, and `index_source`.
   An ephemeral source means the stored index failed to load.
2. Check `indexed_unix_ms` from `repo.index` or the stored snapshot. If edits
   happened after that timestamp, refresh with `repo.update_changed_files`.
3. Verify `path_scope`. A scoped bundle will intentionally omit files outside
   that path.
4. Pass explicit `required_files` for task-named files. Required files rank
   ahead of ordinary search hits when they exist in the manifest.
5. Raise `budget_chars` or `limit` only after confirming the first reads are
   sensible. Tight budgets trim graph edges first, then symbols, then likely
   files.
6. Query the missing term directly with `repo.search` and `repo.symbol`. If it
   appears there but not in the bundle, inspect task term extraction and result
   limits.
7. Use `repo.neighbors` on a known file or symbol to inspect graph connectivity.
8. Use `repo.impact` with changed files to see whether import, config/doc, or
   test-target evidence is present.
9. Fall back to grep/read for unsupported languages, generated code, oversized
   files, binary files, ignored paths, or syntax the MVP extractors do not yet
   parse.
10. Record bundle `gaps` in the run summary when they affect confidence.

Bad bundles should be fixed by improving deterministic extraction, ranking,
scope handling, or query APIs. They should not be patched over with
LLM-generated summaries that lack source provenance.
