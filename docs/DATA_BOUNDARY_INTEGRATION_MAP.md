# Data Boundary Integration Map

Status: research deliverable for TAN-380 (Tandem Secure Data Boundary,
Cycle 1). Maps every path where assembled payloads leave the runtime toward an
LLM provider, the audit/event surfaces a `data_boundary.*` family plugs into,
and the recommended first audit-only integration point for Cycle 2 (TAN-390).
Line numbers are anchors as of this trace, not guarantees — anchor work to the
named functions.

The crate itself (`crates/tandem-data-boundary`, including the
`evaluate_data_boundary` engine) is implemented and tested but has no
dependency edge from any other crate yet; integration is wiring, not design.

## 1. Provider dispatch choke points

All provider egress converges on `ProviderRegistry`
(`crates/tandem-providers/src/lib_parts/part01.rs`):

* `stream_for_provider(provider_id, model_id, messages, tool_mode, tools, sampling, cancel)`
  — part01.rs:818, the streaming choke point (`default_stream` delegates here).
* `complete_for_provider(provider_id, prompt, model_id)` — part01.rs:723.
* `complete_cheapest(prompt, provider_override, model_override)` — part01.rs:741.

Actual network dispatch happens inside each provider impl's
`stream()`/`complete()` (`req.send().await` in `lib_parts/part02.rs`: OpenAI
chat/completions :113, OpenAI responses :557, Anthropic :1293, Cohere :1385).
No egress bypasses `ProviderRegistry` — grep for provider hostnames outside
`tandem-providers` hits only config listings.

**The registry layer has no tenant context** (only provider id, model id,
messages, tools). The narrowest choke point that has *both* the fully
assembled request *and* tenant context is the engine loop:
`run_prompt_async_with_context` in
`crates/tandem-core/src/engine_loop/prompt_execution.rs`. At :706–844 the
request is complete (`messages`, `tool_schemas`, `provider_id`,
`model_id_value`), `context.budget.final` fires (:726), the full-context
hard-budget guard can fail closed (:817–842), and the provider stream
dispatches at :861 via `self.providers.stream_for_provider(...)`.

### Complete egress call-site enumeration

| Path | Location | Notes |
|---|---|---|
| Main engine loop send | `tandem-core/src/engine_loop/prompt_execution.rs:861` | has tenant context + budget events |
| Post-tool narrative synthesis | `tandem-core/src/engine_loop/tool_execution.rs:459` | second in-loop send; **not** covered by `context.budget.final` |
| Legacy `run_prompt`/complete | `tandem-core/src/engine_loop.rs:385,394` | |
| Strict-KB synthesis (direct) | `tandem-server/src/http/session_kb_grounding.rs:1104,1180` | emits `context.budget.bypassed` |
| Workflow planner transport (direct) | `tandem-server/src/http/workflow_planner_transport.rs:53,81` | emits `context.budget.bypassed` |
| Mission builder host (direct) | `tandem-server/src/http/mission_builder_host.rs:222` | emits `context.budget.bypassed` |
| Memory consolidation/distillation | `tandem-memory/src/distillation.rs:214`, `context_layers.rs:54` | `complete_cheapest` sends **memory content** outside all budget/audit telemetry |

### Provider identity and the missing local/remote distinction

Providers are plain `String` ids (`ProviderInfo`/`ModelInfo`,
`crates/tandem-types/src/provider.rs:71/:63`). **No first-class local-vs-remote
flag exists.** The only signals today: base-url convention (`ollama` →
`http://127.0.0.1:11434/v1`, `llama_cpp` → `http://127.0.0.1:8080/v1`,
part01.rs:883/:920) and the cost-ordered id list in
`select_cheapest_provider_id` (part01.rs:780–790). `ProviderBoundaryClass` in
`tandem-data-boundary` is the intended home for this classification, but a
`provider_id → ProviderBoundaryClass` mapping table must be built (TAN-393);
until then callers pass `Unknown` and strict policies fail closed.

(The `provider_is_local()` in `tandem-memory/src/decrypt_broker.rs:209` is
about KMS crypto providers, not LLM providers.)

## 2. Context assembly

