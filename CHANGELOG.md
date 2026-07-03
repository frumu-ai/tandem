# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.5] - 2026-07-03

### Added

- Added enterprise-aware stateful runtime kernel types, adapters, and JSONL
  persistence helpers for durable run events and snapshots.
- Added the `tandem-data-boundary` crate with serializable secure data-boundary
  contract types for policy, provider boundary class, sensitive data class,
  input metadata, findings, decisions, and audit-safe events.
- Added deterministic secure data-boundary detectors for email, phone-like,
  credit-card/Luhn, credential, bearer/API key, private-key, AWS key,
  high-entropy, SSN-like, and simple PHI-marker spans, plus safe redaction and
  tokenization placeholder helpers that avoid raw-value persistence.
- Added stateful runtime definition identity helpers so snapshot-backed
  automation runs expose durable workflow definition versions and `sha256:`
  snapshot hashes for future replay and resume checks.
- Added a shared retry policy schema and decision record for Automation V2
  nodes, preserving legacy `max_attempts` behavior while recording retry
  failure class, attempt, terminal behavior, and next retry timing for future
  durable queue/outbox paths.
- Added persisted Automation V2 execution claim metadata with claim ids,
  claimant ids, lease expiry, and claim epochs so resumed runs can distinguish
  the active executor from abandoned launch claims.
- Added tenant-scoped idempotency key persistence for long-running automation
  operations, keyed by org/workspace/deployment plus operation and request
  fingerprint so retries, duplicate deliveries, and conflicts survive restarts.
- Added Automation V2 webhook dedupe metadata on delivery records and SDK
  models, including idempotency record references, dedupe result/reason codes,
  and original delivery/run correlation for duplicate provider events.
- Added an explicit stateful workflow phase model with guarded transitions,
  phase transition event records, status compatibility mapping, and serialized
  allowed-next-phase exposure on durable runtime records.
- Added Automation V2 webhook raw event inbox persistence with tenant-scoped raw
  payload pointers, body/header digests, redacted header previews, delivery/run
  correlation, and duplicate-event coverage.
- Added a provider-aware Automation V2 webhook signature verifier registry with
  Tandem HMAC, GitHub SHA-256, shared-secret header, and unsigned-dev schemes
  plus queryable delivery verification metadata exposed through clients.
- Added stateful runtime durable wait records and a tenant-filtered wait store
  with tenant-boundary wait identity, due timer selection, missed-wakeup
  recovery queries, timeout policy metadata, and idempotent wake claiming
  foundations.
- Added a durable stateful wait scheduler tick for timer wakeups and wait
  timeouts, including lease-backed claims, idempotent run events, snapshots,
  timeout cancellation/escalation statuses, and scheduler lag metrics.
- Added live Automation V2 durable-wait bridging so approval gates register
  stateful approval waits, complete those waits when the gate settles, and
  timer/webhook wait wakes requeue the authoritative automation run for resume.
- Added tenant-filtered stateful runtime event and snapshot read endpoints for
  replay and control-panel consumers.
- Added canonical stateful runtime run list/detail read endpoints with
  tenant/workspace/status/phase filters, current wait, latest event, latest
  snapshot, and replay-boundary metadata for control-panel consumers.
- Added enterprise scope summaries and filters to canonical stateful runtime run
  list/detail responses, including organization-unit, owner, resource, policy,
  data-class, delegation, and knowledge-source visibility.
- Added enterprise-aware scope metrics, filters, and per-run scope cards to the
  Control Panel stateful runs dashboard.
- Added Automation V2 lifecycle projection into the authoritative stateful
  runtime event log, with idempotent per-run lifecycle event IDs, monotonic
  sequences, summary snapshots, checkpoint digests, and definition version/hash
  metadata at durable execution boundaries.
- Added first-class Automation V2 run definition version/hash fields with
  snapshot-derived load-time backfill and SDK typings for replay/resume
  consumers.
- Added Linear issue destinations for Incident Monitor routing,
  including Linear MCP readiness checks, issue creation, duplicate matching,
  destination-aware receipts, and external-action mirrors.
- Added signed webhook destinations for Incident Monitor routing,
  including env-backed HMAC signing secrets, SSRF-safe URL validation, bounded
  payloads, capped retries, and durable destination-specific receipts.
- Added local telemetry and internal memory destinations for Incident Monitor
  routing, including durable destination-aware post
  receipts, destination-id filtering, duplicate suppression, bounded redacted
  memory summaries, and category-specific memory refs.
- Added generic MCP tool destinations for Incident Monitor routing,
  gated by explicit `allow_publish` configuration and payload mappings, with
  route-preview readiness checks, duplicate suppression, redacted receipts, and
  failed-call post records.
- Added AI-agent safety and risk context fields for Incident Monitor drafts and
  incidents, including redacted actor/model/tool/action
  metadata, policy and approval state, risk category, blast radius, external
  correlation ids, SDK exposure, destination payload support, and route-preview
  matching by risk category.
- Added Incident Monitor deployment-card generation for production authority
  governance, with read-only JSON/Markdown exports from authority inventory,
  operator metadata overlays, required-field posture findings, self-monitoring
  boundaries, evidence refs, SDK helpers, and redaction coverage.
- Added Incident Monitor source data-readiness gates for owner/system-of-record
  metadata, classification/allowed-use, tenant and workspace boundaries,
  lineage, freshness, schema drift, quality notes, authorization markers, and
  redaction/retention coverage. Status, route preview, authority inventory,
  posture checks, assessment reports, deployment cards, and SDK types now
  surface sanitized source-readiness findings without embedding raw source data
  or credentials.
- Fixed monitored-source deployment cards so source posture findings link through
  source/project identifiers as well as canonical source refs.
- Added an Incident Monitor setup surface in Settings for sources,
  destinations, routing, safety defaults, route preview, destination readiness,
  destination-filtered posts, and SDK destination/route helpers.
- Added Incident Monitor security-readiness audit coverage for redacted
  destination/route config mutations, scoped intake-key lifecycle changes, and
  destination-router publish attempts/outcomes, with adversarial regression
  tests for scoped intake keys trying to call privileged routes.
- Added a read-only Incident Monitor authority inventory endpoint and SDK
  helpers for security posture assessment, covering workflows, automations,
  agents, tool/MCP policy, destinations, routes, monitored sources, approvals,
  policy decisions, and external publish surfaces with sensitive values
  summarized or omitted.
- Added read-only Incident Monitor security posture checks and SDK helpers that
  run deterministic baseline rules over the authority inventory and selected
  decision/action history, producing deduped findings with severity, evidence
  refs, mitigation guidance, routing suggestions, and normal Incident Monitor draft
  conversion payloads. Checks default to dry-run and support enabled/disabled
  rule policy modes.
- Added dry-run Incident Monitor security assessment probes for approval-gated
  tool policy, high-risk route approval, fail-closed destination readiness,
  scoped intake restrictions, MCP tool allowlists, and webhook URL policy.
  Probe runs require full API-token/admin context, reject scoped intake keys,
  persist evidence packs as context-run artifacts, emit protected admin audit
  events, and expose TypeScript/Python SDK helpers.
- Added Incident Monitor security gap assessment reports with JSON and Markdown
  output, redacted evidence-pack persistence, posture findings, controlled
  probe results, incident and receipt summaries, Tandem self-monitoring
  boundaries, protected audit export summaries, non-mutating destination route
  previews, and TypeScript/Python SDK helpers.
- Added Incident Monitor AI Agent Security Posture positioning docs with
  buyer-facing packaging, demo narrative, report outline, comparison guidance,
  and explicit boundaries against broad vulnerability-scanner or SIEM claims.
- Added an Incident Monitor terminology regression guard for public UI, SDK,
  docs, examples, scripts, and CI surfaces with a narrow compatibility
  allowlist.
- Added an agent-facing Incident Monitor runtime guide for MCP-connected agents,
  SDK users, and public docs consumers, including auth boundaries, safe
  route-preview/triage/publish flow, governance evidence, and failure handling.
- Added Incident Monitor production governance docs that map deployment cards,
  authority inventory, posture checks, controlled probes, assessment reports,
  route receipts, protected audit evidence, and customer-owned policy decisions
  without overclaiming compliance certification.
- Added Incident Monitor adversarial scenario packs: a versioned, read-only
  pack of production-mirroring abuse scenarios run in dry-run against live
  routing/approval/readiness logic, with per-scenario pass/fail evidence, a
  scenario-pack endpoint, and assessment-report integration.
- Added Incident Monitor governance maturity metrics with operator-tunable
  thresholds and behavioral drift detection over redacted approval,
  incident-response, recurrence, and receipt-integrity signals, exposed via a
  dedicated endpoint and the assessment report.
- Added an Incident Monitor continuous reassessment scheduler with
  change-triggered reviews, versioned previous/current comparisons, stable
  finding fingerprints for duplicate suppression, and per-scope
  next-due/last-completed/overdue schedule status on deployment cards.
- Added native Notion webhook support for Automation V2: canonical `notion`
  provider, `notion_hmac_sha256` signature scheme, verification-token capture
  from the subscription handshake with a one-time authorized reveal endpoint,
  and a Control Panel setup flow.
- Added workflow phase-guard and MCP tool authority enforcement at the runtime
  boundary, plus a pre-send outbox gate for outbound tool dispatch.

### Changed

- Running Automation V2 runs interrupted by a server restart are now queued for
  resume when their persisted checkpoint can be safely rehydrated; in-progress
  nodes receive repair markers, while corrupt records still fail closed.
- Restart recovery now fails closed when a run's recorded definition snapshot
  hash does not match the available snapshot/current definition, preventing
  unsafe resume against mutated workflow definitions.
- Automation V2 supervisors now reclaim expired execution claims that have no
  active session or agent handles, requeueing those runs for a single safe
  claimant instead of leaving them stuck as `Running`.
- Automation V2 webhook queueing now reserves idempotency records before
  creating runs, treats same provider event IDs with different payloads as
  conflicts, and treats same body digests as duplicates without crossing tenant
  boundaries.
- Control Panel settings now label the router setup as Incident Monitor and
  use the canonical Incident Monitor endpoints and config payloads.
- Server routes now expose canonical Incident Monitor endpoints under
  `/incident-monitor/*` and `/config/incident-monitor`, with stale legacy aliases
  removed.
- Renamed shared Incident Monitor contracts to the `tandem-incident-monitor`
  crate, including canonical runtime event names, evidence refs, persisted data
  paths, GitHub host trait methods, and the eval fixture CLI.
- TypeScript and Python SDKs now expose canonical Incident Monitor namespaces,
  types, endpoints, and examples through `client.incidentMonitor` and
  `client.incident_monitor`, including `/incident-monitor/*` routes and
  `incident_monitor` config payloads.
- Control Panel, desktop settings, create-panel templates, docs, examples,
  scripts, and CI workflow labels now use Incident Monitor routes, filenames,
  labels, and examples, with pre-rename route redirects removed.
- Incident Monitor scoped intake keys now default to the
  `incident_monitor:report` scope and `tim_intake_` key prefix, with canonical
  `x-tandem-incident-monitor-intake-key` header support.
- Incident Monitor security docs now call out the default secret-redaction and
  retention posture for reports, receipts, and protected audit evidence.
- Incident Monitor setup, source, reference, and compliance docs now distinguish
  shipped evidence/export surfaces from deployer-owned retention, escalation,
  incident-response, and turnkey SIEM integration responsibilities.
- Incident Monitor destination publishing is now fail-closed by default:
  `safety_defaults.block_unready_destinations` defaults to true, and automated
  and manual publishes always block destinations that are not publish-ready
  regardless of the flag; Recovery mode with the flag disabled remains the
  deliberate operator escape hatch.
- Automation V2 webhook intake now processes deliveries through a durable
  asynchronous inbox: intake persists the raw event and responds, and a worker
  verifies, dedupes, and queues runs from the inbox.
- Notion webhook triggers reject Tandem secret rotation because the signing
  secret is Notion's provider-owned verification token.
- The protected audit ledger now appends with fsync durability at O(1) cost.

### Fixed

- Destination-specific GitHub MCP servers are no longer gated on the global
  GitHub capability flags during Incident Monitor destination readiness checks,
  so a destination with its own connected server publishes correctly under the
  fail-closed gate.
- Fixed stateful wait reminders and scheduler clock regressions so timer
  wakeups stay accurate across restarts.
- Trimmed Incident Monitor rename changes under the CI touched-file-size guard,
  including compacting UI rename formatting and moving server service tests into
  a dedicated module.
- Persisted Automation V2 webhook dedupe outcomes so provider retries after a
  server restart can return the original delivery/run correlation instead of
  creating a second run.
- Legacy stateful runtime snapshots that predate explicit phase fields now
  derive phase, phase history, and allowed next phases from their stored status
  when read.
- Durable wait scheduler and webhook wake completions now reserve only the
  active leased claim before durable wake writes, then terminalize the wait with
  the locked event sequence after those writes finish so duplicate timer/webhook
  wakes cannot race into conflicting per-run sequence numbers.
- Stateful runtime wait, reliability, snapshot, and event-log writes now fsync
  durable files, repair torn JSONL event-log tails before appending, and fail
  closed by moving corrupt wait/reliability stores aside instead of silently
  overwriting them.
- Enterprise policy inheritance now treats ancestor deny/approval rules as a
  non-overridable floor unless explicitly marked overridable, and scope-ID
  matching now trims and case-folds tenant, org-unit, workflow, and phase IDs so
  casing drift cannot silently bypass deny rules or hide stateful runtime
  org-unit summaries and active grants.
- Runtime policy decision recording now resolves through the enterprise policy
  inheritance resolver, loading `enterprise/policy_rules.json`, evaluating every
  recorded data class, enforcing the resolved result in gate and authority
  helpers, fintech protected-action receipts, MCP preflight checks, and memory
  promotion checks, and preserving inherited decision sources for replay instead
  of only writing single-source fallback snapshots.
- Knowledge-scope governance now requires registered source-bound scope for
  workflow-phase memory retrieval and blocks source-bound memory writes or
  promotions that omit explicit `knowledge_scope_registry` metadata or provide
  a registry for a different source resource, source binding, or data class.
- Source-bound manual memory imports now stamp imported chunks with matching
  `knowledge_scope_registry` metadata so workflow-phase reads can authorize
  imported source chunks through the registered source resource instead of
  hiding them as unregistered source-bound memory.
- Source-bound memory promotion checks now allow validated source-binding
  metadata to use the actual bound source resource in `knowledge_scope_registry`
  while keeping the stricter `source_binding` resource requirement for
  authority-only scope claims.
- Backend CI now runs the touched-file size gate before dependency installation
  and Rust lint/build work so oversized files fail fast instead of burning the
  full backend job first.
- Incident Monitor publish and recheck failures now return the full error chain in
  API response details so destination adapter failures expose the underlying
  MCP/provider cause.
- Linear issue destination duplicate handling now preserves matched-issue draft
  status and suppresses repeat `create_issue` attempts after an ambiguous
  failed Linear create response.
- Signed webhook destination validation now classifies parsed IPv4/IPv6 URL
  literals before DNS lookup so IPv4-mapped private IPv6 hosts fail closed
  consistently across platforms.
- TypeScript and Python Incident Monitor SDK helpers now remove routes that
  would otherwise be left with no explicit destinations after deleting a
  destination, preventing accidental fallback to default destinations.

## [0.6.4] - 2026-06-28

### Added

