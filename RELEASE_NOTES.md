# Release Notes

This is the canonical release-notes file used by release tooling.

## v0.6.2 (Unreleased)

Tandem 0.6.2 starts the enterprise MCP identity workstream for multi-employee
runtime deployments. The first change is a source-of-truth design for separating
MCP server definitions from user-owned, service-principal, shared, and delegated
MCP connections.

### Runtime Observability

- Runtime event persistence now consumes an opt-in bounded event-bus queue, so
  canonical run/session events published after persister registration are
  written to `runtime/events.jsonl` without depending on the live broadcast
  stream or retaining events in eval-only buses.

### Workflow Runtime Reliability

- Workflow definitions now have an opt-in strict action validation path backed
  by a typed action registry. Built-in workflow actions validate their `with`
  payloads before execution, strict loads reject unknown actions with
  source/step/field diagnostics, and MCP/tool actions can be checked against the
  host tool catalog schema before a workflow is saved or run.
- Session history, Automation V2 run stores, and per-run history shards now use
  schema-versioned persistence envelopes. Legacy bare-map/bare-record files load
  through explicit v0-to-v1 migrations and rewrite safely, future schema
  versions fail closed without overwriting state, compatibility fixtures protect
  paused awaiting-approval run state, and the memory DB records its bootstrap
  schema in an idempotent `schema_migrations` ledger.

### Runtime Governance

- Tool execution now flows through a governed dispatcher outside the
  `tandem-tools` crate. Engine turns, workflow actions, Automation V2 connector
  preflight calls, direct HTTP tool execution, planner helpers, pack builder,
  and the engine CLI carry tenant context and scope allowlists through one
  dispatch path, which records a single policy/scope ledger event for each
  dispatch before returning the tool result or denial.

### Enterprise MCP Identity

- Added the enterprise MCP identity and delegation design for principal-scoped
  MCP connection records, tenant/actor-bound OAuth ownership, explicit run-as
  resolution, local single-user migration behavior, and MCP audit identity
  fields.
- Added the first runtime scaffolding for scoped MCP connection records. The MCP
  registry can now persist V2 state with separate server definitions and
  connection records, backfill local compatibility connections from legacy
  `mcp_servers.json`, and preserve server definitions that intentionally have no
  bound account credential.
- Added tenant-aware MCP reconnect and refresh paths for tool execution. When an
  enterprise request carries an explicit tenant/actor context, readiness checks
  now connect and discover tools with that same context, and OAuth refresh uses
  tenant-scoped credential helpers instead of silently falling back to local
  credentials.
- Added actor-qualified MCP secret ids and exact tenant-context secret
  resolution so two employees in the same workspace cannot overwrite or resolve
  each other's stored MCP bearer credentials.
- Added tenant/actor-scoped MCP OAuth sessions and callbacks. Hosted OAuth
  sign-ins now record the initiating tenant context, principal, connection id,
  and provider credential id, reject mismatched authenticated callbacks, and
  persist refreshed tokens under the scoped connection instead of a shared
  server-global provider account.
- Tightened tenant/actor OAuth completion after review: pending sign-in polls
  now keep returning the initiating session's authorization URL even when
  another user starts OAuth for the same MCP server, and callback token storage
  updates the scoped connection rather than the shared server row.
- Moved authenticated MCP readiness state onto the scoped connection for
  explicit tenants. Refresh/discovery now records session ids, pending auth, and
  tool caches on the actor's connection, and tenant-aware tool lists read from
  that scoped cache while local single-user mode preserves the existing server
  row behavior.
- Added Automation V2 MCP connection grants plus bridge-level run-as
  enforcement. MCP calls can now carry selected connection/principal metadata,
  cross-actor connection ids and actor-supplied service-principal selection fail
  closed before upstream dispatch, and protected audit records include the
  actual acting MCP principal and connection.
- Added enterprise MCP isolation regressions for cross-tenant connection-id
  denial and OAuth callback mismatch auditing. MCP connect/discovery events now
  include tenant, principal, and connection metadata without credential content.
- MCP inventory now includes redacted scoped connection summaries filtered to
  the requesting tenant/actor plus shared/service/admin-managed rows. The
  control plane can display connection class, owner/upstream account, tenant
  scope, status, and safe tool-name cache entries without exposing credential
  refs, secret headers, OAuth client secrets, or another actor's pending sign-in.
- Updated Automation V2 MCP preflight discovery to use tenant-scoped readiness,
  connection-grant run-as context, tool sync, and remote tool inventory, and
  preserved connection grants through the control-panel workflow editing
  surfaces.
- Updated the control panel to separate MCP provider definitions from acting
  connections. MCP Settings now shows account/shared/service connection
  inventory and uses account-scoped connect/refresh language, while Workflow
  Studio can explicitly select acting MCP connections for agents and custom
  task overrides.
- Added shell sandbox security coverage and documentation for Linux bubblewrap
  argv/write-boundary behavior, fail-closed POSIX sandbox guardrails, and
  Windows shell command translation/rejection policy.
- Extracted the first Automation V2 runtime model layer into the new
  `tandem-automation` crate. Automation specs/runs, execution-profile helpers,
  MCP run-as policy records, routine misfire policy, scheduler queue metadata,
  and shared-context metadata parsing now compile outside `tandem-server`, while
  the server keeps compatibility re-exports for existing call sites.
- Began the AppState domain-manager cleanup by moving provider and MCP OAuth
  callback session maps behind a dedicated OAuth state manager. This reduces
  top-level runtime state sprawl and gives OAuth flows an explicit lock-order
  boundary without changing sign-in behavior.
- Fixed opaque/in-memory MCP reconnects so startup runtime-state resets no
  longer erase seeded tool inventories for local compatibility servers such as
  the test GitHub MCP fixture.
- Fixed Automation V2 filesystem code workflows so connector-only MCP
  allowlists still keep `apply_patch` available for repository edits, and
  removed the related regression from the nextest CI quarantine list.
- Fixed Automation V2 task retry/requeue so manual node requeues preserve the
  prior attempt count and the next executor pass advances to the next attempt.
- `tandem-engine storage cleanup` now reads and writes schema-versioned
  Automation V2 run indexes/shards, so cleanup cannot collapse a v1 hot run
  index to an empty legacy map.
- Automation V2 run-shard envelope writes now borrow the run record instead of
  cloning it, and stack-heavy coder issue-fix regressions run under a high-stack
  test harness for reliable nextest coverage.
- Documented that hosted/enterprise MCP OAuth should follow the existing
  connector control-plane ownership precedent: long-lived secret material stays
  outside the runtime, while the runtime stores credential references and
  enforces asserted authority.

## v0.6.1 (2026-06-20)

Tandem 0.6.1 is a focused workflow-runtime patch release for MCP wrapper
actions. It fixes a failure mode where Automation V2 nodes that only needed to
call a concrete MCP wrapper tool were still assigned an implicit JSON artifact
path, which then made the runtime require unrelated workspace writes before the
workflow could advance.

### Automation V2 MCP Wrapper Actions

- Action nodes can now explicitly disable synthesized default artifact paths
  with `metadata.disable_default_output_path`,
  `builder.disable_default_output_path`, or `builder.output_path_mode = "none"`.
- This lets connector-wrapper workflows, including Composio Gmail draft
  approval demos, hand off provider results without being forced through a
  workspace artifact write path.
- Added regression coverage for the new default-output opt-out behavior.

### Approvals Inbox

- The unified approvals endpoint now rehydrates Automation V2 list rows through
  the full run record before filtering pending gates, so a stale or skeletal run
  list entry can no longer hide a live approval gate.
- Sharded Automation V2 run records now hydrate their pending gate details
  before inbox aggregation, keeping approval cards visible even when the run
  metadata is stored across state shards.
- The control-panel inbox now sorts mixed approval sources by requested time,
  putting a fresh demo approval above older ACA approvals.

### Automation V2 Recovery And Portability

- Legacy `automation-v2-*` context-run directories are now recovered into
  Automation V2 run history. Tandem reconstructs run records, checkpoint state,
  and automation snapshots from `run_state.json`, then merges the newest
  recovered record into history/detail APIs and persists it back to canonical
  run storage.
- Recovered context runs also appear in the Automation V2 library when enough
  snapshot information is present, making interrupted or pre-migration workflow
  runs discoverable again instead of stranded in context-run storage.
- The control panel can now export an Automation V2 JSON spec from the edit
  dialog and import that JSON through the creation wizard, including replacement
  confirmation when the imported automation id already exists.

### Control Panel Connectivity

- ACA availability probing now has a longer default grace window, so the Coder
  dashboard is less likely to flip to disconnected while a recently healthy ACA
  endpoint is slow during task selection or board refresh.
- When ACA is configured and the Tandem engine is healthy, ACA probe timeouts
  are smoothed as degraded-but-available instead of immediately hiding Coder
  actions.
- Engine session list/status endpoints now return lightweight session summaries
  instead of serializing every stored message transcript for every session.
  This prevents large ACA coding runs from making `/session` probes time out
  while preserving full transcript access through direct session detail APIs.

### MCP Provider Guidance

- The automated-agents guide now documents Composio Connect and scoped Composio
  MCP server URLs separately, including generated MCP URL usage, `x-api-key`
  requirements, and REST-only setup examples for creating and generating
  Composio MCP server URLs.

### Release Metadata

- Bumped Tandem Rust workspace, desktop, npm, and Python manifests to `0.6.1`.
- Updated the version bump script so the meta-harness crate and desktop Tauri
  lockfile are included in future release bumps.

## v0.6.0 (2026-06-17)

Tandem 0.6.0 is a security- and assurance-focused release that lays the
foundation for cross-tenant data governance, governed runtime decisions, and
goal-driven capability composition. It adds eval-backed, CI-enforced proof that
tenant boundaries hold at runtime, tenant-scopes approval/audit/provider paths,
hardens MCP and memory egress, and gives operators better ACA cockpit and
feedback surfaces. The later 0.6.0 hardening work also adds an Action
Firewall eval suite, explicit memory ciphertext-at-rest modes, tenant-scoped
protected audit evidence, context-budget/provenance guardrails, EU AI Act
export evidence, repo-intelligence graph queries, runtime observability, and
the first meta-harness evaluation models for scoring workflow candidates.

### Desktop Provider Setup

- Desktop settings now has a dedicated Providers tab, so LLM provider setup is
  no longer buried in the general Settings page.
- OpenAI Codex account auth is available from the desktop provider panel. Users
  can sign in through the browser, import an existing local Codex session,
  reconnect, or disconnect the stored Codex OAuth session.
- Built-in desktop provider coverage now includes OpenAI Codex, OpenRouter,
  Anthropic, OpenAI, Groq, Mistral, Together, Cohere, llama.cpp, Ollama, Poe,
  Azure OpenAI-compatible, Amazon Bedrock-compatible, Vertex-compatible, and
  GitHub Copilot-compatible providers.
- Provider/model selection is more robust: authenticated or local/keyless
  providers can persist the selected model, while unauthenticated hosted
  provider toggles no longer masquerade as configured chat models. Chat session
  creation also falls back to the enabled/default provider slot when the model
  picker has not populated an explicit value yet.
- The engine now honors the managed `OPENCODE_CONFIG` path supplied by the
  desktop launcher, keeping the provider UI and sidecar registry in sync during
  local Tauri development and packaged desktop runs.
- Session creation persistence errors now return structured details instead of
  a bare `500 Internal Server Error`. Windows temp-file sync/replace handling
  retries transient `Access is denied` failures that can happen while saving the
  first chat session.

### Enterprise Hardening Snapshot

- The Action Firewall now has regression coverage and a demo preset for
  protected action decisions before tool execution.
- Tenant-scoped protected audit ledger and governance evidence export surfaces
  make protected decisions traceable across policy decisions, audit records,
  tool-effect ledger entries, and run context.
- Cross-tenant grants now have public contract/server implementation surfaces
  and positive sharing eval coverage. Ordinary tenant boundaries still fail
  closed; cross-tenant access must be represented as an explicit governed grant.
- Default data-boundary and cross-tenant grant design docs describe how
  governed reads, inbound lookup, trust roots, and explicit sharing compose.
- Egress DLP preflight checks outbound agent-team actions before they leave the
  runtime boundary.
