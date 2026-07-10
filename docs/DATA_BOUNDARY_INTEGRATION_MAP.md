# Data Boundary Integration Map

Status: implemented inventory for TAN-674. Maps every production path where an
assembled payload leaves the runtime toward an LLM provider and the centralized
boundary adapter each path uses. Anchor reviews to named functions and the CI
inventory guard; incidental line references below are only navigation aids.

`tandem-data-boundary::evaluate_provider_egress` is the canonical evaluation
API. `tandem-core` maps structured chat fields and runtime events, while
`tandem-memory` maps single-prompt completions. Both use private detector spans
from one evaluation, preserve transformed payloads, and carry an opaque
provider/model-bound permit into `ProviderRegistry`.

## 1. Provider dispatch choke points

All provider egress converges on the guarded `ProviderRegistry` methods
(`crates/tandem-providers/src/lib_parts/part01.rs`):

* `stream_with_egress_permit(permit, provider_id, model_id, messages, ...)`.
* `complete_with_egress_permit(permit, provider_id, prompt, model_id)`.

Both validate that the opaque permit matches the concrete provider/model route
before delegating to the compatibility transport API. The permit cannot be
constructed outside `tandem-data-boundary`; transport retries may borrow it
only for the same already-prepared payload.

Actual network dispatch happens inside each provider impl's
`stream()`/`complete()` in `lib_parts/part02.rs`. No production LLM egress
bypasses `ProviderRegistry`; the CI source guard rejects every production use
of an unguarded registry method outside `tandem-providers`.

The registry layer intentionally has no tenant context. Callers therefore
resolve the concrete route and evaluate the fully assembled payload while the
request/session authority is still available, then invoke the registry with
the exact prepared route and payload. This avoids mutable global authority and
keeps tenant attribution bound to each operation.

### Complete egress call-site enumeration

| Path | Guard | Notes |
|---|---|---|
| Main engine loop send | `evaluate_dispatch_boundary` | Tenant/session/assertion authority; approval continuation supported |
| Post-tool narrative synthesis | `evaluate_dispatch_boundary` | Tool output is evaluated after final narrative assembly; denial fails the run |
| One-shot completion | `evaluate_provider_egress` | No implicit authority; strict mode therefore fails closed |
| Strict-KB synthesis and completion fallback | `prepare_chat_messages` | One prepared payload is reused across stream/fallback attempts |
| Workflow planner stream and completion fallback | `prepare_chat_messages` | Planner sessions inherit request tenant/assertion authority |
| Mission builder | `prepare_chat_messages` | One synthetic session and run identify initial and JSON-repair sends |
| Memory distillation | `complete_memory_prompt` | Request run/session and tenant authority supplied by the HTTP handler |
| Memory consolidation, context layers, recursive retrieval | `complete_memory_prompt` | Legacy callers without authority work only when enforcement is not strict |

`scripts/verify-provider-egress-boundary.mjs` is zero-tolerance for legacy
registry sends outside `tandem-providers`. Its CI self-test replaces a guarded
method with an unguarded one and proves that the bypass is rejected; test-only
provider exercises are excluded from the production scan.

### Trusted semantic classifications

The adapter owner, not payload text, supplies these classes:

| Origin | Trusted classes |
|---|---|
| Main engine/session | CustomerData, SourceCode; UnknownSensitive after tool iterations |
| Docs/KB hook content | Legal plus the session classes |
| Global memory/context scope | ProprietaryBusinessData plus the session classes |
| Post-tool synthesis | CustomerData, SourceCode, ProprietaryBusinessData, UnknownSensitive |
| Mission builder/workflow planner | CustomerData, SourceCode, ProprietaryBusinessData |
| Strict KB synthesis | CustomerData, Legal, ProprietaryBusinessData |
| Memory distillation/consolidation | CustomerData, ProprietaryBusinessData |
| Context layers/recursive retrieval | CustomerData, SourceCode, Legal, ProprietaryBusinessData |

Semantic-only classes have no detector span. Policies can block, require
approval, or require local routing for them; redact/tokenize policies fail
closed rather than claiming a transformation that cannot be located.

