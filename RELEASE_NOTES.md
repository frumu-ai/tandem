# Release Notes

This is the canonical release-notes file used by release tooling.

## v0.6.10 (2026-07-14)

Tandem 0.6.10 turns Control Panel chat into a first-party product-authoring
surface. A user can describe an automation or multi-workflow process in normal
language, let the selected model plan it with Tandem's native tools, inspect a
durable workflow artifact directly in the conversation, revise it over several
turns, and materialize a disabled draft when it is ready. The authoring path
uses the signed-in user's existing Tandem identity, keeps external connection
credentials behind their normal provider boundaries, and applies the same
validation, permission, and confirmation policies as the rest of the product.

### Model-First, Authenticated Product Authoring

Product-authoring prompts now reach the selected model instead of stopping in
a repeated "did you mean" setup loop. Tandem distinguishes authoring,
explanation, inspection, and consequential control intent, then exposes the
appropriate first-party capabilities while leaving the model enough context to
ask useful questions only when information is genuinely missing.

New natural-language workflow and automation requests always begin in the
workflow planner rather than asking the model to synthesize a raw Automation
V2 definition. Disabled-draft authoring records external integrations as
requirements or blockers without discovering or executing MCP connector tools;
live integration inspection happens only when the user explicitly requests it.

The Control Panel session is the authentication boundary for these first-party
tools. Tandem derives tenant and actor identity from the trusted dispatch
session, ignores model-supplied identity fields, and records the verified
author on persisted changes. Users are not asked to provide a Tandem API key
inside Tandem chat. External MCP servers and services remain separate: they use
the user's or organization's principal-scoped connection, and their credentials
are never placed in the prompt or exposed to the model.

### Durable Authoring And Operator Tools

Chat can now drive the workflow planner lifecycle end to end: start and read a
plan, revise it, preview and validate it, inspect available capabilities, and
materialize it. It can also inspect and manage Automation V2 drafts, perform
authorized automation controls, and inspect orchestrations. Validation returns
structured assumptions, blockers, warnings, connection requirements, and
approval requirements instead of treating every tool response as success.

Planner sessions, revisions, chat/run provenance, and artifact links are
durable. Follow-up requests such as "revise it" resolve to an explicitly active
artifact when one is available, and ambiguous conversations do not silently
edit an arbitrary plan. Materialization is draft-first and disabled by default.
Supported consequential controls remain permission- and confirmation-gated,
while unsupported publish and enable requests are not exposed as chat actions.

The run path is designed for interruption. Prompt submission, planning,
revision, materialization, and automation mutations use durable idempotency so
retrying after cancellation or reconnect does not create duplicate drafts or
replay stale state. Conflicting revisions are rejected, cancellation stops the
active planner work, and the chat transcript retains the authoritative link
between the request, tool activity, persisted artifact, and acting user.

### Inline Workflow Artifacts

Created and revised workflows now appear as durable artifacts in the chat
transcript rather than requiring a navigation detour. Each artifact summarizes
the trigger, nodes, named transitions, outputs, approvals, execution
constraints, assumptions, required connections, and validation state. Parallel
branches are presented as concurrent work, so workflows with several nodes
running at once are not misrepresented as a sequential chain.

Users can validate, request another revision in chat, duplicate or create a
draft, and open the exact linked artifact and current revision in the full
workflow planner. Revisions update the artifact in place while preserving the
earlier conversation, and streaming tool events show running, completed,
failed, canceled, and approval-wait states without remounting the artifact.
Stable layout and retained transcript position remove the flashing and viewport
jumps that made earlier updates difficult to follow, including on narrow
viewports and keyboard or screen-reader paths.

### Automation Wizard Reliability And Live Progress

Long-running workflow generation now reports live progress in the automation
wizard. The progress panel identifies whether Tandem is dispatching, waiting,
receiving a response, retrying, or validating the plan, and shows the selected
provider and model, elapsed time, and received response size. These events are
tenant- and request-scoped so concurrent tabs cannot consume one another's
progress, and deliberately exclude prompts, reasoning text, and response
content so users can see that planning is active without exposing model data.

Planner normalization now turns every upstream `input_ref` into an explicit
scheduling dependency before validating the workflow graph. This repairs a
common model-output mismatch that previously produced a fallback scaffold even
when the intended workflow was otherwise valid. When planning still fails, the
Review step presents the concrete diagnostic, states that no automation was
created, and visibly blocks the Create action until the plan is repaired.

Creating an automation from an approved plan is now retry-safe across protected
audit failures. If the mandatory audit record cannot be persisted, Tandem rolls
back the provisional automation and governance record and releases the
idempotency reservation before returning a retryable error that explicitly says
the operation was not applied. The audit reader also recovers valid chains from
legacy files where concurrent appenders placed multiple complete JSON objects
on one physical line, while malformed or truncated records still fail closed.

### Product-Authoring Acceptance Gate

A dedicated acceptance suite protects the conversation-to-artifact contract.
It covers vague, partial, and detailed requests; scheduled and parallel
workflows; follow-up revisions and active-artifact references; explain-only
versus create intent; validation, connection, overlap, and provider failures;
supported control confirmations and honest publish/enable capability gaps;
permission denial; cancellation, retry, and
reconnect; and attempts to disclose credentials or cross tenant boundaries.

The gate checks authoritative outcomes as well as assistant wording: selected
tools and policy decisions, persisted plans and automations, revision
continuity, audit actors, draft-first defaults, tenant isolation, parallel plan
structure, idempotent replay, and the absence of unauthorized side effects. It
also fails when chat intercepts a valid authoring prompt before model execution,
claims success without a corresponding persisted artifact, or asks for an
unnecessary internal Tandem credential.

## v0.6.9 (2026-07-12)

Tandem 0.6.9 delivers the long-running workflow orchestration layer end to
end: versioned orchestration graphs that connect Automation V2 workflows
through governed named transitions, durable goals that survive restarts and
run for months, public durable wait nodes, a transactional SQLite stateful
store, public authoring and runtime APIs with typed TypeScript/Python clients
and governed MCP tools, a visual drag-and-drop Orchestration Studio, live goal
monitoring with replay and operator actions, and enterprise scope, artifact,
and authority enforcement with hosted-KMS sealing. It also makes the core
transactional and portable end to end: sessions, runtime events, and the
stateful runtime now live in crash-safe transactional stores with the legacy
JSON/JSONL sidecars retired, and the stateful store gains a pluggable backend
with an opt-in PostgreSQL implementation alongside the production PostgreSQL
memory backend on the portable `MemoryStore` abstraction. Finally, it proves
the five-profile ACME Slack governance flow on the production path and locks
the Control Panel and orchestration runtime behind mandatory CI gates.

### Long-Running Workflow Orchestration Kernel

Orchestrations are now first-class, versioned definitions: workflow, wait, and
terminal nodes wired by named transition edges that carry artifact contracts
and optional approval boundaries, governed by per-goal policies. Drafts are
mutable and validated; publishing snapshots an immutable version with pinned
workflow definition hashes. Durable goals execute a published version across
many Automation V2 runs with hop-indexed lineage.

The stateful runtime now lives in an embedded transactional SQLite/WAL store —
definitions, runs, goals, run links, handoffs, waits, events, snapshots, and
reliability records — populated by a once-only atomic legacy import, after
which the old JSON/JSONL sidecars are retired: retention sweeps archive them
to a backup directory, and dual-write mirrors are opt-in diagnostics
(`TANDEM_STATEFUL_RUNTIME_COMPATIBILITY_MIRRORS_ENABLED`, default off).
Governed
transitions are atomic: one transaction validates the source run, edge,
artifact contract, scope, authority, idempotency key, hop policy, and pinned
target version, then persists the handoff and provenance, creates the
downstream run with its actual run ID, records lineage and events, and marks
the handoff consumed. Exactly-once behavior holds under concurrent scheduler
races and crash injection at every write boundary.

Goal policies enforce hop limits, deadlines, and token/cost budgets with
pause-for-review or fail semantics; terminal outcomes are explicit; completed
workflows with no matching transition settle into awaiting-transition instead
of silently succeeding; and cancellation propagates to the active run,
pending waits, and claimed handoffs. Public durable wait nodes (timer,
approval, correlated webhook, external condition) run through the checkpoint,
scheduler, webhook, and restart paths with leased claims and idempotent wakes.

### Public APIs, SDK Clients, And MCP Tools

The authoring surface exposes draft create/list/get/update/archive with
optimistic concurrency, validation with referenced-workflow checks,
publishing, version history, stale-reference reporting and refresh, and
dry-run transition previews. The runtime surface exposes idempotent goal
start (atomic with the root run), lifecycle controls, the goal graph and run
lineage, durable events with replayable cursors, an SSE change stream with
`Last-Event-ID` reconnect, governed handoff emission and decisions, workflow
completion settlement, wait inspection and resolution, artifacts, and
budgets.

