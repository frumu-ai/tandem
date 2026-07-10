# Tandem Secure Data Boundary Module

Status: crate foundation, audit integration, configured enforcement, and
centralized provider-egress coverage are implemented. Behind
`TANDEM_DATA_BOUNDARY_MODE` (default `off`) every production LLM-provider
dispatch evaluates its assembled payload, emits `data_boundary.*` evidence, and
tandem-server bridges consequential decisions into the protected audit
ledger. Coverage includes the main engine loop, post-tool synthesis, one-shot
completion, memory distillation/consolidation/context layers/recursive
retrieval, mission builder, workflow planner, and strict-KB grounding. In
`enforce` mode the dispatch gate blocks prohibited/unapproved raw egress,
redacts/tokenizes configured fields before send,
requires human approval via the permission surface for approval classes, and
fails closed on `RouteToLocal` (no routing capability yet — see
`DATA_BOUNDARY_ROUTING_CONTRACT.md`). Providers classify solely via
`TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES` (unmapped providers — including
builtin loopback ids, whose base URLs can be reconfigured to remote
endpoints — stay `unknown`); `TANDEM_DATA_BOUNDARY_STRICT` fails closed on
unclassified providers, incomplete organization/workspace tenancy, or missing either run or session
authority (a local-implicit tenant counts as missing — tenancy must be
positively established). Audit-only guard hooks also scan tool/MCP results and
prompt-context-hook injections as they enter context. CI runs
`scripts/verify-provider-egress-boundary.mjs` to reject any production use of
the legacy unguarded provider registry methods.

> Naming note: this document describes the `tandem-data-boundary` crate — the
> runtime boundary between assembled payloads and external LLM providers. It is
> distinct from `docs/DATA_BOUNDARY_ENFORCEMENT_DESIGN.md`, which covers the
> pre-existing `DataBoundary`/`DataClass` types in `tandem-enterprise-contract`
> governing *memory read* access. The two taxonomies overlap (Credential,
> SourceCode, CustomerData, Financial) and will be mapped when the crate is
> integrated; see "Relationship to existing governance" below.

## Problem statement

Companies need strict data-to-model contracts and runtime controls before
sensitive company data reaches external LLM providers. Tandem already owns
governance, approvals, audit evidence, tenant boundaries, and tool authority —
but nothing today inspects the assembled payload (prompt context, tool results,
memory, workflow artifacts) at the moment it is about to leave for a provider.
This crate is the foundation for enforcing data egress decisions at that model
boundary.

This is not a generic PII regex utility. It is a runtime boundary component
that sits between Tandem context assembly / tool results / memory / workflow
data and external model provider calls.

## Threat model

* Sensitive information disclosure to third-party providers.
* Prompt/context leakage (assembled context contains more than the user typed).
* External provider exposure and model-training retention risk.
* Vector/embedding, tool-result, and memory-retrieval leakage becoming prompt
  context without review.
* Uncontrolled spend from payload/retry expansion.
* Audit gaps: security teams cannot prove what left the boundary, or evidence
  itself leaks the sensitive values it describes.

## What the first crate does

`crates/tandem-data-boundary` is a dependency-minimal crate (serde +
serde_json + sha2, `#![forbid(unsafe_code)]`) providing:

* **Core contract types** — `DataBoundaryMode` (Off/Audit/Enforce),
  `ProviderBoundaryClass` (Local, CustomerHosted, ApprovedExternal,
  UnapprovedExternal, Prohibited, Unknown), `SensitiveDataClass` (PII, PHI,
  Financial, Credential, Secret, SourceCode, CustomerData, EmployeeData, Legal,
  ProprietaryBusinessData, UnknownSensitive), `DataBoundaryAction` (Allow,
  AllowWithAudit, Redact, Tokenize, RouteToLocal, RequireApproval, Block),
  plus policy, input, finding, decision, and event structs.
* **Deterministic detector MVP** — `detect_sensitive_data` finds emails,
  phone-like strings, Luhn-validated card numbers, credential assignments,
  bearer tokens, API-key prefixes, private-key blocks, AWS-style keys,
  high-entropy strings, and simple sensitivity markers. No LLM involvement;
  results carry spans, detector ids, and evidence hashes — never raw values.