### Provider identity and local/remote classification

Providers remain plain `String` ids (`ProviderInfo`/`ModelInfo`). The explicit
`TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES=id=class,...` mapping is the only trusted
classification source. Builtin ids such as `ollama` receive no implicit trust
because their endpoint can be reconfigured; every unmapped provider is
`Unknown`, and strict mode fails closed.

(The `provider_is_local()` in `tandem-memory/src/decrypt_broker.rs:209` is
about KMS crypto providers, not LLM providers.)

## 2. Context assembly

Authoritative doc: `docs/ENGINE_CONTEXT_ASSEMBLY_MAP.md`. Assembly owner:
`run_prompt_async_with_execution_context` (with the legacy
`run_prompt_async_with_context` wrapper retained). Per-iteration order
(:259–774): load history → attach images → runtime + agent system prompt →
`followup_context` → server **prompt context hook** → tool schema selection →
`context.budget.final` → boundary evaluation → full-context guard → send.

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
* **Closed vocabulary**: all six canonical `data_boundary.*` names are in the
  `RuntimeEventType` macro table (`tandem-types/src/runtime_event.rs`) and
  `docs/RUNTIME_EVENTS.md`.
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

`run_prompt_async_with_execution_context` receives the actual server run ID and
trusted execution-surface classes, then derives the remaining boundary input:

* `session_record.tenant_context` → org/workspace/deployment ids
  (prompt_execution.rs:31–36).
* `strict_tool_context = session_record.verified_tenant_context.strict_projection`
  (:20–23).
* `provider_id`, `model_id_value` (:24).

Types: `TenantContext` (`tandem-enterprise-contract/src/lib.rs:972`),
`VerifiedTenantContext` (:1039), `RuntimeAuthMode` (:35). Clone the three ids
up front to avoid borrow friction with the loop.

Direct server and memory sends project complete organization/workspace tenancy,
the actual or explicitly synthetic run and session IDs, and any verified
assertion into `ProviderEgressAuthority` before dispatch. A local-implicit
tenant remains unattributed, preserving strict mode's positive-establishment
requirement. Strict mode requires every one of those org/workspace/run/session
components.

## Implemented dispatch contract

Every caller follows this order:

1. Resolve the concrete provider/model route.
2. Assemble all provider-visible fields and attach tenant/run/session authority.
3. Evaluate once and publish only audit-safe evidence.
4. Block, wait for an available approval continuation, or carry transformed
   fields forward. Paths with no continuation fail closed explicitly.
5. Activate/take the opaque permit and dispatch the exact evaluated route and
   prepared payload. Retries of the same payload reuse that permit and
   preparation rather than evaluating or transforming twice.

## Known coverage gaps and risks

* **Provider classification storage**: classification is an audited env mapping,
  not yet a tenant-managed registry/UI. Unknown providers fail closed in strict
  mode.
* **Approval continuation**: engine, post-tool, mission builder, planner, KB,
  and server-originated memory paths wait on the permission manager and bind
  its approval record ID into the permit. One-shot and legacy memory calls with
  no handler fail closed explicitly.
* **Local routing**: `RouteToLocal` still fails closed until the routing
  contract has a concrete alternate-route implementation.
* **Non-LLM network egress**: this inventory governs `ProviderRegistry` LLM
  calls. Connector HTTP, MCP, browser, and embedding transports retain their
  own authority/egress controls and are outside this provider inventory.
* **Event persistence**: canonical provider-egress events carry `runID` and/or
  `sessionID`, so the runtime-event log can persist them. The protected-audit
  bridge derives tenant scope from the event's audit-safe `tenant` object and
  retains `authorityRef` in the protected payload.
* **Line drift**: `prompt_execution.rs` is actively refactored; re-anchor to
  `context.budget.final` when implementing.
* **Source guards are structural, not semantic**: the CI guard prevents legacy
  registry dispatches and its mutation self-test proves bypass detection.
  Focused tests separately prove strict authority, semantic-only governance,
  approvals, and transformed PII/credential payloads.