TypeScript gains `client.orchestrations` and `client.statefulRuntime`; Python
gains `client.orchestrations` and `client.stateful_runtime`. Ten governed MCP
tools let agents drive the full loop — create/validate/publish, goal
start/get/cancel, handoff emit/approve, wait inspect/resolve — with
fail-closed authority, owner checks, and idempotent replay backed by a sealed
request ledger.

### Visual Orchestration Studio And Live Operations

The Control Panel gains an Orchestrations Library with lifecycle filters and
a Studio canvas: drag Automation V2 workflows from a searchable palette, wire
named transitions with artifact contracts and approval boundaries, configure
waits, terminals, and budgets through typed inspectors, and rely on visual
validation that badges affected nodes and edges and blocks publishing invalid
graphs. Dry-run previews, draft-vs-published comparison, immutable version
history, and a safe refresh/republish flow round out authoring, and the whole
path is usable without drag-and-drop: searchable add controls, form-based
edges, keyboard movement, an ordered outline editor, responsive layouts, and
screen-reader names throughout.

Started goals reuse the same canvas as a read-only live graph with semantic
node states, current budgets, and workflow-stage drilldown. The view stays
current over SSE with reconnect recovery and sequence-gap detection, supports
deterministic historical replay with a timeline scrubber, and exposes
governed operator actions — approve/reject, resolve external conditions,
pause/resume/cancel, retry, and recovery plans — that refresh through durable
events rather than optimistic status.

### Enterprise Scope, Artifact Policy, And Recovery Hardening

Tenant scoping is now a store-level invariant: goal, lineage, handoff, and
event reads carry org/workspace/deployment predicates in SQL, so cross-tenant
IDs are indistinguishable from absence even if an entrypoint check is
skipped. Approvals require `orchestration.approve`, wait resolution requires
`orchestration.resolve_wait`, authoring and goal mutations enforce
owner-or-admin with a recorded, non-writable `created_by`, and audit events
record the effective actor with run-as context. Referencing a workflow whose
delegation authority lapsed blocks validation and publish.

Artifacts pass an admission policy at every emit surface: bounded inline
values and metadata, workspace-relative content paths with traversal and
symlink escapes rejected on canonicalized paths, and digests verified against
the actual file. The MCP tool-replay ledger is sealed with tenant-scoped KMS
envelopes — ciphertext at rest, fail-closed without the key, plaintext
passthrough for local-first deployments.

Recovery hardening completes the store cutover: legacy file handoffs import
exactly once with quarantine for corrupt, foreign, conflicting, and
workspace-escaping envelopes plus a durable migration-attempt journal;
retention is snapshot-aware so compaction never removes a run's replay tail;
snapshots prune with a keep-last-N floor; sweeps run periodically with WAL
checkpointing; every connection applies `synchronous=FULL`; and the engine
lock records its owner with liveness diagnostics for stale-lock recovery.

### Long-Horizon Proof, Guides, And CI Gates

A production-path E2E proof drives Goal → Plan → Execute → Verify → Replan
across roughly 180 virtual days with timer, approval, webhook, and
external-condition waits, bounded budgets, and an explicit final artifact.
The canonical Long-Running Multi-Workflow Goals guide and a stateful workflow
guide document the model for humans and MCP-connected agents. CI now mandates
the orchestration runtime and store suites, TypeScript/Python SDK contracts,
MCP tool tests, Control Panel typecheck/build/unit contracts, Playwright
journeys (including drag/drop, keyboard, mobile, live graph, reconnect, and
replay), accessibility and theme checks, and docs parity.

### Transactional Core Storage And Pluggable Stateful Backends

The move off ad-hoc JSON files is complete across the core. Sessions, session
metadata, snapshots, and interactive questions now persist in a transactional
SQLite store in the engine core, populated by a once-only atomic import of
the legacy JSON files — source digests are recorded inside the import
transaction, so a restart can never import twice. The durable runtime event
log moved onto an indexed transactional store of its own, with a one-time
JSONL import, tenant-scoped windowed queries, retention pruning, and a scale
benchmark guarding write/query performance. And once the stateful runtime's
one-time migration completes, the legacy JSON/JSONL sidecars are retired
outright — retention sweeps archive them to a backup directory, and
dual-write compatibility mirrors are opt-in diagnostics
(`TANDEM_STATEFUL_RUNTIME_COMPATIBILITY_MIRRORS_ENABLED`, default off), so
production runtime state has exactly one writer of record.

The stateful orchestration store's persistence is now pluggable. A neutral
execution facade separates the store's domain logic (idempotency, tenant
scoping, sealing, transactional invariants) from statement execution, with
backend selection at startup: `TANDEM_STORAGE_BACKEND=sqlite` (default) or
`postgres` with `TANDEM_STORAGE_POSTGRES_URL`, validated fail-closed — an
unknown backend or missing URL is a startup error, never a silent fallback.
Builds gate the backends with `storage-sqlite` (default-on) and
`storage-postgres` cargo features.

The PostgreSQL backend preserves the invariants the SQLite store proves:
store SQL is translated at execution time, `BIGSERIAL` `rowid` columns keep
durable SSE `Last-Event-ID` cursors monotonic and gapless, each runtime root
maps to its own PostgreSQL schema through a sticky marker file so multiple
roots share one database safely, a session-level advisory lock extends the
engine lock across hosts, and immediate write transactions serialize through
a schema-scoped advisory lock, reproducing SQLite's single-writer semantics
under concurrent schedulers. KMS-sealed records round-trip unchanged. A
backend conformance suite runs the same scenarios — exactly-once governed
transitions, idempotent replay under commit races, tenant scoping, snapshot
retention, cursor ordering, and engine-lock exclusivity — against every
compiled backend, and CI exercises it against a real PostgreSQL service
container. See `docs/POSTGRES_STATEFUL_STORAGE.md` for configuration and
scope notes.

Operators can now move authoritative state between those backends with
`tandem-engine storage migrate`. The offline command locks the source,
transfers rows in bounded batches, preserves event cursors, generated
reliability ordering IDs, idempotency ledgers, and KMS-sealed values verbatim,
then compares typed SHA-256 fingerprints and row counts before marking the
target complete. A durable transfer journal keeps the source authoritative,
makes a partially populated target unusable by normal startup, and allows a
restart to finish an interrupted transfer without duplicating data. When a
different target state directory is requested, the direct-SQLite session and
runtime-event sidecars are copied through consistent SQLite snapshots and
verified alongside the primary store.

The backend contract is now a required CI surface rather than a best-effort
build. SQLite-only and PostgreSQL-only conformance and scale suites run against
their real backends, the migration path round-trips SQLite to PostgreSQL and
back, feature combinations compile independently, and PostgreSQL-only clippy
guards dialect drift. Startup locking was tightened at the same time: SQLite
takes its filesystem lock before any database initialization, and PostgreSQL
takes both the local guard and schema advisory lock before schema work. The
storage operations guide covers selection, provisioning, TLS/secrets,
migration, retention, maintenance, locking, and backend-specific backups.

### Memory Storage Portability And PostgreSQL

The portable `MemoryStore` abstraction is complete, including private
(subject-scoped) memory, and a production PostgreSQL/pgvector backend ships
with protected search surfaces, scoped consolidation, owner-subject-bound
decrypts, and hardened failure handling. Channel memory consolidation is now
tenant- and subject-scoped and atomic, preserving source ownership.

### Governance Proof, Release Gates, And Test Stability

Rust CI is leaner without reducing its required coverage. Cargo artifacts are
shared by runner OS and feature profile, `sccache` reuses compiled dependencies,
compatible Ubuntu server and PostgreSQL gates build once, and storage-only
changes skip unrelated cross-platform engine matrices. The full workspace and
backend contract suites still run for storage changes.

The five-profile ACME Slack governance demo now runs on the production path:
signed Slack events drive real governed sessions, decisions persist as
governance receipts, and the Control Panel surfaces them in the Slack-request
receipt view. Control Panel releases are gated by required Playwright
journeys and standardized UI contracts (routes, icons, loading, typography,
accessibility), which also flushed out fullscreen and spinner fixes. The
nextest quarantine is empty again — its deterministic async stream
regressions are fixed and a policy guard keeps future quarantine entries
owned and expiring. Governed runtime boundaries (signed Slack ingress and
recovery, canonical provider egress, protected KMS/audit persistence,
tenant-safe OAuth refresh, tenant-qualified routines, and Linux enterprise
release validation) hardened the first remediation milestone. Engine releases
now build the enterprise engine with embeddings, and panel dependencies pin
to published npm versions.

## v0.6.8 (2026-07-09)