* **Redaction and tokenization** — `redact_sensitive_data` /
  `tokenize_sensitive_data` replace detected spans with stable placeholders
  such as `[REDACTED:CREDENTIAL:1]` / `[TOKEN:PII:2]`, handling overlapping
  spans deterministically. No raw value is persisted anywhere.
* **Decision engine** — `evaluate_data_boundary(request, policy)` turns
  payload, provider class, tenant context, and policy into an enforceable
  `DataBoundaryDecision` (see "How decisions work").
* **Canonical provider-egress adapter** — `evaluate_provider_egress` accepts
  labeled payload fields plus tenant/run/session/assertion authority, resolves
  policy and provider classification, and returns an enforceable disposition,
  audit event, transformed fields, and an opaque route-bound dispatch permit.
  Detector spans remain private, are mapped back to fields from the original
  evaluation, and never require a second detection pass. The original public
  `DataBoundaryEvaluation` and `DataBoundaryEvaluationRequest` struct surfaces
  remain unchanged for patch-release compatibility.
* **Trusted semantic origins** — provider adapters attach source-owned classes
  even when deterministic detectors find no spans: source code and customer
  data in engine sessions, proprietary workflow/tool/memory content, legal/KB
  content, and typed memory/direct-server origins. These hints participate in
  block, approval, locality, and raw-egress policy without pretending they can
  be span-redacted.
* **Audit-safe event shape** — `DataBoundaryEvent::from_decision` produces the
  `data_boundary.*` event family with tenant/provider/operation refs, action,
  finding summary (classes/counts/severities), policy fingerprint, payload
  hash, decision latency, and evidence refs. Tests assert raw payload fields
  can never appear.
* **Stable hashing** — `payload_hash` gives `sha256:<hex>` content hashes for
  monitoring and dedupe that are safe to log.

## What it does not do yet

* Workflow-artifact prompt folding remains guarded audit-only (TAN-600,
  `sourceKind: workflow_artifact`) at the automation executor; the final LLM
  dispatch is enforced separately by the provider-egress gate.
* `RouteToLocal` has no routing capability: enforce mode fails closed with
  `route_to_local_unavailable` per the routing contract.
* Approval asks reuse the generic permission surface; `always`-style standing
  rules are not consulted by the boundary gate (every sensitive dispatch asks
  again), and a dedicated ask kind is future work. Engine, post-tool, direct
  server, and server-originated memory calls activate an opaque permit after a
  positive continuation. Legacy memory and one-shot calls with no continuation
  fail closed with `DATA_BOUNDARY_APPROVAL_UNAVAILABLE`.
* No LLM-based classification; detection is deterministic only.
* No raw sensitive value persistence, and no reversible tokenization vault —
  the tokenization map is placeholder-only. Future persistence should go
  through the existing secret resolver or encrypted memory layer.
* Provider classification remains environment-configured rather than stored in
  a tenant policy service. Unmapped ids stay `Unknown`; strict mode blocks them.
* No actual local/private routing; `RouteToLocal` is a decision the caller
  must honor (TAN-396 designs the routing contract).
* No policy UI/API (planned in Cycle 4).

## How decisions work

`evaluate_data_boundary` is pure: same input + same policy = same decision,
with a deterministic `decision_id` derived from input id, operation id, policy
fingerprint, and payload hash. Raw payload text is accepted transiently via
`DataBoundaryEvaluationRequest.payload` for detection and transformation, and
never appears in the returned decision, findings, or events — only in
`transformed_payload`, which callers forward to the provider path only.

Rules, in precedence order (Block > RequireApproval > RouteToLocal > Tokenize
> Redact > AllowWithAudit > Allow); every triggered rule appends a reason code
even when a stronger action wins:

* **Off mode** allows without running detection (`mode_off`).
* **Prohibited providers** (class or id) block (`prohibited_provider`).
* **Strict fail-closed** (`strict_fail_closed`): both a nonblank organization
  and workspace are required, and an `Unknown` provider class blocks
  (`missing_tenant_context`, `missing_tenant_organization`,
  `missing_tenant_workspace`, `unknown_provider_boundary_class`). The canonical
  provider-egress adapter additionally requires both a nonblank run ID and
  session ID (`missing_run_authority`, `missing_session_authority`,
  `missing_execution_authority`).