- Added tenant-scoped Automation V2 webhook trigger and delivery records,
  including secure secret references, private secret material storage, HMAC
  signature verification, replay detection, rotation primitives, and tests for
  cross-tenant isolation.
- Added authenticated Automation V2 webhook management endpoints for listing,
  creating, reading, updating, disabling, deleting, rotating secrets, and
  inspecting sanitized delivery history within the owning tenant and automation.
- Added a Control Panel webhook management section to the `Edit workflow
automation` modal with callback URL copy, one-time secret reveal on
  create/rotate, trigger status badges, and sanitized recent delivery rows that
  link to queued Automation V2 runs.
- Added a public Automation V2 webhook intake route with a dedicated auth bypass
  for signed trigger calls, sanitized rejection delivery records, and
  tenant-scoped validation before any run is queued.
- Added webhook-triggered Automation V2 run queueing with delivery provenance,
  duplicate suppression, untrusted sanitized event previews, trigger data-class
  and risk defaults, and `automation.v2.run.created` events using
  `triggerType: "webhook"`.
- Added a Studio-inspired workflow flow map to the `Edit workflow automation`
  modal so generated automations can be inspected by dependency stage while
  operators edit them.
- Added node-level visual context for upstream dependencies, input references,
  output kind, bound agent, workflow-level MCP inheritance, task MCP overrides,
  send-capable MCP tools, and missing dependency warnings.

### Changed

- Linked flow-map node selection to the existing prompt/model/MCP editor cards,
  including selected-node highlighting and automatic scroll-to-editor behavior.
- Preserved dependency, input-reference, stage, and output-contract metadata in
  workflow edit drafts so generated workflow structure remains visible after an
  automation is opened for editing.
- Opened the prompt editor by default in the workflow automation edit modal so
  generic workflow prompts are immediately reachable from the visual map.

### Fixed

- Persisted Automation V2 webhook delivery idempotency markers before queueing
  runs so provider retries cannot create duplicate runs if delivery linking
  fails after run creation.

## [0.6.3] - 2026-06-26

### Added

- Added destination-neutral Incident Monitor routing foundations behind the
  existing Incident Monitor API surface, including destination readiness, route
  preview APIs, destination-aware idempotency, and TypeScript/Python SDK models.
- Added a centralized Incident Monitor destination router for manual, automatic,
  approval, recovery, timeout, and service publish paths while preserving the
  legacy GitHub adapter as the only executable destination in this phase.
- Added monitored-source route bindings for source kind, route tag, destination
  allow/default policy, tenant/workspace scope, approval policy, redaction, and
  retention metadata. Log watchers, scoped intake, drafts, incidents, route
  preview, publish validation, SDKs, Control Panel display, and regression
  fixtures now carry those bindings consistently.
- Added runtime guardrails and regression coverage for connector-action
  workflows, including concrete source coverage, artifact write completion, MCP
  progress gating, exact-tool prompting, and blocked-run cleanup.

### Changed

- Updated MCP Settings in the control panel so OAuth-required or disconnected
  servers are surfaced first, large tool allowlists default collapsed with an
  animated expand control, and built-in MCP catalog actions use clearer add
  wording.
- Preserved full Automation V2 agent and flow configuration when saving from
  the summary editor, and moved save payload construction into a dedicated
  helper to keep the automations container below the CI file-size limit.
- Compacted scoped MCP inventory output so large connected MCP servers remain
  usable in the control panel and workflow editor.

### Fixed

- Fixed Automation V2 execution-error retries so transient provider request
  failures and missing required-output artifacts get a minimum repair budget
  before terminal failure, preventing low-retry workflow nodes from failing
  branches after one flaky provider or write attempt.
- Fixed MCP-enabled Automation V2 workflows that lost node execution
  configuration after editing, which could leave connector runs blocked before
  any useful work executed.
- Fixed exact MCP-tool automations so prompts no longer require an initial
  `mcp_list` discovery call when concrete connector tools are already bound.
- Fixed workflow artifact/source validation so MCP tool ids are not treated as
  workspace source files, upstream artifact paths satisfy concrete source
  coverage, and Notion page/database writes are classified as outbound
  connector actions.
- Fixed artifact-write nodes so they complete only after a productive write to
  the declared artifact target, avoiding false progress on connector workflows.
- Fixed MCP OAuth persistence and reconnect cleanup so stale OAuth material is
  removed when auth is deleted, canonical credentials can be reused correctly,
  and MCP public base URL handling is shared by the HTTP helpers.
- Fixed connector-backed workflow data collection so large remote tool results
  are materialized into run artifacts before model filtering, preventing
  completed Composio/Notion automations from starving writer nodes with
  truncated preview data or empty row sets.
- Fixed generic connector-row filtering and Notion writer handoff paths so
  compiler-built workflows can preserve full source rows, validate duplicate
  keys, and write/update the intended database without workflow-specific
  prompt patches.
- Fixed Incident Monitor source-bound routing so configured source allowlists,
  approval policy, high-risk raw-source defaults, granted approvals, and raw
  routing-field sanitization are enforced before publish.

## [0.6.2] - 2026-06-23

### Added

- Added the enterprise MCP identity and delegation design for principal-scoped
  MCP connections, tenant/actor-bound OAuth ownership, run-as policy, local
  compatibility migration, and audit evidence needed before implementing
  multi-employee MCP account separation.
- Added first runtime data structures for enterprise MCP server definitions and
  principal-scoped connection records, including legacy local compatibility
  connection backfill and V2 MCP registry state that can represent server
  definitions without account credentials.
- Added tenant-aware MCP connect, refresh, and readiness entry points so
  enterprise tool execution reconnects with the same tenant/actor context used
  for dispatch instead of falling back to the legacy local-implicit connection
  path. OAuth token refresh now uses tenant-scoped credential helpers when an
  explicit tenant context is present.
- Added actor-qualified MCP secret ids and exact tenant-context secret
  resolution so two employees in the same workspace cannot overwrite or resolve
  each other's stored MCP bearer credentials.
- Added tenant/actor-scoped MCP OAuth sessions, callback completion, provider
  credential ids, and audit events so enterprise OAuth sign-ins bind tokens to
  the initiating MCP connection instead of a shared server-global account.
- Added connection-scoped MCP runtime readiness state so explicit tenant
  refresh, discovery, pending auth, session ids, and authenticated tool caches
  live on the acting MCP connection while local mode keeps legacy server-row
  compatibility.
- Added Automation V2 MCP connection grants and MCP bridge run-as enforcement
  so workflows can model user-owned and service-principal MCP connections,
  reject cross-actor and actor-supplied service-principal selection before
  upstream dispatch, and record the actual acting MCP principal/connection in
  protected audit events.
- Added enterprise MCP isolation regression coverage for cross-tenant
  connection-id denial, sanitized OAuth callback mismatch audit records, and
  tenant-tagged MCP connect/discovery runtime events.
- Added redacted MCP connection summaries to the control-plane inventory so
  enterprise tenants can see actor-owned, shared, service-principal, and
  admin-managed connections without exposing credential references, secret
  headers, OAuth client secrets, or another actor's pending sign-in state.
- Added control-panel MCP provider/connection separation, including visible
  connection ownership/status/scope, account-scoped connect/refresh copy, and
  explicit Workflow Studio acting-connection selectors for agent and task MCP
  policies.
- Added shell sandbox security documentation and regression coverage for Linux
  bubblewrap argv/write-boundary behavior, POSIX fail-closed guardrails, and
  Windows shell command translation/rejection policy.
- Added a new `tandem-automation` crate for Automation V2 model types,
  execution-profile policy helpers, MCP run-as policy records, routine misfire
  policy, scheduler queue metadata, and shared-context metadata parsing, with
  server compatibility re-exports and ported unit coverage.
- Added a new `tandem-eval` crate for the eval-runner CLI, dataset/metrics
  types, regression detection, scripted provider, eval bootstrap, and Bug
  Monitor fixture scaffolding so evaluation harness code no longer lives in
  the production server crate.
- Added a strict workflow action validation path with a host-extensible action
  registry, built-in action parameter schemas, structured source/step/field
  diagnostics, and MCP/tool catalog-backed schema checks for workflow steps.
- Added a governed tool dispatcher that is now the compile-enforced execution
  path outside `tandem-tools`, carrying tenant context, scope allowlists,
  policy decisions, and one dispatch ledger event for engine, workflow, HTTP,
  automation preflight, planner, and CLI tool calls.
- Added schema-versioned persistence envelopes for session history and
  Automation V2 run stores/history shards, including explicit v0-to-v1 upgrade
  paths, future-version refusal, compatibility fixtures, and a memory DB
  `schema_migrations` ledger with idempotency coverage.
- Added Automation V2 restart/reload golden coverage for queued,
  awaiting-approval, blocked, and running runs, including duplicate approval
  decision protection and consequential-node no-replay assertions after server
  restart recovery.
- Added Automation V2 approval failure-injection coverage for concurrent
  approve requests, provider failure after approval, half-applied gate decisions
  across restart, stale gate decisions, and corrupted run-checkpoint
  quarantine.
- Added Automation V2 approval gate expiry policies with default and per-gate
  deadlines, auto-cancel, reminder, escalation, protected audit events,
  notification redispatch keys, auto-cancel late-decision rejection, and
  approvals inbox deadline display.
- Added durable PermissionManager state for interactive ask requests,
  provenance-bearing standing rules, restart-failed pending prompts, and
  decision history suitable for unified approval rendering.
- Added dogfooding regression fixtures, an Incident Monitor incident-to-fixture
  scaffold CLI, and a nightly stub-mode eval-runner workflow so manually
  discovered workflow/runtime bugs become permanent replay coverage. Eval-runner
  stub/live modes now use Tokio's multithreaded runtime for in-process
  Automation V2 evals.
- Added Rust supply-chain and coverage CI: cargo-audit, cargo-deny, nightly
  governance-critical llvm-cov reports, and initial coverage baselines for
  `tandem-tools`, `tandem-plan-compiler`, and `tandem-automation`.
- Documented the initial cargo-deny license exception baseline, including
  owner, reason, and expiry metadata for scoped BUSL and third-party dependency
  exceptions.
- Added tenant-tagged observability exports with an authenticated,
  config-gated Prometheus endpoint, bounded runtime/scheduler/tool/provider
  metrics, and optional feature-gated scrubbed Sentry error export.
- Added a seeded approval-gated email demo under `examples/email-approval-demo`
  with a local HTTP MCP stub, `just demo` entry point, approval/rework evidence
  artifacts, and a nightly non-interactive CI lane that exercises draft, gate,
  ledger, and outbox behavior without real credentials.

### Changed

- Began the AppState domain-manager decomposition by moving provider and MCP
  OAuth callback session maps behind a dedicated OAuth state manager with an
  explicit lock-order boundary.
- Began TAN-205 crate-boundary cleanup by relocating the provider auth
  credential store implementation into `tandem-providers` while keeping
  `tandem-core` compatibility re-exports for existing callers.
- Continued TAN-205 crate-boundary cleanup by moving Incident Monitor domain types,
  log parsing/evidence rendering, recurrence summaries, error provenance, and
  GitHub publish logic into a new `tandem-incident-monitor` crate. `tandem-server`
  keeps compatibility re-exports plus the AppState/HTTP/MCP host shim, including
  uncapped duplicate/failure-suppression lookups for GitHub posting.
- Replaced divergent tool-name normalization in parser, registry resolution,
  and approval classification with a shared `tandem-types` canonicalizer, plus
  a structured function-style invocation scanner and 30+ case parser corpus.

### Fixed

- Fixed durable runtime event logging so canonical run/session events use an
  opt-in bounded persister queue instead of the live broadcast stream, avoiding
  event loss after persister registration without retaining events in eval-only
  buses that never start the persister.
- Fixed Incident Monitor log watchers with `start_position = end` so a missing-file
  poll no longer disables the first successful seek-to-EOF when the log appears
  later with bootstrap or historical lines.
- Fixed tenant/actor-scoped MCP OAuth completion so pending sign-in polls return
  the initiating session's authorization URL and callback token storage updates
  the scoped connection instead of the shared server row.
- Fixed non-HTTP in-memory MCP reconnects so seeded runtime tool inventories are
  preserved after startup resets, keeping test and local compatibility GitHub
  MCP tools available.
- Fixed Automation V2 MCP preflight discovery to use tenant-aware readiness,
  connection-grant run-as context, tool sync, and remote tool inventory so
  scheduled and enterprise runs do not fall back to local-implicit or workflow
  creator MCP state while preparing scoped connector tools.
- Fixed filesystem-scoped Automation V2 code workflows so MCP-only explicit
  tool allowlists still retain `apply_patch`, matching Git-backed code workflow
  behavior and allowing the regression test to leave the CI quarantine list.
- Fixed Automation V2 task retry/requeue so resetting a node subtree preserves
  existing attempt counters and the next executor pass records the next attempt;
  moved the regression to route-level coverage and removed the stale quarantine.
- Fixed `tandem-engine storage cleanup` to preserve schema-versioned Automation
  V2 run indexes and shards instead of treating the new envelope format as an
  empty legacy map.
- Fixed schema-versioned Automation V2 run-shard serialization to avoid an
  extra full-run clone, and moved stack-heavy coder issue-fix regression tests
  onto a high-stack harness so nextest can exercise the new persistence path.
- Fixed Automation V2 approval gates so an approve/cancel decision already
  recorded in gate history wins over stale pending-gate state after restart,
  while rework decisions can still re-arm the gate for another review cycle.
- Fixed Automation V2 approval recovery so approving a gate clears stale
  gate-local failure/block markers, and malformed individual run checkpoints in
  an otherwise readable run store now load as blocked diagnostic records instead
  of crashing scheduler startup.
- Fixed session-level permission decisions so generic permission replies write
  protected audit evidence with actor, request, decision, and standing-rule
  provenance.
- Fixed durable PermissionManager replies so stale persisted request IDs cannot
  be replayed after restart, and serialized state-file writes to avoid losing
  concurrent permission asks or decisions.
- Fixed the runtime event log persister startup order so it waits for runtime
  readiness before dereferencing runtime-backed state, avoiding a background
  task panic during fresh engine startup.

## [0.6.1] - 2026-06-20

### Fixed

- Fixed Automation V2 wrapper-action nodes that intentionally do not produce a
  workflow artifact. Nodes can now opt out of synthesized default artifact paths
  with `metadata.disable_default_output_path`, `builder.disable_default_output_path`,
  or `builder.output_path_mode = "none"`, preventing MCP wrapper actions such as
  Composio Gmail draft creation from being forced into an unrelated workspace
  write requirement.
- Fixed the approvals inbox path for Automation V2 runs whose list row is stale
  or skeletal while the full run record is awaiting a gate. Pending approval
  gates are now resolved from the authoritative run detail before the unified
  approvals endpoint filters them, and the control-panel inbox orders mixed
  approval sources newest-first.
- Fixed sharded Automation V2 run-state hydration for approval gates so runs
  split across state shards still surface pending gate metadata in the unified
  approvals inbox.
- Fixed Automation V2 run/library recovery for legacy context-run state. The
  server now scans `automation-v2-*` context run directories, reconstructs run
  records and automation snapshots from `run_state.json`, merges newer recovered
  records into history, and persists them so interrupted or older runs remain
  visible in history, detail, and the automation library.
- Fixed transient ACA disconnects in the control-panel Coder surface during
  slow task/board refreshes. ACA probe timeout smoothing now keeps Coder
  available for a longer grace period after a known-good probe, and configured
  ACA probes can remain available when the Tandem engine itself is healthy.
