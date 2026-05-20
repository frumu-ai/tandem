# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.9] - Unreleased

### Added

- **Workspace access-control contract vocabulary**: Added transport-safe enterprise contract types for resource hierarchy, scoped resources, access permissions, data classes, normalized principals, grant sources, and scoped grants. The new contract can model department data access, cross-functional group access, explicit CEO/executive global grants, down-scoped external delegation, repository path scopes, and MCP tool resource targets.
- **Workspace access-control contract coverage**: Added serde round-trip and modeling tests for Finance data stores, Engineering repository path scopes, CEO org-wide executive access, MCP tool targets, department membership grants, group membership grants, executive/global grants, and expiring delegated grants.
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

- **AI Evaluation Framework (Phases 1-5)**: New `tandem-server/src/eval` and `tandem-server/src/failures` modules provide structured testing, regression detection, and compliance documentation for AI quality assurance. Phases 1-2 add the `AIFailureMode` taxonomy (30+ categorized failure types across validation, provider, repair, resource, timeout, and authorization domains) and the `EvalDataset`/`EvalTestCase`/`AutomationSpecTest` YAML schema with four reference datasets in `eval_datasets/` (critical_path, provider_failures, repair_exhaustion, citation_validation). Phase 3 ships the `eval-runner` CLI binary (`cargo run --bin eval-runner`) with metrics aggregation (`pass_rate`, `avg_repair_iterations`, `total_cost_usd`, `provider_failure_rate`, validator pass rates by class), parallel worker support, tag filtering, and simulation mode for deterministic CI runs without provider calls. Phase 4 adds `EvalBaseline`/`RegressionThresholds`/`RegressionReport` types and `detect_regressions()` for comparing current runs against saved baselines (default thresholds: 5pp pass_rate drop, 20% cost increase, 30% repair iteration increase, 5pp provider failure increase), plus a `.github/workflows/eval-regression-gate.yml` CI workflow that runs the gate on every PR, posts a summary comment, auto-updates the main-branch baseline, and fails CI when critical thresholds are exceeded. Phase 5 ships developer documentation (`docs/dev/EVAL_FRAMEWORK.md`) and user/compliance documentation (`docs/user/AI_QUALITY_ASSURANCE.md`) covering EU AI Act Article 50 transparency obligations.

- **Failure mode taxonomy module** (`tandem-server/src/failures`): `AIFailureMode` enum with 30+ variants (e.g., `ArtifactValidationFailed`, `ContractViolation`, `CitationMissing`, `ProviderTimeout`, `RepairBudgetExhausted`, `TokenBudgetExhausted`, `PathTraversalDetected`, `AuthorizationFailed`), `FailureCategoryKind` severity classification (Critical / High / Medium / Low), `FailureContext` struct for incident tracking, and helpers `classify_error_text()`, `categorize_failure()`, and `should_retry()` for deterministic error categorization. Includes 11 unit tests covering classification, serialization, and retry decision logic.

- **Eval baseline storage** (`eval_baselines/main_branch.json`): Sample baseline format with git metadata (commit SHA, branch), tracked metrics, and validator pass rates for the critical_path dataset. The regression-gate workflow updates this file automatically on main-branch pushes so future PRs are evaluated against the latest production performance.