* **Payload cap** (`max_payload_bytes`): oversized payloads block
  (`payload_too_large`) to bound spend expansion.
* **Class rules** for every sensitive class present (detected or declared via
  `DataBoundaryInput.data_classes`): `block_classes`,
  `approval_required_classes`, `require_local_classes` (external providers
  only), `tokenize_classes`, `redact_classes`. Redact/Tokenize apply only to
  classes the detector actually spanned; a class present only as a declared
  hint (e.g. a governed memory/KB label) has no locatable content to
  transform, so a transform policy on it fails closed
  (`untransformable_redact_class_*` / `untransformable_tokenize_class_*`)
  instead of claiming a transformation that did not happen.
* **Raw-egress rule**: any sensitive class headed to an unapproved provider
  blocks unless the class is in `allow_raw_external_classes`. Local,
  CustomerHosted, and ApprovedExternal providers are intrinsically approved;
  UnapprovedExternal can be exempted via `approved_provider_ids` /
  `approved_provider_classes`.
* **Audit mode never blocks or transforms provider payloads**: decisions that
  would stop dispatch (Block,
  RequireApproval, RouteToLocal) downgrade, keeping their reason codes plus
  `audit_mode_downgrade`. The pure decision result may include a proposed
  span-backed transformation, but `evaluate_provider_egress` records that
  decision as `data_boundary.evaluated` and preserves the original dispatch.
* Sensitive classes with no matching rule yield `AllowWithAudit`
  (`sensitive_classes_present`); a clean payload yields `Allow`
  (`no_findings`).

## How monitoring works

Every decision maps to one event kind: `data_boundary.evaluated`, `.redacted`,
`.tokenized`, `.blocked`, `.approval_required`, or `.routed_local`. Events
carry classes, counts, severities, reason codes, policy fingerprints, payload
hashes, decision latency, and evidence refs (hash/path/detector references).

**Evidence safety rule:** no raw prompt text, tool result, secret, customer
data, or model output may ever appear in boundary events or audit payloads —
only class labels, counts, hashes, spans, detector ids, policy fingerprints,
and provider/model metadata. Crate tests
(`event_contract_omits_raw_sensitive_payload_fields`,
`evaluation_evidence_never_contains_raw_values`) enforce this contract.

Payload hashes are stable sha256 content hashes, supporting dedupe and
time-series monitoring of repeat leakage attempts without content exposure.
Cycle 4 (TAN-398) builds the operator read model on top of these events.

## Relationship to runtime governance, provider routing, approvals, and audit

* **Tenant governance**: `DataBoundaryTenantRef` mirrors the
  organization/workspace/deployment identity used by
  `tandem-enterprise-contract`. Integration must consume the already-asserted
  tenant context — it must never substitute for tenant assertion, tool
  authority, or the pre-send outbox gate, and adding boundary evaluation must
  not weaken any existing check.
* **Existing DataClass boundary**: the enterprise contract's
  `DataBoundary`/`DataClass` govern memory *reads* (what may enter context).
  This crate governs *egress* (what may leave for a provider). At integration
  time, memory-read classifications should flow into
  `DataBoundaryInput.data_classes` so egress decisions can consider classes
  detection alone cannot see.
* **Approvals**: `RequireApproval` decisions are designed to feed Tandem's
  existing approval gates with class/count evidence only (TAN-395).
* **Audit**: `data_boundary.*` events follow `docs/RUNTIME_EVENTS.md` naming
  conventions and are shaped for the protected-audit path — hashes and refs,
  never content.

## Future plan: local/private model routing

`RouteToLocal` exists in the action and event vocabulary now so evidence and
policy can express it from day one. TAN-396 defines the routing contract:
provider registry distinctions for local/customer-hosted/approved-external
models, fallback semantics when no local model is available, and how the
runtime re-dispatches a routed request without losing approvals or audit
continuity.

## Future plan: policy UI and enterprise controls

Cycle 4 adds read/write APIs for boundary policies and control-panel surfaces
for provider classes, allowed raw classes, redaction/tokenization classes,
approval classes, and block classes (TAN-399), plus operator monitoring for
leakage attempts over time (TAN-398) and enterprise strict-mode regression
coverage (TAN-400). Until then, policies are constructed in code by the
integrating component, defaulting to `Off` so existing local flows are
unchanged.
