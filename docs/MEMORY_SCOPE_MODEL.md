# Memory scope model

The dimensions Tandem memory actually stores and enforces, per store. Kept in
sync with the enforcement code (`crates/tandem-memory`); if you add a scope
dimension, update this document and the isolation tests together.

## Stores

Tandem has two memory stores with different identity models:

1. **Vector chunk store** (`MemoryChunk`, tables `session_memory_chunks` /
   `project_memory_chunks` / `global_memory_chunks` + vector siblings) —
   semantic search over session/project/global tiers.
2. **Governed records store** (`GlobalMemoryRecord`, table `memory_records`) —
   the auto-injected conversation/solution memory behind `memory_put`,
   promotion, and prompt injection.

## Scope dimensions

| Dimension | Chunk store | Records store | Enforced by |
| --- | --- | --- | --- |
| Tenant (org / workspace / deployment) | `tenant_org_id` / `tenant_workspace_id` / `tenant_deployment_id` columns | same columns | SQL predicates (`tenant_scope_matches_sql_clause`) on every accessor, plus strict-mode rejection of the `local` scope |
| Org unit (department) | `owner_org_unit_id` in record metadata | same | `MemoryAccessFilter` tenant-local branch: caller must be a member of the owning unit (`org_unit_scope_mismatch`); membership comes from the signed assertion's `org_units` |
| User (subject) | `subject` column, stamped at write | `user_id` column | Chunks: `MemoryAccessFilter` tenant-local branch (`subject_scope_mismatch`). Records: SQL `user_id` predicates on list/search |
| Session / project | `session_id` / `project_id` columns | `project_tag` | Server-resolved partition (never model-supplied), SQL predicates |
| Data class / boundary | `classification` in metadata | same | `MemoryAccessFilter` data-boundary check |
| Source binding (enterprise) | `enterprise_source_binding` metadata | same | Grant path (`evaluate_access`) — org-unit **access grants** apply here, not the membership check |

## Semantics

- **Unset means shared.** A chunk/record without `subject` or
  `owner_org_unit_id` keeps the pre-scoping visibility: shared within its
  tenant/tier scope. Restriction is opt-in at write time.
- **Fail closed.** In governed mode, missing caller information (no verified
  context, no memberships, no resolved subject) denies access to any
  restricted record rather than falling back to shared.
- **Local single-tenant mode is exempt** (`LocalNoop`): one human, no org
  model, legacy visibility preserved.
- **Write-side integrity:** `memory_put` refuses to stamp an
  `owner_org_unit_id` the verified writer is not a member of. Chunk `subject`
  is stamped server-side (session actor), never from model-supplied input.
- **Consolidation and imports are shared:** consolidated summaries and
  imported documents deliberately carry no subject — they are project/session
  knowledge, not user memory.

## Tier model

Vector store tiers: `session`, `project`, `global` — all storage-backed.

Governance contract tiers (`GovernedMemoryTier`): `session`, `project`,
`team`, `curated`. **Team and Curated have no backing store yet**: writes
targeting them are rejected fail-closed (`tier_not_storage_backed` guardrail
in `memory_put`). They remain referenceable in read/auto-use policies (e.g.
the default `allow_auto_use_tiers: [curated]`), where an empty tier simply
never matches — a deliberately conservative default. When team-tier storage
lands, remove the write gate and extend this table.

## Known limitations / follow-ups

- Model-tool chunk writes (`tandem-tools` memory tools) run in local
  single-tenant mode with no caller identity and store shared, `local`-scoped
  chunks; hosted deployments route memory through the governed server paths
  instead.
- Org-unit membership is flat (no parent-unit inheritance), matching the
  org-unit grant projection; hierarchy can layer on at filter-build time.
- The cross-axis isolation regression matrix lives in
  `crates/tandem-server/src/http/tests/memory_parts/part08.rs` (`matrix_*`
  tests), the prompt-injection sender tests in
  `crates/tandem-server/src/app/state/tests/`, and the
  `eval_datasets/cross_user_memory_isolation.yaml` gate dataset (TAN-608).