Tandem 0.6.8 is a hosted-v1 readiness follow-up. It hardens hosted session
isolation, completes the native Linear webhook path and Webhooks hub, expands
memory isolation across tenant/org/user boundaries, and adds hosted-KMS
per-scope memory encryption at rest with department as a cryptographic key
dimension. It ships a department-scoped Slack governance demo harness,
extracts governance decision logic into the BUSL governance engine, encrypts
the file-based governance stores at rest, and lays the storage-portability
groundwork (a `MemoryStore` trait and a PostgreSQL/pgvector design). It also
enforces `retention_days` with real reapers, stabilizes the tandem-server test
path used by CI and local full-suite runs, settles the 0.6.8 BUSL/open-core
licensing posture, removes a production-linked GPL dependency, and updates the
OpenAI Codex catalog to the documented GPT-5.6 preview model ids.

### Linear Webhooks And Webhooks Hub

Automation V2 now supports direct Linear-to-Tandem webhook delivery without a
bridge or unsigned dev mode. Linear triggers use the native
`linear-signature` HMAC-SHA256 scheme over the exact raw request body, validate
the signed `webhookTimestamp` against the replay window, and fail closed with
`provider_secret_not_imported` until an operator imports the Linear-owned
signing secret.

Provider-owned secret lifecycle is now explicit: admins can import or re-import
the Linear signing secret through the management API, re-import rotates the
tenant-scoped material and retires the old version, and Tandem-generated
secret rotation is refused for provider-owned triggers. Trigger responses
surface the verification lifecycle from `awaiting_secret` through active
verified delivery, and the TypeScript client exposes
`importWebhookProviderSecret`.

The Control Panel now has a dedicated Webhooks hub with Linear setup UX. The
guide and internal docs cover Linear verification, secret import, rejection
reasons, SDK/API usage, and troubleshooting. Rejected Linear events now produce
rejection evidence without entering the runnable inbox, while verified Linear
deliveries dedupe by `linear-delivery` id.

### Memory Isolation And Eval Gate

Memory context-tree endpoints and storage are now tenant-scoped. Context URI
resolution, tree traversal, layer generation, and layer writes all use the
request tenant; foreign-tenant nodes behave like missing nodes, so callers do
not get a cross-tenant existence oracle. The store schema now allows different
tenants to own the same context URI independently.

Ordinary tenant-local memory reads now enforce org-unit ownership. Records can
carry an `owner_org_unit_id`, read filters receive the caller's verified
org-unit memberships, and `memory_put` refuses to stamp ownership for an org
unit the writer does not belong to. Source-bound and knowledge-scoped records
continue to flow through the enterprise grant model.

Vector memory chunks now include a per-user subject dimension, and archived
chat exchanges are stamped with the session actor so one user's historical
conversation chunks do not become a tenant-wide recall pool. Prompt injection,
memory list, and memory search paths thread the resolved caller subject through
the governed filter, while imports, consolidation, and model-tool writes remain
shared by design. Unbacked `team` and `curated` write/promote paths now fail
closed instead of accepting tier labels with no storage backing.

A new cross-user memory isolation eval dataset, baseline, CI regression gate,
and HTTP/app-state test matrix cover tenant, subject, DM/group, distillation,
forged project, and org-unit boundaries. This gives the memory scoping work a
standing regression signal instead of relying only on per-feature unit tests.

### Hosted Runtime Isolation

Hosted sessions now enforce actor scope on command routes, including
`/session/{id}/command`, and hosted mode fails closed when actor context is
missing. Local desktop and development flows keep compatibility for implicit
sessions, but hosted deployments now have explicit coverage for cross-actor
session access attempts and assertion-verification edge cases.

### Licensing And Compliance

The BUSL Additional Use Grant for Tandem BUSL components now allows internal
production use for any organization regardless of revenue. A commercial license
is required to provide the licensed work, or a substantially similar product,
to third parties as a managed, hosted, SaaS, white-label, embedded, or other
commercial offering. The BUSL Change Date policy is now rolling: each release
stamps BUSL components to release date + four years, after which that version
converts to the existing Change License.

`tandem-enterprise-server` is now licensed under `BUSL-1.1`, matching the
commercial enterprise layer already used by the governance and plan-compiler
crates. The permissive `@frumu/tandem-enterprise` installer stays permissive,
but its README now discloses that downloaded enterprise binaries contain
BUSL-licensed components. The repository license map now covers all Rust
workspace members, published npm packages, and Python package metadata, with a
CI guard that fails when the map and manifests drift.

The browser/tooling markdown conversion path no longer ships the GPL-3.0
`html2md` dependency. It now uses Apache-2.0 `htmd`, preserves iframe embed
URLs as markdown links, and keeps the remaining `auto_generate_cdp` GPL
exception documented as build-only code that is not linked into distributed
artifacts.

### Provider Catalog And Desktop Healing

The OpenAI Codex provider catalog now exposes the documented GPT-5.6 preview
models as explicit `gpt-5.6-sol`, `gpt-5.6-terra`, and `gpt-5.6-luna` entries
instead of the generic `gpt-5.6` placeholder. Persisted bare `gpt-5.6`
defaults and stale desktop selected-model dispatches now heal to the compiled
`gpt-5.5` default, avoiding startup or dispatch failures from a model id that
is no longer valid.

### Test And CI Stability

The tandem-server suite now serializes environment-mutating state construction,
cleans up process-wide test overrides, and documents local nextest recipes that
match CI's profile, feature flags, stack size, and isolated `TANDEM_HOME`.
This addresses the class of tests that passed in isolation but failed under
full parallel execution.

### Memory Encryption At Rest (Hosted KMS)

Hosted deployments can now encrypt memory at rest under per-scope,
KMS-wrapped data-encryption keys. Each write generates a fresh DEK, encrypts the
field with AES-256-GCM, wraps the DEK with the scope's key-encryption key
(binding it to the scope), and stores the resulting `tce1:` ciphertext alongside
an unencrypted envelope in a new `crypto_envelope` column. On read, a decrypt
broker authorizes every unwrap against the requesting principal — tenant, data
class, and source-binding grants — and the key's lifecycle state before the DEK
is recovered from an envelope-keyed cache or a KMS unwrap. Chunk content and
metadata and context layers all flow through this path.

Department (`org_unit`) is now a cryptographic key dimension, not just an
access-control one: data collected by different departments in the same tenant
and data class no longer shares a DEK, so a raw database dump cannot decrypt one
department's rows with another's key. `tandem-server` projects a request's
verified tenant context into a memory decrypt principal — gated on the caller's
strict resource scope — and threads it into the hosted context-layer reads, so a
sealed row decrypts only for a caller authorized for its scope.

Single-tenant and local instances are unaffected: with no KMS provisioned, the
`crypto_envelope` column stays NULL and read/write behavior is unchanged. The
system fails closed when the scope, principal, or envelope data is missing or
invalid. The searchable FTS index and vector embeddings remain plaintext by
design (they cannot be encrypted without breaking search); that residual at-rest
exposure is documented as an accepted, mitigated decision.

### Department-Scoped Slack Governance Demo

A new signed Slack Events ingress endpoint turns an inbound Slack message into a
governed session prompt under real signature verification. On top of it,
Tandem now ships a department-scoped governance demo: an ACME data
set tagged by department and data class, a small tool set tagged with risk
tiers, five requester profiles (Sales, Engineering, Finance, Leadership, and an
external contractor), and a fixture-driven demo harness that replays one prompt
across all five to show how reachable memory, offered tools, policy decisions,
approvals, redactions, and receipts diverge by requester. The harness validates
the governance-receipt shape over the seeded dataset rather than executing a
live governed run; the production-path end-to-end demo is tracked as follow-up
work. The Control Panel adds a Slack-request governance receipt view that
stitches a run into a single requester → memory → tools → decisions →
approvals → status receipt.