- Sensitive-path basename fallback protections were rechecked, and shared
  SSRF URL/IP validation now covers web fetch and browser navigation paths.

### Evaluation Gate Assurance

- The per-PR evaluation regression gate now fails closed when `eval-runner`
  cannot build or when an evaluation run crashes. Earlier CI behavior could
  emit hardcoded passing JSON for the critical-path, tenant-isolation, and
  action-firewall datasets, creating a plausible green check even when the
  runner was broken. The gate now builds `eval-runner` as a required step, runs
  each dataset through the binary directly, and reports missing results in the
  PR comment instead of fabricating pass rates.
- Full workspace test coverage now runs through `cargo-nextest`, and an
  end-to-end `tandem-engine` smoke-test CLI gives release validation a direct
  runtime path rather than only unit-level proof.
- The meta-harness work now includes prompt-injection exfiltration and
  blast-radius evaluation coverage, expanding the release's regression suite
  beyond workflow scoring into adversarial context handling.

### Compliance Evidence And Exports

- EU AI Act readiness now includes deployment-scope tracking, Article 50
  transparency badges/labels, hash-chained audit ledgers, SIEM export guidance,
  and generated-artifact provenance preservation across exports.
- Protected-action and approval evidence now records completeness checks, so
  operators can distinguish complete, incomplete, and unsupported evidence
  packages when auditing governed runtime decisions.
- Exported artifacts preserve generation provenance and Article 50 labels,
  keeping downstream compliance packages tied to the run and policy evidence
  that produced them.

### Runtime Diagnostics And Release Safety

- Startup config validation catches invalid or surprising runtime configuration
  before the engine proceeds, reducing silent misconfiguration during desktop,
  local, and hosted launches.
- Runtime observability events are persisted, giving operators and debugging
  tools durable lifecycle evidence rather than relying only on transient logs.
- Structured HTTP error codes make API failures easier to classify and recover
  from, including provider/session setup paths that previously collapsed into
  generic server errors.
- Tandem-server panic-surface guards, async runtime hygiene checks, and
  tandem-tools path sandbox regression tests reduce release risk around server
  crashes, blocking runtime mistakes, and filesystem escape regressions.

### Repo Intelligence And Workflow Graphs

- Repo intelligence now has manifest/fact extraction, persistent store/query
  APIs, context bundle queries, exposed tools, quality regressions, metrics,
  debug export, and GraphRAG retrieval improvements.
- Workflow and run graph foundations now support context graph storage,
  governed query envelopes, runtime planning queries, memory/rerun queries,
  failure-causality analysis, workflow impact analysis, routing hints, and
  benchmark reporting.
- The graph and repo-intelligence crates are included in release automation so
  these new diagnostics are built and validated with the shipped workspace.

### Cross-Tenant Data Governance

- Hosted and enterprise requests now rely on signed tenant-context assertions
  as the trust primitive. Assertions are Ed25519 JWS values selected by `kid`,
  validated against issuer/audience, expiry, explicit deployment scope, actor
  consistency, key metadata, allowed resource-scope prefixes, and a replay
  policy (`bound` by default, with `one_shot` available for per-request
  issuers). `docs/CONTEXT_ASSERTION_SECURITY.md` documents configuration and
  operational guidance.
- A dedicated `tenant_isolation` evaluation dataset now runs in the per-PR
  regression gate, covering must-block scenarios: cross-tenant source/secret
  access and cross-tenant memory reads must fail closed and emit audit evidence
  (CT-01).
- The local eval-runner stub mode boots a real in-process `AppState`, so the
  cross-tenant evals exercise real tenant-scoped enforcement instead of
  deterministically echoed output shapes (CT-16).
- A real-engine isolation eval proves that an automation running as tenant A
  cannot read a resource that exists only for tenant B through the runtime tool
  path (CT-02).
- The audit read path (`/audit/stream`) is now tenant-scoped: it fails closed
  for explicit tenants, recognizes both nested `tenantContext` and flat tenant
  tags, and remains a no-op for local/single-tenant deployments. `fintech` and
  tool-effect audit events are now tagged with their originating tenant so they
  remain visible to their own tenant while staying isolated from others. New
  unit, HTTP-integration, and eval coverage assert that tenant B cannot read
  tenant A's audit events (CT-04).
- Memory promotion is now tenant-scoped, preventing untrusted memory from being
  promoted across tenant boundaries. A new eval dataset proves the isolation
  (CT-03).
- Channel interactions (Discord, Slack, Telegram) now enforce tenant routing,
  failing closed at the channel interaction audit layer when cross-tenant access
  is attempted (CT-05).
- Knowledge retrieval now has a dedicated negative eval proving tenant B cannot
  retrieve tenant A's knowledge-base items, and skills isolation is locked by a
  regression test showing project skills resolve only from the executing
  workspace root (CT-08).
- Provider catalog discovery and provider throttles are tenant-scoped. Explicit
  hosted tenants use only their own persisted provider auth, cannot inherit
  shared config/env/local runtime provider keys, and do not get queued behind
  another tenant's provider backoff.
- Store-backed MCP secret headers are checked against the executing tenant
  before OAuth refresh or outbound MCP calls, returning tenant-scope denials for
  cross-tenant secret attempts.
- Governance approval receipts now carry the issuing tenant. Cross-tenant
  approve/deny attempts fail closed without leaking receipt existence, approval
  listing is tenant-scoped, and approval audit events use the real tenant
  context (CT-09).
- Automation spend/quota guards are keyed by tenant for explicit tenants, so
  one tenant's same-named agent cannot pause or block another tenant's run
  lifecycle (CT-10).

### Memory Security Hardening

- A memory retrieval gateway governs channel reads, applying tenant and source
  scoping before retrieved memory is used in channel responses.
- Retrieval egress controls (TAN-102) restrict which retrieved memory and
  knowledge can leave through session knowledge-base grounding and export paths.
- Memory crypto mode is now explicit. Local/default installs remain plaintext,
  local encrypted mode stores encryptable memory payload columns as
  AES-256-GCM ciphertext, and hosted KMS mode fails closed on plaintext writes
  until a KMS-backed decrypt broker is provisioned.
- The memory ciphertext-at-rest documentation names which columns are encrypted,
  which search-required plaintext columns remain residual risk, and how local
  encrypted backups must preserve the key file.
- Memory poisoning trust gates label memory by trust level and gate promotion;
  untrusted search results, channel reads, and prompt context are surfaced as
  trust-scoped evidence. A memory-poisoning eval dataset locks in the behavior.
- A scoped memory decrypt broker brokers per-scope data-encryption-key unwrap
  through tickets (carrying the wrapped DEK) rather than exposing keys broadly,
  and memory envelopes now carry key-scope metadata binding ciphertext to a
  specific key scope.
- A memory key lifecycle evidence gate and a memory database blast-radius
  boundary check are enforced in CI to bound and document the impact of a
  memory-store compromise.

### Governance Enforcement

- Automations V2 gate decisions now require a verified human decider, closing a
  gate-decision self-approval path (GOV-B1).
- Governed approval gates now enforce reviewer eligibility from approval
  metadata. Non-human decisions are rejected and audited, self-approval is
  blocked, reviewer authority is verified, and data-class/resource grants are
  required when the gate demands them.
- A first-class policy decision store records runtime authorization decisions
  with tenant, actor, tool, approval evidence, audit/evidence export, and run
  trace context (CT-17). `GET /governance/policy-decisions` supports tenant and
  run filtering, context-run journaling emits policy decision events, and
  tool-effect ledger records can link back to the policy decision that governed
  execution.
- A shared tool risk-tier taxonomy gives policy gates, approval cards, run
  traces, and evidence exports a stable vocabulary for categories such as
  credential access, destructive delete, money movement, and financial access
  (CT-19). Risk tiers are inferred from tool security descriptors and tool
  profile/name heuristics, with explicit descriptors taking precedence.
- MCP inventory now exposes governed tool registry metadata, including redacted
  credential binding, tenant binding, owner, resource scope, risk tier, default
  access/policy, and explanatory reasons. Scoped `mcp_list` filtering stays
  aligned with those governed registry rows.

### Runtime Authority

- An intra-tenant authority graph models the boundaries _inside_ a single
  tenant (CT-18). It resolves a principal's effective grants from direct grants
  plus organization-unit memberships — honoring role-domain nesting and
  parent-department inheritance — and renders fail-closed decisions: access is
  allowed only on a matching allow grant, an explicit deny grant always wins,
  and the absence of any grant denies. So a junior engineer (or its agent)
  cannot read lead/internal architecture docs without a grant, an engineer
  cannot read finance records by default, and a finance actor can read financial
  records but not engineering secrets unless explicitly (and expirably) shared.
  Server enforcement records every decision as a policy decision and writes a
  tenant-attributed protected audit event on denial. Seeded
  engineering/finance/sales/HR/executive/support personas ship as fixtures.
- A declarative approval gate matrix maps an action's risk tier and data class
  to a gate outcome — allow, deny, or approval-required — together with the
  reviewer eligibility the approval demands and the TTL to apply (CT-20).
  External customer-facing sends pause for approval by default; restricted,
  credential, financial, executive, and regulated data classes require an
  elevated reviewer; high-risk tiers (credential/admin, money movement,
  destructive delete, financial access) require elevated review on a tighter
  TTL; and an action that cannot be classified fails closed to an
  elevated-reviewer approval rather than auto-allowing. An expired approval can
  never authorize execution. Server enforcement records every gate decision as a
  policy decision and writes protected audit evidence for approval-required and
  deny outcomes.

### Context Hygiene And Runtime Guardrails

- The engine context assembly map now traces the provider-facing prompt
  boundary across ordinary chat, workflow, automation, routine, coder-worker,
  strict-KB, workflow-planner, and mission-builder paths.
- `context.budget.final` telemetry reports final message/tool-schema/attachment
  sizes, per-source contribution accounting, compaction counts, and Full-context
  budget diagnostics without logging prompt bodies.
- Full-context mode now has soft and hard budget guardrails. The hard budget
  fails closed before provider send unless explicitly overridden.
- Standard and compact history projection now preserve provenance handles for
  dropped message ranges and stored message ids, while pinning decision/guardrail
  context forward instead of losing approval boundaries.
- Long-session context evals assert both answerability and provenance: they fail
  if context hygiene only passes by injecting too much raw history or omitting
  compaction handles.
- Prompt hook context budgeting now reports per-source additions so identity,
  memory scope, KB grounding, docs, and global memory blocks can be audited as
  distinct context contributors.

### Goal Capability Learning

- A first slice of Goal Capability Learning (GCL) lands as the front end for
  _composing a new workflow toward a goal_, distinct from Workflow Learning,
  which repairs an existing workflow from execution traces (GCL-01). A goal is
  expressed as a `GoalSpec`; discovery decomposes it into tool-agnostic
  `CapabilityRequirement`s, resolves those to available capabilities, and
  produces a ranked `CompositionPath` — demonstrated end-to-end on the smallest
  demo goal, "read and parse a CSV file" (`file_read → csv_parse`) (GCL-02).
- Strategy review reuses the existing governance shape rather than inventing a
  parallel one (GCL-03). A `StrategyCandidate` moves through a fail-closed
  lifecycle (`Proposed → Approved → Applied`, with `Rejected`/`Superseded`
  terminals) — it cannot skip review or re-open once applied — and an approved
  strategy materializes into a `WorkflowProposalDraft` that links into the
  existing planner plan-draft and Automation V2 preview surfaces. Goal-planning
  and strategy/proposal review emit namespaced audit events. Discovery decisions
  are recorded per tenant and exposed through tenant-scoped endpoints that derive
  the tenant from the authenticated context, never from caller input.
- The Workflow Learning v1 production-validation and auto-apply policy is now
  decided and enforced (GCL-04). A declarative policy governs whether a proposed
  learning candidate may be auto-applied and whether an applied candidate has
  regressed against its baseline. Auto-apply is off by default and fails closed
  to human review; structural graph rewrites and plan-bundle changes always
  require a human; confidence, evidence (minimum recent-run sample), and a
  recent human-intervention ceiling gate eligibility; and the before/after
  regression check (post-apply minimum sample, completion- and
  validation-rate thresholds) is centralized and unit-tested with behavior
  identical to the previous inline check. All knobs are configurable via
  `TANDEM_WORKFLOW_LEARNING_*` environment variables.