Authoritative doc: `docs/ENGINE_CONTEXT_ASSEMBLY_MAP.md`. Assembly owner:
`run_prompt_async_with_context` (`prompt_execution.rs:4`). Per-iteration order
(:259–774): load history → attach images → runtime + agent system prompt →
`followup_context` → server **prompt context hook** (:347–366) → tool schema
selection → `context.budget.final` (:726) → full-context guard → send (:861).

* History and tool-result projection: `engine_loop/prompt_runtime.rs`
  (`summarize_tool_invocation_for_history` :257, `mcp_list` compaction :401).
* The server prompt context hook (`augment_provider_messages`, registered in
  `tandem-server/src/app/state/app_state_impl_parts/part01.rs:511`) folds in
  identity, memory scope, KB grounding, embedded docs, and global-memory hits,
  under `TANDEM_PROMPT_HOOK_CONTEXT_BUDGET_CHARS` /
  `TANDEM_DOCS_CONTEXT_BUDGET_CHARS` / `TANDEM_MEMORY_CONTEXT_BUDGET_CHARS`.
* Direct (non-loop) assemblies: strict-KB synthesis, workflow planner, mission
  builder, plus workflow/automation/routine/coder prompt builders (owners
  listed in the assembly-map doc).

## 3. Tool/MCP results becoming prompt context

Tool execution (including MCP via `tandem-runtime`/`tandem-tools`) is owned by
`engine_loop/tool_execution.rs`; results persist as
`MessagePart::ToolInvocation` and are re-projected into provider history each
iteration by `prompt_runtime.rs` (summarized/compacted — raw payloads stay in
session storage). MCP-origin data is therefore already inside `messages` at
the engine-loop choke point; `DataBoundaryOperationKind::ToolCall` anticipates
a finer-grained hook later (TAN-397).

## 4. Memory egress and the governed-read machinery (must not weaken)

* `MemoryAccessFilter` (`tandem-memory/src/types.rs:174`) with
  `GovernedReadMode::{LocalNoop, GovernedStrict}` (:108) and
  `StrictTenantContext`; governed constructor forces `GovernedStrict` (:198).
* Chunk visibility gate: `memory_chunk_visible_to_access_filter`
  (`manager_parts/part01.rs:1612`).
* Governed global memory: `search_global_memory_for_tenant`
  (`tandem-server/src/http/skills_memory_parts/part04.rs:818`); governed
  memory injection fails closed when verified context is missing.
* Design reference: `docs/DATA_BOUNDARY_ENFORCEMENT_DESIGN.md` (TAN-267);
  `StrictTenantContext::evaluate_access`
  (`tandem-enterprise-contract/src/lib.rs:1528`).

**Do-not-weaken rules for boundary integration:**

* Scan the already-assembled `messages` (post-hook). Never re-read memory/KB
  through a non-`GovernedStrict` path to obtain payloads for scanning — that
  would reintroduce the cross-tenant leak TAN-267 closed.
* Boundary evaluation is an *additional*, later gate. An `Enforce` block never
  replaces or reorders approval gates, permission checks, tenant assertion, or
  `tool.execution.denied` — and audit-only evaluation is never a reason to
  relax the prompt hook's own fail-closed memory injection.

## 5. Runtime events and protected audit

* **Event bus**: `EventBus::publish(EngineEvent::new("event.name", json!(...)))`
  (`tandem-core/src/event_bus.rs:83`) stamps the `RuntimeEventEnvelope`,
  persists to the durable log when `run_id`/`session_id` is present
  (rows without them are dropped — event_bus.rs:26), and broadcasts. Engine
  loop emitters call `self.event_bus.publish(...)` inline
  (`context.budget.final` at prompt_execution.rs:726 is the sibling to copy).
* **Closed vocabulary**: new canonical names must be added to the
  `RuntimeEventType` macro table (`tandem-types/src/runtime_event.rs:29`) and
  `docs/RUNTIME_EVENTS.md` in the same change. `data_boundary.*` is not in the
  table yet.
* **Durable event log**: `tandem-server/src/runtime_event_log.rs`, JSONL at
  `runtime/events.jsonl`, tenant-scoped via
  `RuntimeEventLogRow::visible_to_tenant` (:56), replay via
  `GET /runs/{run_id}/events`.