- **Eval runner binary** (`crates/tandem-server/src/bin/eval_runner.rs`): Standalone CLI with `--dataset`, `--output`, `--provider`, `--model`, `--simulation`, `--num-workers`, `--filter-tag`, `--max-duration`, and `--verbose` flags. Exit codes are CI-friendly: 0 (all pass), 1 (one or more failures), 2 (dataset load error or invalid arguments). Output is both human-readable on stdout and a structured JSON results file consumed by the regression-gate workflow.

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
- **Execution Profile in lifecycle metadata and run-failed events**: Every `AutomationLifecycleRecord` written via `record_automation_lifecycle_event_with_metadata` now carries the run's `effective_execution_profile` in its metadata (existing keys are preserved). The `automation_v2.run.failed` engine event also surfaces `effective_execution_profile` and `requested_execution_profile`, so Bug Monitor and downstream observers can attribute failures to the active profile.
- **Execution Profile chokepoint and validator-class telemetry**: Added `classify_unmet_requirement` (mapping existing validator strings such as `missing_required_section`, `weak_markdown_structure`, and the always-blocking critical classes to a structured `ValidatorClass` taxonomy) and `augment_output_with_profile_relaxation` (which writes `relaxed_validator_classes`, `effective_outcome`, `original_validator_outcome`, `execution_profile`, and `experimental` into `artifact_validation` when the active profile would relax all unmet requirements). The executor invokes the augmentation at the single run-acceptance chokepoint per the "Executor Chokepoint Invariant." Strict behavior is unchanged; critical classes (auth, secret access, destructive-action approval, budget caps) and not-yet-classified unmet requirements always block.
- **Execution Profile status downgrade**: When the chokepoint relaxes a node's outcome, `augment_output_with_profile_relaxation` now also rewrites the executor-facing fields so the run continues. Guided runs land as `completed_with_warnings`; YOLO runs land as `completed` with `experimental: true` on the artifact. Validation-related `failure_kind` and `blocked_reason` are cleared, `warning_count` is set to the count of relaxed classes, and the original `status`/`failure_kind` are preserved under `artifact_validation.original_status`/`original_failure_kind` for receipts and replay. Non-validation `failure_kind` values (e.g. `provider_stream_failed`) are left untouched.
- **Execution Profile repair budget multiplier**: `validate_automation_artifact_output_with_context` now applies `effective_repair_budget` (Strict 1.0×, Guided 1.5×, YOLO 2.0×) to the per-node `AutomationOutputEnforcement.repair_budget` before passing it to `infer_artifact_repair_state`. The multiplier is bounded by the existing `AutomationExecutionPolicy` global caps. Multiplier is driven by the saved automation spec's profile; honoring run-level overrides is a follow-up.
- **Execution Profile control panel surfaces**: The TypeScript client `automationsV2.runNow` accepts an optional `executionProfile` ("strict" | "guided" | "yolo") that maps to the server's `execution_profile` field, and `AutomationV2RunRecord` exposes `effective_execution_profile` and `requested_execution_profile`. New helpers (`executionProfileLabel`, `workflowEffectiveExecutionProfile`, `artifactValidationIsExperimental`, `artifactValidationRelaxedClasses`) and components (`ExecutionProfilePill`, `ExperimentalArtifactBadge`, `RelaxationOutcomeSummary`) ship in `tandem-control-panel`. Run summaries now surface the effective execution profile (with any per-run override note), and artifacts marked experimental by the chokepoint display an Experimental badge that lists the relaxed validator classes and original/effective outcome on hover.
- **Execution Profile Tauri desktop run-now plumbing**: `automationsV2RunNow` in `src/lib/tauri.ts` accepts an optional `{ dryRun?, executionProfile? }` payload and forwards it to the engine. The Tauri command and `Sidecar::automations_v2_run_now` accept an optional request body so per-run profile overrides work the same way as in the control panel and HTTP API. Existing callers without options remain compatible.
- **Execution Profile UI parity** (control panel + Tauri desktop): the workflow edit dialog now exposes an Execution Profile select that round-trips through `WorkflowEditDraft.executionProfile` and writes `execution.profile` on update; automation cards expose a "Run as Strict / Guided / YOLO" override picker next to Run now and surface the saved profile as a pill in the card metadata; the per-task run debugger surfaces the Experimental badge on the validation-outcome card and a dedicated profile-relaxation panel listing the structured `relaxed_validator_classes`.
- **Experimental-input propagation**: downstream automation v2 node outputs now inherit `artifact_validation.experimental = true` (and a `tainted_inputs` array of upstream node ids) when any upstream they depend on was accepted under a relaxed profile. Pure metadata: status, `failure_kind`, and `warning_count` are not altered, so cleanly-passing downstream nodes still complete; the receipt and `run_completed` event preserve the experimental provenance through the rest of the run.
- **Run-level profile override propagates through repair budget multiplier**: `create_automation_v2_run_with_profile` and `create_automation_v2_dry_run_with_profile` now stamp the resolved effective profile onto the cloned `automation_snapshot.execution.profile` so downstream code that reads `automation.execution.profile` (the multiplier in `validate_automation_artifact_output_with_context` and similar paths) honors per-run overrides instead of silently falling back to the saved spec profile. The original `AutomationV2Spec` passed in is unchanged.
- **`node_relaxed` lifecycle event**: When the chokepoint downgrades a node's outcome under Guided/YOLO, the executor now emits a top-level `node_relaxed` lifecycle event alongside the usual `node_completed`/`node_completed_with_warnings` event. Metadata carries the structured `relaxed_validator_classes`, `original_validator_outcome`, `effective_outcome`, the original executor `status` that was rewritten, and the `experimental` flag. Run history, the `automation_v2.run.failed` payload, and Bug Monitor receipts now show relaxation directly without consumers having to walk into `artifact_validation`.
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
- **Automation worker sessions no longer appear as Chat conversations**: Chat and Dashboard recent-session lists now request `source=chat`, and the server filters session listings by source. Existing legacy records titled like `Automation ... / ...` are classified as `automation_v2` at the storage/wire boundary, keeping Bug Monitor and Automation V2 audit sessions inspectable through automation/run surfaces without polluting the Chat session picker.
- **Tauri calendar view no longer crashes during startup**: The Automation Calendar now loads FullCalendar after the Tauri/WebKit stylesheet host is ready and keeps FullCalendar in a lazy chunk, avoiding a WebKit timing crash where FullCalendar accessed `style.sheet.cssRules` while the stylesheet was still `null`.
- **Bug Monitor GitHub publishing is idempotent under recovery races**: GitHub issue creation now claims a persisted pending post record before calling GitHub, keyed by the same create-issue idempotency digest used for successful posts. If completion, timeout recovery, and stale-provider recovery all try to publish the same draft at once, only one caller can create the issue; the others return `publish_in_progress` or reuse the completed post instead of creating duplicate GitHub issues.
- **Bug Monitor triage artifact gates accept real structured handoffs**: Proposal quality checks now understand wrapped and array-shaped Bug Monitor node outputs, including completed inspection, validation, and evidence handoffs returned directly in the final response. Placeholder task specs are still rejected, but valid completed handoffs no longer get mistaken for missing artifacts and replaced with low-signal fallback evidence.
- **Bug Monitor fix proposals no longer self-block on nested limitation status**: Structured Bug Monitor triage handoffs can now contain an inner `status: blocked` to describe limited evidence without causing the Automation V2 node itself to be classified as blocked. This keeps `propose_fix_and_verification` useful when it preserves the original workflow failure and bounded next steps from partial tool evidence.
- **Automation V2 stale reaping no longer races active node timeouts**: The stale-run reaper now honors the active node heartbeat maintained by the run registry. Long-running nodes with a 600-second budget can reach their own timeout/repair path instead of being globally paused as `stale_no_provider_activity` at the same 600-second boundary.
- **Research artifacts preserve websearch URLs as citation evidence**: Automation V2 now extracts source URLs from successful `websearch`/`webfetch` tool results and carries them into artifact validation metadata. Sparse JSON research artifacts no longer block as `citations_missing` when the run actually performed successful current web research with source URLs, and repair prompts now tell agents to write raw URLs into `citations`/`web_sources_reviewed` fields.
- **Connector-backed source nodes must use the connector, not just discover it**: Automation V2 now rejects connector-backed source artifacts when the node only runs `mcp_list` and never calls a concrete selected connector tool. Reddit research nodes that select `reddit-gmail` must execute tools such as `mcp.reddit_gmail.reddit_search_across_subreddits` or `mcp.reddit_gmail.reddit_retrieve_reddit_post` before a "no evidence" artifact can be accepted.
- **Connector source prompts steer agents into real MCP calls**: Generated Automation V2 prompts now list the concrete connector tools available to source nodes and explicitly warn that `mcp_list`, filesystem discovery, and edit/patch tools are not source evidence. Non-code connector source nodes also stop offering `edit`, `apply_patch`, or `bash`, reducing the chance that Reddit collection nodes write artifacts without querying Reddit.
- **Connector delivery nodes stay focused on destination tools**: Notion publisher nodes with explicit `mcp.notion.*` tool allowlists no longer inherit generic workspace `read`/`glob` tools from upstream input refs or mutation tools from broad defaults. They still keep `write` for the required run-artifact receipt, and the engine now narrows prewrite MCP gating to only the concrete connector tools that have not yet run, steering save/report nodes from `notion_fetch` to `notion_create_pages` instead of looping on discovery.
- **Required-tool provider calls never send an empty tools list**: Write-required connector nodes now preserve the artifact `write` tool even when the session allowlist is connector-only. If later routing filters still leave no selected tools, Tandem downgrades the provider request away from `tool_choice: required` and omits the empty tools payload, preventing provider errors such as `Tool choice 'required' must be specified with 'tools' parameter.`
- **Transient provider stream decode failures retry in-place**: Provider stream read/decode failures such as `error decoding response body`, unexpected EOFs, and incomplete streamed responses are now classified as transient provider errors and retried inside the current provider iteration before the session is failed. Partial streamed text/tool-call state is cleared before retry, bounded by `TANDEM_PROVIDER_STREAM_DECODE_RETRY_ATTEMPTS`, and retry events are emitted as `provider.call.iteration.retry`.
- **Automation repair prompts now include calm attempt reviews**: Automation V2 attempt verdicts now include a `calm_teammate_v1` review with progress score, what worked, what is still needed, why it matters, and concrete next moves. Repair prompts lead with that review before raw expected/observed contract evidence so retries preserve useful progress instead of feeling like vague validation scolding.
- **Bug Monitor preserves attempt verdict and review chains**: Automation V2 failure events and Bug Monitor submissions now include recent attempt verdicts and attempt reviews when final failure reporting would otherwise only show the last provider/runtime error. This keeps actionable prior failures such as missing workspace files, missing connector calls, citation gaps, and required next actions visible in generated issue details.
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
- **Existing saved synthesis nodes tolerate stale read enforcement**: Runtime validation now treats stale `local_source_reads`/`read` requirements as advisory for `research_synthesis` nodes, while preserving strict `read` enforcement for code workflows, local research, and Bug Monitor/source-inspection tasks that genuinely require repo evidence.
- **Control-panel uploads use the global Tandem data directory**: Panel-managed uploads now prefer `$TANDEM_HOME/data/channel_uploads`, expand `~`, `$HOME`, `${HOME}`, and `%HOME%`, and normalize Windows-style separators on Linux/macOS so uploaded workflow pack images do not create stray literal `%HOME%\...` directories in the repo.