### Coding Workflows

- The control panel has a Coding Workflows Cockpit tab for selected ACA runs,
  showing source identity, run state, GitHub PR/merge state, repository context,
  event summaries, and an operational thread.
- Operators can send run/thread-scoped feedback from the cockpit. Feedback is
  stored in ordered file-backed audit records, delivered to ACA via
  `/operator/feedback`, replayed when pending, and streamed back into the
  cockpit through SSE updates.
- Linear MCP approval classification now distinguishes read tools from write
  gates, adds explicit Linear read/write capability bindings, and reports
  connector readiness states for missing, read-only, and write-capable Linear
  setups.

### Per-Role Sampling Parameters

- The engine runtime and the `tandem-client` Python SDK (now `0.6.0`) accept
  per-role sampling parameters — `temperature`, `top_p`, and `max_tokens`.
  Callers set a session-level default on `sessions.create(...)` and may override
  it per prompt on `prompt_async(...)`; the per-prompt value takes precedence
  field by field. This lets JSON-emitting roles (manager / reviewer / tester) run
  at a low temperature for more deterministic, parseable output while workers can
  use a different value.
- Parameters are mapped to each provider's request shape (OpenAI-compatible chat
  completions, the OpenAI Responses API's `max_output_tokens`, and Anthropic) and
  clamped to the provider's supported range rather than rejected. Models that do
  not accept an explicit `temperature` (such as OpenAI reasoning families) have
  the parameter dropped with a logged warning instead of failing the run.
- All fields are optional and fully backwards compatible: omitting them produces
  a provider request identical to prior releases.

### Meta-Harness Evaluation

- A new `tandem-meta-harness-eval` crate defines stable trace and scoring
  models for workflow/version evaluation.
- Score values must deserialize to finite numbers, preventing `NaN` or
  infinity from entering deterministic candidate ordering.
- Public meta-harness design docs now describe the optimizer loop, candidate
  scoring/promotion lifecycle, and grouped human approval surfaces for reviewing
  proposed workflow improvements.

### Security Review

- Strict tenant enforcement now covers external-effect built-in tools and MCP
  dispatch. Local-implicit contexts are blocked from web, memory, shell/network,
  and MCP calls in strict modes; store-backed MCP secret headers are checked
  against the executing tenant/deployment before dispatch; and built-in tool
  alias/path resolution is covered against namespace spoofing, parent traversal,
  unsafe absolutes, wildcard tokens, and symlink escapes.
- Added a source-verified Rust runtime security analysis covering command
  execution, HTTP API exposure, secrets/crypto, permission/governance defaults,
  and external integration risks. The report records source-location-backed
  remediation findings that informed the 0.6.0 hardening work.

## v0.5.13 (2026-06-02)

Tandem 0.5.13 combines the Linear-backed Coder intake work with a focused
runtime security hardening pass. The release tightens local API exposure,
workspace mutation defaults, shell execution, tenant scoping, audit/event
visibility, secret storage, and browser/provider network guardrails.

### Coder Linear Intake

- The Coder control panel can now register ACA projects backed by Linear teams
  and projects, with optional launch-status, label, and search-query filters.
- The Coder intake board now renders both GitHub Project items and Linear
  issues through one scheduler-aware issue board, including batch launch,
  active-run detection, and direct issue links.
- Coder overview and intake refresh messaging now reflect the selected issue
  source, including Linear MCP connection state when a Linear-backed project is
  selected.

### Automations V2 Reliability

- Automations V2 completion is now gated by terminal checkpoint integrity and
  contract-aware deliverable assertions instead of only checking for empty
  pending-node queues.
- Required file deliverables must exist, be substantive, and pass basic shape
  checks; missing or weak deliverables requeue the owning node while repair
  attempts remain.
- Required email delivery and generic outbound connector actions now need
  successful receipt evidence before the run can complete. Model prose alone no
  longer satisfies governed side effects.
- Workflow graph validation now rejects dependency cycles and keeps `input_refs`
  aligned with readiness dependencies, including through budget compaction and
  strict sequential plans.
- Verification failures now retry through the repair path until attempt budget
  is exhausted, and verification failure detection is scoped to verification
  output rather than unrelated artifact prose.
- Recoverable tool execution errors are surfaced to the model for adaptation,
  while cancellation, shutdown, runtime-not-ready, and write-required
  permission failures remain loud failures.
- Timer-triggered automations now dedupe queued/running runs the same way watch
  triggers do, preventing slow scheduled workflows from accumulating backlogs.
- Parked-state lifecycle handling is explicit: approval gates can be marked as
  visibly stale under a manual-only policy, guardrail-stopped runs can
  auto-resume after approved quota overrides, stale reaping honors active
  run-registry heartbeats, and node execution uses idle/no-progress timeouts
  with an absolute ceiling.
- Warning outcomes are now consistent across runtime and learning surfaces:
  `accepted_with_warnings` remains passable only without unmet requirements,
  but it is not counted as a clean workflow-learning validation pass and does
  not generate positive learning evidence.

### Runtime Security Hardening

- Local engine HTTP API startup now refuses unauthenticated non-loopback binds,
  and token-clearing no longer reopens the API.
- HTTP MCP registration rejects arbitrary `stdio:` transports.
- File write, edit, and patch tools now ask by default instead of silently
  mutating the workspace.
- Batch sub-calls pass through permission and sandbox evaluation so nested tool
  calls cannot skip approval gates.
- Workspace and write-policy checks fail closed when no workspace root can be
  resolved.
- Shell execution uses Linux `bubblewrap` confinement by default, requires
  workspace context, and requires an explicit unsafe opt-out for unsandboxed
  shell execution.
- Automation auto-approval now treats empty allowlists as deny-all and refuses
  to auto-approve shell tools.
- Local single-tenant mode ignores caller-supplied tenant headers; hosted and
  enterprise tenant context continues to require signed assertions.
- Run event streams, audit streams, and project listing now enforce tenant
  ownership/visibility checks.
- API tokens, vault keys, and TUI keystores are written with owner-only Unix
  permissions, and vault passphrases replace the previous 4-digit PIN model.
- Browser navigation fails closed without an allowlist and blocks local/private
  targets; provider base URL validation rejects unsafe remote HTTP endpoints.
- Provider credential debug output and bug-monitor log redaction now avoid
  leaking plaintext secrets.

### Compatibility Notes

- Linux hosts that need shell execution must have `bubblewrap` available, or
  explicitly set `TANDEM_UNSAFE_UNSANDBOXED_SHELL=1` for trusted local-only
  development.
- Local clients should rely on the generated/shared API token rather than
  clearing token auth during development.

## v0.5.12 (2026-05-27)

Tandem 0.5.12 hardens the hosted control-panel sign-in path and starts the
hosted organization access model inside the runtime. Hosted panel sessions now
carry Tandem-signed org-unit membership, capabilities, and policy-version
context, and automation v2 resources can be private, group-shared, or org-wide.

### Hosted Panel Access

- Hosted panel session exchange and refresh payloads now preserve org units,
  effective capabilities, and policy version from Tandem-hosted identity.
- `/api/auth/me` exposes the hosted access context so the control panel can
  filter navigation and actions without treating UI hiding as enforcement.
- The control-panel proxy now uses capabilities for hosted reads, automation
  execution, automation writes, and sharing actions before forwarding requests
  to the engine.

### Runtime Resource Sharing

- Hosted automation v2 creation stamps owner/private access metadata from the
  verified Tandem assertion instead of trusting caller-provided user fields.
- Automation v2 list/read/run routes enforce private, group, and org visibility
  in hosted mode. Private automations remain visible to their creator and
  hosted admins.
- Owners/admins can update automation visibility through
  `POST /automations/v2/{id}/share`, setting private, group, or org-wide
  access.
- Automation mutation and repair routes now require owner/admin access for
  hosted resources, while legacy/local unscoped operation remains compatible.

### Compatibility

- Slack, Discord, and Telegram approval-gate integrations continue to call the
  runtime gate-decision path without requiring a browser hosted assertion.

## v0.5.11 (2026-05-25)

Tandem 0.5.11 prepares the hosted enterprise engine distribution path. The
release now builds a Linux x64 `tandem-engine` binary with browser automation
and enterprise-full routes compiled in, publishes it as a separate enterprise
release asset, and adds a dedicated npm wrapper for hosted sidecar deployments.

### Hosted Enterprise Engine

- Added `tandem-engine-enterprise-linux-x64.tar.gz` to the Linux release asset
  set. The archive extracts to a normal `tandem-engine` binary so existing
  hosted sidecar scripts can keep the same command name.
- Added the public `@frumu/tandem-enterprise` npm package for Linux x64 hosted
  deployments. Its installer downloads only the enterprise Linux asset and
  fails clearly on unsupported platforms.
- Kept the standard `@frumu/tandem` package on the normal public engine asset,
  so desktop and local installs do not pick up enterprise-full dependencies by
  default.
- Gated automatic enterprise npm publishing behind `PUBLISH_NPM_ENTERPRISE=true`
  so the 0.5.11 registry publish can ship existing packages first while the new
  npm package is first-published and configured intentionally.

## v0.5.10 (Released - 2026-05-25)

Tandem 0.5.10 opens the enterprise connector source-binding workstream. The
focus is safe ingestion governance for hosted and enterprise deployments:
connector credentials as secret references, source bindings mapped to
`ResourceRef` and `DataClass`, quarantine/revoke/rotate workflows,
resource-scoped memory retrieval, and hosted authorization hardening around
who may bind company data into Tandem.

This release contains the contract foundation, enterprise
admin shell, storage-backed organization-unit registry, and storage-backed
source-binding registry. Manual memory imports can now optionally target an
enabled source binding so imported chunks carry resource and data-class
metadata. Google Drive now has a guarded admin-triggered import path; Notion,
GitHub, Slack, Gmail, live OAuth, background workers, and production connector
automation remain follow-up implementation phases.

### Enterprise Connector Source Binding

- Added transport-safe enterprise contract types for connector instances,
  connector lifecycle state, connector credential references, credential
  classes, source binding state, ingestion policy, source objects, ingestion
  jobs, ingestion quarantine, quarantine dispositions, and scoped memory chunk
  references.
- Added generic organization-unit taxonomy contract types so admins can model
  company-specific domains such as HR, Doctors, Consultants, Claims Adjusters,
  Board Members, or Platform Oncall without hardcoded Tandem roles.
- Organization-unit memberships can feed `ScopedGrant` through the new
  organization-unit membership grant source while preserving the existing
  principal/resource/data-class projection model.
- Added enterprise admin endpoints for org-unit and source-binding management.
  Organization units now have tenant-scoped storage-backed create/list behavior;
  source bindings now have tenant-scoped storage-backed create/list/update
  behavior with `ResourceRef` tenant validation and admin-gated mutations.
- Verified hosted context now preserves signed assertion roles so enterprise
  admin mutations can fail closed unless the Tandem-signed hosted context
  carries admin/owner/reconfigure authority.
- Added a hidden-by-default Enterprise admin page in the control panel. It reads
  the storage-backed org-unit and source-binding endpoints, displays
  tenant/principal context, creates org units and source bindings, and can move
  source bindings between enabled, disabled, and quarantined states.
- Connector credential references carry only `SecretRef` metadata and default
  to read-only credentials. They intentionally do not model raw credential
  values.
- Source bindings map external source roots to Tandem `ResourceRef` and
  `DataClass` values, giving manual upload and future external connectors a
  common resource-scoped ingestion contract.
- Manual memory imports accept an optional `source_binding_id`, fail closed if
  the binding is outside the tenant or disabled for indexing, and stamp chunks
  with source-binding, resource, data-class, and source-object metadata while
  preserving local/default import behavior when unset.
- Source-bound vector memory now fails closed before ranking. Chunks stamped
  with enterprise source-binding metadata are hidden unless the caller supplies
  a strict tenant access projection with a matching `Read` grant for the bound
  `ResourceRef` and `DataClass`.
- Governed/global memory search now applies the same source-binding guard.
  Records with enterprise source-binding metadata are filtered out unless the
  verified tenant assertion carries a strict projection with matching
  resource/data-class read authority.
