# Tandem Repo Intelligence Architecture

Tandem repo intelligence is the source-derived graph layer that coding agents,
workflow tools, and runtime diagnostics use before broad file discovery. The
core implementation lives in the main Tandem repo so desktop, engine, tools, and
ACA can share one deterministic source of repo facts.

## Ownership Boundary

The shared core is `crates/tandem-repo-intelligence`.

It owns:

- gitignore-aware file discovery
- file manifest entries with size, mtime, and content hash
- deterministic incremental change detection
- repo graph schema primitives for source-derived facts
- future query APIs such as `repo.search`, `repo.symbol`, `repo.impact`, and
  `repo.context_bundle`

It does not own:

- semantic memory ranking
- LLM-generated summaries
- ACA prompt construction
- tenant-specific policy decisions
- UI rendering

ACA should consume the shared core through a tool, CLI, or API boundary. During
rollout, ACA keeps `repo_truth.py` as a fallback when the shared index is
unavailable, stale, or policy-denied.

## Graph Model

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

- `EXTRACTED`: directly parsed from source or config
- `INFERRED`: deterministic but indirect
- `SUMMARY`: LLM-compressed or memory-derived
- `AMBIGUOUS`: useful hint that requires confirmation

Agents may use graph output for discovery and planning, but exact files named in
a task remain mandatory source evidence. Before editing or making final claims,
agents must read concrete files from the repo.

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

## Reuse Points

Existing code that informed this slice:

- desktop memory indexing in `apps/tandem-desktop/src-tauri/src/memory/indexer.rs`
- `tandem-memory` file indexing and memory storage
- `tandem-tools` grep/codesearch/LSP discovery tools
- ACA `repo_truth.py` heuristic discovery and manager prompt injection

The new crate reuses the deterministic file-walking shape from desktop indexing
without depending on Tauri, memory storage, or ACA internals.

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
