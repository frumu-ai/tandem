# Tandem Context Graph Taxonomy

The Tandem context graph is the shared vocabulary for source, workflow, tool,
memory, policy, run, and artifact facts. Repo intelligence is the first adapter,
but the taxonomy is broader so workflow/run graph adapters can reuse the same
IDs and trust semantics.

## Versioning

Graph records carry `GraphSchemaVersion`. Version `1` uses stable string IDs for
node and edge kinds. New kinds should be appended instead of renamed. If a kind
must be replaced, keep the old stable ID as a compatibility alias until all
stored graph facts have migrated.

Stable fact hashes are SHA-256 hashes over the serialized graph fact, including
scope, domain, edge kind, provenance, freshness, visibility, policy decision,
and evidence. Adapters should avoid unordered payload structures when creating
facts that need reproducible hashes.

## Domains

- `repo`: repositories, files, symbols, imports, tests, config, and docs.
- `workflow`: templates, compiled versions, steps, dependencies, and approval
  gates.
- `tool`: MCP servers, tool definitions, credentials, schemas, and authorities.
- `memory`: tiers, collections, retrieved memories, and write candidates.
- `policy`: scopes, budgets, sandbox limits, and data boundaries.
- `run`: runs, model calls, tool calls, errors, retries, outputs, and costs.
- `artifact`: generated files, reports, logs, and reviewable outputs.

## Node Kinds

Node stable IDs live in `tandem-graph-core::NodeKind::stable_id()` and are
grouped by domain:

- Repo: `repo.repository`, `repo.file`, `repo.symbol`, `repo.import`,
  `repo.test_target`, `repo.config_entry`, `repo.doc_section`.
- Workflow: `workflow.template`, `workflow.version`, `workflow.step`,
  `workflow.dependency`, `workflow.approval_gate`.
- Tool: `tool.mcp_server`, `tool.definition`, `tool.credential`,
  `tool.schema`, `tool.authority`.
- Memory: `memory.tier`, `memory.collection`, `memory.retrieved`,
  `memory.write_candidate`.
- Policy: `policy.scope`, `policy.budget`, `policy.sandbox_limit`,
  `policy.data_boundary`.
- Run: `run.run`, `run.model_call`, `run.tool_call`, `run.error`,
  `run.retry`, `run.output`, `run.cost`.
- Artifact: `artifact.artifact`.

## Edge Kinds

Edge stable IDs live in `tandem-graph-core::EdgeKind::stable_id()`:

- Source/repo structure: `contains`, `imports`, `defines`, `references`,
  `configures`, `documents`, `tests`, `likely_related`, `changed_with`.
- Workflow/runtime structure: `depends_on`, `requires_approval`,
  `requires_tool`, `requires_memory`, `produces`, `consumes`, `observed_in`,
  `blocks`, `retries`, `costs`.
- Governance/tooling: `governed_by`, `has_credential`, `has_schema`,
  `has_authority`, `visible_to`, `freshened_by`.

## Trust Semantics

Every graph fact has provenance, freshness, visibility, and policy fields.

- Provenance: `Extracted`, `Configured`, `Observed`, `Inferred`, `Summarized`,
  or `Ambiguous`.
- Freshness: commit, index revision, workflow version, run id, memory snapshot,
  policy hash, tool schema hash, or unknown.
- Visibility: tenant/project/run visibility plus readable path scopes and
  redaction state.
- Policy decision: allowed, denied with reason, or requires approval.

Deterministic facts (`Extracted`, `Configured`, `Observed`) can guide agent
planning directly. `Inferred`, `Summarized`, and `Ambiguous` facts are hints;
agents must confirm with concrete source, run, or policy evidence before final
claims or edits.

## Scope

`GraphScope` requires tenant and project IDs. It can also include workspace,
repo, worktree, and run IDs. Hosted graph queries must fail closed when the
caller lacks required scope; local repo-only adapters may use explicit local
scope values while still populating repo/worktree fields.