- Response-cache entries can now be partitioned by tenant and source binding,
  and invalidated for a specific binding. Source-binding admin changes emit an
  invalidation-required event so future cache consumers can purge stale answers
  after disable, quarantine, revoke, or permission changes.
- Tool schemas now have additive enterprise security descriptors covering
  required permissions, resource kinds, data classes, admin surfaces, external
  side effects, credential access, and default visibility. Built-in tools emit
  descriptors from their metadata, and unannotated MCP/provider tools can be
  classified conservatively before future discovery masking and execution
  enforcement.
- The embedded MCP catalog now carries security metadata for servers and
  cataloged tools. Catalog-provided overrides can mark sensitive tools as
  admin/credential/hidden, while unannotated tools receive conservative
  descriptors from catalog context and action classification.
- Operators can provide JSON/YAML MCP tool-security overrides with
  `TANDEM_MCP_TOOL_SECURITY_OVERRIDES_PATH`, allowing enterprise deployments to
  tune server and per-tool security descriptors without rebuilding the embedded
  catalog.
- `mcp_list` now applies discovery authorization when a signed strict tenant
  projection is present. Unauthorized MCP tools are removed from the discovery
  inventory before the model can see them; local/unscoped discovery remains
  unchanged.
- Provider/model calls now apply the same strict projection to advertised tool
  schemas before invocation. Unauthorized admin, credential, execute, or
  resource-scoped tools are omitted from the provider-visible tool list while
  local/unscoped sessions remain compatible.
- Source-bound manual uploads now create durable source-object lifecycle
  records keyed by tenant, source binding, and native object identity. Changed
  documents keep the same source object ID while their hashes update, and
  `sync_deletes` tombstones removed uploads so later admin workflows can
  reindex, delete, or re-scope by binding/resource.
- Enterprise admins can now list source-object lifecycle records for a source
  binding, request reindex by purging stale chunks/import index rows, hard
  delete a source object and indexed content, or re-scope its resource/data
  class metadata. Each mutation reuses enterprise admin authorization and emits
  source-binding cache invalidation.
- The hidden Enterprise admin page now exposes those source-object lifecycle
  rows for a selected source binding and provides reindex, delete, and re-scope
  controls from the hosted admin UI.
- Hosted/enterprise manual memory imports now require `source_binding_id` so
  company data is scoped to a `ResourceRef` and `DataClass` before indexing.
  Local/default imports can still remain unbound for non-enterprise installs.
- Local/default manual memory imports can now opt into the generated
  `local_manual_upload` binding. This gives local installs source-object
  lifecycle tracking under an internal `document_collection` scope without
  forcing legacy unbound imports to migrate immediately.
- The enterprise connector trust-proof matrix now has explicit coverage for
  hosted non-admin connector creation denial, source-bound upload
  `ResourceRef` lifecycle stamping, and same native source IDs remaining
  tenant-scoped.
- Source-bound memory retrieval now has explicit tenant-isolation proof: tenant
  A cannot retrieve tenant B chunks even when the binding ID, native object
  path, and search phrase overlap.
- Source-object re-scope now has explicit purge coverage: old indexed chunks
  are deleted before lifecycle metadata moves to the new resource/data class,
  preventing stale prompt context from surviving permission changes.
- Source-bound prompt-context assembly now has explicit regression coverage:
  source-bound current-session and history chunks are excluded from assembled
  prompt context unless a strict tenant projection grants read access to the
  bound `ResourceRef` and `DataClass`.
- Governed memory list responses now apply the source-bound visibility guard
  before returning metadata, preventing list/citation browser surfaces from
  exposing source-object IDs, native object paths, or binding IDs without a
  strict read grant.
- Coder governed-memory hit artifacts now fail closed for source-bound records,
  with coverage proving coder memory-hit responses do not expose source-object
  IDs, native object paths, or binding IDs without a strict read grant path.
- Automation upstream evidence now filters source-bound internal identifiers
  from read paths, discovered paths, and citations before downstream workflow
  nodes can reuse them as citation evidence.
- Strict session KB grounding now ignores source-bound internal identifiers
  when extracting source labels and document refs, preventing KB citation
  renderers from exposing source-object metadata.
- Disabling or quarantining a source binding now purges indexed content for its
  lifecycle records and tombstones affected source objects, closing the
  binding-level stale prompt-context path after permission changes.
- Prompt-context injection and coder duplicate-memory scans now skip
  source-bound governed records by default, closing remaining local/default
  memory caller gaps when no strict grant projection is available.
- Managed hosted detection now reports hosted auth availability separately, so
  disconnected local test deployments can still use the engine-token sign-in
  path while connected hosted servers keep Tandem hosted login enforcement.
- Connector instances now have storage-backed tenant-scoped admin endpoints for
  lifecycle management. Source-bound imports require the referenced connector
  to exist and be active, so paused, revoked, or quarantined connectors block
  ingestion before data reaches memory.
- The hidden Enterprise admin page can now create connector lifecycle records,
  list tenant-scoped connector status, and pause, revoke, quarantine, or
  reactivate connectors from the control panel.
- Enterprise organization-unit memberships now have tenant-scoped runtime
  storage, admin-gated create/list/update endpoints, and hidden admin UI
  controls for assigning hosted users, groups, agents, and service accounts to
  company-defined units such as departments, clinical roles, consultants, or
  executive groups. This is the first Phase H execution slice for turning
  company taxonomy into future signed grant projection.
- Organization-unit access grants now provide the missing access-rule layer
  between company taxonomy and resource/data-class permissions. Enterprise
  admins can create tenant-scoped unit grants, preview the effective
  `ScopedGrant` projection for a member, and disable grants before the global
  signing middleware begins appending those projections to verified strict
  contexts.
- Signed hosted/enterprise request ingress now appends active
  organization-unit membership grants into existing verified strict contexts.
  This makes company-defined units such as departments, clinical groups, or
  consultant groups available to strict runtime access checks while preserving
  fail-closed behavior for assertions that do not already carry a strict
  projection.
- Added denial-focused strict-context tests for department and executive
  access: finance grants do not reveal engineering source data, engineering
  grants do not reveal HR compensation records, CEO/global access is explicit,
  and CEO-spawned agents remain narrow unless the signed projection delegates
  broader access.
- Fintech audit package assembly now filters scoped artifacts before export.
  Artifacts carrying `ResourceRef` and `DataClass` metadata are included only
  when a strict projection grants `Read` for that resource/data class; scoped
  artifacts are excluded with an audit-package limitation when authorization is
  missing.
- Connector credential-reference admin endpoints now accept and rotate
  `SecretRef` records without accepting raw credential values. Credential refs
  are tenant-validated, can be source-bound to a resource, and are visible in
  the hidden Enterprise admin page as metadata only.
- Source-bound manual memory imports now write persisted enterprise ingestion
  job audit records with connector/binding scope, job state, timing, and
  touched source-object IDs. The hidden Enterprise admin page can inspect these
  records for a selected binding.
- Review-required source bindings now create persisted quarantine records and
  remove newly indexed chunks before they can be retrieved. Enterprise admins
  can review quarantines from the runtime or hidden admin page and mark a
  release, delete, or reindex disposition.
- Connector revoke/rotate response handling now has an admin-visible impact
  summary. The runtime and hidden Enterprise admin page can report affected
  bindings, source objects, ingestion jobs, quarantines, compromise window, and
  recommended response actions for a connector. Compromise-window timing includes
  source-object lifecycle timestamps so uploaded/indexed content bounds the
  audit window.
- Source-binding, source-object, quarantine-review, connector lifecycle, and
  connector credential changes now invalidate matching source-bound response
  cache entries when the response cache is present.
- Google Drive is now exposed as the first constrained enterprise connector
  provider with v1 read-only/source-bound credential policy guardrails. The
  runtime now includes a read-only Drive client for folder listing, stored-file
  download, and Google Workspace export, plus a runtime-only `env://...`
  secret-ref resolver and admin-gated source-binding preflight endpoint for
  local bearer-token testing.
- The first admin-triggered Google Drive import endpoint now reuses those
  guardrails to fetch supported Drive documents into a stable source-binding
  namespace, create ingestion job and source-object lifecycle records, honor
  review-required quarantine, and invalidate source-bound response-cache entries
  after indexing. This remains an admin-controlled v1 path, not broad automatic
  OAuth ingestion.
- The hidden Enterprise admin page can now run Google Drive source-binding
  preflight, trigger the admin-controlled import endpoint, and refresh
  source-object, ingestion-job, quarantine, and connector-impact views around
  the selected binding.
- The Google Drive import path now has HTTP-level regression coverage proving a
  review-required source binding records a quarantined ingestion job,
  source-object lifecycle row, and quarantine record while keeping resolved
  credential material out of responses.
- Google Drive enterprise preflight and import route handling now live in a
  focused HTTP module, keeping connector-specific orchestration separate from
  the general enterprise admin routes as more connector behavior lands.
- Organization-unit and ingestion/source-object lifecycle endpoints also now
  live in focused enterprise HTTP modules, keeping the primary enterprise admin
  route file under the source-size guideline before the next connector work.
- Google Drive source bindings now have an explicit admin re-fetch/reindex
  operation. It reuses the read-only source-bound credential checks and stable
  binding namespace, records ingestion jobs, honors quarantine policy, evicts
  source-bound cache entries, and keeps resolved credential material out of
  responses.
- Ingestion gating helpers model the required fail-closed behavior for paused,
  revoked, or quarantined connectors, disabled bindings, and review-only
  ingestion policy.
- Added the internal enterprise connector source-binding Kanban covering
  rollout phases, mitigations, memory/retrieval implications, denial tests,
  and open ownership decisions.

## v0.5.9 (2026-05-21)

Tandem 0.5.9 continues the hosted tenant-isolation work for Automation V2. The
focus is denial-driven hardening for background and applied automation paths:
scheduled runs, watch-triggered runs, stale recovery, imported/applied
definitions, Automation V2 event visibility, runtime route isolation, provider
and MCP credential boundaries, vector-backed memory partitioning, and the first
coder artifact tenant boundary. The current unreleased work also starts the
workspace access-control contract layer for Google Workspace-style company data
and resource grants.

### Enterprise Workspace Access Control

- Added public enterprise contract vocabulary for organization/workspace/
  department/project/resource hierarchies: `ResourceKind`,
  `ResourcePathSegment`, `ResourceRef`, and `ResourceScope`.
- Added access-control vocabulary for `View`, `Read`, `Edit`, `Execute`,
  `Delegate`, and `Admin`, plus data classes such as executive, credential,
  source-code, customer-data, and financial-record scopes.
- Added normalized principal references for humans, groups, departments, agent
  workers, automations, service accounts, external delegates, and support
  operators.
- Added `GrantSource` and `ScopedGrant` so access can be attributed to direct
  assignment, group membership, department membership, inherited grants,
  explicit executive/global grants, delegated projections, or break-glass
  authority.
- Added `StrictTenantContext`, `DataBoundary`, and `AssertionMetadata` as the
  additive strict context object for hosted/enterprise projections over tenant
  context, principals, authority chains, resource scopes, grants, data-class
  boundaries, and signed assertion metadata.
- Added allow/deny grant effects, structured access decisions, and
  `StrictTenantContext` evaluation helpers so explicit denies win over
  inherited allows, projected resource scopes bound access, expired grants do
  not apply, and project grants can authorize path-scoped resources.
- Extended Tandem context assertion claims with optional principal,
  resource-scope, scoped-grant, and data-boundary projection fields. Existing
  tenant-only v1 assertions remain valid and deserialize without strict
  projection data.
- Added a typed enterprise signing-key purpose vocabulary for context
  assertions, approval receipts, delegation projections, A2A peer assertions,
  and break-glass/admin assertions.
- Added hosted context assertion key metadata checks so keyring entries can
  bind a public key to the `context_assertion` purpose, org/deployment,
  allowed audiences, allowed resource-scope prefixes, activation windows, and
  active status while preserving legacy string and delimited key formats.
