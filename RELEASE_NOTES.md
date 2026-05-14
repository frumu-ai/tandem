# Release Notes

This is the canonical release-notes file used by release tooling.

## v0.5.6 (Unreleased)

This release hardens approval-gate security and fixes race conditions in cache loading. Three critical vulnerabilities in channel interaction handlers are closed: missing user authorization in Slack/Discord/Telegram, Time-of-Check-Time-of-Use (TOCTOU) cache races allowing duplicate decisions, and path traversal in automation IDs. Additional medium-priority fixes include dedup TTL for webhook replay prevention and file permission validation.

**Security fixes are backport candidates** for deployments running v0.5.5 or earlier.

What ships now:

- **Authorization fix for approval gates**: Slack, Discord, and Telegram interaction handlers now verify that clicking users are in the configured `allowed_users` allowlist before processing approval/rework/cancel decisions. The fix applies `resolve_channel_user(ChannelKind)` at each handler entry point and rejects unauthorized users with `403 Forbidden`. Previously, any user in the platform workspace/server/group could click approval buttons and decide gates regardless of allowlist configuration.

- **TOCTOU race condition fix in automation run cache**: `update_automation_v2_run()` now records a timestamp before dropping the per-run mutation lock to load state from disk. After re-acquiring the lock, it validates that the in-memory entry wasn't modified during the load window (via `updated_at_ms > check_time_ms`). If stale, the loaded copy is skipped and the concurrent update wins, preventing lost gate decisions or duplicate task execution when two operators approve the same gate simultaneously.

- **Path traversal protection for automation identifiers**: Added `sanitize_path_id()` to replace unsafe characters in automation IDs and run IDs with underscores (safe set: alphanumeric + hyphen + underscore), and `validate_path_within_root()` to verify constructed paths stay within their base directory via canonicalization. Applied to `automation_v2_definition_shard_path()` and `automation_v2_run_history_shard_path()` to prevent attacks like `../../../etc/passwd`.

- **Dedup TTL for webhook replay prevention**: Discord and Slack interaction dedup rings now expire entries after 5 minutes, matching platform retry behavior (typically seconds-to-minutes). Updated `DedupRing` to track insertion timestamp and reject duplicates only if the key exists and the TTL window is still active, preventing stale entries from being replayed after ring eviction.

- **File permission validation on startup**: Added `check_file_permissions()` (Unix-only) that logs warnings if state files are world-readable or world/group-writable. Checks run on load of sensitive files: `bug_monitor_config`, `bug_monitor_log_watcher_state`, and `bug_monitor_intake_keys`. Does not fail startup but alerts operators to restrict permissions to mode 0600 (owner read/write only).

- **Empty run_id/node_id validation in Discord modal parsing**: Fixed critical gap where malformed modal custom_id (e.g., `tdm-modal:rework::`) would extract empty run_id/node_id and pass them to dispatch_decision. Now validates both are non-empty after parsing the `{run_id}:{node_id}` format, rejecting malformed requests with `400 Bad Request`.

- **Telegram dedup TTL implementation**: Applied timestamp-based deduplication with 5-minute expiration to Telegram, closing a replay window that existed because update_ids are small integers that can be reused. Previously only capacity-based eviction allowed stale entries to be replayed days later if the dedup ring was cleared or the service restarted.

- **User ID extraction: reject instead of default**: All three channel handlers now validate that user identification is present (Discord/Telegram from payload, Slack from extracted action), rejecting with `400 Bad Request` instead of defaulting to "unknown". Prevents accidental authorization of malformed requests and improves audit trail accuracy.

- **Reason field size validation**: Discord modal rework reason now capped at 4000 characters, matching the UI modal enforcement. Prevents storage exhaustion attacks via oversized reasons submitted via direct API.

- **Error message information disclosure prevention**: Authorization rejection messages changed from `"user {user_id} not in allowed_users"` to `"user not in allowed_users"`, eliminating user enumeration attacks while keeping full user_id in audit logs for operator investigation.

- **JWT structure and algorithm validation**: `decode_codex_jwt_claims()` now validates token structure (header.payload.signature with exactly 3 parts), rejects algorithm-substitution attacks by detecting and blocking `alg: "none"` tokens, validates header claims are present, and validates signature format.

- **JSON merge recursion depth limit**: Added `MAX_JSON_DEPTH` constant (64 levels) to prevent DoS attacks via deeply nested JSON merge operations in provider config handling. The `merge_json_with_depth()` function logs a warning and returns early when recursion exceeds the limit, preventing stack exhaustion.

- **CODEX_HOME path traversal protection**: Environment variable `CODEX_HOME` is now validated to reject paths containing `..`, paths starting with `-`, and absolute paths targeting system directories (`/etc`, `/sys`, `/proc`, `/root`, `/boot`). Invalid paths safely fall back to `~/.codex` with warning log.

- **JWT token expiration validation**: Tokens are now rejected if they lack the `exp` claim, instead of defaulting to 50-minute expiration. Timestamp validation detects unreasonable values (e.g., year 3000+) and rejects negative timestamps, preventing integer overflow during time arithmetic.

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