- Fixed engine session-list endpoints that could time out after large ACA runs.
  Session list/status responses now use lightweight session summaries without
  cloning every stored message transcript, while direct session detail APIs keep
  returning the full conversation history.

### Changed

- Bumped Tandem workspace, desktop, npm, and Python package manifests to
  `0.6.1`.
- Updated the version bump script to include the meta-harness crate and desktop
  Tauri lockfile.
- Added control-panel export/import support for Automation V2 JSON specs so
  operators can download an automation from the edit dialog and restore it from
  the creation wizard.
- Expanded the MCP automated-agents guide with current Composio Connect and
  scoped MCP server setup guidance, including generated URL, `x-api-key`, and
  REST-only setup notes.

## [0.6.0] - 2026-06-17

### Added

- Added a dedicated desktop Settings > Providers tab so LLM provider setup is
  no longer buried among general settings. The provider panel now includes
  Codex account sign-in/import controls, reconnect/disconnect actions,
  default-model selection, and expanded built-in provider coverage for OpenAI
  Codex, Groq, Mistral, Together, Cohere, Azure, Bedrock, Vertex, GitHub
  Copilot-compatible, and existing local/OpenRouter providers.

- Added EU AI Act operational evidence surfaces: deployment-scope tracking,
  Article 50 transparency badges/labels, hash-chained audit ledgers, SIEM
  export guidance, protected-action/approval completeness checks, and
  provenance-preserving export labels for generated artifacts.

- Added repo-intelligence and workflow/context graph capabilities, including
  manifest and fact extraction, persistent store/query APIs, context-bundle and
  failure-causality queries, GraphRAG retrieval improvements, quality
  regressions, metrics/debug export, and graph-backed planning/rerun/impact
  analysis queries.

- Added broader CI and runtime assurance coverage: full workspace tests through
  `cargo-nextest`, an end-to-end `tandem-engine` smoke-test CLI, startup config
  validation, structured HTTP error codes, persisted runtime observability
  events, prompt-injection exfiltration/blast-radius evals, and expanded
  workflow registry coverage.

- Made the per-PR evaluation regression gate fail closed when `eval-runner`
  cannot build or execute (TAN-219). The `critical_path`,
  `tenant_isolation`, and `action_firewall` datasets now run the built
  `eval-runner` binary directly with no hardcoded stub-result fallback and no
  `continue-on-error`; the PR comment reports missing results explicitly
  instead of fabricating pass rates.

- Added per-role sampling parameters (`temperature`, `top_p`, `max_tokens`) to
  the engine runtime and the `tandem-client` Python SDK (bumped to `0.6.0`).
  Callers can set a session-level default on `sessions.create(...)` and override
  it per prompt on `prompt_async(...)`; the per-prompt value wins field by field.
  Values are mapped per provider (OpenAI-compatible, OpenAI Responses, Anthropic)
  and clamped to each provider's supported range; models that reject an explicit
  `temperature` (OpenAI reasoning families) drop it with a logged warning instead
  of failing the run. All fields are optional and omitting them leaves the
  provider request byte-identical to prior behavior.

- Wired a cross-tenant isolation evaluation dataset into the per-PR regression
  gate, with must-block scenarios for cross-tenant source/secret access and
  cross-tenant memory reads (CT-01). Multiple datasets now run in the gate
  alongside `critical_path` rather than overloading it.
- Bootstrapped the local eval-runner stub mode with a real in-process
  `AppState`, so cross-tenant evals exercise real tenant-scoped enforcement
  instead of simulated output shapes (CT-16).
- Added a real-engine tenant tool-execution isolation eval proving that an
  automation running as tenant A cannot read a tenant-B-only resource through
  the runtime tool path (CT-02).
- Added cross-tenant audit-visibility negative coverage for the `/audit/stream`
  read path, plus an eval case asserting tenant B cannot read tenant A's audit
  events (CT-04).
- Added a tenant-scoped memory promotion eval, ensuring untrusted memory cannot
  be promoted across tenant boundaries (CT-03).
- Added channel tenant routing isolation, so cross-tenant interactions via
  Discord, Slack, and Telegram must fail closed at the channel interaction
  audit layer (CT-05).
- Added a cross-tenant knowledge retrieval eval and skills isolation coverage,
  proving tenant B cannot retrieve tenant A's knowledge-base items and that
  project skills remain scoped to the executing workspace root (CT-08).
- Added memory poisoning trust gates: memory now carries trust labels with a
  promotion gate, and untrusted search results, channel reads, and prompt
  context are framed as trust-scoped evidence. Includes a memory-poisoning
  eval dataset.
- Added a source-verified Rust runtime security analysis across command
  execution, HTTP APIs, secrets/crypto, permissions/governance, and external
  integrations, with remediation findings tied to source locations.
- Added an intra-tenant authority graph (CT-18) that resolves a principal's
  effective grants from direct grants plus organization-unit memberships —
  honoring role-domain nesting and parent-department inheritance — and renders
  fail-closed access decisions (allow only on a matching allow grant; deny
  wins; no grant denies). Server enforcement records every decision as a policy
  decision and writes a tenant-attributed protected audit event on denial.
  Ships with seeded engineering/finance/sales/HR/executive/support personas.
- Added a declarative approval gate matrix (CT-20) that maps an action's risk
  tier and data class to a gate outcome (allow / deny / approval-required), the
  reviewer eligibility the approval demands, and the approval TTL. External
  customer-facing sends pause for approval by default; restricted, credential,
  financial, executive, and regulated data classes require an elevated
  reviewer; and unclassified actions fail closed to an elevated-reviewer
  approval. Server enforcement records every gate decision as a policy decision
  and writes a tenant-attributed protected audit event for approval-required
  and deny outcomes. The runtime tool-policy hook resolves high-risk tools
  (external sends, financial/credential access, destructive deletes, money
  movement) through the gate and pauses them, enforced under strict runtime
  auth modes so local/single-tenant deployments stay a no-op.
- Added a first-class runtime policy decision store and
  `GET /governance/policy-decisions`, with tenant/run filtering, context-run
  journal events, tool-effect ledger links, and fintech protected-action
  decision records for allow, deny, and approval-required outcomes (CT-17).
- Added a shared tool risk-tier taxonomy with descriptor- and heuristic-based
  inference, plus canonical risk/default gate hints in policy payloads and
  stored policy decisions (CT-19).
- Added governed MCP tool registry metadata to MCP inventory output and
  per-tool security rows, including redacted credential binding, tenant
  binding, owner, resource scope, risk tier, default access/policy, and
  explanatory reasons.
- Added Goal Capability Learning (GCL) as an additive demo slice: goal and
  capability schemas, fail-closed strategy proposal lifecycle, tenant-scoped
  discovery decisions, REST discovery/read/list endpoints, audit event names,
  and documentation for the "read and parse a CSV file" demo goal.
- Added a Coding Workflows Cockpit tab for selected ACA runs, showing source
  identity, run state, GitHub PR/merge state, repository context, summaries,
  and an operational thread with run actions.
- Added a control-panel ACA feedback coordination API with run/thread-scoped
  file-backed audit storage, `/operator/feedback` delivery, pending-message
  replay, and cockpit SSE updates.