- Re-exported the new contract vocabulary through `tandem-types`.
- Added contract tests covering Finance department data access, Engineering
  repository path scopes, cross-functional group access, CEO org-wide executive
  grants, MCP tool resource targets, expiring delegated vendor-agent access,
  data-boundary denials, project-scoped agent projections, explicit deny
  precedence, expired grants, narrow delegation, scoped assertion projections,
  and legacy assertion compatibility.
- Added the first hosted control-panel login exchange: managed hosted panels
  redirect users through `https://tandem.ac`, Tandem-web authorizes hosted org
  membership, the VM exchanges a one-time code with its host-agent token, and
  the browser receives only a panel session while the engine token remains a
  server-side root transport secret.

### Hosted Runtime Ingress

- Hosted and enterprise runtime modes now require a configured deployment
  transport token before accepting requests.
- Verified hosted context assertions must carry explicit deployment-scoped
  tenant context rather than `local_implicit`.
- Context assertion verification now rejects authority chains whose initiating
  actor does not match the signed human actor.
- Request principals derived from signed context now use the verified assertion
  issuer as their source, preserving the Tandem control-plane trust boundary.
- Managed hosted control panels now forward Tandem-signed context assertions to
  the engine proxy and hide customer dashboard engine-token reveal for managed
  deployments.

### Automation V2 Tenant Isolation

- Workflow planner apply, mission builder apply, and channel automation draft
  confirm now stamp persisted Automation V2 definitions from the request
  `TenantContext`.
- Automation V2 create/apply payloads cannot switch tenant context through
  embedded metadata.
- Scheduled/background-created runs inherit the stored automation tenant.
- Watch-condition runs now inherit the owning automation tenant instead of
  falling back to `local_implicit`.
- Automation V2 context-run blackboard sync inherits the run tenant, so
  background-created context runs do not silently become local implicit.
- Stale reaping and auto-resume regression coverage now proves explicit run
  tenant context survives recovery without an active HTTP request.
- Scheduler-published Automation V2 run-created events now include top-level
  `tenantContext`, allowing hosted/global SSE filters to enforce tenant
  visibility.
- Added finite-body Automation V2 SSE coverage proving a tenant stream receives
  its own event and does not receive another tenant's event.

### Runtime Tenant Isolation

- Session routes now enforce tenant ownership for list, get, delete, messages,
  prompting, attach, and workspace-override flows.
- Global event streams filter emitted events by tenant context so hosted
  tenants do not receive unrelated runtime events.
- Context-run internal routes are hardened by tenant for events/SSE, blackboard
  access, task claim/transition, checkpoints, replay, and ledger state.
- Automation V2 run/gate routes reject cross-tenant list, mutation, start,
  inspect, approve, deny, and rework attempts.
- Legacy workflow routes gained tenant checks so older governance-light paths
  cannot become a bypass around Automation V2 isolation.
- Coder-created context runs now inherit the request tenant, and coder
  status/list/get/artifact reads are filtered through the linked context run
  tenant.
- Coder control and artifact-writing routes now require the caller to match the
  owning context run tenant before approving, cancelling, executing, writing
  artifacts, or listing memory candidates. Added denial coverage proving tenant
  B cannot mutate or inspect tenant A's coder run through those routes.

### Provider And MCP Secrets

- Provider credential records are tenant-scoped for hosted/shared runtime mode.
- Provider create/list/read/update/delete/refresh paths use the request tenant
  and fail closed across tenant boundaries.
- Store-backed MCP secret references validate tenant scope before lookup.
- MCP tool execution now receives effective request/session/run tenant context
  so tenant A cannot execute with tenant B's stored MCP secret.
- Local single-tenant env/store secret behavior is preserved for local mode.

### Memory Isolation

- Governed memory search, list, read, promote, demote, update, and delete paths
  use tenant-aware DB methods.
- `memory_records` dedupe and user-created indexes now include tenant scope.
- Vector-backed session/project/global memory chunks now store tenant
  org/workspace/deployment scope.
- sqlite-vec top-k memory search filters the chunk table by tenant before
  distance ranking, preventing another tenant's closer vectors from suppressing
  the current tenant's results.
- Added denial tests for identical vector content, shared source hashes,
  cross-tenant vector search, cross-tenant vector deletes, tenant-scoped memory
  stats, project vector stats, manual clear, and old-session cleanup.
- Memory manager context retrieval now has tenant-aware APIs that scope recent
  session chunks and vector search before prompt context is assembled.
- Memory file import/index paths now carry tenant scope through import
  requests, index lookup/update/delete, stale file chunk replacement,
  sync-delete cleanup, project file-index stats, and project file-index clear.
- Added denial tests proving same project/path imports, identical file chunks,
  index deletes, stats, and clears do not cross tenant boundaries.
- Memory project/global config rows and old-session hygiene now use tenant
  scope, with tests proving same project ids cannot overwrite retention policy
  or prune another tenant's session memory.
- Knowledge spaces now include tenant-scoped uniqueness, and knowledge item,
  coverage, promotion, manager, and Automation V2 preflight paths use
  tenant-aware lookups so curated knowledge cannot cross hosted tenant
  boundaries.
- Existing local memory rows default to `local/local` during migration.

### Automation V2 MCP Diagnostics

- Required MCP tool validation now reports the exact missing tool ids in
  `missing_required_mcp_tools` and in `required_next_tool_actions`, making
  repair prompts specific instead of saying only that required MCP calls were
  incomplete.
- MCP connector results that return string errors such as `MCP error -32602`
  are now treated as failed tool results, so invalid connector arguments do not
  satisfy required-tool validation.
- Structured JSON nodes that declare `output_contract.schema` now validate the
  final artifact against that schema, so raw MCP account/quota/search payloads
  cannot pass as completed handoff artifacts.
- Automation V2 node preflight now derives concise MCP tool contracts from the
  offered tool schemas, including required arguments, minimal examples, and
  non-blocking schema warnings that are injected into prompts and diagnostics.
- MCP contract examples now respect positive `minLength` constraints for
  required string fields, preventing invalid empty-string examples for tools
  such as Notion search.
- Structured connector nodes now short-circuit across empty batch, empty
  candidate, empty high-value-contact, and empty write-row handoffs, writing the
  appropriate empty artifact instead of spending calls on account, inventory,
  enrichment, or write checks.
- Automation blocker panels now read checkpoint lifecycle history in addition
  to node outputs and event streams, surfacing node repair and run pause
  reasons that were previously hidden behind generic blocked status.

### Compatibility

- Local/default single-tenant behavior remains unchanged.
- This release does not start Zitadel/OIDC, SCIM, private sidecar, broader
  artifact isolation, or audit-export isolation work.
- File import/index isolation, governed knowledge-memory isolation, and broader
  memory-derived cache hardening remain follow-up work.

## v0.5.8 (2026-05-17)

Tandem 0.5.8 begins the enterprise auth, tenant context, and execution-time
verification implementation. This is the first runtime-facing slice: it adds
provider-agnostic tenant-context contract types and starts carrying tenant
context into tool policy evaluation, while preserving local and single-tenant
behavior by default.

### Enterprise Tenant Context Foundation

- Added explicit runtime auth-mode names for local single-tenant,
  hosted single-tenant, and enterprise-required operation.
- Added canonical parsing and operator-friendly aliases for the runtime auth
  modes, with `TANDEM_RUNTIME_AUTH_MODE` resolving to local single-tenant by
  default.
- Extended the enterprise contract with human actor metadata, deployment-aware
  tenant context, verified tenant-context assertion metadata, hosted tenant
  constructors, authenticated request principals, and request authority-chain
  helpers.
- Added provider-agnostic tenant context assertion header and claims types for
  the future Tandem-signed JWS passed from `tandem-web` to runtime/ACA.
- Re-exported the new contract types through `tandem-types` for runtime and
  server consumers.
- Added runtime verification for compact Tandem context assertions signed with
  Ed25519, including public-key configuration, issuer/audience checks, expiry
  checks, and tamper rejection in hosted and enterprise auth modes.
- Added `kid`-based context assertion keyring support through
  `TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS` / `_FILE`, with JSON object keyrings
  preferred and the existing single-key env vars preserved as fallback.
- Added hosted control-plane signer prep in `tandem-web`: a provider-neutral
  context assertion signer shape, local Ed25519 test signer, and Google Cloud
  KMS Software Ed25519 adapter for future hosted assertions.

### Runtime Policy Plumbing

- `ToolPolicyContext` now carries the session tenant context into runtime tool
  policy hooks.
- The engine loop loads the current session tenant before evaluating policy for
  a tool call.
- Hosted and enterprise runtime auth modes now reject raw tenant/actor headers
  and fail closed unless a configured Tandem-signed context assertion verifies.
- Verified hosted assertions are attached to request extensions alongside the
  derived tenant context and request principal, giving downstream runtime code a
  trustworthy identity object to consume in later sprints.
- Fintech strict protected-tool policy now rejects execution when the session
  tenant context does not match the owning Automation V2 run tenant context.
- In hosted and enterprise auth modes, fintech strict protected tools now fail
  closed if execution reaches the policy hook without a non-local tenant context
  and human actor.
- Sessions now persist verified tenant assertion metadata and pass it into
  `ToolPolicyContext`, so strict protected-tool policy can reject expired signed
  tenant assertions at execution time instead of trusting only the original HTTP
  ingress decision.
- Added regression coverage proving local/default session creation still works
  without hosted auth headers or signed context assertions.

### Coder Reliability Upgrade

Tandem Coder now behaves more like a coding supervisor than a generic prompt
runner. Issue-fix work is scheduled as real implementation work, runs in a
managed worktree, requires evidence of code changes and validation, and hands
off through a PR instead of marking project work as done prematurely.

- Issue-fix worker sessions now use a dedicated coding contract: inspect the
  repository, read scoped instructions, make a plan, patch files, run
  validation, repair failures where possible, and report concise evidence.
- Strict tool/write enforcement is applied to issue-fix workers, including
  prewrite inspection requirements before mutation. Non-issue-fix worker types
  such as triage, review, and merge recommendation keep their existing
  non-writing execution mode.
- Managed worktrees are preserved until handoff completes, allowing Tandem to
  collect and expose `git diff`, changed files, validation output, branch name,
  commit SHA, PR URL, and completion-gate evidence.
- Completion is now gated: no patch blocks completion, failed validation blocks
  completion, failed push/PR handoff blocks completion, and successful PR
  creation moves the GitHub Project item to Review rather than fake Done.
- Coder run records now include worker/session ids, worker run ids, managed
  worktree paths, branch/commit/PR metadata, changed files, validation status,
  handoff status, and completion-gate details.
- Project policy now defaults to PR-required handoff, native Tandem delegation,
  max two parallel issue runs, and no manual out-of-order runs unless the
  project explicitly opts in.
- GitHub Project intake payloads now include scheduler explanations: parent
  cards, phase, blockers, scheduler rank, runnable state, active run id, run
  state, and handoff URL.
- Parent cards are treated as planning/grouping headers only. The scheduler
  launches child issues by the lowest open phase and dependency order.

### Coder Control Panel

- The Coder intake view now renders as the primary board: TODO, In Progress,
  Blocked, Review, and Done columns with parent/phase grouping, next-runnable
  badges, disabled run buttons with reasons, and handoff links.
- `Run scheduler next` launches only the lowest open phase's scheduler-approved
  child issues. `Run selected` stays disabled for out-of-order work unless the
  project policy enables manual override.
- The board shows Tandem's spinner while GitHub Project sync is active instead
  of leaving the intake area visually blank.
- Active run payloads now carry enough status for the control panel to show the
  current worker session, latest action/log context, changed files, validation
  output, branch, PR link, and failure reason using the existing coder routes
  and event streams.

### Boundaries

- This release does not add Zitadel integration yet.
- `tandem-agents` still does not depend on Zitadel or raw IdP tokens.
- `tandem-web` is the intended owner of Tandem-signed hosted context
  assertions; runtime and ACA should consume Tandem assertions/public keyrings,
  not raw Zitadel or Google identity tokens.
- Hosted strict auth is not enabled by default.
- Local, desktop, and single-tenant workflows continue to run without hosted
  auth, signed context assertions, or approval signing keys.
- GitHub Project `Done` remains a post-review/merge state. Coder implementation
  handoff now stops at Review with a PR link and evidence.

### Versioning

- Rust crates, npm packages, Python client metadata, Tauri config, and lockfiles
  are bumped to `0.5.8`.