* **Protected audit ledger** (hash-chained, fsync'd, tenant-scoped):
  `append_protected_audit_event(state, event_type, tenant_context, actor, payload)`
  (`tandem-server/src/audit.rs:164`); readers
  `load_protected_audit_events_for_tenant` (:137). Existing boundary-style
  precedents: `audit.export.denied`, `workflow.governance.gate_decided`,
  `tool.execution.denied`. Consequential decisions (`blocked`,
  `approval_required`, `routed_local`) belong here; the engine loop does not
  hold `AppState`, so the ledger write should live in the server, subscribed
  to the bus event.

## 6. Config pattern for `TANDEM_DATA_BOUNDARY_*` (TAN-389)

* Per-var resolvers live in `tandem-server/src/config/env.rs` — copy
  `resolve_runtime_auth_mode()` (:76) for
  `TANDEM_DATA_BOUNDARY_MODE=off|audit|enforce` (`DataBoundaryMode` already
  derives snake_case serde with `Off` default), and the bool/presence patterns
  (`prometheus_metrics_enabled` :65, `context_assertion_verifier_configured`
  :90) for the rest.
* Validation plus the `ConfigVar { name, default, notes }` documentation
  registry live in `tandem-server/src/config/engine.rs` (:221/:240 validation,
  :603+ registry). New vars need resolver + registry rows. Default mode must
  be `off` so local behavior is unchanged.

## 7. Tenant context availability at the choke point

`run_prompt_async_with_context` already derives everything the boundary input
needs, in scope, with no new plumbing:

* `session_record.tenant_context` → org/workspace/deployment ids
  (prompt_execution.rs:31–36).
* `strict_tool_context = session_record.verified_tenant_context.strict_projection`
  (:20–23).
* `provider_id`, `model_id_value` (:24).

Types: `TenantContext` (`tandem-enterprise-contract/src/lib.rs:972`),
`VerifiedTenantContext` (:1039), `RuntimeAuthMode` (:35). Clone the three ids
up front to avoid borrow friction with the loop.

The direct sends (§1 rows 4–6) and the memory `complete_cheapest` path carry
tenant/session context in their own handlers but do not pass it to
`ProviderRegistry`; each needs explicit wiring for full coverage later.

## Recommendation: smallest safe audit-only integration point (TAN-390)

**Emit `data_boundary.evaluated` in the engine loop immediately after the
`context.budget.final` publish and before the full-context guard**
(`prompt_execution.rs`, after :774 as of this trace — anchor to the publish
call, not the line number).

Why there:

1. Request fully assembled; provider/model/tenant ids all in scope — no
   signature churn.
2. Post-hook, so it observes exactly what will egress and structurally cannot
   substitute for any upstream gate.
3. Sits beside an existing `EventBus::publish` — purely additive, with no
   early return, so audit mode can never block or alter the provider call
   (unlike the adjacent budget guard's deliberate `bail!`, which must not be
   mirrored in the audit-only PR).
4. Payload uses only hashes/refs/counts from `tandem-data-boundary`,
   satisfying the RUNTIME_EVENTS.md content-by-reference rule.

First-PR shape: add `tandem-data-boundary` as a `tandem-core` dependency;
`resolve_data_boundary_mode()` (default `off`) + registry rows; thread the
mode into engine-loop construction; emit the event (mode only gates whether it
fires — `enforce` is not honored yet); add `DataBoundaryEvaluated` to the
`RuntimeEventType` table and `docs/RUNTIME_EVENTS.md` in the same commit.
Defer protected-audit writes to the server side as a follow-up.

## Known coverage gaps and risks

* **`provider_id → ProviderBoundaryClass` mapping is unbuilt** (TAN-393).
  Until then integrations pass `Unknown`; strict policies fail closed on it.
* **Uncovered egress**: the post-tool synthesis send
  (`tool_execution.rs:459`), the three direct server sends, and memory
  consolidation/distillation's `complete_cheapest` (which sends memory content
  with zero telemetry today) are not covered by the engine-loop hook. Full
  coverage needs a second registry-level hook or per-caller wiring — but the
  registry layer lacks tenant context, so that requires design work first.
* **Event persistence gating**: bus events without `run_id`/`session_id` are
  broadcast but not persisted; the emission must carry canonical ids so audit
  records stay tenant-scoped.
* **Line drift**: `prompt_execution.rs` is actively refactored; re-anchor to
  `context.budget.final` when implementing.
* **Dependency direction**: verify `tandem-core → tandem-data-boundary` adds
  no cycle during implementation (the crate depends only on serde + sha2, so
  none is expected).