- Added a first slice of Goal Capability Learning (GCL-01/02/03): the front end
  for composing a new workflow toward a goal, distinct from Workflow Learning's
  repair of existing workflows. A `GoalSpec` is decomposed into tool-agnostic
  `CapabilityRequirement`s, resolved to available capabilities, and assembled
  into a ranked `CompositionPath` (demonstrated on the demo goal "read and parse
  a CSV file"). A `StrategyCandidate` carries a fail-closed review lifecycle
  (`Proposed → Approved → Applied`, with `Rejected`/`Superseded` terminals) and,
  once approved, materializes into a `WorkflowProposalDraft` that links into the
  existing planner plan-draft and Automation V2 preview surfaces; goal-planning
  and strategy/proposal review emit namespaced audit events. Discovery decisions
  are persisted per tenant and exposed through tenant-scoped HTTP endpoints.
- Decided and enforced the Workflow Learning v1 production-validation and
  auto-apply policy (GCL-04). A single declarative
  `WorkflowLearningPromotionPolicy` now governs whether a proposed learning
  candidate may be auto-applied (`AutoApply` / `RequireHumanReview` / `Block`)
  and whether an applied candidate has regressed against its baseline
  (`Insufficient` / `Healthy` / `Regressed`). Auto-apply is off by default and
  fails closed to human review; structural graph patches and plan-bundle changes
  are categorically blocked from auto-apply; and the previously-inlined
  before/after regression check is centralized behind the policy with identical
  default thresholds. Configurable via `TANDEM_WORKFLOW_LEARNING_*` env knobs.
- Added Action Firewall regression coverage and a demo preset, exercising
  governed action decisions before protected tool execution.
- Added a tenant-scoped protected audit ledger and governance evidence export
  surfaces so protected decisions can be traced, exported, and tied back to
  run context.
- Added cross-tenant grant contracts, server routes, and positive sharing evals
  for explicitly governed tenant-to-tenant access, while keeping ordinary
  tenant isolation fail-closed.
- Added default data-boundary and cross-tenant grants design docs to describe
  how governed reads and explicit sharing should compose.
- Added engine context assembly mapping, context-budget telemetry, Full-context
  guardrails, provenance-aware compaction, per-source prompt hook budgets, and
  long-session context evals with provenance assertions.
- Added memory crypto mode diagnostics and ciphertext-at-rest support for
  encryptable memory payloads, with a public residual-risk document for
  search-required plaintext.
- Added an egress DLP preflight for agent-team outbound actions.
- Added the `tandem-meta-harness-eval` crate with trace/scoring models, finite
  score deserialization, and design docs for optimizer loops, candidate
  scoring/promotion, and human approval surfaces.

### Changed

- Desktop provider selection now persists an explicit selected model only when
  the provider is authenticated or local/keyless, and chat session creation
  falls back to the enabled/default provider slot when no selected model has
  been stored.
- The engine now honors the managed `OPENCODE_CONFIG` path supplied by the
  desktop launcher, avoiding split-brain behavior between the desktop provider
  UI and the sidecar engine's provider registry.
- Development builds reduce Rust debug-info output at the workspace root to
  keep local Tauri/engine builds from exhausting disk space.

- Tagged `fintech.protected_action` and `tool.effect.recorded` audit events
  with their originating tenant context so consequential actions are
  attributable on the tenant-scoped audit stream.
- Scoped provider live catalog discovery to the request tenant's persisted
  provider auth, prevented explicit hosted tenants from inheriting shared
  runtime provider credentials, and tenant-scoped provider throttles so one
  tenant's backoff cannot queue another tenant's runs.
- Scoped automation spend/quota guardrail keys and pause checks by tenant for
  explicit tenants while preserving local/single-tenant raw agent behavior.
- Tenant-scoped governance approval receipts and listing: approval requests now
  carry the issuing tenant, cross-tenant approval/denial attempts fail closed
  without leaking receipt existence, and approval audit events use the real
  tenant context (CT-09).
- Fixed Linear MCP approval classification so Linear read tools are not caught
  by the broader write gate, added Linear read/write coverage and built-in
  bindings, and exposed connector readiness state for missing/read-only/write
  Linear capability states.
- Hardened ACA engine session follow-up handling and stabilized ACA capability
  probing/status reporting.
- Split large prompt execution and memory database modules into smaller
  implementation parts while preserving existing runtime behavior.

### Fixed

- Fixed OpenAI Codex OAuth sign-in from the desktop app by calling the engine's
  provider OAuth authorize endpoint with the expected HTTP method, surfacing
  OAuth errors in the provider card, and supporting local Codex session import.
- Fixed Codex OAuth disconnect/reconnect in desktop settings by adding a local
  credential-store cleanup fallback when the sidecar OAuth-session delete call
  cannot complete.
- Fixed desktop chat session creation after connecting Codex OAuth by resolving
  provider/model configuration from enabled/default provider slots instead of
  requiring a separately populated model-picker value.
- Fixed opaque `500 Internal Server Error` responses during session creation:
  persistence failures now return structured error details, the engine logs the
  failing session save, and Windows temp-file sync/replace handling is retried
  to avoid `Access is denied` failures from transient filesystem interference.

### Security

- Tightened enterprise/runtime hardening with tandem-server panic-surface
  guards, async runtime hygiene checks, tandem-tools path sandbox regression
  tests, governed strict memory-read enforcement, actor-bound memory subjects,
  provider ACL sync classification, public-demo channel security gates, and
  explicit connector OAuth/control-plane ownership decisions.

- Hardened hosted/enterprise tenant-context assertions: signed Ed25519 JWS
  assertions now enforce key metadata (purpose, status, lifetime, audience,
  organization/deployment, and resource-scope prefixes) and a configurable
  replay policy (`bound` by default, `one_shot`, or `off`). Added
  `docs/CONTEXT_ASSERTION_SECURITY.md` to document key configuration, replay
  behavior, and recommended assertion lifetimes.
- Added strict tenant-scope guards for external-effect built-in tools and MCP
  dispatch. In strict modes, local-implicit contexts are denied before web,
  memory, shell/network, or MCP calls can dispatch, store-backed MCP secret
  headers must belong to the executing tenant/deployment, and built-in tool
  alias/path resolution is pinned with traversal, absolute-path, wildcard, and
  symlink-escape coverage.

- Added a CI guard (EAA-11) proving the public engine build (`tandem-ai`)
  excludes enterprise-only and heavyweight crates (`tandem-enterprise-server`,
  `tandem-governance-engine`, `fastembed`, `ort-sys`). It checks the exact
  feature sets the public artifacts ship with — default features and the
  `browser` feature used by the release/desktop/engine builds — fails closed,
  and prints the offending dependency path so enterprise/governance code cannot
  silently leak into public builds.
- Scoped the Goal Capability Learning endpoints to the authenticated tenant:
  the discover/list/get handlers derive the tenant from the request's
  `TenantContext` instead of a caller-supplied `tenant_id`, and reading a
  discovery decision owned by another tenant fails closed as not-found, so an
  authenticated client cannot enumerate or read another tenant's discovery
  history.
- Enforced a verified human decider on Automations V2 gate decisions, closing a
  gate-decision self-approval path (GOV-B1).
- Enforced governed approval reviewer eligibility for explicit approval gates:
  non-human decisions are rejected and audited, self-approval is blocked,
  reviewer authority is verified against approval metadata, and data-class or
  resource grants are required when the gate demands them.
- Tenant-scoped the audit read path (`/audit/stream`): it now fails closed for
  explicit tenants and recognizes both nested `tenantContext` and flat tenant
  tags, while remaining a no-op for local/single-tenant deployments.
- Blocked explicit tenants from using store-backed MCP secret headers owned by
  another tenant before OAuth refresh or outbound MCP calls, and returned
  tenant-scope denials for cross-tenant MCP secret attempts.
- Added a memory retrieval gateway that governs channel reads, so memory pulled
  into channel responses passes tenant and source scoping before egress.
- Added retrieval egress controls (TAN-102) restricting which retrieved memory
  and knowledge can leave through session knowledge-base grounding and export
  paths.
- Added a scoped memory decrypt broker that brokers per-scope data-encryption-key
  unwrap through tickets (including the wrapped DEK) instead of exposing keys
  broadly.
- Added key-scope metadata to memory envelopes, binding encrypted memory to a
  specific key scope.
- Added a memory key lifecycle evidence gate, enforced by a CI verification
  check.
- Added a memory database blast-radius boundary check, enforced in CI, to bound
  the impact of a memory-store compromise.
- Rechecked sensitive-path protections in read basename fallback paths.
- Added shared SSRF URL/IP validation across web fetch and browser navigation,
  and tightened standing shell approval guidance to avoid overly broad
  approvals.
- Isolated memory authority jobs so memory maintenance work runs under explicit
  authority rather than inheriting ambient session power.
- Governed memory promotion outcomes so promotion decisions are policy-visible
  instead of implicit side effects.

## [0.5.13] - 2026-06-02

### Added

- Added Linear task-source registration to the Coder control panel, including
  team/project filters, launch statuses, label filters, query filters, and
  Linear MCP connection status.

### Changed

- Generalized the Coder intake board so GitHub Project and Linear issue sources
  share the same preview, scheduler launch, batch run, and active-run controls.
- Hardened local engine HTTP API defaults by refusing unauthenticated
  non-loopback binds and preventing token-clearing from reopening the API.
- Tightened local runtime tenancy so caller-supplied tenant headers are ignored
  in local single-tenant mode unless a hosted/enterprise signed context is used.
- Completed the Automations V2 reliability remediation workstream. Run
  completion now requires contract-aware deliverable checks, current-run
  evidence for publish targets, successful receipts for governed external
  actions, and consistent terminal checkpoint accounting.
- Strengthened Automations V2 workflow planning and repair behavior, including
  cycle/input-ref validation, timer-trigger deduplication, verification retry
  routing, exact-enough verification command matching, recoverable tool-error
  handling, and repair requeueing for missing or weak deliverables.
- Added explicit parked-state lifecycle handling for Automations V2: approval
  gates now surface visible stale manual status, guardrail-stopped runs can
  auto-resume after approved overrides, node execution uses idle/no-progress
  timeouts with absolute ceilings, and stale auto-resume limits are
  configurable.
- Normalized warning and budget policy: `accepted_with_warnings` no longer
  produces positive workflow-learning evidence, and `.env.example` now matches
  the enforced tool-guard budget default while documenting the email cap.

### Security

- Blocked HTTP registration of arbitrary `stdio:` MCP transports.
- Changed default file write, edit, and patch permissions to ask instead of
  silently allowing workspace mutation.
- Added batch sub-call permission checks so nested tool calls cannot skip the
  normal approval and sandbox gates.
- Made workspace and write-policy checks fail closed when no workspace root can
  be resolved.
- Added Linux shell execution confinement through `bubblewrap` by default, with
  an explicit unsafe opt-out for unsandboxed shell execution.
- Hardened automation auto-approval so empty allowlists deny by default and
  shell tools are not auto-approved.
- Hardened secret storage by writing API tokens, vault keys, and TUI keystores
  with owner-only permissions on Unix, and replaced the fixed 4-digit vault PIN
  with longer passphrases.
- Closed browser and provider SSRF edge cases by failing closed on empty browser
  allowlists, blocking local/private browser targets, and validating provider
  base URLs.
- Enforced tenant checks on run event streams, audit streams, project listing,
  and local-mode tenant context resolution.
- Redacted provider credentials in debug output and improved incident-monitor log
  redaction for repeated secrets.

## [0.5.12] - 2026-05-27

### Added

- Added hosted single-tenant context assertion claims for org units,
  capabilities, and policy version so Tandem-hosted panel sessions can carry
  customer org policy into the runtime without exposing the root engine token.
- Added hosted automation ownership metadata for automation v2 resources.
  New hosted automations are private to their creator by default, while
  owners/admins can share resources with org units/groups or the whole hosted
  org.
- Added `POST /automations/v2/{id}/share` to update hosted automation
  visibility and audience metadata under runtime owner/admin enforcement.

### Changed

- Updated the hosted control panel session model and `/api/auth/me` response to
  include hosted org units, effective capabilities, and policy version.
- Replaced the hosted panel proxy's broad role checks with capability-aware
  route checks for automation reads, execution, writes, and sharing.
- Extended hosted runtime request verification so automation v2 list/read/run
  surfaces honor private, group, and org visibility derived from the signed
  Tandem assertion.

### Fixed

- Fixed hosted automation mutation paths so hosted users cannot edit, pause,
  resume, delete, recover, or repair another user's private automation unless
  they are the owner or have hosted admin authority.
- Preserved channel approval compatibility for Slack, Discord, and Telegram
  automation gate decisions after adding hosted assertion-aware route handling.

## [0.5.11] - 2026-05-25

### Added

- Added a hosted-only Linux x64 enterprise engine distribution path. Release
  builds now produce `tandem-engine-enterprise-linux-x64.tar.gz` with browser
  automation and enterprise-full routes compiled into `tandem-engine`.
- Added the public `@frumu/tandem-enterprise` npm wrapper package for hosted
  Linux deployments. It installs the enterprise release asset while exposing
  the same `tandem-engine` command used by existing sidecar scripts.

### Changed

- Refactored the npm engine binary installer into reusable artifact-resolution
  logic so the standard and enterprise engine wrappers can share download,
  extraction, version-check, and platform-validation behavior.
- Updated release/version/publish automation to know about the new enterprise
  package and release asset, while keeping the enterprise npm package publish
  gated behind `PUBLISH_NPM_ENTERPRISE=true` for the first package publish.

### Fixed

- Kept automatic npm registry publishing from failing on the first release that
  contains `@frumu/tandem-enterprise` by skipping that package unless it is
  explicitly enabled.

## [0.5.10] - Released - 2026-05-25

### Added

- **Enterprise connector source-binding contract foundation**: Added the first
  transport-safe enterprise contract vocabulary for connector instances,
  secret-reference-only connector credentials, source bindings, source objects,
  ingestion jobs, ingestion quarantine, and scoped memory chunk references.
  This starts the 0.5.10 connector-ingestion governance track without enabling
  live external connector ingestion.
- **Generic company taxonomy contract foundation**: Added additive enterprise
  contract vocabulary for admin-defined organization units and memberships so
  companies can model HR, Doctors, Consultants, Claims Adjusters, Board Members,
  or other custom domains without Tandem hardcoding role names.
- **Enterprise admin placeholder endpoints**: Added noop enterprise admin
  endpoints for organization units and source bindings that thread verified
  request tenant/principal context without claiming persistence or live
  connector ingestion.
- **Enterprise admin UI shell**: Added a hidden-by-default control-panel
  Enterprise route that reads the noop organization-unit and source-binding
  endpoints, surfaces tenant/principal context, and shows connector governance
  lanes without implying live persistence or ingestion.
- **Enterprise organization-unit registry**: Added the first storage-backed
  organization-unit registry for enterprise admin routes, including
  tenant-scoped create/list behavior and signed hosted assertion role
  preservation so hosted mutations can distinguish admin/owner/reconfigure
  authority from ordinary members.
- **Enterprise source-binding registry**: Added storage-backed source-binding
  create/list/update behavior with admin-gated mutations, request-tenant
  isolation, and `ResourceRef` tenant validation. This records which external
  source root may feed which Tandem resource/data class without enabling live
  OAuth or ingestion.
- **Enterprise admin UI management**: Wired the hidden Enterprise admin page to
  the storage-backed org-unit and source-binding routes with typed create
  forms, readable governance rows, and source-binding enabled/disabled/
  quarantined controls.
- **Enterprise manual memory import source binding**: Added optional
  `source_binding_id` support to manual memory imports, validates that the
  binding belongs to the request tenant and allows indexing before import,
  stamps imported chunks with source-binding/resource/data-class/source-object
  metadata plus matching knowledge-scope registry metadata, and keeps
  local/default manual imports unchanged.
- **Enterprise source-bound memory retrieval guard**: Added memory access
  filtering for source-bound chunks so bound enterprise memory is hidden by
  default and can only participate in vector ranking when an explicit strict
  tenant projection grants `Read` on the bound `ResourceRef` and `DataClass`.
- **Enterprise governed-memory source-binding guard**: Extended the same
  resource/data-class enforcement to governed global memory search so records
  carrying source-binding metadata are hidden unless the signed strict tenant
  projection grants `Read` on that bound resource and data class.
- **Enterprise response-cache source-binding partitioning**: Added tenant and
  source-binding scope metadata to the response cache, scoped cache-key helpers,
  and source-binding invalidation APIs. Source-binding admin create/update now
  emits an explicit cache-invalidation-required event for revoke, quarantine,
  permission, or policy changes.
- **Enterprise tool security descriptors**: Added additive `ToolSchema`
  security descriptors that record required permissions, resource kinds, data
  classes, admin surfaces, external side effects, credential access, and default
  visibility. Built-in tool metadata now emits these descriptors and the core
  tool capability classifier can derive conservative descriptors for
  unannotated provider/MCP tools.
- **MCP catalog security metadata**: The embedded MCP catalog now exposes
  server and per-tool security metadata, honors explicit catalog
  `tool_security_overrides`, and derives conservative descriptors from catalog
  server context plus tool action classification when no override is present.
- **Operator MCP tool-security overrides**: Added a JSON/YAML override format
  via `TANDEM_MCP_TOOL_SECURITY_OVERRIDES_PATH` so hosted/self-hosted operators
  can override server and per-tool MCP security descriptors without editing the
  embedded catalog.
- **MCP discovery authorization filtering**: `mcp_list` now carries
  per-tool security metadata in inventory snapshots and redacts unauthorized
  tool names when a signed strict tenant projection is present, while preserving
  legacy/local unscoped discovery behavior.
- **Provider tool-schema authorization filtering**: Provider/model invocations
  now filter advertised tool schemas through the signed strict tenant
  projection before the model call. Unauthorized admin, credential, execute, or
  resource-scoped tools are omitted from the provider-visible tool list, while
  legacy/local unscoped sessions preserve their existing behavior.
- **Enterprise source-object lifecycle records**: Added source-bound uploaded
  document lifecycle records in memory storage so manual imports can track
  active and tombstoned source objects by tenant, binding, resource, data class,
  and native object identity. Reimporting changed content preserves the stable
  source object ID while updating hashes, and `sync_deletes` tombstones removed
  source-bound uploads for future reindex/delete/re-scope workflows.
- **Enterprise source-object lifecycle admin actions**: Added admin-gated
  source-object lifecycle endpoints under source bindings to list tracked
  uploaded objects, request reindex by purging stale chunk/index rows, hard
  delete a source object and its indexed content, and re-scope lifecycle
  resource/data-class metadata while invalidating source-binding cache scope.
- **Enterprise source-object lifecycle UI**: Wired the hidden Enterprise admin
  control-panel page to inspect source-object lifecycle records for a selected
  source binding and trigger reindex, delete, or re-scope actions from the
  tenant-scoped admin surface.
- **Hosted manual import source-binding enforcement**: Hosted/enterprise memory
  imports now fail closed unless a valid `source_binding_id` is supplied, while
  local/default imports can remain explicitly unbound. The control-panel import
  dialog also requires a source binding when opened from a hosted principal.
- **Local manual source-binding projection**: Local/default manual memory
  imports can opt into a generated `local_manual_upload` binding that stamps
  source-object lifecycle records with an internal `document_collection`
  resource scope, while leaving the empty/unbound legacy import path available.
- **Enterprise connector trust-proof tests**: Added explicit denial coverage
  for hosted non-admin connector creation, source-bound upload lifecycle
  `ResourceRef` stamping, and same-native-source-object IDs across tenants.
- **Enterprise source-bound retrieval tenant proof**: Added memory-manager
  coverage proving tenant A cannot retrieve tenant B source-bound chunks even
  when both tenants share the same binding ID, native object path, and query
  phrase.
- **Enterprise source-object re-scope purge proof**: Added admin lifecycle
  coverage proving a source-object re-scope purges old indexed chunks before
  updating lifecycle metadata, preventing stale resource grants from retrieving
  old prompt context.
- **Enterprise prompt-context source-bound proof**: Added memory retrieval
  coverage proving source-bound current-session and history chunks are filtered
  before prompt assembly unless a strict tenant projection grants read access to
  the bound resource/data class.
- **Enterprise memory citation visibility guard**: Applied source-bound access
  filtering to governed memory list responses and added coverage proving list
  views cannot expose source-object IDs, native object paths, or binding IDs
  without a strict read grant.
- **Coder memory-hit source-bound guard**: Coder governed-memory hit artifacts
  now skip source-bound records unless a future strict grant path is plumbed,
  preventing coder retrieval surfaces from exposing source-object metadata by
  default.
- **Automation evidence source-bound guard**: Automation upstream evidence
  collection now filters source-bound internal identifiers from read paths,
  discovered paths, and citations before later nodes can reuse them.
- **Session KB source-bound citation guard**: Strict KB grounding now ignores
  source-bound internal identifiers when extracting source labels and document
  refs, preventing KB citation renderers from exposing source-object metadata.
- **Enterprise binding disable purge**: Disabling or quarantining a source
  binding now purges indexed content for its lifecycle records and tombstones
  affected source objects so stale grants cannot retrieve old chunks.
- **Memory caller source-bound audit**: Prompt-context injection and coder
  duplicate-memory scans now skip source-bound governed records by default,
  closing remaining local/default memory caller gaps without a strict grant.
- **Hosted panel auth availability split**: Control-panel capabilities now
  distinguish managed hosted deployments from deployments with usable hosted
  auth exchange credentials, allowing disconnected local test deployments to
  use engine-token sign-in while real hosted panels keep Tandem sign-in.
- **Enterprise connector lifecycle registry**: Added storage-backed connector
  instance admin endpoints for tenant-scoped create/list/update and lifecycle
  states (`active`, `paused`, `revoked`, `quarantined`). Source-bound memory
  imports now require the referenced connector to exist and allow ingestion.
- **Enterprise connector lifecycle UI**: Wired the hidden Enterprise admin
  control-panel page to create tenant-scoped connector records, list connector
  lifecycle status, and move connectors between active, paused, revoked, and
  quarantined states.
- **Enterprise connector credential refs**: Added admin-gated connector
  credential-reference attach and rotate endpoints that accept secret
  references only, reject raw credential values, validate tenant/resource
  scope, and return credential metadata without credential material. The hidden
  Enterprise admin page can attach read-only/read-write/admin refs and rotate
  existing refs.
- **Enterprise ingestion job audit records**: Added persisted tenant-scoped
  ingestion job records for source-bound manual imports, including running,
  completed, and failed states with connector/binding scope and source-object
  references. Enterprise admins can list ingestion jobs from the runtime and
  inspect them in the hidden control-panel admin page.
- **Enterprise ingestion quarantine review**: Review-required source bindings
  now quarantine source-bound manual import output by purging indexed chunks,
  marking source objects quarantined, recording `IngestionQuarantine` records,
  and exposing admin review dispositions for release, delete, or reindex in the
  runtime and hidden control-panel admin page.
- **Enterprise connector impact response**: Added an admin-gated connector
  impact endpoint and control-panel view for revoke/rotate response handling.
  Admins can inspect affected source bindings, source objects, ingestion jobs,
  quarantines, compromise-window timing, cache-invalidation need, and
  recommended response actions for a connector. The compromise window now uses
  source-object lifecycle timestamps as well as ingestion and quarantine audit
  records.
- **Enterprise response-cache invalidation**: Source-binding, source-object,
  quarantine-review, connector lifecycle, and connector credential changes now
  evict matching source-bound response-cache entries when the response cache is
  present, while keeping unrelated tenant/source-binding entries intact.
- **Enterprise Google Drive provider guardrails**: Added the first Google Drive
  provider descriptor and v1 policy guards requiring read-only, source-bound
  credentials before Drive ingestion is enabled.
- **Enterprise Google Drive read client**: Added a read-only Google Drive API
  client for listing admin-labeled folder roots, downloading stored file bytes,
  and exporting Google Workspace files once a future secret resolver supplies a
  bearer token.
- **Enterprise secret-ref resolver**: Added a runtime-only secret resolver
  abstraction with an `env://...` bearer-token resolver for local Google Drive
  testing. Resolved token values stay in memory and redact from debug output.
- **Enterprise Google Drive preflight orchestration**: Added a source-binding
  preflight layer and admin-gated runtime endpoint that validate active Google
  Drive connectors, enabled source bindings, source-bound read-only credentials,
  and resolver-backed folder listing before indexing is enabled.
- **Enterprise Google Drive admin import path**: Added the first admin-triggered
  Google Drive import endpoint behind the existing enterprise admin, active
  connector, enabled source-binding, read-only credential, and secret-ref
  guardrails. The import path fetches supported Drive documents into a stable
  source-binding namespace, records ingestion jobs/source-object lifecycle rows,
  honors review-required quarantine, and invalidates source-bound response-cache
  entries after indexing.
- **Enterprise Google Drive admin UI wiring**: Wired the hidden Enterprise admin
  page to run Google Drive source-binding preflight and trigger the
  admin-controlled import endpoint, then refresh source-object, ingestion-job,
  quarantine, and connector-impact views so admins can inspect the resulting
  audit trail from the control panel.
- **Enterprise Google Drive import regression proof**: Added HTTP-level
  coverage for the admin-controlled Google Drive import flow, proving
  review-required Drive imports create quarantined ingestion jobs,
  source-object lifecycle rows, and quarantine records without exposing resolved
  credential values.
- **Enterprise route module split**: Moved Google Drive enterprise preflight and
  import route handling into a focused HTTP module so connector-specific logic
  can evolve without further growing the general enterprise admin route file,
  and split organization-unit plus ingestion/source-object lifecycle routes into
  focused modules so the primary enterprise admin route file stays below the
  source-size guideline.
- **Enterprise Google Drive reindex path**: Added an admin-gated Google Drive
  re-fetch/reindex endpoint and hidden admin UI control that reuse read-only
  source-bound credentials, stable binding namespaces, ingestion job auditing,
  quarantine policy, and source-bound cache invalidation without returning
  resolved credential material.
- **Enterprise org-unit memberships**: Added the first Phase H runtime and
  hidden-admin controls for assigning hosted users, groups, agents, and service
  accounts to company-defined organization units such as departments, clinical
  roles, consultants, or executive groups. Memberships are tenant-scoped,
  storage-backed, admin-gated, and ready to feed future signed grant projection.
- **Enterprise org-unit access grants**: Added the Phase H access-rule layer
  between company-defined organization units and resource-scoped permissions.
  Enterprise admins can define tenant-scoped org-unit access grants, preview
  effective `ScopedGrant` projections for a member, and disable grants before
  the signing middleware begins injecting these projections globally.
- **Enterprise org-unit grant ingress projection**: Verified signed strict
  contexts now receive active organization-unit membership grants at HTTP
  ingress. The runtime appends matching tenant-scoped `ScopedGrant`
  projections from stored org-unit memberships and access grants without
  creating strict context for assertions that did not already carry one.
- **Enterprise department/executive denial tests**: Added strict-context
  regression coverage proving department grants do not cross resource or
  data-class boundaries, CEO/global access is explicit, and CEO-spawned agents
  stay narrow unless a delegation projection grants broader access.
- **Enterprise artifact export filtering**: Fintech audit package assembly now
  treats artifacts carrying `ResourceRef` and `DataClass` metadata as scoped
  content. Scoped artifacts are excluded unless the caller supplies a strict
  projection with `Read` access for the artifact resource and data class, and
  scoped artifacts fail closed when no strict projection is available.

### Fixed

- **Automation artifact validation hardening**: Tightened automation output
  validation so stale preexisting artifacts and failed external connector
  mutations cannot be accepted as successful current-attempt output during
  retries. This keeps enterprise delivery and source-bound workflows from
  reporting success when the required current-attempt write or protected
  mutation did not actually happen.
- **Optional web-context tool exposure**: Restored optional `webfetch`
  availability for workflows that request optional web context while preserving
  stricter gating for required research workflows.
- **Workflow-learning memory summaries**: Preserved terminal run detail in
  completed-run learning summaries so generated memory facts keep the
  operator-facing outcome context alongside node output summaries.
- **Eval runner determinism**: Fixed eval priority-order assertions,
  pass-rate threshold boundary handling, case-insensitive scripted-provider
  matching, and stub/live local engine-mode errors when no `AppState` is
  attached.
- **Context rollback checkpoint isolation**: Moved rollback execution tests to
  explicit temporary workspaces and serialized the checkpoint test module so
  rollback file-deletion coverage cannot mutate the real repository
  `src/lib.rs` during parallel test runs.

### Documentation

- **Enterprise connector source-binding Kanban**: Added the internal enterprise
  board for connector credential handling, resource-scoped source bindings,
  safe ingestion, quarantine, revoke/rotate flows, and retrieval isolation
  acceptance tests.

## [0.5.9] - 2026-05-21

### Added

- **Workspace access-control contract vocabulary**: Added transport-safe enterprise contract types for resource hierarchy, scoped resources, access permissions, data classes, normalized principals, grant sources, and scoped grants. The new contract can model department data access, cross-functional group access, explicit CEO/executive global grants, down-scoped external delegation, repository path scopes, and MCP tool resource targets.
- **Workspace access-control contract coverage**: Added serde round-trip and modeling tests for Finance data stores, Engineering repository path scopes, CEO org-wide executive access, MCP tool targets, department membership grants, group membership grants, executive/global grants, and expiring delegated grants.
- **Strict workspace context contract**: Added `StrictTenantContext`, `DataBoundary`, and `AssertionMetadata` so hosted/enterprise flows can carry base tenant context, normalized principal, authority chain, projected resource scope, scoped grants, data-class boundary, and signed assertion metadata as one additive contract object.
- **Workspace grant evaluation contract**: Added allow/deny grant effects, structured access decisions, and strict-context grant evaluation helpers where explicit denies win over inherited allows, resource scopes bound access, expired grants do not apply, and project grants can cover path-scoped resources.
- **Scoped context assertion projections**: Extended Tandem context assertion claims with optional principal, resource-scope, scoped-grant, and data-boundary projection fields while keeping legacy tenant-only assertions backward compatible.
- **Enterprise signing key purpose vocabulary**: Added typed signing-key purposes for context assertions, approval receipts, delegation projections, A2A peer assertions, and break-glass/admin assertions, and re-exported the vocabulary through `tandem-types`.
- **Context assertion key metadata gates**: Hosted context assertion keyrings can now carry key purpose, org/deployment binding, allowed audiences, allowed resource-scope prefixes, activation windows, and status so runtime verification can reject reused approval/admin/delegation keys or assertions outside a key's intended hosted scope.
- **Hosted panel login exchange**: Added the managed-hosted control-panel redirect/exchange path so `tandem-web` can authorize a hosted org member, issue a one-time panel login code, exchange it with the deployment host-agent token, and return a short-lived user context assertion without exposing the root engine token to the browser.
- **Automation V2 MCP contract diagnostics**: Added MCP input-contract summaries, required-argument examples, schema warnings, and required-tool static-argument diagnostics to node preflight metadata and prompts.
- **Hosted tenant isolation denial coverage**: Added regression tests proving Automation V2 tenant payloads cannot override the request tenant, scheduled/background-created runs retain their owning automation tenant, watch-condition runs keep tenant context, background context-run sync does not fall back to `local_implicit`, and stale recovery preserves explicit tenant context without an active HTTP request.
- **Automation V2 event tenant coverage**: Added tenant visibility and finite-body SSE coverage for Automation V2 events so cross-tenant event streams depend on explicit matching `tenantContext`.
- **Runtime resource tenant denial coverage**: Added denial-driven tests for sessions, event streams, context-run internals, Automation V2 runs/gates, legacy workflow routes, provider credentials, MCP secrets, and memory surfaces.
- **Tenant-partitioned vector memory**: Added tenant scope to vector-backed memory chunks and regression tests proving tenant A cannot retrieve, suppress, delete, or dedupe against tenant B vector memory, including identical content/source-hash cases.
- **Tenant-scoped memory stats and cleanup helpers**: Added tenant-aware memory stats, project vector stats, manual clear, and old-session cleanup helpers with tests proving cross-tenant rows are not counted or deleted.
- **Tenant-scoped memory context retrieval**: Added tenant-aware manager retrieval APIs and coverage proving current-session context injection does not mix same-session chunks across tenants.
- **Tenant-scoped memory file import/indexes**: Added tenant-aware import index, file chunk deletion, project file-index stats, and project file-index clear paths with regression tests proving same-path imports cannot cross tenants.
- **Tenant-scoped knowledge memory**: Added tenant-aware knowledge-space indexes and DB/manager APIs for spaces, items, coverage, promotion, and Automation V2 knowledge preflight with denial coverage for cross-tenant reads and mutations.
- **Coder artifact and control tenant denial coverage**: Added regression coverage proving tenant B cannot list, get, read artifacts, approve, cancel, execute, write triage artifacts, or list memory candidates for a coder run created under tenant A.

### Changed

- **Hosted runtime ingress hardening**: Hosted and enterprise runtime modes now require configured transport-token authentication in addition to verified Tandem context assertions, reject local-implicit or deploymentless hosted assertions, bind authority-chain initiators to the human actor, and derive the request principal source from the verified assertion issuer.
- **Hosted root-token handling**: Managed hosted deployments now treat the engine token as server-side root transport only; the deployed control panel switches to Tandem hosted login, forwards the root token only from server memory, forwards `x-tandem-context-assertion`, and hides managed-mode token reveal from the customer dashboard.
- **Enterprise contract re-exports**: Re-exported the new workspace access-control vocabulary through `tandem-types` for downstream runtime/server consumers.
- **Automation V2 background tenant propagation**: Watch-condition run creation now stamps runs from the stored automation tenant instead of `local_implicit`, and Automation V2 context-run blackboard sync now inherits the run tenant.
- **Applied automation tenant stamping**: Workflow planner apply, mission builder apply, and channel automation draft confirm now stamp persisted Automation V2 definitions from the request `TenantContext`, preventing imported/applied payloads from switching tenant context.
- **Scheduled/watch event scoping**: Scheduler-published Automation V2 run-created events now include top-level `tenantContext` so hosted/global SSE filters can make tenant decisions without inspecting nested run payloads.
- **Session, context-run, and automation route isolation**: Hosted tenant checks now hide cross-tenant session/context-run/automation resources with empty results or not-found behavior instead of exposing resource existence.
- **Provider and MCP secret isolation**: Provider credentials and store-backed MCP secrets now carry tenant scope through request and execution paths so hosted explicit tenants cannot resolve or execute with another tenant's credentials.
- **Memory route and DB isolation**: Governed memory search/list/read/update/delete/promote/demote paths now use tenant-aware DB methods, while sqlite-vec top-k ranking filters by tenant before calculating the returned candidates.
- **Memory manager retrieval isolation**: Context retrieval now has tenant-aware wrappers for recent session chunks and vector search, preserving existing local retrieval through local/default wrappers.
- **Memory config and hygiene isolation**: Memory config rows and old-session hygiene now use tenant-aware project/global config and pruning paths so same project ids cannot overwrite or clean another tenant's memory policy/state.
- **Coder run tenant propagation**: Coder-created context runs now inherit the request tenant, coder status/list/get/artifact reads filter through the linked context run tenant, and coder control/artifact-writing handlers require the caller to match the owning context run tenant before mutating state.

### Fixed

- **Automation V2 MCP required-tool diagnostics**: Required MCP tool validation now records the exact missing `required_tool_calls`, includes them in repair guidance, and reports MCP string errors such as `MCP error -32602` as failed tool results instead of successful connector calls.
- **Automation V2 MCP string-argument examples**: MCP contract guidance now respects positive `minLength` constraints for required string args, so connectors like Notion search no longer receive generated examples with invalid empty query strings.
- **Automation V2 structured JSON schema enforcement**: Structured JSON nodes with an `output_contract.schema` now reject artifacts that do not match the declared shape, preventing raw connector responses from passing as valid handoff artifacts.
- **Automation V2 empty connector batches**: Structured connector nodes now short-circuit across empty batch, empty candidate, empty high-value-contact, and empty write-row handoffs, writing the schema-shaped empty artifact instead of spending MCP calls on account, inventory, enrichment, or write checks.
- **Automation blocker visibility**: Automation debugger blocker panels now include checkpoint lifecycle events, so `node_repair_requested`, `workflow_state_changed`, and `run_paused` reasons surface directly when node outputs or top-level run fields omit the actionable blocker.

### Notes

- Local/default single-tenant behavior remains unchanged.
- This release continues the hosted tenant-isolation hardening work; broader artifact paths, audit exports, SCIM, Zitadel, and private sidecar work remain separate follow-up surfaces.

## [0.5.8] - 2026-05-17

### Added

- **Enterprise tenant context foundation**: Added strict runtime auth-mode and verified tenant-context contract types for the enterprise hosted-auth roadmap, including hosted/single-tenant mode names, human actor metadata, assertion metadata, deployment-aware tenant context, explicit hosted tenant constructors, and request authority-chain helpers.
- **Runtime auth mode parser**: Added canonical parsing and operator-friendly aliases for `local_single_tenant`, `hosted_single_tenant`, and `enterprise_required`, plus a `TANDEM_RUNTIME_AUTH_MODE` resolver for later server enforcement.
- **Tandem tenant context assertion wire shape**: Added provider-agnostic tenant context assertion header and claims types for the future Tandem-signed JWS passed from `tandem-web` to runtime/ACA.
- **Runtime Tandem context assertion verification**: Hosted and enterprise runtime ingress can now verify compact Tandem tenant-context assertions signed with Ed25519 before accepting tenant/actor identity.
- **Context assertion keyring support**: Runtime verification now supports multiple Ed25519 public keys by `kid` through `TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS` / `_FILE`, preserving the single-key env vars as legacy fallback for hosted deployments.
- **Hosted context assertion signer prep**: The hosted control-plane workstream now has a provider-neutral context assertion signer shape, a local Ed25519 test signer, and a Google Cloud KMS Software Ed25519 adapter in `tandem-web`.
- **Coder run handoff artifacts**: Coder run records now expose worker/session ids, managed worktree paths, branch and commit metadata, PR URLs, changed files, validation state, handoff state, and completion-gate evidence so the control panel can show what a coding worker actually did.
- **Coder project scheduling policy defaults**: Project policy now includes PR-required handoff, native Tandem delegation, a max parallel issue-run limit, and a default ban on manual out-of-order runs.
- **Coder intake scheduler fields**: GitHub Project intake payloads now include parent-card detection, phase, blockers, scheduler rank, runnable state, active run id, run state, and handoff URL for board-style scheduling.

### Changed

- **Tool policy context now carries tenant context**: Runtime tool policy hooks now receive the session's tenant context when evaluating tool calls, giving protected execution paths the tenant/actor scope needed for future enterprise authorization and approval-receipt verification.
- **Hosted/enterprise ingress fail-closed scaffold**: `hosted_single_tenant` and `enterprise_required` runtime auth modes now reject raw tenant/actor headers and fail closed until Tandem signed context assertion verification is implemented, preventing operators from accidentally trusting spoofable hosted identity headers.
- **Hosted/enterprise ingress trust boundary**: Strict hosted modes now require a configured Tandem context assertion public key, validate assertion issuer/audience/expiry, reject tampered assertions, and attach the verified tenant context to request extensions.
- **Fintech strict tenant mismatch guard**: Fintech strict protected-tool policy now rejects calls when the session tenant context does not match the owning Automation V2 run tenant context.
- **Strict protected-tool context guard**: In hosted/enterprise auth modes, fintech strict protected tools now fail closed when tool execution lacks a verified non-local tenant context with a human actor.
- **Tool-time assertion expiry guard**: Sessions now retain verified tenant assertion metadata and pass it into runtime tool policy, allowing hosted/enterprise protected tools to reject expired signed tenant assertions at execution time.
- **Local auth regression coverage**: Added a local-mode session smoke test proving hosted auth and signed assertions are not required by default.
- **Coder issue-fix workers use a strict coding contract**: Issue-fix worker sessions now run with required tools, prewrite inspection requirements, and an explicit native Tandem coding contract to inspect the repo, plan, patch files, validate, repair failures, and report evidence.
- **Coder completion is gated by real handoff evidence**: Issue-fix runs now block when no patch is produced, validation fails, or push/PR handoff fails. Successful implementation handoff moves GitHub Project work to Review instead of claiming Done.
- **Managed coder worktrees are preserved through handoff**: Coder keeps worker worktrees until handoff completes so diffs, changed files, validation output, branch metadata, commits, and PR artifacts remain inspectable.
- **Parent cards are planning-only**: Coder scheduling keeps parent Project cards non-runnable and launches scheduler-approved child issues by phase/dependency order.
- **Coder control panel board-first intake**: The control panel now renders intake as a TODO / In Progress / Blocked / Review / Done board with next-runnable badges, disabled run buttons with reasons, scheduler-only launch controls, handoff links, and a Tandem spinner while GitHub board sync is active.
- **Coder active-run visibility**: Active coder runs surface worker/session identity, log/transcript tails, changed files, validation state, branch/PR handoff details, and failure reasons in the run payload used by the existing coder routes and streams.
- **Version bump**: Rust crates, npm packages, Python client metadata, Tauri config, and lockfiles move to `0.5.8`.

### Notes

- This release starts the enterprise auth and execution-time verification implementation without enabling hosted strict auth by default.
- Local, desktop, and single-tenant runtime behavior remains unchanged unless a later strict hosted/enterprise mode is explicitly configured.
- `tandem-web` remains the intended owner of Tandem-signed hosted context assertions; runtime and ACA consume Tandem assertions/public keyrings, not raw Zitadel or Google identity tokens.
- Coder treats GitHub Project `Done` as post-review/merge state; completed implementation work is handed off as a PR and moved to Review.

## [0.5.7] - 2026-05-17

### Added

- **Enterprise AI runtime infrastructure positioning**: README and public docs now present Tandem as governed AI runtime infrastructure for long-running agentic work. New docs cover the runtime architecture, enterprise readiness status, and a platform-engineering proof walkthrough with clear boundaries between shipped runtime primitives and planned enterprise capabilities.
- **Fintech strict runtime profile foundation**: Added an internal `fintech_strict` profile marker for Automation V2 metadata. Fintech strict mode reuses Strict execution semantics while adding domain-specific runtime policy for compliance and risk workflows.
- **Protected fintech action classifier and runtime gate**: The runtime now classifies account actions, customer communications, regulatory filings, system-of-record updates, credit decisions, money movement, and evidence publication as protected fintech actions. Fintech strict automations block protected actions and unknown external mutation tools until an approval path is used.
- **Connector proof and compliance artifact validation helpers**: Added core helpers for extracting connector proof from successful source retrieval tool records, treating discovery/listing as insufficient evidence, and validating compliance/risk brief artifacts for required fields, citations, limitations, approval state, and audit IDs.
- **Fintech audit evidence assembly**: Added an internal audit package shape and an Automation V2 helper that assembles run, tenant, actor, tool ledger, artifact, approval, and policy-decision evidence for compliance review.
- **Persisted fintech audit package artifact**: Added an internal helper that writes assembled fintech audit packages into the linked context-run artifact store for compliance-review handoff.
- **Fintech compliance/risk eval dataset**: Added proof-sprint eval fixtures for unsupported claim rejection, connector proof-of-use, protected-action bypass attempts, cross-tenant source denial, and incomplete evidence limitations.
- **Coder workspace live status badges and progress** (`src/components/coder`): Added shared `CoderRunStatusBadge`, `CoderRunProgress`, and `CoderRunsSummary` components plus `runStatusTone`/`runIsActive`/`runProgress`/`relativeTimeFromMs` helpers in `coderRunUtils.ts`. The Runs view now opens with an always-visible summary strip tallying Running / Needs approval / Paused / Failed / Completed across the workspace with a ticking "Updated Xs ago" indicator. Status renders as colored chips with animated indicators (Running spinner + pulse, Queued pulse, Needs approval amber + pulse, Paused, Failed, Cancelled, Completed) on every run card and the detail header, and each card/detail also shows a tone-tinted progress bar with `completed / total` (and blocked) step counts derived from the run checkpoint.
- **Extracted `CoderGithubProjectPanel`**: GitHub Project binding and inbox UX moved out of the 1,500-line `CoderWorkspacePage.tsx` into a dedicated component with explicit Not connected / Connected states. Once bound the card collapses to a one-line `Connected · owner #N` summary with Refresh and Change buttons, with status mapping and saved/live schema fingerprints behind an Advanced disclosure.

### Changed

- **Tool effect ledger source identifiers**: Tool ledger summaries now preserve safe source identifiers such as `source_id`, `document_id`, `ticket_id`, and `record_id` while continuing to avoid raw query text.
- **Context-run ledger fintech proof summary**: Existing context-run ledger summaries now include `fintech_connector_proof` derived from successful source retrieval calls.
- **Fintech approval override hardening**: Mission runtime projection now ignores `metadata.approval.skip_approval` for fintech strict nodes, so UI/planner metadata cannot suppress injected approval gates on fintech strict work.
- **Fintech protected-action denial language**: Protected fintech tool denials now fail closed with explicit call-site approval/policy verifier status in the denial reason and protected audit payload.
- **Fintech protected-action call-site verifier**: Automation gate decisions can now carry protected-action metadata, and fintech strict protected tools are allowed only when a matching approved receipt proves tenant, category, tool, action hash, and non-expired approval at execution time.
- **Workflow-level fintech brief validation**: Explicitly marked fintech compliance/risk brief nodes now persist connector proof and validation results in artifact validation metadata, and reject citations that cannot be mapped to recorded connector proof.
- **Planner fintech strict stamping**: Workflow plans that explicitly ask for fintech compliance/risk brief artifacts now materialize with `fintech_strict` runtime metadata and artifact markers by default, while generic finance workflows remain unstamped.
- **Eval runner fintech metadata mapping**: Eval specs now carry `runtime_profile`, `tenant_id`, and artifact-contract config into Automation V2 metadata so fintech strict fixtures can exercise the same runtime gates as generated workflows.
- **Audit stream coverage**: `/audit/stream` now normalizes `fintech.protected_action.denied` and `fintech.protected_action.approved` events into admin-readable audit rows.
- **Version bump**: Rust crates, npm packages, Python client metadata, Tauri config, and lockfiles move to `0.5.7`.
- **Coder workspace awaiting-gate prompts elevated**: When a coder run is waiting on an operator decision, the detail card now shows the prompt title, instructions, and Approve & continue / Request rework buttons in an amber alert at the very top of the card instead of in the Overview tab's Gate State panel. Matching list cards grow an amber "Waiting on you: …" banner so the signal is visible without selecting the run.
- **Coder workspace project header consolidated**: The Coder page header now embeds `ProjectSwitcher` directly and shows the detected git slug / current branch / default branch as a subtitle, replacing the previous duplicate Active Project stat box and separate Project Context card. Tabs became accent pills with badge counts (e.g. `Runs · 3`) that switch to amber/red tones when runs need approval or have failed, and the page auto-defaults to the Runs tab on first load when the workspace has active runs.

### Removed

- **Coder workspace dev-noise sections**: Removed the "First Slice" and "Compatibility" stat boxes from the Coder header, the standalone User Repo Context card (the same info now renders as a subtitle under the project switcher), the duplicate Project Context card, and the "Selected preset … is UI scaffolding in this slice" copy from the Mission Builder. The legacy `DeveloperRunViewer` ("Legacy Compatibility") is no longer pinned open at the bottom of the Runs view — it now lives behind a collapsed "Legacy coder inspector" disclosure so the live coder runs are the default view.

### Documentation

- Added `docs/AI_RUNTIME_INFRASTRUCTURE.md`, `docs/ENTERPRISE_READINESS.md`, and `docs/ENTERPRISE_PROOF_WALKTHROUGH.md`.

### Notes

- This release does not add public HTTP API changes for fintech strict mode.
- `fintech_strict` is an internal profile marker, not mandatory isolation by itself; approval gates are runtime control points, not complete authorization.
- OIDC, SCIM, SIEM export, SOC2, full RBAC, private sidecar enforcement, automatic protected-action approval routing, enterprise policy authorization, and persisted fintech audit exports remain planned or follow-up work.
- The Coder workspace restructure is pure UI: no changes to the `tandem-agents` API surface, the Tauri command surface, the Automation V2 contract, the coder metadata schema, or the GitHub Project MCP tools. Saved coder templates, saved GitHub Project bindings, and the existing run detail tabs (Overview, Transcripts, Context, Artifacts, Memory) continue to work unchanged.

## [0.5.6] - 2026-05-14

### Added

- **AI Evaluation Framework (Phases 1-5)**: The evaluation framework (now under `crates/tandem-eval`) and `tandem-server/src/failures` modules provide structured testing, regression detection, and compliance documentation for AI quality assurance. Phases 1-2 add the `AIFailureMode` taxonomy (30+ categorized failure types across validation, provider, repair, resource, timeout, and authorization domains) and the `EvalDataset`/`EvalTestCase`/`AutomationSpecTest` YAML schema with four reference datasets in `eval_datasets/` (critical_path, provider_failures, repair_exhaustion, citation_validation). Phase 3 ships the `eval-runner` CLI binary (`cargo run -p tandem-eval --bin eval-runner`) with metrics aggregation (`pass_rate`, `avg_repair_iterations`, `total_cost_usd`, `provider_failure_rate`, validator pass rates by class), parallel worker support, tag filtering, and simulation mode for deterministic CI runs without provider calls. Phase 4 adds `EvalBaseline`/`RegressionThresholds`/`RegressionReport` types and `detect_regressions()` for comparing current runs against saved baselines (default thresholds: 5pp pass_rate drop, 20% cost increase, 30% repair iteration increase, 5pp provider failure increase), plus a `.github/workflows/eval-regression-gate.yml` CI workflow that runs the gate on every PR, posts a summary comment, auto-updates the main-branch baseline, and fails CI when critical thresholds are exceeded. Phase 5 ships developer documentation (`docs/dev/EVAL_FRAMEWORK.md`) and user/compliance documentation (`docs/user/AI_QUALITY_ASSURANCE.md`) covering EU AI Act Article 50 transparency obligations.

- **Failure mode taxonomy module** (`tandem-server/src/failures`): `AIFailureMode` enum with 30+ variants (e.g., `ArtifactValidationFailed`, `ContractViolation`, `CitationMissing`, `ProviderTimeout`, `RepairBudgetExhausted`, `TokenBudgetExhausted`, `PathTraversalDetected`, `AuthorizationFailed`), `FailureCategoryKind` severity classification (Critical / High / Medium / Low), `FailureContext` struct for incident tracking, and helpers `classify_error_text()`, `categorize_failure()`, and `should_retry()` for deterministic error categorization. Includes 11 unit tests covering classification, serialization, and retry decision logic.

- **Eval baseline storage** (`eval_baselines/main_branch.json`): Sample baseline format with git metadata (commit SHA, branch), tracked metrics, and validator pass rates for the critical_path dataset. The regression-gate workflow updates this file automatically on main-branch pushes so future PRs are evaluated against the latest production performance.

- **Eval runner binary** (`crates/tandem-eval/src/bin/eval_runner.rs`): Standalone CLI with `--dataset`, `--output`, `--provider`, `--model`, `--simulation`, `--num-workers`, `--filter-tag`, `--max-duration`, and `--verbose` flags. Exit codes are CI-friendly: 0 (all pass), 1 (one or more failures), 2 (dataset load error or invalid arguments). Output is both human-readable on stdout and a structured JSON results file consumed by the regression-gate workflow.

### Security

- **CRITICAL: Authorization bypass in channel interaction endpoints** - Slack, Discord, and Telegram approval interactions now fail closed unless the acting user resolves through the configured channel allowlist before approval, rework, or cancel decisions are processed.

- **CRITICAL: TOCTOU race condition in automation run cache loading** - Automation run state reloads now detect concurrent in-memory updates before accepting disk-loaded state, preventing stale cache loads from overwriting gate decisions or duplicating execution.

- **HIGH: Path traversal protection for automation IDs and run IDs** - Automation definition and run-history paths now sanitize identifier-derived filenames and verify resolved paths stay inside their intended state roots.

- **Dedup TTL for webhook interaction replay attacks** - Discord and Slack interaction deduplication now uses a bounded retry window, reducing stale replay risk while preserving normal platform retry handling.

- **File permission validation on state file load** - Startup now warns when sensitive state files have overly broad Unix permissions so operators can tighten local storage access.

- **CRITICAL: Discord modal identifier validation** - Discord rework modal submissions now reject malformed or incomplete identifiers before any gate decision is dispatched.

- **CRITICAL: Telegram dedup ring missing TTL-based expiration** - Telegram approval callbacks now use the same retry-window deduplication model as Discord and Slack, reducing stale callback replay risk.

- **HIGH: Missing channel user IDs now reject** - Channel approval handlers now reject malformed requests without a resolvable acting user instead of assigning a placeholder identity.

- **HIGH: Reason field in rework requests now size-limited** - Discord rework feedback is now bounded server-side before being stored with gate decisions.

- **HIGH: Authorization denial responses no longer echo user IDs** - Public channel rejection messages now use generic denial text while retaining detailed audit logs for operators.

- **JWT structure and algorithm validation** - Codex identity token parsing now validates token shape, header presence, allowed algorithm behavior, and signature encoding before processing claims.

- **HIGH: JSON merge recursion depth limit** - Provider configuration merging now enforces a maximum nesting depth to avoid stack exhaustion on deeply nested input.

- **MEDIUM: CODEX_HOME path validation** - Codex CLI home resolution now rejects unsafe or system-sensitive paths and falls back to the default home directory with a warning.

- **MEDIUM: Safer token expiration handling** - Codex identity resolution now rejects tokens without valid expiration claims and bounds-checks expiration timestamps before time arithmetic.

### Added

- **Approval notification fan-out and rich channel delivery**: Slack, Discord, and Telegram channel adapters now implement interactive card delivery, posting native approval cards via Block Kit, Discord embeds/components, and Telegram inline keyboards.
- **Approval message handle map**: Added persisted `approval_message_map.json` state so delivered approval cards can be looked up by request ID for later lifecycle updates.
- **Slack approval notifier wiring**: Server startup now registers Slack approval fan-out from the pending approvals source when Slack bot credentials are configured, with shared notifier scaffolding for Slack, Discord, and Telegram.
- **Shared automation gate state helpers**: Automation V2 gate pause and decision mutations now live behind shared `pause_automation_run_for_gate` and `apply_automation_gate_decision` helpers, so executor pause behavior and HTTP gate decisions use one state-transition path.
- **Per-step approval override controls**: Workflow edit prompts now expose per-step approval overrides. Operators can keep the default approval gate, mark a step for conditional auto-approval metadata, or explicitly skip approval with a confirmation; saved node metadata feeds the compiler's existing `metadata.approval.skip_approval` hook and clears stale injected gates for skipped steps.
- **Telegram approval rework completion**: Telegram approval cards now use persisted opaque callback IDs for long run/node identifiers, while legacy truncated callbacks remain fail-closed. Rework taps send a force-reply prompt, capture the operator's next valid reply for that chat/user, and dispatch the feedback as a `rework` gate decision.
- **Threaded approval status replies**: Channel adapters now expose a shared thread-reply primitive. After an approval decision updates the original card, Tandem posts a short follow-up into the stored Slack thread, Discord thread/channel target, or Telegram topic when available.
- **Channel command capability tiers**: Built-in slash commands now carry read/act/approve/reconfigure tiers, and dispatcher execution checks those tiers against the channel security profile before running a command.
- **Persisted channel user capabilities**: Added `channel_user_capabilities.json` state for explicit per-channel user capability assignments, with load/persist/upsert helpers and profile-tier fallback for users that have not enrolled yet.
- **Channel enrollment pairing codes**: Added `POST /channels/enroll` issue/confirm flow for short-lived pairing codes that bind Slack, Discord, or Telegram user IDs to persisted channel capability tiers. Approval interactions now require an explicit `Approve`-or-higher user capability unless the channel security profile already grants that tier.
- **Channel outbound redaction**: Added a shared outbound redaction pass for dispatcher replies, stripping common secret patterns and filesystem paths outside the workspace boundary before Slack, Discord, or Telegram sends. Operators can extend patterns via `TANDEM_CHANNEL_REDACTION_PATTERNS_FILE`.
- **Per-user channel rate limiting**: Added in-memory token buckets keyed by channel user, with separate prompt and approval-decision budgets. Channel-origin `prompt_sync` requests default to 10 prompts/minute, approval interactions default to 30 decisions/minute, and `429` responses include `Retry-After`.
- **Workspace pinning for channel sessions**: Sessions can now carry `pinned_workspace_id`; channel-created sessions pin to the server workspace and enrollment records can preserve an explicit pin. Tool execution and sandbox checks use the pinned workspace for channel sessions and return `ToolDenied { reason: WorkspaceScope }` when file paths target another workspace.
- **Streaming audit export**: Added `GET /audit/stream` as an admin-gated newline-delimited JSON feed for approval decisions, tool execution ledger events, and channel capability changes.
- **Step-up confirmation for channel reconfiguration**: Reconfigure-tier channel commands such as `/providers`, `/model`, `/schedule`, `/automations`, and `/config` now stop at a dispatcher middleware unless the message includes a fresh desktop-issued PIN. PINs expire after 5 minutes and are stripped before slash-command parsing so the confirmation token cannot leak into command arguments.

### Fixed

- **Slack approval cards update after decisions**: Successful automation gate decisions now best-effort edit the original Slack approval card to remove action buttons and show approved, rework, or cancelled status.
- **Channel dispatcher test baseline**: Updated dispatcher tests to match registry-driven help text and concrete operator tool allowlists.

## [0.5.5] - 2026-05-13

### Added

- **Execution Profiles foundation (Strict / Guided / YOLO)**: Added the type-level scaffolding for runtime execution profiles. `ExecutionProfile` enum, `ValidatorClass` taxonomy with `is_relaxable_in(profile)` mapping, `decide_profile_validation` chokepoint, and `effective_repair_budget` helper now live in `automation_v2::execution_profile`. `AutomationExecutionPolicy` carries an optional `profile` and `resolve_effective_execution_profile` resolves the precedence (run override → workflow policy → Strict). No runtime behavior change yet — subsequent v0.5.5 work wires the chokepoint to the executor and adds the receipt/UI surfaces.
- **Execution Profile run-now override**: `AutomationV2RunNowInput` now accepts an optional `execution_profile`. New `create_automation_v2_run_with_profile` / `create_automation_v2_dry_run_with_profile` helpers resolve the effective profile and persist it on `AutomationV2RunRecord` as `effective_execution_profile` (typed) and `requested_execution_profile`. Run-now audit metadata now carries `requestedExecutionProfile` and `effectiveExecutionProfile`. Existing run-now payloads without a profile continue to resolve to Strict and behave identically.
- **Execution Profile in lifecycle metadata and run-failed events**: Every `AutomationLifecycleRecord` written via `record_automation_lifecycle_event_with_metadata` now carries the run's `effective_execution_profile` in its metadata (existing keys are preserved). The `automation_v2.run.failed` engine event also surfaces `effective_execution_profile` and `requested_execution_profile`, so Incident Monitor and downstream observers can attribute failures to the active profile.
- **Execution Profile chokepoint and validator-class telemetry**: Added `classify_unmet_requirement` (mapping existing validator strings such as `missing_required_section`, `weak_markdown_structure`, and the always-blocking critical classes to a structured `ValidatorClass` taxonomy) and `augment_output_with_profile_relaxation` (which writes `relaxed_validator_classes`, `effective_outcome`, `original_validator_outcome`, `execution_profile`, and `experimental` into `artifact_validation` when the active profile would relax all unmet requirements). The executor invokes the augmentation at the single run-acceptance chokepoint per the "Executor Chokepoint Invariant." Strict behavior is unchanged; critical classes (auth, secret access, destructive-action approval, budget caps) and not-yet-classified unmet requirements always block.
- **Execution Profile status downgrade**: When the chokepoint relaxes a node's outcome, `augment_output_with_profile_relaxation` now also rewrites the executor-facing fields so the run continues. Guided runs land as `completed_with_warnings`; YOLO runs land as `completed` with `experimental: true` on the artifact. Validation-related `failure_kind` and `blocked_reason` are cleared, `warning_count` is set to the count of relaxed classes, and the original `status`/`failure_kind` are preserved under `artifact_validation.original_status`/`original_failure_kind` for receipts and replay. Non-validation `failure_kind` values (e.g. `provider_stream_failed`) are left untouched.
- **Execution Profile repair budget multiplier**: `validate_automation_artifact_output_with_context` now applies `effective_repair_budget` (Strict 1.0×, Guided 1.5×, YOLO 2.0×) to the per-node `AutomationOutputEnforcement.repair_budget` before passing it to `infer_artifact_repair_state`. The multiplier is bounded by the existing `AutomationExecutionPolicy` global caps. Multiplier is driven by the saved automation spec's profile; honoring run-level overrides is a follow-up.
- **Execution Profile control panel surfaces**: The TypeScript client `automationsV2.runNow` accepts an optional `executionProfile` ("strict" | "guided" | "yolo") that maps to the server's `execution_profile` field, and `AutomationV2RunRecord` exposes `effective_execution_profile` and `requested_execution_profile`. New helpers (`executionProfileLabel`, `workflowEffectiveExecutionProfile`, `artifactValidationIsExperimental`, `artifactValidationRelaxedClasses`) and components (`ExecutionProfilePill`, `ExperimentalArtifactBadge`, `RelaxationOutcomeSummary`) ship in `tandem-control-panel`. Run summaries now surface the effective execution profile (with any per-run override note), and artifacts marked experimental by the chokepoint display an Experimental badge that lists the relaxed validator classes and original/effective outcome on hover.
- **Execution Profile Tauri desktop run-now plumbing**: `automationsV2RunNow` in `src/lib/tauri.ts` accepts an optional `{ dryRun?, executionProfile? }` payload and forwards it to the engine. The Tauri command and `Sidecar::automations_v2_run_now` accept an optional request body so per-run profile overrides work the same way as in the control panel and HTTP API. Existing callers without options remain compatible.
- **Execution Profile UI parity** (control panel + Tauri desktop): the workflow edit dialog now exposes an Execution Profile select that round-trips through `WorkflowEditDraft.executionProfile` and writes `execution.profile` on update; automation cards expose a "Run as Strict / Guided / YOLO" override picker next to Run now and surface the saved profile as a pill in the card metadata; the per-task run debugger surfaces the Experimental badge on the validation-outcome card and a dedicated profile-relaxation panel listing the structured `relaxed_validator_classes`.
- **Experimental-input propagation**: downstream automation v2 node outputs now inherit `artifact_validation.experimental = true` (and a `tainted_inputs` array of upstream node ids) when any upstream they depend on was accepted under a relaxed profile. Pure metadata: status, `failure_kind`, and `warning_count` are not altered, so cleanly-passing downstream nodes still complete; the receipt and `run_completed` event preserve the experimental provenance through the rest of the run.
- **Run-level profile override propagates through repair budget multiplier**: `create_automation_v2_run_with_profile` and `create_automation_v2_dry_run_with_profile` now stamp the resolved effective profile onto the cloned `automation_snapshot.execution.profile` so downstream code that reads `automation.execution.profile` (the multiplier in `validate_automation_artifact_output_with_context` and similar paths) honors per-run overrides instead of silently falling back to the saved spec profile. The original `AutomationV2Spec` passed in is unchanged.
- **`node_relaxed` lifecycle event**: When the chokepoint downgrades a node's outcome under Guided/YOLO, the executor now emits a top-level `node_relaxed` lifecycle event alongside the usual `node_completed`/`node_completed_with_warnings` event. Metadata carries the structured `relaxed_validator_classes`, `original_validator_outcome`, `effective_outcome`, the original executor `status` that was rewritten, and the `experimental` flag. Run history, the `automation_v2.run.failed` payload, and Incident Monitor receipts now show relaxation directly without consumers having to walk into `artifact_validation`.
- **Tenant-level default execution profile**: `TANDEM_DEFAULT_EXECUTION_PROFILE` (accepting `strict` / `guided` / `yolo` plus operator-friendly aliases like `assisted`, `exploratory`, `lenient`) lets operators flip a workspace from Strict-by-default to Guided-by-default during validator hardening without editing every saved automation. Precedence is now: explicit run override → saved workflow policy → tenant default → system default of Strict.
- **Human disposition signal on relaxed artifacts (graduation-loop scaffolding)**: New `HumanDisposition` enum (`unmarked` / `accepted` / `rejected` / `re_ran_strict`) plus `parse_human_disposition_str` (canonical names + operator-friendly aliases like `accept` / `reject` / `rerun`) and `set_human_disposition_on_output` (idempotent setter that writes `human_disposition` into `artifact_validation`) land in `automation_v2::execution_profile`. This is the data-model hook the graduation loop reads alongside `relaxed_validator_classes` to compute per-class accept-rate over a rolling window.
- **Disposition HTTP endpoint**: `PATCH /api/automations/v2/runs/{run_id}/tasks/{node_id}/disposition` records a human accept/reject decision on a single node output. Body takes `disposition` (canonical or alias) plus optional `reason`; returns 200 with `changed: bool` so callers can detect idempotent re-applies. The endpoint deliberately does not require the run to be terminal — humans can disposition in-progress runs while reviewing experimental Guided/YOLO outputs.
- **Graduation summary aggregate**: `GET /api/automations/v2/graduation/summary?window_hours=&automation_id=&limit=` walks recent runs and returns per-`ValidatorClass` disposition counts (`accepted` / `rejected` / `re_ran_strict` / `unmarked`) with `total()` and `accept_rate()` (excludes unmarked, returns `None` when no humans have reviewed that class — render as "insufficient signal"). The aggregator is pure (`aggregate_human_dispositions_by_class`) so any caller can produce the same shape from an arbitrary slice of node outputs. Window defaults to 168h, capped at 720h; scan limit defaults to 200 runs and is capped at 500.
- **Disposition control in run debugger (control panel)**: the per-task profile-relaxation panel now exposes Accept / Reject / Re-ran Strict / Clear buttons. Clicking fires `client.automationsV2.setTaskDisposition` and invalidates the automations query so the badge re-renders. The current `human_disposition` is surfaced inline (e.g. "current: accepted") so reviewers can confirm what was previously recorded. Buttons disable while a save is pending; idempotent re-applies surface as "Already marked …" instead of an error.
- **Disposition control in Tauri run-detail view**: the Node Outputs section in `AgentAutomationPage` mirrors the control-panel UI. New Tauri command `automations_v2_run_task_disposition` (registered in the generate_handler list), Sidecar PATCH method, and `automationsV2RunTaskDisposition` wrapper in `src/lib/tauri.ts` expose the same accept/reject / re-ran-strict / clear flow on each relaxed output, with the run refetched via `loadSelectedRunDetail` after a successful save.
- **Graduation summary dashboard**: the control-panel Dashboard route now embeds a `GraduationSummaryPanel` that reads `/automations/v2/graduation/summary` and renders a per-`ValidatorClass` table with accepted / rejected / re-ran-strict / unmarked counts and an accept rate. Window selector toggles 24h / 7d / 30d. Rows are sorted by reviewed-count (most-disposed classes first); the rate reads "insufficient signal" until at least one human has reviewed that class, and is colored green ≥80%, amber 40–80%, rose ≤40% once enough signal is present.
- **Session records now carry explicit source metadata**: Engine sessions can now record `source_kind` and `source_metadata`, with wire responses and TypeScript client types exposing the same data. New user-created sessions default to `chat`, while automation-owned runtime sessions can be classified separately.
- **Per-task workflow tool access controls**: Automation V2 flow nodes now support first-class `tool_policy` and `mcp_policy` fields, letting workflows scope built-in and MCP tools per node instead of relying only on workflow/agent-level access. Workflow Studio and the automation edit dialog expose default-collapsed "Task tool access" controls with inherit/custom markers, MCP tool selectors, and send-capable warnings so approval-gate workflows can give draft creation and post-approval send steps different concrete Gmail tools.
- **Channel strict KB grounding control**: Channel settings now expose an explicit `Strict KB grounding` toggle for Telegram, Discord, and Slack, making knowledgebase-grounded answer behavior visible and configurable instead of hidden in raw channel config.

### Fixed

- **Disabled channel MCP servers are now a hard access boundary**: Channel MCP server checkboxes now gate every path that can expose MCP tools to an agent. Exact MCP tool selections only apply when their owning server is enabled; stale exact-tool preferences are filtered by the server namespace, route allowlists cannot re-enable tools from disabled MCP servers, and default channel tool scopes no longer fall back to a wildcard that could accidentally expose MCP connections. Automation draft context now reports only the exact MCP tools still active after that filtering.
- **Approval-gated Gmail draft workflows no longer expose send tools to pre-approval tasks**: Node-level tool/MCP policies are now enforced as a hard runtime scope, including explicit empty policies and concrete `mcp.*` allowlists. A draft-creation node can be limited to create-draft tools, while a separate post-approval node can be limited to `gmail_send_draft`; unrelated Gmail send-email tools are filtered out even if broader workflow or server policy would otherwise expose them.
- **Automation worker sessions no longer appear as Chat conversations**: Chat and Dashboard recent-session lists now request `source=chat`, and the server filters session listings by source. Existing legacy records titled like `Automation ... / ...` are classified as `automation_v2` at the storage/wire boundary, keeping Incident Monitor and Automation V2 audit sessions inspectable through automation/run surfaces without polluting the Chat session picker.
- **Tauri calendar view no longer crashes during startup**: The Automation Calendar now loads FullCalendar after the Tauri/WebKit stylesheet host is ready and keeps FullCalendar in a lazy chunk, avoiding a WebKit timing crash where FullCalendar accessed `style.sheet.cssRules` while the stylesheet was still `null`.
- **Incident Monitor GitHub publishing is idempotent under recovery races**: GitHub issue creation now claims a persisted pending post record before calling GitHub, keyed by the same create-issue idempotency digest used for successful posts. If completion, timeout recovery, and stale-provider recovery all try to publish the same draft at once, only one caller can create the issue; the others return `publish_in_progress` or reuse the completed post instead of creating duplicate GitHub issues.
- **Incident Monitor triage artifact gates accept real structured handoffs**: Proposal quality checks now understand wrapped and array-shaped Incident Monitor node outputs, including completed inspection, validation, and evidence handoffs returned directly in the final response. Placeholder task specs are still rejected, but valid completed handoffs no longer get mistaken for missing artifacts and replaced with low-signal fallback evidence.
- **Incident Monitor fix proposals no longer self-block on nested limitation status**: Structured Incident Monitor triage handoffs can now contain an inner `status: blocked` to describe limited evidence without causing the Automation V2 node itself to be classified as blocked. This keeps `propose_fix_and_verification` useful when it preserves the original workflow failure and bounded next steps from partial tool evidence.
- **Automation V2 stale reaping no longer races active node timeouts**: The stale-run reaper now honors the active node heartbeat maintained by the run registry. Long-running nodes with a 600-second budget can reach their own timeout/repair path instead of being globally paused as `stale_no_provider_activity` at the same 600-second boundary.
- **Research artifacts preserve websearch URLs as citation evidence**: Automation V2 now extracts source URLs from successful `websearch`/`webfetch` tool results and carries them into artifact validation metadata. Sparse JSON research artifacts no longer block as `citations_missing` when the run actually performed successful current web research with source URLs, and repair prompts now tell agents to write raw URLs into `citations`/`web_sources_reviewed` fields.
- **Connector-backed source nodes must use the connector, not just discover it**: Automation V2 now rejects connector-backed source artifacts when the node only runs `mcp_list` and never calls a concrete selected connector tool. Reddit research nodes that select `reddit-gmail` must execute tools such as `mcp.reddit_gmail.reddit_search_across_subreddits` or `mcp.reddit_gmail.reddit_retrieve_reddit_post` before a "no evidence" artifact can be accepted.
- **Connector source prompts steer agents into real MCP calls**: Generated Automation V2 prompts now list the concrete connector tools available to source nodes and explicitly warn that `mcp_list`, filesystem discovery, and edit/patch tools are not source evidence. Non-code connector source nodes also stop offering `edit`, `apply_patch`, or `bash`, reducing the chance that Reddit collection nodes write artifacts without querying Reddit.
- **Connector delivery nodes stay focused on destination tools**: Notion publisher nodes with explicit `mcp.notion.*` tool allowlists no longer inherit generic workspace `read`/`glob` tools from upstream input refs or mutation tools from broad defaults. They still keep `write` for the required run-artifact receipt, and the engine now narrows prewrite MCP gating to only the concrete connector tools that have not yet run, steering save/report nodes from `notion_fetch` to `notion_create_pages` instead of looping on discovery.
- **Required-tool provider calls never send an empty tools list**: Write-required connector nodes now preserve the artifact `write` tool even when the session allowlist is connector-only. If later routing filters still leave no selected tools, Tandem downgrades the provider request away from `tool_choice: required` and omits the empty tools payload, preventing provider errors such as `Tool choice 'required' must be specified with 'tools' parameter.`
- **Transient provider stream decode failures retry in-place**: Provider stream read/decode failures such as `error decoding response body`, unexpected EOFs, and incomplete streamed responses are now classified as transient provider errors and retried inside the current provider iteration before the session is failed. Partial streamed text/tool-call state is cleared before retry, bounded by `TANDEM_PROVIDER_STREAM_DECODE_RETRY_ATTEMPTS`, and retry events are emitted as `provider.call.iteration.retry`.
- **Automation repair prompts now include calm attempt reviews**: Automation V2 attempt verdicts now include a `calm_teammate_v1` review with progress score, what worked, what is still needed, why it matters, and concrete next moves. Repair prompts lead with that review before raw expected/observed contract evidence so retries preserve useful progress instead of feeling like vague validation scolding.
- **Incident Monitor preserves attempt verdict and review chains**: Automation V2 failure events and Incident Monitor submissions now include recent attempt verdicts and attempt reviews when final failure reporting would otherwise only show the last provider/runtime error. This keeps actionable prior failures such as missing workspace files, missing connector calls, citation gaps, and required next actions visible in generated issue details.
- **Stale provider/session pauses auto-resume when repair budget remains**: Stale reaping still cancels dead sessions and marks the in-progress node as `needs_repair`, but it now auto-requeues stale-reaped runs by default while the node has remaining attempt budget. The recovery loop remains bounded by the existing auto-resume cap, and operators can opt out with `TANDEM_DISABLE_STALE_AUTO_RESUME`.
- **Chat live responses no longer disappear before refresh**: The control-panel Chat view now reloads the exact active session until the final assistant message is persisted before clearing the live thinking/streaming block. Streamed deltas are kept as a local fallback, so a completed answer does not leave an empty assistant slot until the operator manually refreshes.
- **Hosted Files no longer probes missing workspace routes**: Workspace file browsing now requires an explicit `workspace_files_api_available` capability before the Files page calls `/api/workspace/files/*`, preventing repeated 404s on deployments that expose managed files but not workspace browsing.
- **Chat prompt sends avoid visible active-run conflicts**: Before posting a new prompt, the Chat view now preflights and settles any active run for the session, and still uses the 409 conflict body as a fallback source for the exact run id to cancel. This avoids noisy `prompt_async` conflict requests when the session still has a stale active run.
- **Coder GitHub Project intake follows ACA launch lanes**: The control-panel Coder board now treats `Todo` / `TODOS` GitHub Project statuses as launchable, matching ACA's current intake rules. Planned GitHub tasks are published into the detected launch lane instead of assuming a `Ready` column, so agents can accept board jobs on projects whose actionable lane is named `TODOS`.
- **Control-panel running workflows no longer look stalled while sessions are active**: The Automation V2 running view no longer derives `stalled` for a run that still reports active sessions. Background-tab polling gaps now render as a softer "waiting on active session" detail, while the backend stale reaper remains the authority for real `stale_no_provider_activity` pauses.

## [0.5.4] - Released 2026-05-05

### Added

- **Workflow packs are now the default workflow sharing format**: Planner sessions can be exported as marketplace-ready `.zip` packs containing `tandempack.yaml`, `README.md`, an embedded workflow plan bundle, and an optional cover image. The Workflows page now prioritizes pack upload/preview/install and keeps raw JSON bundle import available as an advanced fallback.
- **Workflow pack import/export APIs and SDK helpers**: Added `/workflow-plans/export/pack`, `/workflow-plans/import/pack/preview`, and `/workflow-plans/import/pack`, plus TypeScript client helpers for exporting workflow packs and previewing/importing workflow pack ZIPs.
- **Hosted-safe workflow pack downloads**: Workflow pack exports now include a download URL, and the Workflows page renders a browser Download ZIP action so hosted users do not need filesystem access to retrieve generated packs.
- **Workflow pack provenance**: Imported workflow-pack sessions now retain pack identity and version metadata alongside the source bundle digest, making installed workflow origins inspectable after import.

### Changed

- **Pack manifest reference validation understands cover images**: Pack marketplace metadata can now reference `marketplace.listing.cover_image`, and workflow pack import previews render supported PNG, JPEG, and WebP covers with size/path validation.

### Fixed

- **Automation cron schedules preserve local wall-clock time**: Runtime scheduling now accepts the 5-field cron expressions emitted by the control panel and normalizes them for the server cron parser before computing `next_fire_at_ms`. Cron schedules are evaluated in the saved IANA timezone, with a Budapest weekday 9:00 AM regression test covering DST-aware wall-clock behavior.
- **Automation schedule UI carries timezone context**: Guided schedule summaries, creation review, workflow editing, automation calendar labels, and standup scheduling now display and save against the selected timezone instead of implying UTC. `Europe/Budapest` is now included in the common timezone picker.
- **Research-synthesis workflows no longer require unrelated workspace reads**: Final report/brief nodes that synthesize upstream MCP, Reddit, web, and run-artifact evidence no longer inherit `local_source_reads` as a hard requirement. This prevents concise research-to-Notion workflows from blocking with `research brief cited workspace sources without using read` when the workflow never needed repository source files.
- **Existing saved synthesis nodes tolerate stale read enforcement**: Runtime validation now treats stale `local_source_reads`/`read` requirements as advisory for `research_synthesis` nodes, while preserving strict `read` enforcement for code workflows, local research, and Incident Monitor/source-inspection tasks that genuinely require repo evidence.
- **Control-panel uploads use the global Tandem data directory**: Panel-managed uploads now prefer `$TANDEM_HOME/data/channel_uploads`, expand `~`, `$HOME`, `${HOME}`, and `%HOME%`, and normalize Windows-style separators on Linux/macOS so uploaded workflow pack images do not create stray literal `%HOME%\...` directories in the repo.

## [0.5.3] - Released 2026-05-03

### Changed

- **Automation V2 definitions are stored as per-workflow shards**: Saved workflow definitions now persist under `data/automations-v2/<automation-id>.json` with a small index instead of rewriting every workflow into one growing `automations_v2.json` aggregate. Existing aggregate files are migrated on load and archived as `automations_v2.legacy-aggregate.json`.
- **Generated workflow planning uses deterministic task-budget compaction**: AI-generated workflow plans now have a hard 8-step budget. Oversized planner output is compacted into request-aware macro steps before preview or chat-revision storage, preserving source/tool intent and destinations such as Notion collection ids instead of falling back to a generic `execute_goal` plan. Manual Studio workflows and explicit imports remain exempt.
- **Planner diagnostics expose task-budget status**: Preview/revision diagnostics now include `task_budget.max_generated_steps`, `generated_step_count`, `status`, and `original_step_count` when compaction occurs; the control panel surfaces messages such as "Planner compacted 29 generated tasks into 6 runnable workflow steps."

### Fixed

- **Connector-backed workflow nodes receive their actual MCP tools**: Natural node objectives such as "Use the connected Reddit MCP" now match hyphenated MCP server ids such as `reddit-gmail`, so generated research nodes request `mcp.reddit_gmail.*` instead of being offered only `mcp_list` and local file tools.
- **Research artifacts no longer self-block on connector limitations**: Artifact prompts and repair guidance now tell agents to record unavailable connectors or partial evidence under limitation fields while keeping finished JSON artifacts terminal (`status: completed`), preventing `artifact_status_not_terminal` loops that stop downstream workflow and Incident Monitor reporting.
- **Apply/session boundaries reject runaway generated plans**: `/workflow-plans/apply` and planner-session creation reject over-budget generated plans with `WORKFLOW_PLAN_TASK_BUDGET_EXCEEDED` if an uncompacted oversized plan reaches them.

## [0.5.2] - Released 2026-05-03

### Changed

- **Incident Monitor triage evidence is advisory, not report-blocking**: Automation V2-backed Incident Monitor triage still asks agents to search the configured repo and prefer concrete source `read` evidence, but missing or inconclusive reads no longer hard-block Incident Monitor's own inspection/research/validation/fix artifacts. This keeps GitHub reporting focused on the original workflow failure instead of recursively failing on `no_concrete_reads`.
- **Incident Monitor blocked triage can still publish fallback evidence**: Blocked Incident Monitor triage Automation V2 runs are now treated as terminal enough for fallback summary synthesis and GitHub publication, so issue drafts can preserve the real workflow failure even when triage cannot satisfy every evidence preference.
- **Generated compact research-to-destination workflows stay compact**: The workflow planner now recognizes concise research/report/save prompts, caps them around 5-8 leaf tasks, avoids splitting every report section into its own node, and rejects over-budget LLM plans in favor of a compact fallback.
- **Connector-backed inspection and research nodes get the long workflow budget**: Structured JSON nodes that inspect or fetch external sources such as Notion collections, Reddit, or web research now inherit the long-running workflow timeout instead of the generic 180-second structured JSON default.

### Fixed

- **Incident Monitor no longer masks workflow failures with its own source-read gate**: Triage artifacts now use artifact-only validation and preserve tool/search limitations in completed JSON instead of blocking issue publication when an agent searches but does not produce a concrete `read` receipt.
- **Notion collection inspection nodes no longer default to 3-minute timeouts**: Generated workflow nodes such as `inspect_notion_collection` that call external data sources now receive the long-running automation budget, reducing premature `automation node ... timed out after 180000 ms` failures.

## [0.5.1] - Released 2026-05-03

### Added

- **Incident Monitor external project log intake**: Added monitored-project/log-source config, deterministic JSON-lines and plaintext log parsing, persisted offset state, evidence artifact writing, storm control, and a background watcher that turns local external project log failures into Incident Monitor incidents without requiring a workflow to hold the full engine token.
- **Scoped Incident Monitor external report intake**: Added limited per-project intake keys plus `POST /incident-monitor/intake/report` so CI systems and external apps can submit normalized failure reports without access to the full engine API token.
- **Incident Monitor intake-key management APIs**: Added protected key list/create/disable endpoints under `/incident-monitor/intake/keys`, storing only key hashes and returning raw keys only at creation.
- **Incident Monitor log evidence artifacts**: Added state-managed `tandem://incident-monitor/...` evidence refs and JSON evidence artifacts for log candidates, including byte offsets, source ids, redacted excerpts, and fingerprints.

### Changed

- **Incident Monitor triage receives explicit repo-root inputs**: Automation V2-backed Incident Monitor triage nodes now carry the resolved `workspace_root` in node inputs and prompt guidance, making local source reads target the selected repo checkout instead of relying on implicit workspace context.
- **Incident Monitor setup explains hosted repo layout**: The control panel now shows a hosted path map for Incident Monitor, quick actions for `/workspace/repos/<repo>`, setup warnings for parent/runtime-state folders, and Coder sync hints so operators know which checkout Incident Monitor will inspect.
- **Incident Monitor triage is project-aware**: Triage run creation now prefers the linked incident or monitored project `workspace_root`, `model_policy`, and `mcp_server` before falling back to global Incident Monitor config, so external project failures are inspected in the correct repo/workspace.
- **Incident Monitor config supports monitored projects**: The existing single-project/global config remains compatible, while `monitored_projects` can now define external repos, workspace roots, log sources, and project policy.
- **Incident Monitor status exposes watcher health**: Status snapshots now include log watcher running state, enabled project/source counts, source health, offsets, file size, last poll/candidate/submission times, and source errors.

### Fixed

- **Incident Monitor research retries missing concrete reads more reliably**: Triage research now gets an additional repair attempt when it searches the repo but fails to perform the required concrete source-file `read`, reducing blocked `no_concrete_reads` demo failures.
- **External log paths fail closed**: Monitored log paths are validated under their configured workspace root, including symlink/absolute path escape rejection, before watcher polling.
- **Split log lines keep correct evidence offsets**: Partial trailing lines now preserve their starting byte offset so failures spanning polls produce accurate evidence ranges.
- **External project dedupe avoids cross-repo collisions**: Watcher-created incidents dedupe by `repo + fingerprint` instead of fingerprint alone.