## [0.5.3] - Released 2026-05-03

### Changed

- **Automation V2 definitions are stored as per-workflow shards**: Saved workflow definitions now persist under `data/automations-v2/<automation-id>.json` with a small index instead of rewriting every workflow into one growing `automations_v2.json` aggregate. Existing aggregate files are migrated on load and archived as `automations_v2.legacy-aggregate.json`.
- **Generated workflow planning uses deterministic task-budget compaction**: AI-generated workflow plans now have a hard 8-step budget. Oversized planner output is compacted into request-aware macro steps before preview or chat-revision storage, preserving source/tool intent and destinations such as Notion collection ids instead of falling back to a generic `execute_goal` plan. Manual Studio workflows and explicit imports remain exempt.
- **Planner diagnostics expose task-budget status**: Preview/revision diagnostics now include `task_budget.max_generated_steps`, `generated_step_count`, `status`, and `original_step_count` when compaction occurs; the control panel surfaces messages such as "Planner compacted 29 generated tasks into 6 runnable workflow steps."

### Fixed

- **Connector-backed workflow nodes receive their actual MCP tools**: Natural node objectives such as "Use the connected Reddit MCP" now match hyphenated MCP server ids such as `reddit-gmail`, so generated research nodes request `mcp.reddit_gmail.*` instead of being offered only `mcp_list` and local file tools.
- **Research artifacts no longer self-block on connector limitations**: Artifact prompts and repair guidance now tell agents to record unavailable connectors or partial evidence under limitation fields while keeping finished JSON artifacts terminal (`status: completed`), preventing `artifact_status_not_terminal` loops that stop downstream workflow and Bug Monitor reporting.
- **Apply/session boundaries reject runaway generated plans**: `/workflow-plans/apply` and planner-session creation reject over-budget generated plans with `WORKFLOW_PLAN_TASK_BUDGET_EXCEEDED` if an uncompacted oversized plan reaches them.

