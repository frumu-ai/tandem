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

## Context Node Payloads

`tandem-graph-core::ContextNodePayload` defines the typed payload families for
non-repo context that affects execution:

- Tool nodes: MCP server, tool definition, credential reference, schema hash, and
  authority/risk summary.
- Memory nodes: tier, collection, retrieved memory evidence, and write
  candidates.
- Policy nodes: policy scope, budget, sandbox limits, data boundaries, and
  approval gates.
- Artifact nodes: generated reports, files, logs, and reviewable outputs.

Payloads are deliberately display-safe. They store opaque credential refs,
schema/content hashes, summaries, scopes, and booleans. They must not store raw
tokens, refresh tokens, private keys, artifact bodies, or unredacted sensitive
payloads. If secret material exists behind a credential, graph payloads represent
that as metadata such as `secret_material_present=true`, not as the secret.

Adapters can convert typed payloads to `GraphPayload` for storage through
`display_safe_payload()`. Governance-sensitive details that are needed for
execution stay in the owning runtime/config store and are linked by opaque ref.

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

Freshness carries the fact source, revision, optional check time, and optional
stale-after timestamp. Stale or unknown facts can still help discovery, but
agents and runtime planners must either refresh/reindex or fall back to source
reads before treating the fact as current.

Visibility scopes facts to tenant, project, optional run, readable paths, and
redaction state. A fact that fails visibility checks must be omitted or replaced
with aggregate denied counts/reasons.

## Scope

`GraphScope` requires tenant and project IDs. It can also include workspace,
repo, worktree, and run IDs. Hosted graph queries must fail closed when the
caller lacks required scope; local repo-only adapters may use explicit local
scope values while still populating repo/worktree fields.

## Storage Partitions and Retention

`tandem-graph-core::GraphStoragePartition` defines the storage boundary for
graph facts before a backend such as JSON, SQLite, or hosted storage persists
them. Partition keys include tenant, project, workspace, repo, worktree, run,
partition kind, and revision so temporary work cannot silently overwrite durable
repo facts.

Partition kinds:

- `tenant_project`: shared project-level context that is not specific to one
  repo, workflow version, or run.
- `repo_canonical`: durable source-derived repo facts for a committed revision
  or index revision.
- `repo_worktree`: temporary repo facts for an isolated worktree. These require
  explicit promotion before they become canonical.
- `workflow_version`: compiled workflow graph facts for one template/version.
- `run_ephemeral`: per-run facts, diagnostics, and intermediate context. These
  require explicit promotion and should normally expire.

`GraphRetentionPolicy` records whether a partition is durable, ephemeral, or
audit-retained; optional TTL; project/workspace deletion behavior; and optional
history compaction timing. Project and workspace deletion must remove matching
graph partitions. Audit-retained partitions can compact detailed history while
keeping safe aggregate evidence.

## Query Envelope

Every agent-facing graph query must carry a `GraphQueryEnvelope` with:

- graph scope: tenant, project, and repo/worktree/run identifiers where
  applicable
- actor and optional automation/run identifiers
- readable and writable path scopes
- allowed tool IDs, memory tiers, budgets, approvals, and context assertion
  metadata

Adapters validate the envelope before running a query. Tenant, project, repo,
actor, tool, or readable-path failures are fail-closed. Path filtering may still
return allowed results while reporting denied counts and reasons, but base
scope/tool failures return no graph payload.

## Audit Events

Graph reads, writes, denials, fallbacks, and context bundle creation should emit
`tandem-graph-core::GraphAuditEvent` records. Audit events include:

- event type, such as `graph.index.started`, `graph.query.denied`,
  `graph.context_bundle.created`, `graph.policy.filtered`, or
  `graph.index.stale_fallback`
- graph scope and run id when available
- actor id
- target partition, tool, query kind, or artifact reference
- decision: allowed, denied with reason, or fallback with reason
- safe metrics such as node count, edge count, denied count, duration, cache hit,
  and estimated token savings
- display-safe details only

Audit payloads must not include raw tokens, private file contents, hidden path
names, or unredacted artifacts. Store opaque refs, hashes, counts, and reasons
that explain decisions without leaking denied data.

## Workflow and Run Graphs

`tandem-graph-core::WorkflowGraph` converts a compiled workflow description into
versioned graph data without depending on the plan compiler. The graph contains
template, version, step, tool, memory tier, approval, policy, and artifact nodes.
Step edges use `depends_on`, `requires_tool`, `requires_memory`,
`requires_approval`, `governed_by`, and `produces`. Each workflow graph is stored
in a `workflow_version` partition with workflow hash freshness, which keeps
future incremental reruns tied to the exact template/policy/prompt/tool-schema
revision that produced them.

`tandem-graph-core::RunTraceGraph` converts observed runtime events into
run-scoped graph nodes. It records model calls, tool calls, memory reads/writes,
approvals, policy checks, artifacts, errors, retries, costs, and outputs as
display-safe nodes linked back to the run, workflow version, and step when known.
Run trace graph nodes are redacted by default and use `run_ephemeral` storage
with audit-retained retention, so sensitive payloads stay behind governed
artifact or event-log references. Successful capture emits
`graph.run_trace.captured` with safe counts and run scope.