## v0.5.7 (2026-05-17)

Tandem 0.5.7 moves the project positioning and first domain-specific runtime hardening toward governed AI infrastructure for enterprise work. The release adds public enterprise runtime docs, a fintech strict-mode foundation for compliance and risk workflows, and the first runtime evidence paths needed to prove that Tandem can govern long-running AI work with scoped tools, citations, approvals, artifacts, audit events, and replayable run records. It also restructures the desktop Coder workspace so the live state of running work is visible at a glance.

### Enterprise Runtime Infrastructure Positioning

- The README now opens with Tandem as governed AI runtime infrastructure for long-running agentic work.
- `docs/AI_RUNTIME_INFRASTRUCTURE.md` explains the runtime model: engine-owned state, canonical run journal, task graph, tool/MCP policy, approvals, validators, artifacts, receipts, replay, and enterprise sidecar boundaries.
- `docs/ENTERPRISE_READINESS.md` separates what is available now from in-progress and planned enterprise capabilities.
- `docs/ENTERPRISE_PROOF_WALKTHROUGH.md` gives platform engineers a repo-grounded path for verifying one governed run from intent through plan, scoped tools, approval, artifact validation, audit evidence, and replay/debug.

### Fintech Strict Runtime Foundation

This release adds an internal `fintech_strict` profile marker for Automation V2 metadata. It is aimed at compliance and risk operations proof sprints, especially compliance/risk update briefs.

What ships now:

- A protected fintech action classifier in `tandem-core` for account actions, customer communications, regulatory filings, system-of-record updates, credit decisions, money movement, and evidence publication.
- Server tool-policy hook enforcement for fintech strict Automation V2 sessions.
- Protected fintech tools and unknown external mutation tools are blocked with clear denial reasons until an approval path is used.
- Denied protected fintech actions emit runtime events and protected audit records.
- `/audit/stream` maps fintech protected-action denials and verified approvals into admin-readable audit rows.
- Mission runtime projection ignores `metadata.approval.skip_approval` for fintech strict nodes, so UI/planner metadata cannot suppress injected approval gates for fintech strict work.
- Protected fintech tool denials now fail closed with explicit call-site approval/policy verifier status in the denial reason and protected audit payload.
- Automation gate decisions can now carry protected-action metadata, and fintech strict protected tools are allowed only when a matching approved receipt proves tenant, category, tool, action hash, and non-expired approval at execution time.

### Evidence, Citations, And Audit Packages

- Tool effect ledger summaries now preserve safe source identifiers such as `source_id`, `document_id`, `ticket_id`, and `record_id`, while still avoiding raw query text.
- Connector proof helpers only accept successful source retrieval calls as evidence; connector discovery/listing alone is not enough.
- Existing context-run ledger summaries now include `fintech_connector_proof` derived from successful source retrieval tool records.
- Compliance/risk brief validation checks required fields, citations, limitations, reviewer status, approval state, and audit IDs.
- Explicitly marked fintech brief workflow nodes persist connector proof and validation results in artifact validation metadata, and reject citations that cannot be mapped to connector proof.
- Workflow plans that explicitly ask for fintech compliance/risk brief artifacts now materialize with `fintech_strict` runtime metadata and artifact markers by default; generic finance workflows are left alone.
- An internal audit package helper can assemble run, tenant, actor, tool calls, connector proof, artifacts, approvals, and policy decisions from Automation V2 run state.
- The assembled fintech audit package can be persisted as a context-run artifact for compliance-review handoff.
- `eval_datasets/fintech_compliance_risk.yaml` adds proof-sprint fixtures for unsupported claim rejection, connector selected-but-unused rejection, protected-action bypass attempts, cross-tenant source denial, and incomplete evidence surfaced as limitations.
- Eval runner spec mapping now carries fintech runtime profile, tenant, and artifact-contract metadata into generated Automation V2 specs for stub/live evals.

### Coder Workspace UX

The desktop Coder panel was rebuilt so the state of running work is visible at a glance and the intake flow is no longer dominated by setup chrome. The previous panel showed task status as a tiny outlined pill in the corner of each card, hid awaiting-approval prompts inside a tab, duplicated the project picker between a stat box and a separate card, and kept the GitHub Project intake fully expanded even after a binding was saved. The redesigned layout makes "what's running, what needs me, what failed" the dominant signal and reduces the amount of scrolling needed before a coding swarm is launched or inspected.

- **Live status badges with animated indicators**: New `CoderRunStatusBadge` renders run status as colored chips — `Running` (primary spinner + pulse dot), `Queued` (primary pulse), `Needs approval` (amber + pulse), `Paused` (amber), `Failed` (red), `Cancelled` (muted), `Completed` (emerald). Used on every run card in the list and at the top of the run detail card so the running/queued/awaiting state is the first thing the eye lands on. A run's status tone now also drives the color of the progress bar (amber when paused or awaiting, red on failure, emerald on completion, primary while running).

- **Always-visible runs summary strip**: New `CoderRunsSummary` component at the top of the Runs view tallies Running / Needs approval / Paused / Failed / Completed across the workspace and shows a live "Updated Xs ago" indicator that ticks every 15 seconds (so the relative time stays fresh between sidecar event-driven refreshes). The summary surfaces totals even when individual runs scroll off-screen and emphasizes attention categories so they remain visible at a consistent spot.

- **Step progress on every card and the detail header**: New `CoderRunProgress` component draws a thin progress bar plus `completed / total` (and blocked) counts derived from each run's checkpoint node IDs. The bar appears on each list card under the status banner and at the top of the detail card next to the status badge, so a run's actual position in its workflow is visible without expanding the Context tab.

- **Elevated awaiting-gate prompts**: When a run is waiting on an operator decision, the detail card now shows the prompt title, instructions, and `Approve & continue` / `Request rework` buttons in an amber alert at the very top of the card — above the action toolbar — instead of in the Overview tab's "Gate State" panel. List cards for awaiting runs grow a matching amber "Waiting on you: …" banner so the same signal is visible in the list without selecting the run.

- **Consolidated project context header**: The Coder page header now embeds `ProjectSwitcher` directly and shows the detected git slug / current branch / default branch as a subtitle (with a short "Detecting git repo…" hint while resolution is in flight). The previous "Active Project" stat box, the standalone "Project Context" card, and the four-stat "User Repo Context" card are gone — the same information is in one place, taking ~1/4 the vertical space.

- **Tab pills with attention counts and smart default tab**: The Create / Runs tabs are now accent-pill buttons. The Runs tab shows a badge with the count of active or failed runs and switches to an amber tone when any run needs approval (red when any failed) so the operator can spot work that needs them from the Create tab. On first load, the page auto-defaults to Runs when the workspace has any active runs and stays on Create otherwise, instead of always landing on Create.

- **Collapsing GitHub Project intake**: GitHub Project binding and inbox UX moved into a dedicated `CoderGithubProjectPanel` component. When no project is bound, the connect form (Owner + Project Number + Connect) is the only thing in the card. Once bound, the configuration collapses to a single-line `Connected · owner #N` summary with `Refresh` and `Change` buttons, and status mapping (TODO / In Progress / In Review / Blocked / Done) plus saved/live schema fingerprints move behind an "Advanced" disclosure that is closed by default. Inbox items render in a tighter row layout with linked issue numbers and the primary action reads `Pull into Coder`.

- **Dev-noise sections removed**: The "First Slice" and "Compatibility" stat boxes from the original Coder header card, the "Selected preset … is UI scaffolding in this slice" copy under the Mission Builder, and the always-open `DeveloperRunViewer` ("Legacy Compatibility") at the bottom of the Runs view are all gone. The legacy inspector now lives behind a collapsed "Legacy coder inspector" disclosure so it is one click away when needed but no longer dominates the Runs view.

The Coder restructure is pure UI: no changes to the `tandem-agents` API surface, the Tauri command surface, the Automation V2 contract, the coder metadata schema, or the GitHub Project MCP tools. Saved coder templates, saved GitHub Project bindings, and the existing run detail tabs (Overview, Transcripts, Context, Artifacts, Memory) continue to work unchanged. Internally, new shared helpers (`runStatusTone`, `runIsActive`, `runProgress`, `relativeTimeFromMs`) in `coderRunUtils.ts` let the list, detail, summary, and progress components classify status through one code path.

### Boundaries

- No public HTTP API changes were added for fintech strict mode.
- This is not a production-ready regulated fintech deployment claim.
- `fintech_strict` is an internal profile marker, not mandatory isolation by itself.
- Approval gates are runtime control points, not complete authorization; regulated protected-tool execution should fail closed unless the runtime verifies matching policy/approval evidence at call time.
- Automatic protected-action approval routing, persisted fintech audit exports, OIDC, SCIM, SIEM export, SOC2, full RBAC, and private sidecar enforcement remain follow-up work.

### Versioning

- Rust crates, npm packages, Python client metadata, Tauri config, and lockfiles are bumped to `0.5.7`.

## v0.5.6 (2026-05-14)

### AI Evaluation Framework

This release ships the complete AI Evaluation Framework (Phases 1-5): a production-ready system for structured testing, regression detection, and compliance documentation of AI quality. The framework enables automated quality gates that prevent AI performance regressions from reaching production, provide quantitative proof of AI safety practices for compliance audits (e.g., EU AI Act Article 50 transparency), and make it easy for teams to experiment with new AI features while maintaining quality baseline.

**What ships now:**

- **Failure Mode Taxonomy** (`AIFailureMode` enum with 30+ variants): Formalized categorization of AI failures across validation (artifact validation, contract violations, citation missing, web sources missing), provider (timeout, rate limit, model not found, stream failures), repair (budget exhausted, loop detection, unavailable), resource (token exhausted, cost exhausted, memory exhausted), authorization (failed, invalid API key, permission denied), and system-level (config error, dependency missing, feature disabled) domains. Each failure is classified into a severity category (Critical, High, Medium, Low) for incident response prioritization. Includes deterministic classifiers (`classify_error_text()`, `categorize_failure()`, `should_retry()`) for automatically tagging failures from real error text.

- **Evaluation Dataset Format** (YAML/JSON): Structured `EvalDataset` schema with `EvalTestCase` definitions, `AutomationSpecTest` node specifications, and configurable `EvalExpectedOutput` validation criteria. Four example datasets ship in `eval_datasets/`: **critical_path.yaml** (happy-path scenarios for core features), **provider_failures.yaml** (resilience tests for timeouts and rate limits), **repair_exhaustion.yaml** (budget depletion edge cases), and **citation_validation.yaml** (web research validation). Dataset format is version-controlled and extensible for adding new test scenarios.

- **Eval Runner CLI** (`cargo run --bin eval-runner`): Standalone command-line tool for bulk test execution with the following capabilities:
  - **Metrics aggregation**: Computes pass_rate, avg_repair_iterations, total_cost_usd, avg_cost_per_test, provider_failure_rate, and per-validator-class pass rates from test results
  - **Simulation mode** (`--simulation`): Deterministic test execution without making AI provider calls, making evals safe to run in CI and cost-free during development
  - **Parallel workers** (`--num-workers`): Configurable parallelism for faster test suite runs
  - **Tag filtering** (`--filter-tag`): Run only tests matching a specific tag (e.g., `regression`, `happy_path`)
  - **CLI arguments**: `--dataset` (required), `--output`, `--provider`, `--model`, `--max-duration`, `--verbose`
  - **Output format**: Human-readable summary on stdout + structured JSON to file for programmatic CI consumption
  - **Exit codes**: 0 (all tests passed), 1 (one or more test failures), 2 (dataset load error or invalid arguments)

- **Regression Detection System**: Baseline comparison infrastructure that prevents AI quality degradation. `EvalBaseline` stores snapshots of evaluation metrics with git commit/branch metadata. `detect_regressions()` function compares current run metrics against a saved baseline using configurable thresholds (default: 5 percentage point pass_rate drop, 20% cost increase, 30% repair iteration increase, 5 percentage point provider failure increase). Results are reported as `RegressionStatus` (Pass / Warning / Regression) with human-readable messages explaining the deltas.