Ordinary tenant-local memory reads now enforce department ownership:
`owner_org_unit_id` is a first-class, indexed, Postgres-portable column that also
scopes vector search, the active department is stamped onto every ingestion path
from the verified context, and department-unscoped records are fail-closed to
department-scoped callers unless explicitly marked `tenant_shared` or owned by
the calling subject (the writer's own `private`/subject-scoped memory). An opt-in
`private` flag additionally restricts a record to its collecting user on top of
tenant + department. A cross-department isolation matrix and eval cases guard
the model.

### Governance Engine, Encrypted Stores, And Storage Portability

Governance decision logic moved out of `tandem-server` into the BUSL governance
engine, and enterprise routes plus governance now ship in every standard release
artifact. The file-based audit, policy-decision, and org-unit/grant stores are
encrypted at rest behind a governance store abstraction without breaking the
audit hash chain. As groundwork for a PostgreSQL backend alongside SQLite, a
`MemoryStore` trait with scope types and chunk operations, and a
PostgreSQL/pgvector portability design, now decouple the memory layer from
`rusqlite`.

### Retention And Channel-Memory Hygiene

`retention_days` is now enforced by real reapers that delete expired rows and
record a cleanup log, rather than being advertised without teeth. Automatic
archiving of channel exchanges is removed and runtime tool-output capture is
opt-in, so agent-collected memory is not silently accumulated. Coder and channel
memory retrieval is scoped to the tenant and channel subject (with coder-candidate
garbage collection), retrieval-gateway channel subjects are bound, and event
ingestion fails closed when it cannot attribute a run context.

## v0.6.7 (2026-07-05)

Tandem 0.6.7 completes the secure data-boundary foundation and ships a focused
channel reliability pass for Telegram, Discord, and Slack. The data-boundary
work now spans the standalone policy/evidence crate, runtime provider-gate
integration, enforcement actions, protected audit records, source-side guards,
and an admin monitoring read model. The channel work fixes the recent
production failure modes around Codex OAuth refresh, retired provider defaults,
partial channel configs, false-positive auth-error sanitization, and
cross-user/cross-scope memory isolation.

### Secure Data Boundary

The new `tandem-data-boundary` decision engine now evaluates sensitive-data
findings against provider boundary policy without persisting raw prompt text,
tool output, secrets, customer data, or model output. Decisions, findings, and
runtime events carry only audit-safe evidence such as classes, counts, spans,
reason codes, payload hashes, policy fingerprints, provider/model metadata, and
tenant/workspace/deployment refs.

Runtime integration is now available behind `TANDEM_DATA_BOUNDARY_MODE`
(`off`, `audit`, or `enforce`). In audit mode the engine evaluates the fully
assembled provider request immediately before dispatch and records
`data_boundary.*` events plus consequential protected-audit entries without
blocking or mutating requests. In enforce mode the gate can block dispatch,
redact/tokenize matched spans, require explicit operator approval, or fail
closed when strict posture requirements are not met. Provider classification is
configurable through `TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES`, with built-in
local defaults for loopback providers and strict fail-closed behavior for
unclassified or prohibited providers when enabled.

Data-boundary coverage also now observes source material before it becomes
provider context: tool/MCP results, prompt-context-hook injections, and
workflow email-delivery artifacts emit audit-safe `data_boundary.evaluated`
events when findings exist. A new admin-gated
`GET /audit/data-boundary/monitoring` endpoint aggregates protected-audit
records by tenant, provider, model, boundary class, action, sensitive class,
source kind, classification source, policy fingerprint, and repeated payload
hashes. Planning docs define the future policy UI/API and local-routing
contract without adding a second enforcement path.

### Context And Memory Safety

Long tool-heavy sessions now demote stale historical tool invocations in
provider-facing chat history to concise summaries with provenance handles,
while preserving recent invocations and leaving stored session records
untouched. This reduces repeated provider context load and adds telemetry for
demoted invocation count and saved characters.

Channel memory isolation was tightened across the write, read, and tool paths.
Channel prompt requests now include a platform-and-sender memory subject so one
Telegram/Discord/Slack user no longer shares the generic `default` global
memory subject with every other bot user. Channel memory tools now trust the
engine-injected session/project scope instead of model-supplied overrides, and
channel models can no longer write global-tier memory. Governed run-memory
ingestion now preserves tenant context on user/assistant/tool records, and
`memory_delete`/`memory_demote` enforce subject ownership before mutating
global memory records outside local unrestricted mode.

### Channel And Provider Reliability

OpenAI Codex OAuth credentials managed by `codex-cli` can now refresh in
process from the persisted refresh token. The session run retry path forces a
refresh and retries once after an `AUTHENTICATION_ERROR`, and refresh failures
publish internal reauth signals instead of only surfacing raw channel errors.
Web-served control panels now derive the Codex OAuth callback from the public
panel URL or forwarded origin when available, while preserving localhost
callbacks for local desktop flows.

The Codex model catalog removed the phantom `gpt-5.1-codex-max` entry, added
`gpt-5.6`, and now heals a persisted default model when it no longer exists in
the supported catalog, falling back to the compiled `gpt-5.5` default instead
of failing every channel run. Channel startup now tolerates saved Slack,
Discord, or Telegram entries that are partially configured or redacted, filters
incomplete listeners before startup, and reports connection state from runnable
listener config instead of merely from saved channel entries.

Channel auth-error sanitization is now more precise: real leading
`ENGINE_ERROR: AUTHENTICATION...` replies are still hidden from Telegram,
Discord, and Slack users, but ordinary explanatory responses that merely
mention `AUTHENTICATION_ERROR` or `ENGINE_ERROR` are no longer replaced with the
generic "assistant temporarily unavailable" message.

### Setup And Release Tooling

Setup no longer lets repo-root `.env.example` hijack machine-specific engine or
control-panel state directories. Example values for `TANDEM_STATE_DIR` and
`TANDEM_CONTROL_PANEL_STATE_DIR` are ignored during bootstrap, while real
overrides from the user's own `.env` or existing generated env file still win.
This prevents Linux deployments from accidentally writing state into a literal
Windows-style `%HOME%\...\.bench-state` path.

The release-note extractor now refuses to publish notes that still say
`Unreleased` or long unstructured walls of text without headings/bullets, so
GitHub release bodies fail closed instead of shipping unreadable generated
notes.

## v0.6.6 (2026-07-04)

Tandem 0.6.6 fixes a production reliability gap in channel messaging: Telegram,
Discord, and Slack replies (along with automations and scheduled runs) could
fail with `AUTHENTICATION_ERROR` once the OpenAI Codex OAuth access token
expired, because the token was previously only refreshed as a side effect of
the control panel polling its own status endpoint. An engine-side background
task now keeps that credential fresh on its own schedule, and any run that
still hits an expired token refreshes and retries once transparently before
surfacing an error to the channel. Channel status reporting was also
corrected so a connected listener no longer shows as disconnected from a
stale boot-time snapshot, and a config parse failure that used to silently
disable every channel now logs loudly instead.
This release also continues hardening the stateful automation runtime:
retrying a dead-lettered tool effect now actually re-executes it through the
normal governed dispatch path instead of only recording intent, a new
compensation execution engine drives `compensate_pending_effects` recovery
choices through an auditable runtime path with idempotent completions, and a
startup-recovery sweep replays durable-wait wakes that could previously be
lost if the engine crashed between finalizing a wait and requeuing its run.
Webhook intake retention no longer runs a synchronous full-tenant prune
inline on every request (now bounded by a hard event cap and handled by the
existing background reaper), and an early-arriving correlated webhook now
replays against a wait registered just after it arrives instead of orphaning
its run. Three security hardening passes landed alongside this work:
knowledge-scope memory governance now fails closed for records missing
scope metadata, stateful runtime tenant scoping was tightened across
approvals and MCP phase tool authority, and unsigned dev-mode webhook
signatures can no longer be enabled outside local single-tenant mode
regardless of the opt-in flag.
The Control Panel received a substantial design-consistency pass: a real
`Icon` component replaces the previous DOM-scan/MutationObserver icon
mechanism that could drop or duplicate icons under Preact's diffing;
hardcoded colors that broke the Porcelain light theme (most visibly,
near-invisible panel headings) and an off-theme calendar were routed through
theme tokens; border-radius, caption/micro text sizes, and dead glow/glass
CSS were consolidated onto single mechanisms; the sidebar's 19 routes are now
grouped into labeled sections; Enterprise Admin's six creation forms moved
into on-demand drawers; and a dismissible setup banner replaced the floating
onboarding modal. Several smaller fixes round out the pass: the chat right
rail is now reachable on tablet/small-laptop widths via a drawer, a blocked
automation status no longer renders as success-green, first paint no longer
flashes the wrong theme, and a handful of icon-only controls gained
accessible names.

## v0.6.5 (2026-07-03)

Tandem 0.6.5 completes the Incident Monitor production-governance suite —
adversarial scenario packs, governance maturity metrics, a continuous
reassessment scheduler, and fail-closed destination publishing — adds native
Notion webhook support to Automation V2, and starts the stateful agent
runtime work with enterprise-aware durable run scope, event, and snapshot
foundations. Snapshot-backed automation runs now expose stable definition
versions and `sha256:` snapshot hashes for future replay and resume checks, and
restart-interrupted Automation V2 runs are queued for resume when their
persisted checkpoint is recoverable while corrupt in-flight records continue to
fail closed. Automation V2 retry handling now also has a shared policy schema
and structured retry decision record so node failures can explain retryability,
attempt budget, terminal behavior, and next retry timing while preserving legacy
`max_attempts` compatibility.
The new `tandem-data-boundary` crate establishes the secure data-boundary
foundation for future provider-routing and context-governance work. It defines
serializable policy, input, finding, decision, and event contract types that
carry provider/model metadata, tenant/workspace/deployment refs, payload hashes,
policy fingerprints, reason codes, action tags, and finding counts without
embedding raw prompts, tool results, secrets, customer data, or model outputs.
It now also includes deterministic local detector findings for common PII,
financial, credential, secret, private-key, AWS-key, high-entropy, and simple
PHI marker spans, with evidence hashes plus redaction/tokenization placeholder
maps that preserve prompt structure without persisting raw matched values.
Automation V2 run claims are now persisted with lease metadata, and
expired launch claims without active session or agent handles are reclaimed back
to the queue so only one executor can safely resume the run.
Automation V2 webhook intake now records durable tenant-scoped inbox events and
idempotency keys at the edge and then verifies, dedupes, and queues runs
asynchronously from that inbox, reporting accepted/duplicate/conflict dedupe
outcomes on delivery records and SDK types and keeping original delivery/run
correlation available after restarts so provider retries do not fan out
duplicate automation runs.
Durable stateful runs now carry explicit workflow phases, transition history,
and allowed next phases so future long-running automation APIs can resume,
pause, and inspect runs through a guarded state machine instead of ad hoc
status strings. Legacy stateful runtime snapshots that predate those phase
fields derive a compatible phase state from their stored status when read.
Automation webhooks now persist tenant-scoped raw inbox events with raw payload
pointers, body/header digests, redacted header previews, and delivery/run
correlation for accepted, rejected, and duplicate intake paths.
Automation webhooks now use a provider-aware signature verification registry
with queryable delivery verification metadata, keeping Tandem HMAC compatibility
while preparing GitHub-style and shared-secret provider schemes.
Durable wait foundations now persist timer/webhook/approval wait metadata,
tenant-boundary identity, wake times, timeout policy, and wake claim state for
future sleep/resume scheduling.
The Automation V2 executor now runs a durable stateful wait scheduler tick that
claims due waits, recovers missed timer wakeups after downtime, records
idempotent wake/timeout events and snapshots, and marks timeout cancellations
or escalations for operator visibility.
Timer and webhook wait completions now reserve only the active leased claim
before durable wake writes, then terminalize the wait with the locked per-run
event sequence after those writes finish so concurrent completions cannot race
into duplicate sequence numbers.
Stateful runtime persistence now hardens wait, reliability, snapshot, and event
logs for crash recovery: JSON store mutations fail closed by sidelining corrupt
files, atomic writes sync temp files before rename, and event-log appends repair
torn JSONL tails before writing the next durable event.
Enterprise policy inheritance now preserves compliance floors before runtime
wiring: ancestor deny and approval-required rules block weaker descendant rules
unless explicitly marked overridable, and tenant/org-unit/workflow/phase scope
IDs are normalized for matching so case drift cannot suppress deny rules or hide
stateful runtime org-unit summaries and active grants.
Runtime policy decisions now consume that resolver directly: the server loads
`enterprise/policy_rules.json`, resolves each recorded policy decision through
the enterprise inheritance model across every recorded data class, enforces the
resolved result in gate, authority, fintech protected-action receipt, MCP
preflight, and memory promotion helpers, and stores inherited sources for replay.
Knowledge-scope memory governance now fails closed for source-bound gaps:
workflow-phase retrieval requires registered source-bound scope metadata, and
source-bound memory writes or promotions are blocked unless the derived memory
carries explicit `knowledge_scope_registry` policy metadata that matches the
source resource, source binding, and data class being written or promoted.
Source-bound manual imports now stamp imported chunks with that matching
registry metadata so governed workflow-phase reads can authorize imported
source chunks instead of hiding them as unregistered source-bound memory.
Promotion checks also preserve that source-resource registry shape when
validated source-binding metadata is present, while authority-only scope claims
still require a `source_binding` registry resource.
Automation V2 runs now also bridge those durable waits back into the live run
store: approval gates register and complete stateful approval waits, while
timer and webhook wait wakes requeue the authoritative automation run so the
executor can resume after a persisted wake event.
Stateful runtime event and snapshot read endpoints are now available for
tenant-filtered replay/debug and future control-panel views.
Stateful runtime run list and detail endpoints now provide canonical
tenant-filtered run records with current wait, latest event, latest snapshot,
and replay-boundary metadata, and the control-panel run list prefers those
endpoints while retaining the legacy fan-out fallback.
Canonical stateful runtime run responses now also include enterprise scope
summaries with resolved organization units, active org-unit grants, visible
knowledge source bindings, and filters for org unit, owner, resource, policy,
data class, risk tier, delegation grant, and source binding. The Control Panel
run dashboard surfaces those fields as scope metrics, filters, and per-run scope
cards.
Automation V2 lifecycle transitions now project into the authoritative
stateful runtime event and snapshot stores. Each newly recorded lifecycle
boundary gets an idempotent stateful event, monotonic per-run sequence, summary
snapshot, checkpoint digest, and workflow definition version/hash metadata so
future replay and resume paths can inspect durable run history without reading
only the hot embedded checkpoint.
Automation V2 run records now also persist first-class workflow definition
version and snapshot hash fields, backfill them from existing run snapshots on
load, expose them through SDK types, and fail restart recovery closed if the
recorded snapshot hash no longer matches the available definition.
Incident Monitor routing now supports Linear issue destinations
with MCP readiness checks, duplicate issue matching, destination-aware receipts,
and external-action records, and publish/recheck errors include the underlying
destination failure chain for easier operator diagnosis.
Incident Monitor routing also supports signed webhook destinations
with env-backed HMAC secrets, default SSRF blocking for private/internal URLs,
bounded payloads and response excerpts, capped retry attempts, and durable
per-delivery receipts. URL validation classifies parsed IPv4/IPv6 literals
before DNS lookup so IPv4-mapped private IPv6 webhook hosts fail closed
consistently across platforms.
Incident Monitor routing now also supports local telemetry and
internal memory destinations. Telemetry publishes durable destination-aware post
receipts that can be filtered by destination id, while internal memory
destinations store bounded, redacted summaries with category-specific record
refs and duplicate suppression.
Generic MCP tool destinations can now be configured for Incident Monitor routing.
These destinations fail closed unless an admin-configured
destination names the MCP server/tool, sets `allow_publish`, and provides an
explicit payload mapping. Route preview reports server/tool/mapping readiness
without executing tools, and publish attempts record destination, route, tool,
redacted receipt, duplicate-suppression, and failure details.
Incident Monitor incidents can now carry AI-agent safety and risk context without
breaking existing submissions. Drafts and incidents persist
redacted actor, model, tool, action, policy, approval state, risk category,
blast radius, and external correlation ids; SDK models and destination payloads
expose the context, and route preview can match risk categories for targeted
safety routing.
Incident Monitor can now generate production deployment cards for governed
agents, workflows, monitored sources, and Tandem self-monitoring. The read-only
endpoint combines authority inventory with operator-supplied purpose, owner,
accountability, approval, escalation, data-classification, and review metadata,
exports JSON/Markdown cards, and returns structured posture findings when
required production-governance fields are missing. Monitored-source cards now
link source posture evidence by source/project identifiers as well as canonical
source refs.
Incident Monitor monitored sources now have first-class data-readiness gates.
Projects and log sources can declare owner, system of record, classification,
allowed use, lineage/source-of-truth, freshness SLA and observation timestamp,
expected schema version, schema drift status, quality notes, legal basis or
authorization marker, and redaction/retention profiles. Status, route preview,
authority inventory, posture checks, assessment reports, deployment cards, and
TypeScript/Python SDK types surface sanitized source-readiness warnings and
findings without embedding raw source data, source paths, credentials, or
authorization marker values.
Incident Monitor setup is now available from Control Panel Settings. Operators
can edit source, destination, route, default destination, and safety-default
configuration in one place, run route previews before publishing, inspect
destination readiness badges, filter post receipts by destination, and use
TypeScript/Python SDK helpers for destination and route CRUD plus
destination-targeted draft publishing.
The server now exposes the same monitor APIs through canonical
`/incident-monitor/*` and `/config/incident-monitor` routes, and the pre-rename
aliases have been removed.
Shared Incident Monitor contracts now use the `tandem-incident-monitor` crate
identity and canonical `incident_monitor`/`incident-monitor` wire names for
runtime events, evidence refs, persisted data paths, GitHub host methods, and
the eval fixture CLI.
TypeScript and Python SDKs now use Incident Monitor as the canonical developer
surface: `client.incidentMonitor`, `client.incident_monitor`,
`IncidentMonitor*` types, `/incident-monitor/*` endpoints, and
`incident_monitor` config payloads. Scoped intake keys created through the
server now default to `incident_monitor:report`, `tim_intake_` key material,
and the `x-tandem-incident-monitor-intake-key` header.
Incident Monitor security readiness now records redacted protected audit events
for destination/route config changes, scoped intake-key lifecycle changes, and
destination-router publish attempts, completions, approval-required outcomes,
and failures. Scoped intake keys remain report-only under full API-token auth
and cannot use route preview, publish, normal report, or intake-key management
routes.
Incident Monitor now exposes a read-only authority inventory for security
posture assessment. The inventory summarizes workflows, Automation V2 specs,
agents, tool/MCP policy, destinations, routes, monitored sources, scoped intake
keys, approvals, policy decisions, and external publish surfaces while omitting
raw credentials, intake-key material, secret-backed destination values, action
receipts, and arbitrary metadata values. TypeScript and Python clients include
helpers for the endpoint. Incident Monitor also adds read-only,
dry-run-by-default security posture checks over that inventory and selected
policy decision/action history, producing deduped findings with severity, affected
objects, evidence refs, mitigation guidance, routing suggestions, and normal
Incident Monitor draft-conversion payloads. The docs now define AI Agent Security
Posture positioning, packaging, demo narrative, report outline, self-monitoring
boundaries, and comparisons with SAST, DAST, SIEM, CSPM, EDR, and traditional
workflow automation without overclaiming full vulnerability scanning.
Incident Monitor now also provides controlled dry-run security assessment
probes for approval-gated tool policy, high-risk route previews, fail-closed
destination readiness, scoped intake-key restrictions, MCP destination
allowlists, and webhook URL policy. Probe runs reject scoped intake keys,
require full API-token/admin context when token auth is configured, persist
evidence packs as context-run artifacts, emit admin audit events, and return
draft-conversion suggestions for failed probes through the TypeScript and
Python SDKs.
Incident Monitor security assessment reports now turn the authority inventory,
posture findings, controlled probes, incidents, destination receipts, and
protected audit rows into a redacted JSON report plus Markdown summary. Reports
persist context-run evidence artifacts by default, distinguish Tandem
self-monitoring (`tandem_runtime` / `tandem_monitor`) from external-system
monitoring, expose protected audit export summaries without raw payloads, and
include non-mutating destination route previews so report artifacts can move to
customer-owned systems of record after approval. TypeScript and Python SDKs
include helpers for the report endpoint.
Control Panel, desktop settings, create-panel templates, docs, examples,
scripts, and CI workflow labels now use Incident Monitor routes, filenames,
labels, and examples, with pre-rename redirects removed.
CI now also runs an Incident Monitor terminology guard over public UI, SDK,
docs, examples, scripts, and release surfaces so stale public terminology is
reported with file and line details.
The public guide now includes an agent-facing Incident Monitor runtime guide
that teaches MCP-connected agents and SDK clients the safe readiness,
route-preview, intake, triage, approval, publish, receipt, and governance
evidence sequence instead of direct external mutation.
The guide and compliance starter docs now also include an Incident Monitor
production governance map. It connects deployment cards, authority inventory,
posture checks, controlled probes, assessment reports, route previews, publish
receipts, protected audit evidence, and destination exports to the
operator-owned decisions needed for production readiness, while separating
current Tandem evidence from customer-owned retention, escalation,
incident-response, and turnkey SIEM integration responsibilities.
The Incident Monitor rename also stays under the CI touched-file-size guard by
compacting UI rename formatting and moving server service tests into a dedicated
module.
The SDK destination removal helpers now also drop routes that would otherwise
be left with no explicit destinations, preventing accidental fallback to the
default destination set.
Linear duplicate handling preserves matched-issue status on repeated publishes
and suppresses retrying an ambiguous failed Linear `create_issue` response that
may already have created an external issue.
Incident Monitor now ships production-mirroring adversarial scenario packs: a
versioned, read-only pack of abuse scenarios (forged severity downgrades,
cross-tenant route injection, approval bypass, unready-destination publish,
redaction leaks) runs in dry-run against the live routing, approval, and
readiness logic and feeds per-scenario pass/fail evidence into the security
assessment report.
Incident Monitor governance maturity metrics compute redacted approval,
incident-response, recurrence, and receipt-integrity metrics over a
configurable window, compare them against operator-tunable thresholds, and
flag behavioral drift between windows; results are exposed through a dedicated
endpoint and folded into the assessment report.
Incident Monitor governance reassessment is now continuous: a background
scheduler re-runs authority, data-readiness, routing, approval, and destination
posture on a cadence and on governance-relevant change events (route or
destination edits, monitored-source changes, tenant re-binds, model-policy,
MCP, and approval-policy changes), producing versioned results with
previous/current comparison, stable finding fingerprints that suppress
duplicate noise, and per-scope next-due/last-completed/overdue status on
deployment cards.
Behavior change: Incident Monitor destination publishing is now fail-closed by
default. `safety_defaults.block_unready_destinations` defaults to true, and
automated and manual publishes always block a destination that is not
publish-ready regardless of the flag; Recovery mode with the flag disabled
remains the deliberate operator escape hatch. Destination-specific GitHub MCP
servers are validated against their own server instead of the global GitHub
capability flags.
Automation V2 webhooks now support Notion natively: `notion` provider triggers
use the `notion_hmac_sha256` signature scheme, capture Notion's
`verification_token` from the subscription handshake without queueing a run for
it, expose the token exactly once through an authorized reveal endpoint, verify
`X-Notion-Signature` on subsequent events, and reject Tandem secret rotation
because the signing secret is Notion's provider-owned token. The Control Panel
webhook manager walks operators through the Notion verification flow.
Workflow phase transitions and MCP tool authority are now enforced at the
runtime boundary, and outbound tool dispatch passes through a pre-send outbox
gate. The protected audit ledger appends with fsync durability at O(1) cost,
and stateful wait reminder and scheduler clock regressions were fixed so timer
wakeups stay accurate across restarts.

## v0.6.4 (2026-06-28)

Tandem 0.6.4 adds secure Automation V2 webhook management and improves the
control-panel editing experience for generated workflow automations. Operators
can configure scoped webhook triggers, inspect sanitized delivery history, and
use the Studio-inspired workflow map to understand and tune generated nodes,
especially prompts and MCP-bound steps, from the existing edit modal.

### Automation Webhooks

- Automation V2 now exposes authenticated webhook management APIs for trigger
  list/create/read/update/disable/delete, one-time secret rotation, and scoped
  delivery history inspection without making public intake routes part of the
  management surface.
- The `Edit workflow automation` modal now includes webhook trigger management:
  create triggers, copy callback URLs, reveal generated secrets once, rotate or
  disable triggers, and inspect sanitized recent deliveries with queued run
  references.
- Management responses redact raw secrets, stored secret references, secret
  digests, raw payloads, auth headers, cookies, bearer tokens, and API-key-like
  preview fields while preserving safe delivery metadata for operators.

### Automation Webhooks

- Automation V2 now accepts signed public webhook deliveries at trigger-scoped
  URLs without requiring the normal Tandem transport token on that one route.
  The handler resolves tenant and automation only from the stored trigger,
  verifies HMAC signatures and replay windows, enforces JSON payload handling,
  and records sanitized internal rejection delivery records without leaking
  trigger, tenant, automation, or signature details in public errors.
- Accepted webhook deliveries now queue Automation V2 runs with
  `trigger_type = "webhook"`, delivery/trigger/provider provenance on the run
  snapshot, tenant context inherited from the trigger-bound automation,
  duplicate suppression, and `automation.v2.run.created` events with
  `triggerType: "webhook"`.

### Visual Workflow Editing

- The `Edit workflow automation` modal now starts with a Studio-inspired flow
  map that groups workflow nodes into dependency stages, shows start nodes and
  upstream dependency counts, and flags missing dependencies before operators
  start editing individual node prompts.
- Flow-map node cards summarize the details operators need while reviewing a
  generated automation: bound agent, objective preview, input-reference count,
  output kind, inherited workflow MCP servers, task-specific MCP overrides, and
  send-capable MCP tool usage.
- Selecting a node in the map scrolls directly to that node's prompt/model/MCP
  editor card and highlights it, making generated workflows easier to adjust
  without hunting through a long modal.
- Workflow edit drafts now retain dependency, input-reference, stage, and output
  metadata from the saved automation payload so the visual editor can represent
  the actual generated flow instead of only showing editable prompt text.
- The prompt editor section now opens by default, which makes the map useful for
  generic reusable workflow tuning where the common edit is changing a node's
  objective prompt while keeping the generated structure intact.

## v0.6.3 (2026-06-26)

Tandem 0.6.3 is a patch release for workflow-runtime reliability and Bug
Monitor routing. It repairs the MCP/Notion/Composio workflow paths exercised by
the Reddit infrastructure leads automation, and it advances Incident Monitor toward
destination-neutral incident routing while preserving the current GitHub
publishing behavior.

### Automation V2 MCP Workflows

- Automation V2 execution errors now classify provider request failures as
  transient and give provider/write-related artifact misses a minimum retry
  budget before terminal failure. This prevents low-retry Reddit/Notion workflow
  branches from failing immediately on flaky provider calls or missing artifact
  writes.
- Workflow edits now preserve the full Automation V2 agents and flow payload
  when saving from the summary editor, so MCP-enabled workflows do not lose node
  execution configuration after operators change model routing, swarm settings,
  or MCP access.
- Exact connector-tool workflows no longer receive contradictory instructions
  to call `mcp_list` first when concrete MCP tools are already bound. This keeps
  Notion/Composio-style automations on the intended tool path and avoids
  required-tool failures caused by discovery-only turns.
- Workflow artifact and source validation now treats MCP tool ids as connector
  tools instead of workspace files, carries upstream artifact paths into
  concrete source coverage, and recognizes Notion page/database operations as
  outbound connector actions.
- Connector action progress is guarded more tightly: artifact-write nodes only
  complete after a productive write to the declared artifact target, blocked
  runs clean up correctly, and runtime tests cover the connector-action review
  and artifact-write failure modes.
- MCP OAuth cleanup now removes stale secret-header and OAuth credential
  material when auth is deleted, canonical OAuth credentials can be reused during
  refresh, and MCP public base URL handling is shared through one helper.
- Connector-backed workflow searches now capture full remote result files into
  run artifacts before model filtering. This prevents large Composio Reddit
  responses from being reduced to model-visible previews, which previously let
  runs complete cleanly while leaving downstream Notion writer nodes with too
  few or zero rows.
- Generic connector-row filtering now reads validated source artifacts,
  preserves writer-ready fields such as title, link, author, and duplicate-key
  values, and hands Notion writers complete rows without relying on
  workflow-specific prompt fixes. The Reddit infrastructure leads workflow was
  used as the validation shape, but the materialization path is connector
  generic for larger future data-collection workflows.

### MCP Control Panel

- MCP Settings now keeps large tool allowlists collapsed by default with an
  animated expand control, reducing the amount of space each connected server
  consumes in the list.
- MCP servers that still require OAuth or are otherwise disconnected are sorted
  to the top so operators can see and complete the required connection step
  before trying to use the server in workflows.
- The built-in MCP catalog copy now uses clearer add wording instead of the
  previous pack-oriented label, and scoped MCP inventory output is more compact
  for large connected servers.

### Incident Monitor Routing

- Incident Monitor now has destination-neutral Incident Monitor routing foundations:
  destination readiness, route preview APIs, destination-aware post
  filtering/idempotency, and TypeScript/Python SDK types.
- A centralized `incident_monitor::router` handles route preview, route matching,
  destination readiness checks, and publish dispatch for manual, automatic,
  approval, recovery, timeout, and service paths. GitHub remains the only
  executable destination in this phase, and unsupported destination overrides
  fail closed without creating posts.
- Monitored projects and log sources can now carry route bindings for source
  kind, route tag, destination allow/default policy, tenant/workspace scope,
  approval policy, redaction, and retention metadata.
- Source bindings propagate through log watcher intake, scoped raw reports,
  drafts, incidents, route preview, recovery publishes, and publish validation.
  Raw source-supplied routing fields are sanitized before configured source
  lookup, granted approvals are respected, and high-risk raw sources default to
  safer approval behavior.
- Control Panel source setup display, SDK event models, external-log intake
  documentation, and regression coverage were updated for source-bound routing.

## v0.6.2 (2026-06-23)

Tandem 0.6.2 starts the enterprise MCP identity workstream for multi-employee
runtime deployments. The first change is a source-of-truth design for separating
MCP server definitions from user-owned, service-principal, shared, and delegated
MCP connections.

### Runtime Observability

- Runtime event persistence now consumes an opt-in bounded event-bus queue, so
  canonical run/session events published after persister registration are
  written to `runtime/events.jsonl` without depending on the live broadcast
  stream or retaining events in eval-only buses.
- Observability events and exports now carry tenant IDs (`org_id`,
  `workspace_id`) as ID-only fields. A config-gated authenticated `/metrics`
  endpoint exposes scheduler queue depth, scheduler tick latency, run duration,
  gate wait, tool decision, and provider error metrics, while optional Sentry
  export is feature-gated and rebuilt through a scrubber that drops prompts,
  completions, tool arguments, and free-text details.

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
- Automation V2 restart coverage now exercises persisted reload behavior for
  queued, awaiting-approval, blocked, and running runs. Golden assertions compare
  uninterrupted and restarted outcomes, duplicate approval clicks remain
  idempotent, and in-flight consequential work is failed on server restart
  without replaying node attempts or fabricating outputs.
- Approval-gated Automation V2 runs now have failure-injection coverage for
  concurrent approvals, provider failure immediately after approval, stale gate
  decisions, half-applied gate decisions after restart, and corrupted checkpoint
  entries. Restart recovery settles already-recorded approve/cancel decisions,
  preserves rework re-arming, clears stale gate-local failure markers on
  approval, and quarantines malformed individual run checkpoints as blocked
  diagnostics instead of crashing scheduler startup.
- Approval gates now support expiry policy metadata for default and per-gate
  deadlines. The executor can auto-cancel expired gates, record reminder or
  escalation lifecycle/audit events, redispatch approval notifications with
  changing notification keys, reject late human decisions after auto-cancel
  expiry, and expose gate deadlines in the unified approvals API and
  control-panel inbox.
- Session-level PermissionManager asks now persist to durable state with
  decision history and provenance-bearing standing rules. Pending prompts left
  behind by a restart are marked `runtime_restarted` so the next tool attempt
  re-asks deterministically, and permission replies now write protected audit
  evidence with actor, request, decision, and rule provenance. Stale persisted
  request IDs are rejected after restart, and concurrent state-file writes are
  serialized so simultaneous prompts and decisions remain durable.
- Dogfooding bugs now have a permanent replay lane: `eval_datasets/dogfooding_regressions.yaml`
  seeds five recent workflow/runtime bug classes, `incident-monitor-fixture` scaffolds
  sanitized eval fixtures from Incident Monitor incidents, and a nightly workflow runs
  the dogfooding regression dataset through `eval-runner --engine-mode stub`.
  Stub/live eval-runner modes now use Tokio's multithreaded runtime so local
  in-process Automation V2 evals do not overflow the single-thread runtime stack.
- Rust CI now has a security and coverage lane: cargo-audit, cargo-deny,
  documented exception handling, and nightly governance-critical llvm-cov
  artifacts for `tandem-tools`, `tandem-plan-compiler`, and `tandem-automation`.
  The first cargo-deny baseline records scoped license exceptions with owners,
  reasons, and expiry dates so follow-up hardening is auditable.
- A seeded approval-gated email demo now exercises the enterprise MCP approval
  pattern without real credentials. `just demo` starts an isolated engine plus a
  local HTTP MCP email stub, drafts mail, pauses at an Automation V2 approval
  gate, supports approve/cancel/rework paths, sends only after approval, and
  writes audit evidence for gate decisions, tool ledger events, drafts, and the
  outbox. A nightly non-interactive workflow runs the same path in CI.

### Runtime Governance

- Provider auth credential storage now lives in `tandem-providers` instead of
  `tandem-core`, with compatibility re-exports left in core for existing
  callers. This starts the TAN-205 crate-boundary cleanup without changing
  persisted credential file names or tenant-scoped credential behavior.
- Incident Monitor domain logic now lives in a dedicated `tandem-incident-monitor` crate.
  The moved layer owns Incident Monitor records, log parsing and evidence artifact
  rendering, recurrence comment summaries, deterministic error provenance, and
  the GitHub publish algorithm, while `tandem-server` provides a thin
  AppState/HTTP/MCP host shim and compatibility re-exports for current callers.
  The host shim keeps duplicate post detection and ambiguous failed-create
  suppression on scoped, uncapped storage lookups rather than the public capped
  post-list API.
- Incident Monitor log sources that start at EOF now keep missing-file health updates
  separate from successful first positioning. If a watched log appears later
  with bootstrap/history content, Tandem seeks to the end before ingesting new
  lines instead of replaying the preexisting file.
- The eval framework now lives in a dedicated `tandem-eval` crate. The
  eval-runner CLI, datasets/metrics/regression helpers, scripted provider,
  isolated AppState bootstrap, and Incident Monitor fixture scaffold moved out of
  `tandem-server`; eval CI now builds `cargo build --bin eval-runner -p
tandem-eval` while the server exposes only narrow public eval-support
  wrappers for the harness.
- Tool execution now flows through a governed dispatcher outside the
  `tandem-tools` crate. Engine turns, workflow actions, Automation V2 connector
  preflight calls, direct HTTP tool execution, planner helpers, pack builder,
  and the engine CLI carry tenant context and scope allowlists through one
  dispatch path, which records a single policy/scope ledger event for each
  dispatch before returning the tool result or denial.
- Tool-call parsing, registry resolution, and approval classification now share
  the same canonical tool-name normalizer. Function-style invocation parsing is
  backed by a structured scanner with a committed 30+ case corpus and generated
  parser drift coverage, reducing approval/dispatch disagreement for wrapper
  prefixes, aliases, and concrete MCP tool names.

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
- Provider credential debug output and incident-monitor log redaction now avoid
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
  with source-binding, resource, data-class, source-object, and matching
  knowledge-scope registry metadata while preserving local/default import
  behavior when unset.
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

- **Eval Runner CLI** (`cargo run -p tandem-eval --bin eval-runner`): Standalone command-line tool for bulk test execution with the following capabilities:
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
- **Lifecycle and event observability**: `record_automation_lifecycle_event_with_metadata` automatically merges the run's `effective_execution_profile` into every `AutomationLifecycleRecord` so existing audit, replay, and Incident Monitor surfaces see the profile without per-call-site changes. The `automation_v2.run.failed` engine event now includes both `effective_execution_profile` and `requested_execution_profile`, so Incident Monitor and downstream observers can attribute failures to the active profile.
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

Sessions now carry explicit source metadata. New interactive sessions default to `sourceKind: chat`, Automation V2/Incident Monitor worker sessions are classified as `automation_v2`, and session listing supports filtering by source. The TypeScript client and wire model expose the same fields so control-panel views can ask for the session class they actually need.

The Chat sidebar and Dashboard recent-session list now request only `source=chat`, so Incident Monitor submissions such as `Automation automation-v2-incident-monitor-triage-failure-draft-... / inspect_failure_report` no longer appear as conversations. Legacy automation records with the existing title format are classified at the storage/wire boundary, preserving backward compatibility for already-written sessions.

The Tauri desktop Automation Calendar no longer crashes the app while loading. FullCalendar is now isolated into its own lazy bundle and imported only after the WebKit stylesheet host is ready, preventing the `Cannot read properties of null (reading 'cssRules')` startup failure seen when opening the calendar view.

Incident Monitor GitHub issue creation now uses a persisted pending idempotency claim before calling GitHub. Completion finalization, stale-provider recovery, deadline recovery, and status-sweep recovery can all wake up around the same draft, but only the first caller that claims the create-issue digest is allowed to create the GitHub issue. Concurrent callers now see `publish_in_progress` or reuse the posted record instead of producing duplicate issues with the same fingerprint and triage run.

Incident Monitor proposal quality gates also recognize the structured handoff shapes that triage nodes actually return, including wrapped objects such as `{ "incident_monitor_inspection": ... }` and array responses containing the artifact followed by a compact status object. Placeholder task specs still fail the gate, but valid completed inspection, research, validation, and fix-proposal artifacts no longer get treated as missing and replaced with broad fallback evidence.

Incident Monitor triage status detection now treats nested `status: blocked` fields inside structured Incident Monitor handoffs as evidence/limitation data, not as the node's own runtime status. This prevents `propose_fix_and_verification` from recursively blocking the debugger when it has produced a useful partial fix proposal with acceptance criteria and bounded next steps.

Automation V2 long-running nodes now get to own their timeout path. The stale-run reaper honors the run-registry heartbeat that active node execution already emits every few seconds, so a first task with a 600-second budget is not globally paused as `stale_no_provider_activity` at the exact timeout boundary before the node can fail or repair normally.

Automation V2 research validation now preserves source URLs from successful `websearch` and `webfetch` tool results. If a generated JSON artifact is too sparse and omits raw links, the validator can still see the current web evidence that was actually gathered instead of blocking the node as `citations_missing`. The prompt and repair guidance also now explicitly tell research agents to include raw URLs in `citations` or `web_sources_reviewed` fields.

Connector-backed source research now has to use the selected connector, not merely discover it. A node that says to use Reddit MCP and resolves `reddit-gmail` can no longer complete after only `mcp_list` plus a JSON write; it must call a concrete source tool such as `mcp.reddit_gmail.reddit_search_across_subreddits` or `mcp.reddit_gmail.reddit_retrieve_reddit_post`, preserving real returned evidence or an actual connector/tool limitation.

The prompt and tool surface now reinforce that rule before validation has to catch it. Connector source prompts list concrete `mcp.*` tools and state that `mcp_list`, `glob`, `grep`, `edit`, and `apply_patch` are not source evidence, while non-code connector source nodes no longer offer edit/patch/bash tools that can distract agents from calling the connector.

Connector-backed delivery nodes now keep their destination MCP tools focused all the way through artifact creation. Notion save/report nodes with explicit `mcp.notion.*` tool allowlists no longer inherit generic workspace `read`/`glob` or mutation tools from upstream input refs, but they still retain the required `write` tool for the run artifact receipt. The engine loop also narrows prewrite MCP gating to the specific concrete connector tools that have not yet run, steering a Notion publisher from `notion_fetch` to `notion_create_pages` instead of letting it loop on already-completed discovery or local inspection.

Required-tool provider calls now fail closed inside Tandem instead of being rejected by the provider when routing filters remove every tool. Write-required connector nodes keep the artifact `write` tool even when their session allowlist is connector-only, and if a later filter still produces an empty tool set Tandem downgrades the provider request away from `tool_choice: required` rather than sending an invalid no-tools request.

Transient provider stream decode failures are now treated as recoverable provider infrastructure failures. Stream errors such as `error decoding response body`, unexpected EOF, and incomplete streamed responses are retried inside the current provider iteration with partial streamed text/tool-call state cleared before retry. The retry budget is bounded by `TANDEM_PROVIDER_STREAM_DECODE_RETRY_ATTEMPTS`, and each retry emits a `provider.call.iteration.retry` event for debugging.

Automation V2 governance now gives repair attempts a calmer, more actionable handoff. Attempt verdicts include a `calm_teammate_v1` review with a progress score, what the agent completed correctly, what is still needed, why the missing work matters, and the next concrete moves. Repair prompts show that review before the raw expected/observed contract JSON, so retries can keep good evidence and fix the smallest missing piece rather than restarting from a vague validation failure.

Incident Monitor failure reports now preserve both the final failure and the useful prior attempt evidence. Automation V2 failure events carry recent attempt verdict chains and attempt review chains into Incident Monitor submissions, making issue details show earlier contract misses such as missing workspace files, missing connector calls, citation gaps, or required next actions even when the final observed failure is a provider stream/runtime error.

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

Final report/brief nodes that synthesize already-collected Tandem MCP notes, Reddit MCP signals, web findings, and run artifacts no longer require fresh workspace `read` calls. The planner stops adding `local_source_reads` to new `research_synthesis` contracts, and the runtime validator waives stale local-read enforcement on existing saved synthesis nodes. Code-change, local-research, and Incident Monitor source-inspection nodes still retain their strict repo-read gates.

This prevents research-to-destination workflows from blocking with messages such as `research brief cited workspace sources without using read` when the workflow only cites MCP/web/upstream artifact evidence and does not need repository source files.

Workflow packs are now the preferred portable format for created workflows. The Workflows page can upload a `.zip` pack, preview its manifest, cover image, workflow entries, capabilities, and validation results, then install it and open the resulting planner session. Raw JSON workflow bundle import remains available under Advanced for debugging and internal handoffs.

Planner sessions can also be exported as marketplace-ready workflow pack ZIPs containing `tandempack.yaml`, `README.md`, the embedded workflow plan bundle, and an optional PNG/JPEG/WebP cover image. New workflow-pack APIs and TypeScript client helpers support export, preview, and import, while imported sessions keep pack provenance (`source_pack_id`, version, and source bundle digest) for later inspection.

Exported workflow packs now include a hosted-safe download URL, and the Workflows page shows a browser Download ZIP action after export so operators can retrieve generated packs without access to the server filesystem path. Control-panel uploads also now prefer `$TANDEM_HOME/data/channel_uploads` and expand home-directory placeholders such as `~`, `$HOME`, `${HOME}`, and `%HOME%`, avoiding stray literal upload directories when hosted or Windows-style environment values are used on Linux/macOS.

## v0.5.3 (Released 2026-05-03)

Automation V2 workflow definitions now use per-workflow storage shards. Instead of rewriting every saved workflow into one large `automations_v2.json` file, Tandem writes each definition to `data/automations-v2/<automation-id>.json` and keeps a small `index.json` alongside the shards. On startup, existing aggregate installs are migrated automatically and the old aggregate is preserved as `automations_v2.legacy-aggregate.json` for rollback/debugging.