## [0.5.2] - Released 2026-05-03

### Changed

- **Bug Monitor triage evidence is advisory, not report-blocking**: Automation V2-backed Bug Monitor triage still asks agents to search the configured repo and prefer concrete source `read` evidence, but missing or inconclusive reads no longer hard-block Bug Monitor's own inspection/research/validation/fix artifacts. This keeps GitHub reporting focused on the original workflow failure instead of recursively failing on `no_concrete_reads`.
- **Bug Monitor blocked triage can still publish fallback evidence**: Blocked Bug Monitor triage Automation V2 runs are now treated as terminal enough for fallback summary synthesis and GitHub publication, so issue drafts can preserve the real workflow failure even when triage cannot satisfy every evidence preference.
- **Generated compact research-to-destination workflows stay compact**: The workflow planner now recognizes concise research/report/save prompts, caps them around 5-8 leaf tasks, avoids splitting every report section into its own node, and rejects over-budget LLM plans in favor of a compact fallback.
- **Connector-backed inspection and research nodes get the long workflow budget**: Structured JSON nodes that inspect or fetch external sources such as Notion collections, Reddit, or web research now inherit the long-running workflow timeout instead of the generic 180-second structured JSON default.

### Fixed

- **Bug Monitor no longer masks workflow failures with its own source-read gate**: Triage artifacts now use artifact-only validation and preserve tool/search limitations in completed JSON instead of blocking issue publication when an agent searches but does not produce a concrete `read` receipt.
- **Notion collection inspection nodes no longer default to 3-minute timeouts**: Generated workflow nodes such as `inspect_notion_collection` that call external data sources now receive the long-running automation budget, reducing premature `automation node ... timed out after 180000 ms` failures.