- **Regression Detection CI Gate** (`.github/workflows/eval-regression-gate.yml`): GitHub Actions workflow that:
  - Triggers on every PR against main/develop and on main branch pushes
  - Builds and runs `eval-runner` against critical_path.yaml dataset
  - Compares results to eval_baselines/main_branch.json baseline
  - Posts a summary comment on the PR with pass rate and test counts
  - **Fails the check** if any regression threshold is exceeded (protecting main from quality degradation)
  - **Auto-updates the baseline** on successful main branch push so future PR comparisons use the latest production metrics
  - Uploads eval results as CI artifact for debugging and audit trail

- **Developer Documentation** (`docs/dev/EVAL_FRAMEWORK.md`, ~500 lines): Comprehensive guide including:
  - Quick start with CLI examples
  - Architecture overview and data flow diagrams
  - Step-by-step guide for adding new eval datasets
  - Threshold customization patterns
  - Failure mode taxonomy reference
  - Running tests locally and in CI
  - Troubleshooting common issues (model differences, rate limits, nondeterminism)
  - FAQ covering simulation mode, parallelization, validators, costs, baseline updates

- **User & Compliance Documentation** (`docs/user/AI_QUALITY_ASSURANCE.md`, ~350 lines): Non-technical guide covering:
  - **Core quality metrics** explained for end users (pass rate, repair iterations, cost, provider reliability)
  - **Test scenarios** described (happy path, edge cases, regression tests)
  - **Quality assurance process** (automated regression detection, continuous monitoring, failure analysis)
  - **EU AI Act Article 50 compliance**: Explicit mapping of Tandem's testing practices to transparency requirements for natural persons interacting with the AI system
  - **Trust model**: What Tandem can and cannot guarantee, limitations of AI systems
  - **Incident response**: How issues are detected, investigated, and resolved
  - **Support contacts** and glossary

### Quality Gate Features

The regression-detection workflow is designed to be **low-friction and CI-friendly**:

- Default thresholds are conservative but tunable per team/metric
- Simulation mode means evaluations complete in seconds with zero API cost
- Pass/Warning/Regression statuses provide clear go/no-go signals for CI gates
- JSON output integrates with custom CI/CD tooling for advanced use cases
- PR comments make quality trends visible to the whole team

### EU AI Act Article 50 Compliance

This framework explicitly supports Article 50 transparency obligations for hosted Tandem services:

- ✅ **Documented AI system**: The framework demonstrates how AI components are systematically tested
- ✅ **Quality assurance**: Automated gates and metrics prove ongoing quality practices
- ✅ **Failure categorization**: 30+ failure types enable post-mortem root-cause analysis
- ✅ **Performance tracking**: Metrics track AI quality before, during, and after deployment
- ✅ **Regression prevention**: Automated safeguards block degradations from reaching users
- ✅ **Audit trail**: All test results are timestamped and logged for compliance audits

Users and auditors can request detailed evaluation metrics, regression reports, and failure analysis logs via support channels.

---

### Security

- **Authorization fix for approval gates**: Slack, Discord, and Telegram approval interactions now fail closed unless the acting user resolves through the configured channel allowlist before approval, rework, or cancel decisions are processed.

- **TOCTOU race condition fix in automation run cache**: Automation run state reloads now detect concurrent in-memory updates before accepting disk-loaded state, preventing stale cache loads from overwriting gate decisions or duplicating execution.

- **Path traversal protection for automation identifiers**: Automation definition and run-history paths now sanitize identifier-derived filenames and verify resolved paths stay inside their intended state roots.

- **Dedup TTL for webhook replay prevention**: Discord and Slack interaction deduplication now uses a bounded retry window, reducing stale replay risk while preserving normal platform retry handling.

- **File permission validation on startup**: Startup now warns when sensitive state files have overly broad Unix permissions so operators can tighten local storage access.

- **Discord modal identifier validation**: Discord rework modal submissions now reject malformed or incomplete identifiers before any gate decision is dispatched.

- **Telegram dedup TTL implementation**: Telegram approval callbacks now use the same retry-window deduplication model as Discord and Slack, reducing stale callback replay risk.

- **User ID extraction: reject instead of default**: Channel approval handlers now reject malformed requests without a resolvable acting user instead of assigning a placeholder identity.

- **Reason field size validation**: Discord rework feedback is now bounded server-side before being stored with gate decisions.

- **Error message information disclosure prevention**: Public channel rejection messages now use generic denial text while retaining detailed audit logs for operators.

- **JWT structure and algorithm validation**: Codex identity token parsing now validates token shape, header presence, allowed algorithm behavior, and signature encoding before processing claims.

- **JSON merge recursion depth limit**: Provider configuration merging now enforces a maximum nesting depth to avoid stack exhaustion on deeply nested input.

- **CODEX_HOME path validation**: Codex CLI home resolution now rejects unsafe or system-sensitive paths and falls back to the default home directory with a warning.

- **JWT token expiration validation**: Codex identity resolution now rejects tokens without valid expiration claims and bounds-checks expiration timestamps before time arithmetic.

- **Approval card delivery fan-out**: Slack, Discord, and Telegram channel adapters now support native interactive card sends. Approval requests can render as Block Kit messages, Discord embeds with components, or Telegram inline-keyboard messages instead of plain text fallbacks.

- **Slack approval card lifecycle updates**: The server records delivered approval message handles in `approval_message_map.json` and best-effort edits Slack approval cards after approve, rework, or cancel decisions so stale buttons disappear and operators see the final decision inline.

- **Shared automation gate state helpers**: Automation V2 gate pause and decision mutations now route through shared state helpers. The executor uses `pause_automation_run_for_gate` when an approval node blocks downstream work, and the gate-decision HTTP handler uses `apply_automation_gate_decision` for approve/rework/cancel state transitions, preserving the existing race-safe one-winner behavior.

- **Per-step approval override controls**: Workflow edit prompts now let operators keep default approval, set conditional auto-approval metadata, or skip approval for an individual step with confirmation. The saved node metadata drives the compiler's existing approval-skip hook and clears stale injected gates on skipped steps.

- **Telegram approval rework completion**: Telegram approval cards now prefer persisted opaque callback IDs so long run/node identifiers do not rely on unsafe truncation. Rework button taps send a force-reply prompt, capture the operator's next valid reply for that chat/user, and dispatch it as a `rework` gate decision with feedback.

- **Threaded approval status replies**: Slack, Discord, and Telegram adapters now share a thread-reply primitive. Approval decisions update the original card and post a short status reply into the stored native thread/topic target when one is available.

- **Channel command capability tiers**: Slash commands now carry read/act/approve/reconfigure tiers, and dispatcher execution checks the required tier against the channel security profile. Read contexts can inspect status without gaining approval or reconfiguration powers.

- **Persisted channel user capabilities**: Tandem now has `channel_user_capabilities.json` state for explicit per-channel user capability assignments. Missing users fall back to the channel profile tier until enrollment binds them to a higher tier.

- **Channel enrollment pairing codes**: `POST /channels/enroll` can issue a short-lived pairing code and confirm it out-of-band to bind a Slack, Discord, or Telegram user ID to a persisted capability tier. Approval button handlers now check the resolved user's tier and require `Approve` or higher before deciding a gate.

- **Channel outbound redaction**: Dispatcher replies now pass through a shared redaction filter before Slack, Discord, or Telegram sends. The filter replaces common secret patterns, private-key markers, JWTs, and absolute paths outside the workspace root while preserving markdown structure; deployments can add regexes with `TANDEM_CHANNEL_REDACTION_PATTERNS_FILE`.

- **Per-user channel rate limiting**: Tandem now applies per-user token buckets to channel-origin prompts and approval decisions. Prompts default to 10/minute, decisions default to 30/minute, limits are keyed by `(channel, user_id)`, profile-specific env overrides are supported, and rejected requests return `429 Too Many Requests` with `Retry-After`.

- **Workspace pinning for channel sessions**: Channel sessions now carry a pinned workspace boundary. New channel-created sessions pin to the server workspace, enrollment records can preserve an explicit `pinned_workspace_id`, and file tools are denied with `ToolDenied { reason: WorkspaceScope }` if a channel session tries to read or write outside the pinned workspace.

- **Streaming audit export**: `GET /audit/stream` now exposes an admin-gated newline-delimited JSON feed for external SIEM-style consumers. The stream normalizes approval decisions, tool execution ledger records, and channel capability changes into records with actor, command, workspace, tool call, result, timestamp, and channel fields where available.

- **Step-up confirmation for channel reconfiguration**: Reconfigure-tier slash commands now require a fresh second-surface confirmation before execution. The dispatcher blocks `/providers`, `/model`, `/schedule`, `/automations`, and `/config` with a "step-up required" response unless the chat message carries a desktop-issued PIN from the last 5 minutes. The PIN token is removed before command parsing so it is not treated as a model id, schedule prompt, or config argument.

- **Dispatcher baseline cleanup**: Channel dispatcher tests now match the registry-driven help output and concrete operator tool allowlist behavior, keeping the approval-channel test suite aligned with the current dispatcher contract.

## v0.5.5 (2026-05-13)

This release lays down the **Execution Profiles** foundation — a runtime governance toggle (Strict / Guided / YOLO) that will let users keep working while validators and contracts continue to harden, without abandoning Tandem's runtime ownership of state, receipts, replay, spend tracking, and approvals. The motivation is operational: full governance still has a high run-fail rate as bugs are ironed out, and a meaningful share of those failures are over-strict (false-positive validation, missing-but-non-essential sections, recoverable artifact issues) rather than real defects. Execution Profiles are the structured bridge that lets affected runs continue with the relaxation captured in receipts, so the data we collect can drive validator classes back to Strict-by-default once they mature.

The v0.5.5 cut is **backend telemetry-only**. Strict, Guided, and YOLO runs all produce identical run outcomes today; the only difference is in receipts. This is intentional. The status-downgrade behavior change (where Guided actually warns instead of blocking, and YOLO actually continues as experimental) is gated on the next slice, which can calibrate against the validator-class telemetry collected here. No existing automation changes behavior in this release.

What ships now:

- **Type foundation** (`automation_v2::execution_profile`): `ExecutionProfile` enum (`strict`/`guided`/`yolo`), `ValidatorClass` taxonomy with `is_relaxable_in(profile)` and a conservative `is_critical()` allowlist for never-relaxable classes (auth, secret access, destructive-action approval, budget caps, kill switch, deterministic verifier failures). `decide_profile_validation` is the single chokepoint; `augment_output_with_profile_relaxation` is the executor-facing helper; `classify_unmet_requirement` maps existing validator strings to the taxonomy.
- **Run record and API**: `AutomationExecutionPolicy.profile` is now optional and persisted. Every `AutomationV2RunRecord` carries typed `effective_execution_profile` and `requested_execution_profile`. `POST /automations/v2/{id}/run_now` accepts an optional `execution_profile` override (Strict, Guided, or YOLO) that applies for the single run only without mutating the saved automation. `resolve_effective_execution_profile` enforces a deterministic precedence: run override → workflow policy → Strict.
- **Lifecycle and event observability**: `record_automation_lifecycle_event_with_metadata` automatically merges the run's `effective_execution_profile` into every `AutomationLifecycleRecord` so existing audit, replay, and Bug Monitor surfaces see the profile without per-call-site changes. The `automation_v2.run.failed` engine event now includes both `effective_execution_profile` and `requested_execution_profile`, so Bug Monitor and downstream observers can attribute failures to the active profile.
- **Executor chokepoint (telemetry-only)**: The executor invokes `augment_output_with_profile_relaxation` at the single run-acceptance moment. When every `unmet_requirement` on a node output is relaxable under the active profile, it writes `relaxed_validator_classes` (structured), `effective_outcome`, `original_validator_outcome`, `execution_profile`, and `experimental: true` (YOLO) into the `artifact_validation` block. Strict runs are unchanged. Critical classes (destructive-action approval, budget cap, etc.) always block; if any classification is unknown, the augmentation conservatively skips so behavior stays Strict-equivalent.
- **24 unit tests** covering serde round-trip, default-to-Strict, critical-class blocking, soft-class relaxation per profile, tenant-denylist enforcement, classifier mapping, augmentation purity, and lifecycle metadata merge semantics.

What is intentionally deferred to follow-up slices and tracked in `docs/internal/execution-profiles/KANBAN.md`:

