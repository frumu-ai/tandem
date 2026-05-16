# Release Notes

This is the canonical release-notes file used by release tooling.

## v0.5.7 (2026-05-16)

Tandem 0.5.7 moves the project positioning and first domain-specific runtime hardening toward governed AI infrastructure for enterprise work. The release adds public enterprise runtime docs, a fintech strict-mode foundation for compliance and risk workflows, and the first runtime evidence paths needed to prove that Tandem can govern long-running AI work with scoped tools, citations, approvals, artifacts, audit events, and replayable run records.

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
- `eval_datasets/fintech_compliance_risk.yaml` adds proof-sprint fixtures for unsupported claim rejection, connector selected-but-unused rejection, protected-action bypass attempts, cross-tenant source denial, and incomplete evidence surfaced as limitations.
- Eval runner spec mapping now carries fintech runtime profile, tenant, and artifact-contract metadata into generated Automation V2 specs for stub/live evals.

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