## [0.5.1] - Released 2026-05-03

### Added

- **Bug Monitor external project log intake**: Added monitored-project/log-source config, deterministic JSON-lines and plaintext log parsing, persisted offset state, evidence artifact writing, storm control, and a background watcher that turns local external project log failures into Bug Monitor incidents without requiring a workflow to hold the full engine token.
- **Scoped Bug Monitor external report intake**: Added limited per-project intake keys plus `POST /bug-monitor/intake/report` and `/failure-reporter/intake/report` so CI systems and external apps can submit normalized failure reports without access to the full engine API token.
- **Bug Monitor intake-key management APIs**: Added protected key list/create/disable endpoints under `/bug-monitor/intake/keys`, storing only key hashes and returning raw keys only at creation.
- **Bug Monitor log evidence artifacts**: Added state-managed `tandem://bug-monitor/...` evidence refs and JSON evidence artifacts for log candidates, including byte offsets, source ids, redacted excerpts, and fingerprints.

### Changed

- **Bug Monitor triage receives explicit repo-root inputs**: Automation V2-backed Bug Monitor triage nodes now carry the resolved `workspace_root` in node inputs and prompt guidance, making local source reads target the selected repo checkout instead of relying on implicit workspace context.
- **Bug Monitor setup explains hosted repo layout**: The control panel now shows a hosted path map for Bug Monitor, quick actions for `/workspace/repos/<repo>`, setup warnings for parent/runtime-state folders, and Coder sync hints so operators know which checkout Bug Monitor will inspect.
- **Bug Monitor triage is project-aware**: Triage run creation now prefers the linked incident or monitored project `workspace_root`, `model_policy`, and `mcp_server` before falling back to global Bug Monitor config, so external project failures are inspected in the correct repo/workspace.
- **Bug Monitor config supports monitored projects**: The existing single-project/global config remains compatible, while `monitored_projects` can now define external repos, workspace roots, log sources, and project policy.
- **Bug Monitor status exposes watcher health**: Status snapshots now include log watcher running state, enabled project/source counts, source health, offsets, file size, last poll/candidate/submission times, and source errors.

### Fixed

- **Bug Monitor research retries missing concrete reads more reliably**: Triage research now gets an additional repair attempt when it searches the repo but fails to perform the required concrete source-file `read`, reducing blocked `no_concrete_reads` demo failures.
- **External log paths fail closed**: Monitored log paths are validated under their configured workspace root, including symlink/absolute path escape rejection, before watcher polling.
- **Split log lines keep correct evidence offsets**: Partial trailing lines now preserve their starting byte offset so failures spanning polls produce accurate evidence ranges.
- **External project dedupe avoids cross-repo collisions**: Watcher-created incidents dedupe by `repo + fingerprint` instead of fingerprint alone.