- Phase 4b: status-downgrade behavior change so Guided actually warns and YOLO actually continues as experimental, gated on telemetry calibration.
- Phase 5: wiring the existing `effective_repair_budget` multiplier (1.0 / 1.5 / 2.0 by profile) into the repair-decision call sites.
- Phase 6: control-panel UI (profile selector, run pill, experimental badge).
- Phase 7: Tauri desktop UI (matching control panel).
- Experimental-input propagation rule for downstream nodes.
- Tenant-level relaxation denylist and default-profile administration.

This patch keeps automation-owned runtime sessions out of the user Chat session list without hiding their audit trail from the rest of Tandem.

Sessions now carry explicit source metadata. New interactive sessions default to `sourceKind: chat`, Automation V2/Bug Monitor worker sessions are classified as `automation_v2`, and session listing supports filtering by source. The TypeScript client and wire model expose the same fields so control-panel views can ask for the session class they actually need.

The Chat sidebar and Dashboard recent-session list now request only `source=chat`, so Bug Monitor submissions such as `Automation automation-v2-bug-monitor-triage-failure-draft-... / inspect_failure_report` no longer appear as conversations. Legacy automation records with the existing title format are classified at the storage/wire boundary, preserving backward compatibility for already-written sessions.

The Tauri desktop Automation Calendar no longer crashes the app while loading. FullCalendar is now isolated into its own lazy bundle and imported only after the WebKit stylesheet host is ready, preventing the `Cannot read properties of null (reading 'cssRules')` startup failure seen when opening the calendar view.

Bug Monitor GitHub issue creation now uses a persisted pending idempotency claim before calling GitHub. Completion finalization, stale-provider recovery, deadline recovery, and status-sweep recovery can all wake up around the same draft, but only the first caller that claims the create-issue digest is allowed to create the GitHub issue. Concurrent callers now see `publish_in_progress` or reuse the posted record instead of producing duplicate issues with the same fingerprint and triage run.

Bug Monitor proposal quality gates also recognize the structured handoff shapes that triage nodes actually return, including wrapped objects such as `{ "bug_monitor_inspection": ... }` and array responses containing the artifact followed by a compact status object. Placeholder task specs still fail the gate, but valid completed inspection, research, validation, and fix-proposal artifacts no longer get treated as missing and replaced with broad fallback evidence.

Bug Monitor triage status detection now treats nested `status: blocked` fields inside structured Bug Monitor handoffs as evidence/limitation data, not as the node's own runtime status. This prevents `propose_fix_and_verification` from recursively blocking the debugger when it has produced a useful partial fix proposal with acceptance criteria and bounded next steps.

Automation V2 long-running nodes now get to own their timeout path. The stale-run reaper honors the run-registry heartbeat that active node execution already emits every few seconds, so a first task with a 600-second budget is not globally paused as `stale_no_provider_activity` at the exact timeout boundary before the node can fail or repair normally.

Automation V2 research validation now preserves source URLs from successful `websearch` and `webfetch` tool results. If a generated JSON artifact is too sparse and omits raw links, the validator can still see the current web evidence that was actually gathered instead of blocking the node as `citations_missing`. The prompt and repair guidance also now explicitly tell research agents to include raw URLs in `citations` or `web_sources_reviewed` fields.

Connector-backed source research now has to use the selected connector, not merely discover it. A node that says to use Reddit MCP and resolves `reddit-gmail` can no longer complete after only `mcp_list` plus a JSON write; it must call a concrete source tool such as `mcp.reddit_gmail.reddit_search_across_subreddits` or `mcp.reddit_gmail.reddit_retrieve_reddit_post`, preserving real returned evidence or an actual connector/tool limitation.

The prompt and tool surface now reinforce that rule before validation has to catch it. Connector source prompts list concrete `mcp.*` tools and state that `mcp_list`, `glob`, `grep`, `edit`, and `apply_patch` are not source evidence, while non-code connector source nodes no longer offer edit/patch/bash tools that can distract agents from calling the connector.

Connector-backed delivery nodes now keep their destination MCP tools focused all the way through artifact creation. Notion save/report nodes with explicit `mcp.notion.*` tool allowlists no longer inherit generic workspace `read`/`glob` or mutation tools from upstream input refs, but they still retain the required `write` tool for the run artifact receipt. The engine loop also narrows prewrite MCP gating to the specific concrete connector tools that have not yet run, steering a Notion publisher from `notion_fetch` to `notion_create_pages` instead of letting it loop on already-completed discovery or local inspection.

Required-tool provider calls now fail closed inside Tandem instead of being rejected by the provider when routing filters remove every tool. Write-required connector nodes keep the artifact `write` tool even when their session allowlist is connector-only, and if a later filter still produces an empty tool set Tandem downgrades the provider request away from `tool_choice: required` rather than sending an invalid no-tools request.

Transient provider stream decode failures are now treated as recoverable provider infrastructure failures. Stream errors such as `error decoding response body`, unexpected EOF, and incomplete streamed responses are retried inside the current provider iteration with partial streamed text/tool-call state cleared before retry. The retry budget is bounded by `TANDEM_PROVIDER_STREAM_DECODE_RETRY_ATTEMPTS`, and each retry emits a `provider.call.iteration.retry` event for debugging.

Automation V2 governance now gives repair attempts a calmer, more actionable handoff. Attempt verdicts include a `calm_teammate_v1` review with a progress score, what the agent completed correctly, what is still needed, why the missing work matters, and the next concrete moves. Repair prompts show that review before the raw expected/observed contract JSON, so retries can keep good evidence and fix the smallest missing piece rather than restarting from a vague validation failure.

Bug Monitor failure reports now preserve both the final failure and the useful prior attempt evidence. Automation V2 failure events carry recent attempt verdict chains and attempt review chains into Bug Monitor submissions, making issue details show earlier contract misses such as missing workspace files, missing connector calls, citation gaps, or required next actions even when the final observed failure is a provider stream/runtime error.

Stale provider/session recovery now retries by default instead of stopping at a pause. When the stale reaper cancels a dead session, the in-progress node is marked `needs_repair` and the stale-reaped run is automatically requeued while attempt budget remains. The existing auto-resume cap keeps truly wedged providers from looping forever, and operators can opt out with `TANDEM_DISABLE_STALE_AUTO_RESUME`.

The control panel also avoids presenting active workflow sessions as stalled. A running Automation V2 run with active sessions stays visually `running`, and background-tab polling gaps are shown as a softer "waiting on active session" detail. The backend stale reaper remains the authority for real `stale_no_provider_activity` pauses.

The control-panel Chat view now waits for the completed assistant message to materialize in the exact active session before clearing the live thinking/streaming state. This closes the blank-response gap where an answer was saved on the server and appeared after refresh, but the live UI had already removed `Thinking...` without rendering the final assistant message.

Hosted Files now distinguishes workspace-root configuration from workspace-files API availability. The Files page only enables workspace browsing when capabilities explicitly advertise the API route, so managed-file deployments no longer spam `/api/workspace/files/list?dir=` 404s.

Chat also preflights active-run cleanup before sending a new prompt. If a stale session run is still registered, the UI cancels and waits for idle before posting `prompt_async`, with the 409 conflict payload still used as a fallback if a race appears between the preflight and send.

The Coder board now matches ACA's updated GitHub Project intake rules for launchable work. `Todo` and `TODOS` lanes are recognized as runnable in the control panel, and planned GitHub tasks are moved into the detected launch lane rather than assuming the project has a `Ready` status. This fixes projects where the coding agent should accept cards from `TODOS` but the board UI left them looking unlaunchable or published new tasks into the wrong lane.

Workflow tasks now have first-class per-node tool access. Automation V2 nodes can carry their own `tool_policy` and `mcp_policy`, and the runtime treats those policies as a hard session scope rather than a hint layered on top of broader workflow access. This is especially important for approval-gated Gmail draft workflows: the compose and draft-create steps can be scoped away from send tools, while the post-approval step can be scoped to the concrete send-draft MCP tool that should run only after approval.

The control panel exposes this in both Workflow Studio and the existing automation edit dialog. Each node has a default-collapsed Task tool access panel with clear inherit/custom markers, MCP server/tool selectors, and a send-capable marker so operators can quickly spot which task is allowed to send. Saving a workflow preserves node-level built-in tool allowlists/denylists plus exact MCP server/tool choices.

The runtime also understands node MCP policy when computing concrete MCP allowlists and connector discovery behavior. Explicit node policies, including empty custom policies, are treated as intentional constraints. A regression test covers the Gmail approval case by allowing `mcp.reddit_gmail.gmail_send_draft` on the post-approval node while filtering out `gmail_create_email_draft` and `gmail_send_email`.

Channel-level MCP server toggles are now enforced as a hard runtime boundary. If an MCP server is disabled for a channel or conversation scope, agents do not receive tools from that connection, even when stale exact-tool preferences or a route-level allowlist still mention those tools. Exact MCP tool selections only apply while their owning server is enabled; selecting exact tools now narrows access rather than layering on top of a server wildcard. Channel defaults also avoid a broad `*` tool allowlist so MCP access must be explicitly granted.

The channel settings UI now mirrors that model. Disabling an MCP server clears exact-tool selections for that server on save, exact-tool pickers are visibly inactive until the server is enabled, and the summary counts only active exact MCP tools. Telegram, Discord, and Slack settings also expose a `Strict KB grounding` toggle so operators can intentionally opt a channel into factual-question KB grounding without confusing that behavior with MCP tool access.

## v0.5.4 (Released 2026-05-05)

This patch fixes automation schedule timezone handling, tightens the distinction between local source-code research and final research synthesis, and introduces marketplace-ready workflow pack import/export.

Automation cron schedules now preserve the selected local wall-clock time end to end. The server accepts the 5-field cron expressions emitted by the control panel, normalizes them for the Rust cron parser, and evaluates them in the saved IANA timezone when computing `next_fire_at_ms`. The control panel now carries that timezone through guided schedule summaries, creation review, workflow editing, calendar labels, and standup scheduling, with `Europe/Budapest` available in the common timezone picker. A regression test covers weekday 9:00 AM in Budapest resolving correctly through DST-aware UTC storage.

Final report/brief nodes that synthesize already-collected Tandem MCP notes, Reddit MCP signals, web findings, and run artifacts no longer require fresh workspace `read` calls. The planner stops adding `local_source_reads` to new `research_synthesis` contracts, and the runtime validator waives stale local-read enforcement on existing saved synthesis nodes. Code-change, local-research, and Bug Monitor source-inspection nodes still retain their strict repo-read gates.

This prevents research-to-destination workflows from blocking with messages such as `research brief cited workspace sources without using read` when the workflow only cites MCP/web/upstream artifact evidence and does not need repository source files.

Workflow packs are now the preferred portable format for created workflows. The Workflows page can upload a `.zip` pack, preview its manifest, cover image, workflow entries, capabilities, and validation results, then install it and open the resulting planner session. Raw JSON workflow bundle import remains available under Advanced for debugging and internal handoffs.

Planner sessions can also be exported as marketplace-ready workflow pack ZIPs containing `tandempack.yaml`, `README.md`, the embedded workflow plan bundle, and an optional PNG/JPEG/WebP cover image. New workflow-pack APIs and TypeScript client helpers support export, preview, and import, while imported sessions keep pack provenance (`source_pack_id`, version, and source bundle digest) for later inspection.

Exported workflow packs now include a hosted-safe download URL, and the Workflows page shows a browser Download ZIP action after export so operators can retrieve generated packs without access to the server filesystem path. Control-panel uploads also now prefer `$TANDEM_HOME/data/channel_uploads` and expand home-directory placeholders such as `~`, `$HOME`, `${HOME}`, and `%HOME%`, avoiding stray literal upload directories when hosted or Windows-style environment values are used on Linux/macOS.

## v0.5.3 (Released 2026-05-03)

Automation V2 workflow definitions now use per-workflow storage shards. Instead of rewriting every saved workflow into one large `automations_v2.json` file, Tandem writes each definition to `data/automations-v2/<automation-id>.json` and keeps a small `index.json` alongside the shards. On startup, existing aggregate installs are migrated automatically and the old aggregate is preserved as `automations_v2.legacy-aggregate.json` for rollback/debugging.
