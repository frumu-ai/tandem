# Release Notes

Canonical release notes live in `docs/RELEASE_NOTES.md`.

## v0.4.2 (Unreleased)

- Desktop orchestrator + command center stabilization
  - Fixed orchestrator resume so runs with no tasks re-enter planning instead of getting stuck trying to execute an empty plan.
  - Restored run-list visibility across mixed storage by merging context runs with legacy local orchestrator runs.
  - Hardened run deletion for context runs by removing shared `data/context_runs/<run_id>` state and surfacing real delete failures.
  - Replaced native desktop `window.confirm` prompts in orchestrator controls with in-app confirmation dialogs.
  - Added in-app toast surfacing for payment/quota failures (`payment required`, credit-limit style provider errors).
  - Tuned planner guidance so non-trivial report/objective requests avoid collapsing into a single task.
  - Reduced terminal log spam by suppressing duplicate in-flight `tool.lifecycle.start` events for the same tool part even when provider args stream updates.
  - Fixed command center action visibility so selected runs reliably expose pause/cancel/continue/delete controls.

- Bug Monitor settings foundation and server config/status surface
  - Added persisted bug-monitor config in `tandem-server` for repo, selected MCP server, provider preference, and dedicated `model_policy.default_model` routing.
  - Added fail-closed readiness/status evaluation for selected model availability, MCP connectivity, and required GitHub capabilities.
  - Fixed the Bug Monitor settings-page initialization crash caused by early query access.
  - Changed reporter model selection to allow typed/manual model IDs with provider-backed suggestions, and fixed model persistence across reloads.
  - Generalized GitHub MCP capability readiness so arbitrary MCP server instance names can satisfy reporter issue capabilities.
  - Added reporter HTTP endpoints:
    - `GET /config/bug-monitor`
    - `PATCH /config/bug-monitor`
    - `GET /bug-monitor/status`
    - `GET /bug-monitor/drafts`
    - `GET /bug-monitor/drafts/{id}`
  - Control-panel Settings now has a dedicated `Bug Monitor` tab with repo/MCP/provider/model selection, readiness indicators, and recent draft visibility.
  - Added `#/bug-monitor` as the canonical route for direct access to the Bug Monitor settings surface.
  - Desktop Settings now has a matching engine-backed `Bug Monitor` card with MCP deep-linking and recent draft visibility.
  - Added a Tauri bridge for reporter config, status, draft listing, draft lookup, and manual draft submission.
  - Added `POST /bug-monitor/report` so desktop logs and failed orchestrator runs can create deduped local issue drafts through the engine.
  - Fixed the desktop sidecar reporter config path to use the canonical `GET/PATCH /config/bug-monitor` route.
  - Added engine-backed draft approval/deny actions at `POST /bug-monitor/drafts/{id}/approve` and `POST /bug-monitor/drafts/{id}/deny`, and surfaced those actions in desktop Settings.
  - Control-panel Settings now uses those same draft approval endpoints, keeping Bug Monitor decisions consistent across desktop and web surfaces.
  - Added `POST /bug-monitor/drafts/{id}/triage-run`, which promotes an approved draft into a minimal engine-owned `bug_monitor_triage` context run with seeded inspection and validation tasks.
  - Desktop and control-panel Settings can now create those triage runs directly from approved Bug Monitor drafts.
  - Control-panel Dashboard now includes those `bug_monitor_triage` context runs in the existing context-run visibility drawer.
  - Added `POST /bug-monitor/drafts/{id}/issue-draft`, which renders a template-aware issue draft artifact from the repo bug template before GitHub publish.
  - Bug Monitor GitHub publish now uses that rendered issue-draft artifact instead of opening issues directly from raw incident details.
  - Auto-publish now defers with `triage_pending` until a triage-backed issue draft exists, preventing premature low-signal issue creation.
  - Fixed Bug Monitor incident persistence so draft-creation failures leave a visible incident error instead of a half-created tracker row.
  - Approving a Bug Monitor draft no longer fails the operator action just because the follow-up GitHub publish step is blocked.
  - Split Bug Monitor readiness into local ingest vs GitHub publish readiness so live tracker surfaces can show “watching locally” when incident capture is healthy but GitHub posting is blocked.
  - Added `POST /bug-monitor/drafts/{id}/triage-summary` so Bug Monitor triage can persist a structured summary artifact for issue drafting.
  - Bug Monitor issue-draft generation now prefers that structured triage summary over raw incident detail when rendering the repo issue template.
  - Bug Monitor now suppresses duplicate incidents earlier in both runtime ingest and manual `POST /bug-monitor/report` flows by consulting stored `failure_pattern` memory before opening a fresh draft.
  - Bug Monitor incidents now persist a compact duplicate summary when suppression happens so tracker UIs can explain duplicate suppression after reload/reconnect without overloading the raw source-event payload.
  - Bug Monitor triage summaries now persist governed `failure_pattern` memory for subject `bug_monitor`, so structured triage can suppress later matching reports even without a prior coder-run artifact.
  - Approving a Bug Monitor draft without triage now also persists governed `failure_pattern` memory from the approved draft itself, so operator-approved issues still teach duplicate suppression.
  - `failure_pattern` memory now carries recurrence metadata and stronger issue-linkage metadata, and duplicate ranking uses recurrence as a tie-breaker after exact fingerprint matches.
  - Duplicate-suppressed Bug Monitor incidents now persist a normalized `duplicate_summary` envelope with match count, best-match details, recurrence metadata, and linked-issue unions so tracker UIs can explain suppression deterministically after reload/reconnect.
  - Manual `POST /bug-monitor/report` suppression now returns that same normalized `duplicate_summary` envelope, and failure-pattern matching reuses the exact-fingerprint -> recurrence -> score ordering so the reported best match stays aligned with runtime suppression.
  - Bug Monitor failure-pattern reuse responses now attach the same normalized `duplicate_summary` envelope alongside any raw `duplicate_matches`, and coder-originated duplicate matches now emit a stable `match_reason` so exact-fingerprint priority survives through shared ranking and summary shaping.

- Initial Tandem Coder engine API foundation
  - Added the first engine-owned coder endpoints:
    - `POST /coder/runs`
    - `GET /coder/runs`
    - `GET /coder/runs/{id}`
    - `GET /coder/runs/{id}/artifacts`
  - Coder runs now persist as thin metadata records linked to engine context runs rather than introducing a frontend-owned workflow store.
  - `issue_triage` coder run creation now seeds a deterministic context-run task template for issue normalization, memory retrieval, repo inspection, reproduction, and triage artifact writing.
  - Added initial `coder.run.created` engine event emission and backend regression coverage for coder create/get/list/artifact behavior.
  - Added an initial fail-closed readiness gate for `issue_triage`: required GitHub issue capability bindings must exist, and any explicitly requested MCP servers must be configured and connected.
  - Added `POST /coder/runs/{id}/memory-candidates` so `issue_triage` runs can persist engine-owned memory candidate payloads and attach them to the linked context run as `coder_memory_candidate` artifacts.
  - New `issue_triage` runs now seed their retrieval task with prior repo/issue memory candidate hints from earlier coder runs.
  - Added `POST /coder/runs/{id}/triage-summary` so the engine can write a concrete `triage.summary.json` artifact and attach it as `coder_triage_summary`.
  - Added `GET /coder/runs/{id}/memory-hits` so clients can inspect ranked triage retrieval hits for the current coder run.
  - `issue_triage` bootstrap now combines prior `coder_memory_candidate` payloads with project semantic memory search and writes a `coder_memory_hits` artifact into the linked context run.
  - Triage summary writes now auto-generate reusable `triage_memory` and `run_outcome` memory candidates so later coder runs can reuse structured triage conclusions without a separate manual candidate write.
  - `issue_triage` memory retrieval now also ranks governed/shared memory hits from the existing engine memory database alongside project semantic memory and prior coder-local candidates.
  - Added `POST /coder/runs/{id}/memory-candidates/{candidate_id}/promote` so reviewed coder memory candidates can be stored in governed memory and optionally promoted to shared visibility with reviewer metadata.
  - Added `POST /coder/runs/{id}/approve` and `POST /coder/runs/{id}/cancel` as thin coder control endpoints over the existing context-run transition model.
  - Those control endpoints now emit `coder.run.phase_changed`, and cancelled coder runs now project a dedicated `cancelled` phase.
  - `issue_triage` readiness now reuses the shared engine capability-readiness evaluator, so coder run creation blocks on the same missing/unbound/disconnected/auth-pending conditions surfaced by `/capabilities/readiness`.
  - Explicit `mcp_servers` requested by coder runs still remain hard requirements on top of that shared readiness check.
  - Coder memory promotion now reuses the generic governed-memory `memory_put` / `memory_promote` path instead of a coder-specific direct DB bridge.
  - Run-scoped governed-memory capability issuance is now shared through `skills_memory.rs` helpers, so coder workflows derive subject and tier policy through the same helper path as the generic memory routes.
  - Fixed cold-start global memory initialization so `/memory/*` routes create the memory DB parent directory before opening SQLite.
  - Coder lifecycle and artifact events now share a normalized payload shape, and `coder.artifact.added` includes explicit `kind` metadata so desktop and other clients can consume coder events without per-event special casing.
  - Added `POST /coder/runs/{id}/pr-review-summary` so `pr_review` runs can write a structured `coder_pr_review_summary` artifact and emit a first `run_outcome` memory candidate.
  - Added the first `pr_review` coder workflow skeleton with GitHub PR readiness checks, seeded review task graphs, and direct MCP GitHub pull-request capability bindings.
  - `pr_review` now defaults to pull-request-specific memory queries, bootstraps a `coder_memory_hits` artifact at run creation, and reuses prior review `run_outcome` memory during later reviews of the same repo/PR.
  - `pr_review` summary writes now also emit reusable `review_memory` candidates, and follow-on PR reviews can retrieve that review-specific memory through the shared coder memory-hits path.
  - `pr_review` summary writes now also emit `regression_signal` candidates when review input includes historical regression signals, and later PR reviews can retrieve those signals through the same repo/PR memory-hits path.
  - Added the first `merge_recommendation` coder workflow skeleton with PR-backed readiness checks, seeded merge-assessment tasks, bootstrapped `coder_memory_hits`, and `POST /coder/runs/{id}/merge-recommendation-summary` for structured merge recommendation artifacts.

## v0.4.1 (2026-03-07)

- Strict swarm write reliability and cross-client engine retries
  - Fixed streamed OpenAI/OpenRouter tool-call parsing so multi-chunk `write` args keep the correct tool-call identity and no longer lose later argument chunks when the provider omits the tool name on follow-up deltas.
  - Hardened write-argument recovery for truncated/malformed JSON: the engine can now recover `content` even when `path` is missing, and the default provider output budget was raised from `2048` to `16384` so large single-file artifacts are less likely to be clipped.
  - Session/tool history persistence now preserves write args/results through the verifier path, avoiding false `NO_TOOL_ACTIVITY_NO_WORKSPACE_CHANGE` / strict-write failure classifications when tools actually ran.
  - Swarm planner/worker prompts now favor one implementation task for single-file goals, reducing over-decomposition on greenfield artifact generation.
  - Added consistent local-engine retry handling for transient transport failures and `ENGINE_STARTING` responses across the control-panel orchestrator, Tauri desktop sidecar client, and Rust TUI client.

- Provider catalog honesty in Settings and `/provider`
  - `GET /provider` now returns explicit catalog metadata so clients can distinguish live remote model catalogs from config-defined catalogs and manual-entry-only providers.
  - Removed the synthetic single-model fallback behavior that made most non-OpenRouter providers appear to have exactly one available model.
  - Live remote model discovery is now attempted for supported providers (`openrouter`, `openai`, `groq`, `mistral`, `together`), while unsupported/non-generic providers stay configurable through manual model entry.
  - Control-panel Settings now renders provider rows more honestly:
    - real counts for remote catalogs
    - `configured models` for config-defined catalogs
    - `manual entry` when live discovery is unavailable

- Headless-first Chromium browser automation with readiness diagnostics
  - Added a new `tandem-browser` sidecar for local Chromium automation over stdio, with typed browser actions for open, navigate, snapshot, click, type, press, wait, extract, screenshot, and close.
  - Browser automation is now explicitly headless-first: it works on a VPS with no GUI as long as the sidecar and a Chromium-based browser are installed on the same host as `tandem-engine`.
  - Added readiness diagnostics that do real browser launch smoke tests, detect missing Chrome/Chromium/Edge/Brave installs, surface Linux install hints, and explain non-runnable states instead of silently omitting browser capability.
  - Added engine-managed sidecar install/distribution:
    - `tandem-engine browser install`
    - `POST /browser/install`
    - managed sidecar discovery from Tandem shared `binaries/` storage so standard installs do not need `TANDEM_BROWSER_SIDECAR`
    - standalone `tandem-browser-*` assets in the release workflows
  - Added operator/browser status surfaces across the stack:
    - `GET /browser/status`
    - browser summary on `GET /global/health`
    - `tandem-engine browser status`
    - `tandem-engine browser doctor`
    - `POST /browser/smoke-test`
    - `tandem-browser doctor --json`
    - TUI `/browser status` and `/browser doctor`
    - control-panel Browser Diagnostics card in Settings with sidecar install and smoke-test actions
  - Replaced the generic model-facing browser action shape with typed engine tools:
    - `browser_status`
    - `browser_open`
    - `browser_navigate`
    - `browser_snapshot`
    - `browser_click`
    - `browser_type`
    - `browser_press`
    - `browser_wait`
    - `browser_extract`
    - `browser_screenshot`
    - `browser_close`
  - Browser tool registration now works in both server and one-shot runtime paths, and `browser_status` remains available even when full browser execution is blocked.
  - Screenshots and oversized extracts now persist as artifacts/files rather than being returned as large inline base64 payloads.
  - Session cancel, global dispose, stale-run reaping, and routine-pause flows now clean up tracked browser sessions to avoid leaked headless browser state.
  - Added an engine-owned smoke test that opens `https://example.com`, snapshots it, extracts visible text, and closes the browser session so operators can validate the full runtime path from the control panel or direct API calls.
  - Sidecar stdio failures now preserve sidecar stderr and retry once after an unexpected disconnect before returning an error.

- Orchestrator multi-run fan-in + run workspace parity
  - tandem-engine now provides multiplex orchestration event fan-in at `GET /context/runs/events/stream` so one SSE connection can track multiple run IDs concurrently.
  - stream cursor decode/encode support was added so reconnects can resume from the last acknowledged event window.
  - control panel now exposes `/api/orchestrator/events` backed by multiplex stream proxying with legacy fallback logic for older engine behavior.
  - added `/api/orchestrator/events/health` to quickly verify orchestrator streaming status and fallback mode.
  - control panel orchestration state now tracks a run registry (active + historical runs) instead of single-run replacement.
  - added a dedicated Orchestrator workspace view with run selection and start-state behavior aligned to desktop orchestration UX.
  - orchestration and chat history UI now use a consistent history icon style for session/run lists.

- Blackboard as central coordination layer + control panel parity
  - Engine blackboard now includes first-class task coordination state (`blackboard.tasks`) with workflow references, task lineage fields, lease metadata, retries, and optimistic task revision (`task_rev`).
  - Added append-only task patch operations:
    - `add_task`
    - `update_task_lease`
    - `update_task_state`
  - Added task lifecycle endpoints:
    - `POST /context/runs/{run_id}/tasks`
    - `POST /context/runs/{run_id}/tasks/claim`
    - `POST /context/runs/{run_id}/tasks/{task_id}/transition`
    - `GET /context/runs/{run_id}/blackboard/patches`
  - Added task lifecycle event emission on context runs (`context.task.created/claimed/started/completed/failed/...`) carrying `patch_seq` + `task_rev` for deterministic UI refresh.
  - Added optional `context_run_id` support on `/pack-builder/preview`, `/pack-builder/apply`, `/pack-builder/cancel`, and `/pack-builder/pending`; when provided, pack-builder lifecycle is mirrored into blackboard task state/events for that context run.
  - `automation_v2_dag` node status now projects into context blackboard tasks; `GET /automations/v2/runs/{run_id}` now includes `contextRunID` for that projection.
  - Added optional `context_run_id` on `/skills/router/match` and `/skills/compile`; skill routing/compile outputs now emit blackboard task records for workflow coordination.
  - Desktop convergence: Tauri `orchestrator_get_blackboard` and `orchestrator_get_blackboard_patches` now read engine context-run blackboard/patch streams first, with local orchestrator storage as temporary fallback.
  - Additional desktop convergence: legacy read commands (`orchestrator_get_events`, `orchestrator_list_runs`, `orchestrator_load_run`) now use engine context-run APIs first, with local fallback only for legacy run data.
  - Added compatibility guard for legacy persisted blackboards that predate `tasks`; engine now loads them with safe defaults.
  - Control panel SSE parity: `/api/swarm/events` now emits incremental `blackboard_patch` events alongside run events so blackboard-only updates refresh live.
  - Swarm task board UI hardening: very long task/prompt titles now clamp and wrap by default with `More/Less` expansion, preventing layout blowout.
  - Replay now includes blackboard parity checks for task revision/count/status and returns replayed/persisted blackboard payloads for drift diagnostics.
  - Control panel swarm shim now forwards blackboard patch streams, and `SwarmPage` now supports blackboard docked, expanded, and fullscreen debug views with decision lineage, agent lanes, workflow progress, artifact lineage, and drift indicators.
  - Added contract/regression tests for claim races, command-id idempotency, optimistic revision mismatch, monotonic patch sequence, and replay parity.
  - Fixed Swarm Continue/Resume no-op path in control panel executor:
    - if driver returns no `selected_step_id` because a step is already `in_progress`, executor now resumes that step instead of exiting
    - continue/resume API responses now include execution diagnostics (`started`, `requeued`, `selectedStepId`, `whyNextStep`)
    - Swarm page now surfaces `lastError` to make provider/session failures visible without inspecting server logs
    - execution sessions now fall back to configured swarm provider/model when older runs do not have provider/model fields populated
  - Added swarm fail-closed model resolution and dispatch guards:
    - execution model resolution now follows `run model -> swarm state model -> engine default provider/model` and fails fast if unresolved
    - `prompt_sync` empty/no-op responses now fail the step with explicit diagnostics instead of reporting false completion
    - loop guard stops repeated same-step execution when step state does not advance after completion (`STEP_STATE_NOT_ADVANCING`)
    - compatibility reconcile path now marks stale completed steps as `done` via engine API when run-state transitions lag, and emits `step_completion_reconciled`
    - `/api/swarm/status` now returns resolved model + executor state/reason for faster diagnosis
  - Added run list cleanup controls in control panel:
    - per-run `Hide` action and bulk `Hide Completed` action in Swarm view
    - hidden runs are user-scoped and persisted locally (`~/.tandem/control-panel/swarm-hidden-runs.json`)
    - hidden runs are filtered from `/api/swarm/runs` by default without deleting engine run data
  - Fixed completed-run false error UI state and added explicit output visibility:
    - normal completion no longer sets swarm `lastError` to `all steps are done; marking run completed`
    - Swarm now shows a `Run Output` panel with latest completed step, session ID, and assistant output preview
    - task `Open Session` links now resolve from step completion events (`session_id`) for reliable output access
    - status badges now use semantic colors for done/completed, failed, and in-progress states
  - Increased duplicate-signature retry headroom for write/edit loops:
    - `write`/`edit`/`multi_edit`/`apply_patch` duplicate call signature limit now defaults to `200` (was `3`)
    - `pack_builder` remains strict at `1`; shell tools remain strict at `2`
    - global override `TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT` remains supported
- Automation creation UX — simplified to "just describe what you want"
  - Replaced the fragmented `Agents`, `Packs`, and `Teams` pages with a single **Automations** hub (`AutomationsPage`).
  - New **4-step creation wizard**: describe your goal in plain English → pick a recurring schedule → choose how agents run → review & deploy. No YAML, no route navigation between pages.
  - **Execution mode selector** surfaces the orchestration options most users never found before: `Single Agent`, `Agent Team` (recommended), or `Swarm`. Agent Team is now the default — you get an orchestrated multi-agent run out of the box.
  - **My Automations** tab combines installed packs, scheduled routines, and recent run history in one scrollable view.
  - **Teams & Approvals** tab shows active agent-team instances and pending spawn approvals so operators can approve/deny without leaving the page.
  - Legacy deep-links (`#/agents`, `#/packs`, `#/teams`) all redirect to `/automations` — existing bookmarks keep working.
  - Primary sidebar trimmed from 12 items to 7: Dashboard, Chat, Automations, Swarm, Memory, Live Feed, Settings.

- Pack Builder orchestration/swarm execution modes
  - `pack_builder` tool now accepts `execution_mode` (`single` | `team` | `swarm`) and `max_agents` fields.
  - Generated routine YAML now includes an `args.orchestration` block so the runtime knows to dispatch to an agent team or parallel swarm instead of a single loop.
  - Default changed to `team` — new packs created via chat now schedule against an orchestrated agent team by default.

- Pack Builder zip storage race condition fixed
  - Generated pack zip artifacts are now written to a persistent directory (`~/.tandem/data/pack_builder_zips/` or `TANDEM_STATE_DIR/pack_builder_zips/`) instead of the OS temp directory.
  - This closes a silent failure mode where OS-level temp cleanup between the `preview` and `apply` steps caused `preview_artifacts_missing` errors, making pack creation via chat unreliable in practice.
  - Old staging directories for evicted plan IDs are now cleaned up automatically.

## v0.4.0 (2026-03-03)

- Pack Builder MCP-first automation flow (`v0.4.1` scope)
  - Added built-in `pack_builder` tool with `preview` and `apply` modes for creating Tandem packs from plain-English goals.
  - External capabilities are now connector-first: builder maps to MCP catalog servers by default and only falls back to built-ins when no match exists (with warnings).
  - Generated mission/agent artifacts now include explicit `mcp.*` tool IDs so routines invoke registered connectors directly.
  - Preview now returns connector candidates (docs/transport/auth/setup), selected MCP mapping, required secrets, and explicit approval requirements.
  - Apply now enforces user approvals for connector registration and pack install; routines are registered paused by default unless explicitly enabled.
  - Added persisted `pack_presets` metadata with registered MCP servers and required credentials for reliable preset reloads.
  - Added channel intent routing to automatically dispatch pack-creation requests to the `pack_builder` agent.
  - Added MCP-focused regression tests across server HTTP, preset registry, and channel routing.
  - Fixed engine boot race where pre-ready background tasks could panic with `runtime accessed before startup completion`; startup loops now gate on runtime readiness.
  - `pack_builder` is now baseline-allowed in engine permission defaults so first-run pack creation from chat/channels does not timeout on tool approval prompts.
  - Added runtime loop guardrails for `pack_builder` to reduce token waste on repeated identical tool calls (terminal follow-up behavior + duplicate-signature limit `1`).

- Semantic tool retrieval for MCP-heavy toolsets (`v0.4.1` scope)
  - Added embedding-backed semantic tool retrieval in `ToolRegistry` so the engine can send a relevant top-K tool subset instead of all schemas every turn.
  - Added startup bulk indexing (`tools.index_all`) plus incremental indexing on runtime tool registration (`register_tool`) so MCP servers connected after startup are indexed immediately.
  - Added index cleanup on `unregister_tool` and `unregister_by_prefix` so MCP disconnect/refresh does not leave stale vectors.
  - Engine loop now uses semantic retrieval by default (`TANDEM_SEMANTIC_TOOL_RETRIEVAL=1`) with `TANDEM_SEMANTIC_TOOL_RETRIEVAL_K` default `24` (aligned with existing expanded tool cap behavior).
  - Explicit allowlist/policy matches are unioned from the full tool list to prevent required tools from being dropped by top-K retrieval.
  - Runtime system prompt now includes a compact connected-integrations catalog (MCP server names only), gated by `TANDEM_MCP_CATALOG_IN_SYSTEM_PROMPT`.
  - Graceful fallback remains intact: when embeddings are unavailable/disabled, retrieval falls back to full tool listing.
  - Reliability hardening:
    - action-heavy prompts now trigger automatic fallback to full tool list when semantic top-K omits required web/email tool families
    - non-offered tool calls are rejected with available-tool hints instead of silently executing guessed names
    - assistant cannot claim email delivery succeeded unless a successful email-like tool action was executed in the run

- Pack Builder autonomous routines + routine-run connect reliability (`v0.4.1` scope)
  - Pack Builder apply now defaults to autonomous execution for unattended automation:
    - generated routines are enabled by default (`status=active`)
    - generated routines default to `requires_approval=false`
    - pack-builder apply API defaults approval flags to `true` unless explicitly overridden
  - Generated routine YAML from pack-builder now reflects no-approval default.
  - Increased default provider stream connect timeout from `30000` to `90000` ms to reduce routine runs failing before the model begins streaming.
  - Updated env/setup defaults accordingly:
    - `.env.example`
    - `examples/vps-web-portal/engine.env.example`
    - quickstart/VPS setup scripts

- Routine builder MCP tool selection UX (`v0.4.1` scope)
  - The routine editor now includes an MCP tool picker so users can search discovered MCP tools and add them directly to the routine allowlist.
  - Added connected-server filtering to quickly scope tool selection to a specific MCP integration.
  - Prevents fragile manual typing of long `mcp.*` tool IDs in routines.

- MCP catalog moved into engine and exposed to frontends
  - Added engine-managed embedded catalog endpoints:
    - `GET /mcp/catalog`
    - `GET /mcp/catalog/{slug}/toml`
  - Added generated catalog assets under `crates/tandem-server/resources/mcp-catalog` (index + per-server TOML manifests).
  - Added catalog generator tooling at `scripts/generate-mcp-catalog.mjs` plus control-panel refresh script `npm run mcp:catalog:refresh`.
  - Catalog now includes curated official remote MCP entries for:
    - GitHub: `https://api.githubcopilot.com/mcp/`
    - Jira (Atlassian): `https://mcp.atlassian.com/v1/mcp`
    - Notion: `https://mcp.notion.com/mcp`
  - Default curated GitHub pack no longer assumes Docker; it points to the official remote endpoint.

- MCP capability readiness preflight (fail-closed)
  - Added `POST /capabilities/readiness` for pre-run validation of required capability IDs.
  - Readiness output now reports blocking issues for:
    - missing capability bindings
    - unbound required capabilities
    - missing/disconnected required MCP servers
    - auth-pending MCP tools
  - Added SDK/UI surface parity:
    - TypeScript SDK: `client.capabilities.readiness(...)`
    - Tauri command and frontend wrapper: `capability_readiness` / `capabilityReadiness(...)`
    - Control-panel MCP and Pack builder flows now call readiness before key save/run paths.

- Control panel MCP UX now includes searchable remote pack catalog
  - MCP settings page now displays searchable “Remote MCP Packs” sourced from engine catalog.
  - Added quick actions to apply pack transport/name and open generated TOML.
  - Added MCP-side readiness check UI with structured result rendering.

- Desktop (Tauri) MCP UX now includes searchable remote catalog
  - Added Tauri-side `mcp_catalog` command/wrapper to consume engine `GET /mcp/catalog`.
  - Extensions -> Integrations now includes searchable “Remote MCP catalog” list.
  - Added “Apply” action to prefill remote server name/URL and “Docs” quick-open links.

- Engine provider auth persistence + status API overhaul
  - Provider keys set via `PUT /auth/{provider}` are now durable across engine restarts.
  - Added engine-wide provider auth persistence in `tandem-core` with keychain-first storage and secure file fallback.
  - Engine startup now restores persisted provider keys into runtime provider config before provider registry init.
  - `GET /provider/auth` now returns real per-provider status including `has_key`, `configured`, `connected`, and `source`.
  - `DELETE /auth/{provider}` now removes both runtime and persisted provider auth state.

- Web control panel provider onboarding reliability
  - Provider readiness now enforces key presence for providers that require API keys (non-local providers).
  - Settings “Test Model Run” now pins explicit provider/model and blocks early when key prerequisites are missing.
  - Custom provider IDs are normalized consistently during save/test/delete key flows.
  - Reduced false “No stored key detected” states by consuming the real `/provider/auth` status response.
  - Replaced browser-native delete confirms with themed in-app confirmation modals for session, file, and pack deletion actions.
  - Moved toast notifications to the top-center anchor for better visibility across wide layouts.
  - Grouped Automations tabs and tab content inside one shared panel container for a cleaner, consistent layout.

- Marketplace Pack architecture/spec expansion
  - Added a full marketplace-ready spec set under `specs/packs/`:
    - `MARKETPLACE_PACK_REQUIREMENTS.md`
    - `PUBLISHING_AND_TRUST.md`
    - `STORE_LISTING_SCHEMA.md`
    - `DIFF_V1_TO_MARKETPLACE.md`
  - Promoted pack identity fields to core manifest requirements in the spec model:
    - top-level immutable `pack_id`
    - top-level `manifest_schema_version`
  - Added explicit `contents` list validation requirements so installers can verify pack completeness before registration.
  - Added trust/signing hooks:
    - root `tandempack.sig`
    - publisher verification tiers (`unverified`, `verified`, `official`)
    - signature/trust status requirements for install UX
  - Added marketplace scanner/rejection policy coverage:
    - marker validation
    - archive safety checks
    - secret detection
    - SPDX/license checks
    - portability flagging for provider-specific dependencies
  - Added strict routine safety defaults in spec:
    - routines installed disabled by default
    - auto-enable allowed only under trusted source + explicit policy.

- New marketplace-ready example packs
  - Added `examples/packs/skill_minimal_marketplace/`.
  - Added `examples/packs/workflow_minimal_marketplace/`.
  - Each template includes a marketplace-ready `tandempack.yaml`, required content files, README, and sample marketplace assets/changelog.

- Modular Preset System specification (enterprise scale)
  - Added comprehensive preset architecture docs under `specs/presets/`:
    - `PRESET_CONCEPTS.md`
    - `PRESET_STORAGE_AND_OVERRIDES.md`
    - `PROMPT_COMPOSITION.md`
    - `UI_REQUIREMENTS.md`
    - `API_CONTRACT.md`
    - `IMPLEMENTATION_PLAN.md`
  - Defined three core entities:
    - `SkillModule` (reusable capability/prompt building block)
    - `AgentPreset` (base persona + composed modules + policy profile)
    - `AutomationPreset` (mission DAG + routines + task-agent bindings)
  - Added deterministic prompt composition contract:
    - fixed ordering phases
    - params schema validation
    - stable separators and composition hash requirements
  - Added capability and policy merge semantics:
    - capability union with required dominance
    - least-privilege policy merge with deny precedence
  - Added immutable source + fork override model:
    - built-ins and installed packs are read-only
    - editing creates project-local fork/override
    - tracking-fork update/diff semantics with scope-increase re-approval gate
  - Added shared PackManager + PresetRegistry API contracts designed for both frontends:
    - Tandem Desktop (Tauri)
    - `packages/tandem-control-panel`
  - Added explicit chat attachment ingestion contract for pack detection/install cards (`tandempack.yaml` marker-driven).

- Preset registry runtime/API foundation (first tranche)
  - Added layered preset indexing in server runtime across:
    - built-ins
    - installed packs
    - project overrides
  - Added `GET /presets/index` endpoint returning unified index shape:
    - `skill_modules`
    - `agent_presets`
    - `automation_presets`
    - `generated_at_ms`
  - Added deterministic prompt composition endpoint:
    - `POST /presets/compose/preview`
    - enforces deterministic ordering (`core` -> `domain` -> `style` -> `safety`)
    - returns stable `composition_hash` and ordered fragment IDs for testability
  - Added immutable-source fork/edit/save APIs for project overrides:
    - `POST /presets/fork` (fork from builtin/pack/runtime path into overrides)
    - `PUT /presets/overrides/{kind}/{id}` (save editable override)
    - `DELETE /presets/overrides/{kind}/{id}` (remove override)
  - Added capability summary API for agent + automation composition:
    - `POST /presets/capability_summary`
    - merges required/optional capability sets with required precedence
    - returns normalized agent view, automation view, and totals
  - Added project override export API:
    - `POST /presets/export_overrides`
    - bundles project override presets into a portable zip with root `tandempack.yaml`
  - Preset index records now include parsed `publisher` and `required_capabilities` metadata for UI filtering.

- PackManager runtime/API implementation (first tranche)
  - Added initial server PackManager endpoints:
    - `GET /packs`
    - `GET /packs/{selector}`
    - `POST /packs/install`
    - `POST /packs/install_from_attachment`
    - `POST /packs/uninstall`
    - `POST /packs/export`
    - `POST /packs/detect`
    - `GET /packs/{selector}/updates` (stub)
    - `POST /packs/{selector}/update` (stub)
  - Added zip root-marker detection for pack eligibility (`tandempack.yaml` at archive root only).
  - Added install safety controls for zip extraction:
    - path traversal rejection
    - max file count/size/depth enforcement
    - max extracted bytes guardrail
    - entry/archive compression-ratio guardrails to reduce zip-bomb risk
  - Added deterministic pack install/index layout under `TANDEM_HOME/packs` with atomic index updates.
  - Added per-pack install/uninstall locking (by pack name) while preserving atomic index writes.
  - Added install lifecycle event emission for UI progress/status:
    - `pack.detected`
    - `pack.install.started`
    - `pack.install.succeeded`
    - `pack.install.failed`
    - `registry.updated`
  - Added/expanded pack route regression coverage:
    - `detect` returns `is_pack=false` for zip files without root marker
    - install writes deterministic `.../packs/<name>/<version>` and updates `current`
    - detect/install emit expected pack lifecycle events for UI surfaces
  - Pack inspect now returns computed trust/risk summary derived from installed files/manifest:
    - signature status reflects root `tandempack.sig` presence (`present_unverified` vs `unsigned`)
    - publisher verification tier is surfaced and normalized into UI-safe badge levels (`unverified`, `verified`, `official`)
    - risk summary includes capability counts, routine declaration flag, and non-portable dependency signal
  - Added structured `permission_sheet` in pack inspect responses for pre-install/install review UX:
    - required/optional capabilities
    - provider-specific dependency list
    - routine declaration list + enabled state
    - derived risk level (`standard` or `elevated`)
  - Added optional local secret-scanning enforcement in install flow:
    - scanner checks extracted text files for common high-risk token patterns
    - examples/placeholders (e.g. `secrets.example.env`, `.example`) are ignored
    - strict reject mode enabled with `TANDEM_PACK_SECRET_SCAN_STRICT=1`
  - Update check/apply stubs now return structured `permissions_diff` and `reapproval_required` flags for future permission re-approval workflows.

- Capability Resolver runtime/API implementation (first tranche)
  - Added capability endpoints:
    - `GET /capabilities/bindings`
    - `PUT /capabilities/bindings`
    - `GET /capabilities/discovery`
    - `POST /capabilities/resolve`
  - Added data-driven capability bindings storage at:
    - `TANDEM_HOME/packs/bindings/capability_bindings.json`
  - Added runtime discovery of available tools from:
    - MCP `list_tools()` namespaced catalog
    - local tool registry schemas
  - Added provider preference-based resolution with MVP default priority:
    - `composio` -> `arcade` -> `mcp` -> `custom`
  - Added alias-aware tool-name matching in resolver:
    - supports separator/casing variation via normalized matching
    - supports explicit per-binding `tool_name_aliases`
  - Expanded curated capability spine defaults for GitHub + Slack bindings across Composio/Arcade/MCP/custom without introducing full-catalog mapping.
  - Added structured resolver conflict payload (`missing_capability`) for unresolved required capabilities.
  - Added resolver test coverage for explicit provider preference selection when both Composio and Arcade tool mappings are present.

- TypeScript SDK parity updates (`@frumu/tandem-client`)
  - Added `client.packs` namespace:
    - `list`, `inspect`, `install`, `installFromAttachment`, `uninstall`, `export`, `detect`, `updates`, `update`
  - Added `client.capabilities` namespace:
    - `getBindings`, `setBindings`, `discovery`, `resolve`
  - Added public TS types for pack and capability contracts used by these APIs.

- Python SDK parity updates (`tandem-client-py`)
  - Added `client.packs` namespace:
    - `list`, `inspect`, `install`, `install_from_attachment`, `uninstall`, `export`, `detect`, `updates`, `update`
  - Added `client.capabilities` namespace:
    - `get_bindings`, `set_bindings`, `discovery`, `resolve`
  - Added README examples for pack and capability workflows.

- Channel attachment ingestion updates (pack-aware)
  - Channel dispatcher now inspects `.zip` uploads via `/packs/detect` to trigger Tandem Pack detection flow.
  - Added trusted-source auto-install policy for channel uploads:
    - `TANDEM_PACK_AUTO_INSTALL_TRUSTED_SOURCES`
  - For trusted sources, dispatcher calls `/packs/install_from_attachment` automatically.
  - For untrusted sources, dispatcher responds with manual install guidance instead of auto-installing.

- Rust UI client parity updates (`tandem-tui` network client)
  - Added pack API helpers:
    - `packs_list`, `packs_get`, `packs_install`, `packs_uninstall`, `packs_export`, `packs_detect`, `packs_updates`, `packs_update`
  - Added capability API helpers:
    - `capabilities_bindings_get`, `capabilities_bindings_put`, `capabilities_discovery`, `capabilities_resolve`
  - Added preset API helpers:
    - `presets_index`, `presets_compose_preview`, `presets_capability_summary`, `presets_fork`, `presets_override_put`
  - Added desktop command-first preset builder flows in TUI:
    - `/preset index`
    - `/preset agent compose <base_prompt> :: <fragments_json>`
    - `/preset agent summary required=<csv> [:: optional=<csv>]`
    - `/preset agent fork <source_path> [target_id]`
    - `/preset automation summary <tasks_json> [:: required=<csv> :: optional=<csv>]`
    - `/preset automation save <id> :: <tasks_json> [:: required=<csv> :: optional=<csv>]`

- Control Panel Pack Library UI (`packages/tandem-control-panel`)
  - Added dedicated `Packs` management view (now launched from `Settings`).
  - Added Pack Library view with actions:
    - list installed packs
    - inspect metadata
    - install from URL/path
    - export
    - uninstall
    - update checks and update stub calls
  - Added capability discovery action to inspect currently discovered tools from UI.
  - Added inspect-time trust/risk summary panel in Pack Library:
    - verification badge + signature status
    - required/optional capability counts
    - provider-specific dependency count
    - routines declared/enabled summary
  - Pack update actions now surface `reapproval_required` warnings when update permission scope expands.
  - Added Skill Module Library in the Packs view:
    - data source: `/presets/index`
    - filters: text/id/tag/layer, publisher, required capability
  - Added Agent Preset Builder in the Packs view:
    - source preset selection + fork action
    - deterministic prompt preview (`/presets/compose/preview`)
    - capability summary (`/presets/capability_summary`)
    - save override (`PUT /presets/overrides/agent_preset/{id}`)
  - Added Automation Preset Builder in the Packs view:
    - task-agent binding rows with add/remove and per-step capability inputs
    - merged automation capability summary (`/presets/capability_summary`)
    - save override (`PUT /presets/overrides/automation_preset/{id}`)
  - Updated control-panel information architecture for settings-centric management:
    - moved `Packs`, `Channels`, `MCP`, and `Files` out of primary sidebar nav
    - added settings tabs for these surfaces under a unified `Settings` view (`General`, `Packs`, `Channels`, `MCP`, `Files`)
    - added migration prompts in Automations and legacy pages to route users to Settings

- Control Panel pack event cards + actions
  - Added `pack.*` event-specific cards in `Live Feed` with direct actions:
    - open Pack Library
    - install from detected path
    - install from attachment metadata (`attachment_id` + `path`)
  - Added `Pack Events` rail in Chat with the same one-click actions so pack detection/install can be handled without leaving chat context.

- Control panel packs compatibility fix
  - Fixed `Packs` view failures on environments where `state.client.packs` or `state.client.capabilities` namespaces are not present by adding direct `/api/engine/*` fallback calls for list/inspect/install/uninstall/export/update/discovery.
- Settings tab visual polish
  - Reworked Settings section switching UI from generic buttons to dedicated tab styling for stronger active-state clarity and cleaner presentation.
  - Kept Settings content grouped under one parent Settings card and fixed missing tab icon rendering by registering required Lucide icons (`package`, `sliders-horizontal`).
  - Added an Appearance section in Settings with a shared theme selector backed by `tandem.themeId`, and applied desktop-style color/font tokens at control-panel boot for tighter desktop parity.
  - Updated control-panel shell styles (cards, nav, buttons, inputs, tabs) to consume theme tokens for cleaner first paint and more consistent transitions across route/tab changes.
  - Replaced plain loading copy with themed skeleton placeholders and tokenized sidebar/brand surfaces for a more polished first impression during route changes.
  - Expanded registered Lucide icon set used in packs/preset/settings surfaces to avoid missing icons during rerenders (`archive`, `copy-plus`, `sparkles`, `shield-check`, `arrow-up-circle`, `badge-check`, `binary`, `list`, `pencil`).
  - Fixed Settings tab icon disappearance on section switches by rehydrating icons at full Settings view scope (not only inner subview scope).
  - Updated Chat surface styling to shared theme tokens (removed hardcoded `zinc/slate` color islands in chat rails/messages/composer and dynamic chat cards), so changing theme in Settings now applies across chat UI as expected.
  - Improved Porcelain light-theme readability by darkening text/border token values and applying markdown light-mode contrast overrides for clearer chat and formatted content.
  - Updated Automations page theme fidelity:
    - wrapped automations UI in a scoped theme surface
    - remapped residual `slate/zinc` utility classes to shared tokens
    - converted automations tab chips and wizard step chips to token-based colors
  - Added starter import templates for hands-on learning:
    - `examples/packs/daily_github_pr_reviewer/`
    - `examples/packs/slack_release_notes_writer/`
    - `examples/packs/customer_support_drafter/`
  - Added a practical personal tutorial to build/zip/import/run your first pack:
    - `specs/packs/PERSONAL_TUTORIAL_FIRST_PACK.md`
  - Fixed Settings icon disappearance edge cases by rehydrating icons after async tab-content mutations.
  - Updated Automations (`#/agents`) top tabs to the same Settings-style underline tab treatment for consistent UX.
  - Restored multi-theme switching in Settings Appearance:
    - full theme list is back (`Web Control`, `Electric Blue`, `Emerald Night`, `Hello Bunny`, `Porcelain`, `Neon Riot`)
    - dropdown + quick-swatch selectors both apply themes instantly across the control panel
  - Hardened provider test behavior in Settings:
    - removed mixed `prompt_sync` + `prompt_async` test path
    - now runs a single async probe request to avoid run-conflict/stuck-wait behavior
    - extended provider-test wait timeout and improved status copy for non-`READY` successful replies
  - Isolated provider-test sessions from normal chat UX:
    - provider test uses internal non-workspace session metadata
    - chat session lists now hide internal `__provider_test__` sessions

- Internal execution tracking
  - Added implementation Kanban board:
    - `docs/internal/PACKS_PRESETS_IMPLEMENTATION_KANBAN.md`

## v0.3.28 (Unreleased)

- Control panel UX and workflow hotfixes
  - Replaced login hero animation with a uniform silicon-chip/data-flow visual.
  - Clicking `New` in chat now auto-collapses the history sidebar.
  - Added dashboard charts/summary cards for run and automation activity to improve at-a-glance operational visibility.
  - Added first-class **Automations + Cost** dashboard section with token and estimated USD KPIs (`24h`/`7d`) and top automation/routine cost breakdown.
  - Updated control panel automation copy to present advanced automation features without exposing internal V2 labels.
  - Refactored the `Automations` workspace (`#/agents`) into tabs: `Overview`, `Routines`, `Automations`, `Templates`, and `Runs & Approvals`.
  - Added guided walkthrough wizard (first-run + manual launch) for both routine and advanced automation setup flows.
  - Added URL-deep-linkable Automations UI state (`tab`, `wizard`, `flow`, `step`) for support and team handoff links.
  - Added lightweight Motion animation runtime for smoother tab/wizard panel transitions.
  - Extended animation coverage across routed views (cards/list/nav) with reduced-motion safeguards.
  - Fixed Swarm refresh race where polling/SSE re-renders could leave Swarm UI content visible after route changes.
  - Clarified Swarm positioning in UI as live orchestration (`Swarm (Live)`), with guidance to use Automations for persistent scheduled flows.
  - Fixed same-route hash/query navigation churn causing visible full-page flash on tab/wizard interactions (notably Automations), by using soft in-place rerenders and stale-render guards.
  - Improved Automation Builder clarity for per-agent model routing: provider/model now use settings-aware dropdowns with custom override options instead of manual free-text-only entry.
  - Fixed Automation Builder custom selection controls so choosing `Custom provider` / `Custom model` correctly enables manual input fields and keeps model selection usable.
  - Simplified Automation Builder agent policy inputs:
    - Skills input is plain text tags (comma-separated), not markdown/file-based.
    - MCP policy now uses connected-server selections from MCP config.
    - Tool policy now uses clear modes (`Standard`, `Read-only`, `Custom allow/deny`) instead of raw CSV-only mental model.
- Chat reliability and approvals-state fixes
  - Fixed delayed user-message rendering: user messages now appear immediately on send (optimistic render).
  - Fixed missing right-rail tool activity by normalizing additional tool event families (`session.tool_call`, `session.tool_result`, and tool message-part updates).
  - Fixed stale approval requests not clearing: pending-status filtering, `once` semantics for one-shot approvals, and session-change refresh now keep approvals list consistent.
- MCP and Composio connection fixes
  - Web control panel MCP form now supports auth modes (`auto`, `x-api-key`, `bearer`, `custom`, `none`) with Composio-aware auto-header behavior.
  - MCP connect failures now surface server-side `last_error` details in UI/toasts.
  - Fixed runtime MCP parser to accept streamable/SSE JSON-RPC responses during discovery (`initialize`, `tools/list`), resolving Composio `Invalid MCP JSON response: expected value at line 1 column 1` failures.
- Persistent Automations V2 backend rollout (additive APIs)
  - Added `automations/v2` endpoints for create/list/get/patch/delete, run-now, automation pause/resume, run pause/resume/cancel, run history, and SSE events.
  - Added persistent V2 stores (`automations_v2.json`, `automation_v2_runs.json`) with checkpoint metadata for resumable runs.
  - Added V2 scheduler/executor loops with DAG node dispatch and run checkpoint updates.
- Per-agent model selection in V2 flows
  - Added per-agent `model_policy` support and node-level model resolution, enabling mixed-cost agent fleets (cheap models for easy tasks, stronger models for hard tasks).
- Automation cost telemetry and accounting
  - Added run-level `prompt_tokens`, `completion_tokens`, `total_tokens`, and `estimated_cost_usd` fields for routine/automation records.
  - Added provider-usage aggregation to attribute token usage to active automation/routine runs.
  - Added configurable estimation rate via `TANDEM_TOKEN_COST_PER_1K_USD` for dashboard cost metrics.
- Scheduler + policy enforcement hardening
  - Replaced cron no-op behavior with real cron evaluation (timezone-aware next-fire + misfire handling).
  - Tool allowlist/denylist enforcement now supports wildcard/prefix matching (`*`, `mcp.github.*`, `mcp.composio.*`) in runtime/session/capability checks.
- Hard pause behavior for active routine runs
  - Pausing a running routine now cancels active tracked session IDs immediately (not status-only), and pause responses include canceled session IDs.
- SDK/API management parity
  - Added API support for agent template lifecycle (`POST/PATCH/DELETE /agent-team/templates`).
  - Added TypeScript client support for `automationsV2` and agent-template create/update/delete methods.
  - Added Python client parity with `client.automations_v2` and agent-template create/update/delete methods.
- Control panel V2 builder and operator flow
  - Added Automation Builder V2 UI in `Agents` view with mission metadata, schedule fields, dynamic agent rows, per-agent model policy inputs, and DAG node editor.
  - Added Automations V2 list controls for run-now and pause/resume, plus run inspector actions for pause/resume/cancel.
  - Added one-click presets:
    - Engineering: `GitHub bug hunter`, `Code generation pipeline`, `Release notes + changelog`
    - Marketing/productivity: `Marketing content engine`, `Sales lead outreach`, `Inbox to tasks`
- Guide documentation updates (`guide/src`)
  - Updated SDK docs (`sdk/index`, `sdk/typescript`, `sdk/python`) for `automationsV2` / `automations_v2`, per-agent model routing examples, and agent template CRUD examples.
  - Updated MCP automation guide with a full `/automations/v2` DAG example and operations workflow.

## v0.3.27 (Unreleased)

- Engine dynamic tool routing + context compaction
  - Added intent-aware tool router in `tandem-core` to avoid publishing full tool catalogs on every provider call.
  - New request controls on async prompt paths:
    - `toolMode`: `auto | none | required`
    - `toolAllowlist`: explicit tool-name allowlist
    - `contextMode`: `auto | compact | full`
  - In `toolMode=auto`, engine now runs a no-tools first pass and escalates only when completion/user intent indicates tools are required.
  - MCP tools are hidden by default unless explicitly requested by intent or allowlist.
  - Added router tunables:
    - `TANDEM_TOOL_ROUTER_ENABLED` (default enabled)
    - `TANDEM_TOOL_ROUTER_MAX_TOOLS` (default `12`)
    - `TANDEM_TOOL_ROUTER_MAX_TOOLS_EXPANDED` (default `24`)
  - Added runtime observability events:
    - `tool.routing.decision`
    - `context.profile.selected`
- Prompt/token efficiency improvements for simple chats
  - Added compact context profile for short/simple prompts.
  - Server prompt-context hook now skips memory search/injection for low-signal short greetings/chitchat, reducing unnecessary token bloat.
- SDK parity updates for routing controls
  - TypeScript client: `promptAsync` / `promptAsyncParts` now accept routing options (`toolMode`, `toolAllowlist`, `contextMode`).
  - Python client: `prompt_async` / `prompt_async_parts` now accept `tool_mode`, `tool_allowlist`, `context_mode`.
- Engine/channel stability hardening for stuck runs
  - Fixed startup race in server prompt-context hook that could panic with `runtime accessed before startup completion` during boot.
  - Added provider stream timeout fail-safes in engine loop so stuck upstream calls fail and release active runs:
    - `TANDEM_PROVIDER_STREAM_CONNECT_TIMEOUT_MS` (default `30000`)
    - `TANDEM_PROVIDER_STREAM_IDLE_TIMEOUT_MS` (default `90000`)
    - `TANDEM_PROMPT_CONTEXT_HOOK_TIMEOUT_MS` (default `5000`)
    - `TANDEM_BASH_TIMEOUT_MS` (default `30000`)
  - This prevents long-lived stuck active runs that caused downstream `SESSION_RUN_CONFLICT` symptoms across web chat and channel adapters.
- Control panel chat stream watchdog hardening
  - Increased stream no-event and max-window thresholds.
  - Added run-settlement polling before declaring a stuck run, reducing false stuck/error toasts during slow provider/tool phases.

- Bot identity + personality configuration
  - Added canonical identity API: `GET /config/identity` and `PATCH /config/identity`.
  - Added built-in personality preset catalog: `balanced`, `concise`, `friendly`, `mentor`, `critical`.
  - Added soft legacy migration: `bot_name` and `persona` patch payloads are accepted and normalized into canonical `identity` fields.
  - Updated TypeScript and Python SDKs with identity namespace support:
    - TypeScript: `client.identity.get()`, `client.identity.patch(...)`
    - Python: `client.identity.get()`, `client.identity.patch(...)`
- Runtime identity/personality prompt injection
  - Server prompt-context augmentation now injects assistant name + personality guidance per run iteration.
  - Supports per-agent personality overrides while keeping hidden utility agents (`compaction`, `title`, `summary`) on neutral default behavior.
- Branding/identity surface updates
  - VPS Web Portal now reads configured identity aliases and renders portal/bot labels dynamically.
  - Control Panel now reads configured identity aliases and renders control-panel/bot labels dynamically.
  - Control Panel Settings now includes an Identity & Personality editor (canonical name, control-panel alias, preset, custom instructions).
  - Desktop (Tauri) Settings now includes a Bot Identity section (canonical name, desktop alias, preset, custom instructions).
  - Control Panel chat and Desktop chat now show configured bot identity names in assistant message/header labels.
  - Added optional bot avatar support via `identity.bot.avatar_url`, including avatar upload controls in Desktop/Control Panel settings and avatar rendering in Desktop chat plus portal/control-panel shell/chat identity UI.
  - Avatar uploads are now normalized server-side (decode/resize/re-encode), so larger source images are accepted and stored in a bounded form.
  - Core UI/TUI assistant-facing copy reduces hardcoded Tandem naming in primary runtime labels/placeholders.
- npm publish reliability (control panel)
  - Added missing `repository` metadata in `packages/tandem-control-panel/package.json`.
  - Fixes npm provenance validation failure for `@frumu/tandem-panel` publishes from CI (`publish-registries`).
- Setup flow updates
  - Desktop onboarding wizard now includes an identity setup step with direct navigation to the Settings identity section.
- Compatibility and protocol naming updates
  - OpenRouter `X-Title` now supports configurable protocol title via `AGENT_PROTOCOL_TITLE` with `TANDEM_PROTOCOL_TITLE` compatibility fallback.
  - Engine auth header parsing now accepts both canonical `x-agent-token` and compatibility `x-tandem-token`.
  - Guide docs now include identity/personality configuration + SDK coverage and use canonical `X-Agent-Token` examples (with compatibility note for `X-Tandem-Token`).
- Engine tool-loop retry protection and tuning
  - Added duplicate-signature loop guard for non-read-only tools (including repeated `bash` signatures) to stop runaway repeated provider/tool cycles.
  - Added deterministic terminal summary when duplicate-signature guard triggers, reducing token burn from repeated retries.
  - Added env tuning controls:
    - `TANDEM_MAX_TOOL_ITERATIONS` (max provider/tool loop turns per run; default `25`)
    - `TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT` (max repeated identical mutable-tool signatures before guard; defaults: shell `2`, other mutable tools `3`)

- Telegram MarkdownV2 rendering and delivery hardening
  - Telegram heading rendering now outputs readable heading text style (instead of visible escaped `###` markers).
  - Long Telegram responses now split on markdown-safe boundaries where possible, reducing entity breakage across chunks.
  - Existing Telegram fallback behavior remains: if MarkdownV2 send fails, message retries as plain text.
- Telegram style profiles (bot presentation control)
  - Added Telegram style profile support: `default`, `compact`, `friendly`, `ops`.
  - Profile can be configured through channel config and is applied before MarkdownV2 conversion.
  - Added control-panel support for selecting Telegram style profile in Channels settings.
- Control panel routine automation UX + approvals
  - Added one-click routine policy shortcut: **Allow everything** (all tools, external integrations allowed, no approval gate).
  - Added Approve/Deny controls for pending routine/automation runs across:
    - routine cards
    - automations list
    - recent runs
    - run inspector
  - Closes the control-panel gap where approval-gated runs had no in-UI resolution path.

## v0.3.26 (Unreleased)

- Channel attachment ingestion + engine dispatch parity
  - Channel dispatcher now sends uploads as explicit `file` parts to `prompt_async` (instead of text-only attachment summaries).
  - Telegram/Discord/Slack flows preserve attachment metadata (`path`, `url`, `mime`, `filename`) for run prompts, resource references, and memory records.
  - Discord and Slack adapters now download inbound attachments to local `channel_uploads` storage, matching Telegram’s local-persistence behavior.
- Multimodal image dispatch for OpenAI-compatible providers
  - Provider message model is now attachment-aware and can encode image attachments in OpenAI-compatible `messages[].content` array payloads.
  - Engine loop now maps inbound image file parts into runtime provider attachments on first-iteration user context.
  - Local file attachment paths are converted to bounded data URLs before provider dispatch when needed.
  - Added model capability guardrails that fail fast when image input is attempted on a likely non-vision model.
- Engine storage visibility API
  - Added `GET /global/storage/files` to list files under the engine storage root with optional `path` and `limit` query controls.
  - Response includes root/base metadata and per-file size/mtime fields.
  - Added traversal protection: rejects absolute/parent-directory path traversal in requested subpath.

## v0.3.25 (Unreleased)

- Agent Swarm headless example (`examples/agent-swarm`)
  - Added manager/worker/reviewer/tester prompts and a full manager orchestration script using Tandem sessions, runs, event bus updates, shared resources, approvals, and routines.
  - Added managed worktree helper scripts (`create_worktree.sh`, `cleanup_worktrees.sh`) and a routine health-check script for Telegram notifications and GitHub MCP check polling via Arcade.
  - Added routine definition `routines/check_swarm_health.json` (cron every 10 minutes, approval-gated).
  - Added example test coverage for deterministic task transitions, manager registry updates, worktree idempotency, and MCP auth-loop prevention.
- Shared resource key policy update
  - `swarm.active_tasks` is now accepted as a valid shared resource key for swarm task registry storage.

- Global Memory first-class runtime path (engine/server)
  - Added durable global memory storage in `memory.sqlite` with FTS5-backed retrieval (`memory_records` + `memory_records_fts`), replacing transient in-process memory API state.
  - Added automatic ingestion capture from run outputs and event streams:
    - user messages
    - assistant finals
    - tool inputs/outputs
    - permission requests/replies
    - MCP auth-required/pending challenges
    - todo/question planning events
  - Added secret-safety write gate with scrub/block behavior before persistence and redaction metadata on stored records.
- Memory retrieval now runs during planning loops, not only run start
  - Introduced engine-loop prompt-context augmentation hook and server implementation that performs per-iteration memory search/injection.
  - Added memory context injection observability and scoring telemetry:
    - `memory.search.performed`
    - `memory.context.injected`
  - Added write lifecycle observability:
    - `memory.write.attempted`
    - `memory.write.succeeded`
    - `memory.write.skipped`
- Memory API surface expansion
  - Added `POST /memory/demote` for private/demoted tier fallback without deleting memory.
  - Existing `/memory/put`, `/memory/search`, `/memory/list`, `/memory/promote`, `/memory/{id}` now operate on durable global records.
  - Updated TypeScript/Python SDK memory clients for response-shape compatibility and added memory demote client helpers.
  - Synced API/Event contracts (`contracts/http.md`, `contracts/events.json`) and OpenAPI route summaries with the global-memory endpoint/event surface.
  - Refreshed user-facing docs and guides (README EN/zh-CN, SDK READMEs, guide SDK/headless/tools/engine command pages) for global-memory defaults and demotion workflow.

- MCP auth/retry behavior and loop hardening
  - Improved MCP auth challenge extraction to prefer structured payload fields (`structuredContent.message`, `structuredContent.authorization_url`) over noisy nested text blobs.
  - Sanitized/truncated auth-required messaging so web/TUI/channel users see clean authorization prompts instead of escaped JSON/instruction payload dumps.
  - Added stronger run-time auth/guard short-circuit behavior to reduce repeated MCP/tool-call churn in a single run.
- Engine run guard and failure ergonomics
  - Tightened guard-budget handling to fail fast in non-productive tool loops and return deterministic run-scoped messaging.
  - Reduced model-generated fallback ambiguity around “session limits” by keeping budget diagnostics explicit and per-run.
- Agent quickstart runtime stability updates
  - Chat send path now best-effort cancels stale active runs before starting a new run, reducing cross-send run carryover issues.
  - `Auto-allow all` preference now persists across portal restarts/reloads.
  - Quickstart run submission remains model-explicit and aligned with selected provider/model routing behavior.
- Provider compatibility hardening
  - Strengthened OpenAI-compatible function/tool serialization (name normalization + schema normalization + alias round-trip) to reduce strict-validator 400 failures with MCP-heavy tool sets.
- Quality and regression coverage
  - Added targeted tests for MCP auth parsing/message sanitization and per-run guard-budget summarization behavior.

## v0.3.24 (Unreleased)

- MCP reliability and auth hardening
  - Clarified and hardened MCP auth signaling via `mcp.auth.required` across engine/runtime and web chat handling.
  - MCP server runtime state now tracks auth/session continuity fields for operators and UIs:
    - `last_auth_challenge`
    - `mcp_session_id`
  - MCP connect/refresh failure handling now clears stale tool cache/session state and returns deterministic reconnect/error outcomes.
  - MCP connection bootstrap and reconnect behavior now better aligns tool availability with actual upstream server state.
- MCP tool-call resilience
  - Added engine-level MCP argument normalization before `tools/call` to recover common schema/key mismatches (for example camelCase/snake_case drift and common alias recovery such as `name -> task_title` where applicable).
  - This normalization runs in engine runtime and benefits all clients (web, TUI, channels), not only manual CLI calls.
- Agent quickstart runtime stability fixes
  - Provider setup gating and startup flow now prevent entering broken chat state when provider/model config is missing.
  - Chat now surfaces clearer run failure details and MCP auth-required diagnostics in-stream.
  - Stabilized chat/runtime behavior for non-response and blank-run regressions.
  - Aligned quickstart memory defaults with expected global-memory usage behavior in deployed engine setups.
  - Corrected quickstart proxy/auth behavior for `/engine` and portal key handling.
- SDK/npm publish reliability
  - Fixed `@frumu/tandem-client` npm provenance requirements with repository metadata alignment.
  - Hardened TS client npm publish flow in CI by ensuring install/build prerequisites (`typescript`, DOM libs, `tsup` + `tsc` declaration build path).
- Registry workflow controls and docs sync
  - Added independent PyPI publish toggle in release workflow so Python publishing can be controlled separately from npm.
  - Synced docs and top-level readme artifacts (including zh-CN parity and docs URL/badge updates) to match current release/runtime behavior.

## v0.3.23 (Unreleased)

- First SDK release announcement (TypeScript + Python)
  - `v0.3.23` is the first release where Tandem officially announces both client SDKs as release deliverables:
    - TypeScript: `@frumu/tandem-client`
    - Python: `tandem-client`
- SDK and example alignment (TypeScript + Python)
  - Added explicit TS SDK token lifecycle support via `TandemClient.setToken(token)` to support auth updates without rebuilding calling code.
  - Migrated `examples/agent-quickstart` to the current `@frumu/tandem-client` API surface:
    - replaced legacy `client.config.*` and `client.tools.*` usage
    - updated session/message/permissions flows to current namespaces
    - aligned run-state and event-stream handling with current SDK event/model contracts
  - Resolved strict TypeScript compile issues across quickstart pages so `agent-quickstart` builds cleanly.
- Release tooling parity
  - Updated `scripts/bump-version.sh` and `scripts/bump-version.ps1` to include both SDK packages:
    - `packages/tandem-client-ts/package.json`
    - `packages/tandem-client-py/pyproject.toml`
  - Ensures version bumps stay synchronized across engine/runtime crates, npm packages, and Python SDK releases.

## v0.3.22 (Unreleased)

- Engine-first context-driving reliability expansion
  - Extended engine context-run/runtime wiring used by Desktop + TUI: sequenced events, replay/checkpoint access, deterministic next-step selection, and todo->step synchronization for long-running workflows.
  - Reinforced source-of-truth model so operator surfaces consume engine state/events rather than transcript inference.
- Premium Blackboard UX for long runs (Desktop Orchestrator + Command Center)
  - Added shared Blackboard panel with docked/expanded/fullscreen modes.
  - Added decision spine + lineage rail views with selectable decision history and attached context nodes.
  - Added predictable follow behavior:
    - auto-focus only on new `meta_next_step_selected` decisions
    - manual navigation pauses follow until explicitly re-enabled
  - Added drift details drawer with mismatch flags, checkpoint/event markers, and copyable debug bundle payload.
  - Added keyboard-first controls (`E`, `F`, `Space`, `/`, `Esc`) and baseline fullscreen accessibility handling.
- Performance and test hardening
  - Switched blackboard refresh to event-family-gated + debounced fetches with `last_blackboard_refresh_seq` watermark to avoid redundant refresh bursts.
  - Added dedicated blackboard test target (`pnpm test:blackboard`) covering projection/filtering, follow-mode state, refresh policy, and drift UI state contracts.
- Orchestrator execution reliability and continuity
  - Planning now runs in two passes (analysis -> planner), improving task decomposition quality for complex objectives.
  - Builder prompts now include continuation context from the orchestrator context-pack so retries/resumes keep decision rationale.
  - Failed-task retry now preserves task session context by default; load/restart restores task session bindings from checkpoint snapshots.
  - Budget token usage now accounts for prompt + response across planner analysis, planner, builder, and validator calls.
  - Added explicit fail-fast checks for file-modifying tasks when recovery attempts invoke no tools, or only read-only tools.
  - Resume now preserves per-task failure rationale in prompt context to prevent "start from scratch" retries.
- Blackboard parity improvements (Orchestrator + Command Center)
  - Blackboard decision/reliability/task-sync projections now recognize orchestrator runtime events (for example `context_pack_built`, planning, task lifecycle, and run failure events), not only context-run `meta_next_step_selected`.
  - Refresh triggers were expanded to these orchestrator event families so blackboard state updates consistently during active runs.
  - Added `task_trace` coverage in projection/refresh/filtering so first-tool/stage details are visible in blackboard rails.
- Filesystem and tool-path reliability hardening
  - Path normalization now rejects synthetic placeholders (for example `files/directories`, `tool/policy`) and recognizes document extensions (`.pdf/.docx/.pptx/.xlsx/.rtf`) when recovering file targets.
  - `read` now returns explicit structured failures (`path_not_found`, `path_is_directory`, `read_text_failed`) instead of silent empty output.
  - Sandbox-denied responses now include actionable diagnostics (`workspace_root`, `effective_cwd`, and suggested in-workspace target path).
  - Windows verbatim paths (`\\?\...`) are accepted when they are still inside workspace root, reducing false sandbox denials.

## v0.3.21 (Unreleased)

- Global storage path standardization
  - Standardized global Tandem storage fallbacks to OS-native app-data roots (`.../tandem`) across engine/runtime/server/channels/skills/core, replacing scattered relative `.tandem` fallbacks for global state.
  - Added `TANDEM_HOME` override support in shared storage path resolution for deterministic custom install roots (servers/CI).
- Registry publish reliability
  - Fixed crates publish ordering so `crates/tandem-agent-teams` is published before `crates/tandem-tools`, preventing crates.io dependency-resolution failures during release runs.

## v0.3.20 - 2026-02-25

- Tandem TUI reliability and workflow upgrade
  - Small pastes (1-2 lines) now insert directly without `[Pasted ...]` markers; CRLF paste payloads are normalized to prevent line-overlap rendering issues.
  - Fixed multiline composer height calculation so explicit newlines grow input height correctly (prevents second-line overlap/cropping).
  - Agent fanout now auto-switches mode from `plan` to `orchestrate` before team delegation to avoid plan-mode approval/clarification blockers.
  - Expanded agent-team fanout integration: coordinated `TeamCreate` + delegated `task` routing, local mailbox/session binding, and teammate alias normalization (`A2`/`a2`/`agent-2`).
- Provider/tool runtime hotfixes for OpenRouter/OpenAI-compatible flows
  - Fixed streaming parser compatibility for providers that emit content/tool calls in `choices[].message` (instead of only `choices[].delta`), eliminating empty assistant replies and missed tool execution in affected sessions.
  - Added explicit default `max_tokens` cap (`2048`, env-overridable via `TANDEM_PROVIDER_MAX_TOKENS`) on OpenAI-compatible requests to prevent accidental high-budget sends (for example `65536`) that can cause 402 Payment Required failures during simple tool prompts.

## v0.3.19

- Stress benchmark parity + reliability upgrade
  - Server-side VPS Stress Lab scenarios (`remote`, `file`, `inline`) now measure true end-to-end async run completion (provider/tool included), not fast submit-path timing.
  - Added explicit provider/model resolution + injection for server-side stress prompt runs so LLM calls are guaranteed in prompt scenarios.
  - Fixed Stress Lab line-chart NaN rendering edge cases on sparse/all-zero series.
- Cross-system comparison support (Tandem vs OpenCode)
  - Added OpenCode benchmark API client wiring in portal (`/results/latest`, `/results/history`, `/results/by-date`, `/health`).
  - Added in-page comparison panel with scenario-mapped deltas (avg/p95 and error context) and 30-day baseline context.
- Engine + diagnostics improvements
  - Added request latency instrumentation for key engine routes: `session.command`, `session.get`, and `session.list`.
  - Improved external benchmark-service compatibility by aligning `/api/v1` health/read routing expectations.
- Tandem TUI reliability and workflow upgrade
  - Upgraded TUI terminal stack to `ratatui 0.30` + `crossterm 0.29` and replaced third-party throbber usage with local spinner rendering.
  - Added safer Windows paste handling with token placeholders to prevent line-by-line replay floods and accidental sends during large pastes.
  - Fixed plan-mode request/question handoff loops that could repeatedly hit `409 session has active run` by queueing follow-up prompts safely when runs are busy.
  - Improved request-center answer handling and visibility with clearer selection behavior and explicit "submitted answers" confirmation.
  - Restored task-list persistence when reopening historical plan sessions by parsing all persisted tool-call variants (`tool`, `tool_call`, `tool_use`).
  - Added sessions list deletion controls (`d`/`Delete`) and explicit `/agent fanout [n]` command to force multi-agent grid fanout (default 4).

## v0.3.18

- Provider model selection hotfix (OpenRouter/API-key env interactions)
  - Fixed env-layer provider bootstrap so setting `OPENROUTER_API_KEY` no longer hard-overrides OpenRouter default model to `openai/gpt-4o-mini`.
  - Preserves configured model choices (for example `z-ai/glm-5`) across engine restarts in VPS/web setups.
  - Model override from env is now explicit-only: applied only when a model env var is set, not just because an API key exists.

## v0.3.17

- Channel reliability and permission bootstrap
  - Channel-created sessions now include practical default permission rules to avoid hidden permission deadlocks.
  - Channel dispatcher now streams events at session scope and parses `message.part.updated` text deltas + additional terminal run variants for more reliable connector replies.
- Telegram diagnostics quality
  - Poll failures now include richer debug context and non-success status/body previews for easier production debugging.
- Portal run-debug quality (limited to observability)
  - Added global pending-approval visibility/action and clearer “no pending” messaging.
  - Web examples now prefer session-level SSE attach plus clearer watchdog trace events to reduce “connected/ready with no deltas” confusion.

## v0.3.16

- AI hotfix: What's New release-note alignment
  - The What's New overlay now fetches release notes by installed app tag from GitHub at runtime.
  - If release-note fetch fails or body text is unavailable, the overlay no longer shows stale bundled notes and instead points users to the latest release page.
- Plan execution task-state integrity
  - **Execute Pending Tasks** now enforces a strict completion contract: task execution is only considered complete when todo statuses are updated via `todowrite`.
  - Assistant text like "all tasks completed" is no longer trusted on its own; mismatches now surface an explicit execution-incomplete error.
  - Execution payload now includes pending-only tasks so the chat prompt and Tasks sidebar stay aligned.

## v0.3.15

- Breaking web tool migration:
  - `webfetch_document` was removed.
  - `webfetch` is now markdown-first and returns structured JSON output by default.
  - Use `webfetch_html` when raw HTML output is explicitly required.
- Custom provider + llama-swap compatibility and diagnostics:
  - Fixed custom provider config propagation so enabled custom endpoints/models are registered in engine runtime config (`providers.custom`) and selected correctly.
  - Fixed OpenAI-compatible endpoint normalization to prevent malformed URLs (for example `/v1/v1/chat/completions`) from trailing/duplicated path input.
  - Added support for custom/non-built-in provider IDs in engine provider registry (prevents fallback to `local`-only configured providers).
  - Added short retry handling for transient connection/timeout provider-call failures.
  - Improved provider error messaging with endpoint + failure category details to make connectivity issues actionable.
- Settings UX improvements for custom providers:
  - Added explicit success/error feedback after clicking **Save Custom Provider**.
  - Updated Anthropic/OpenAI model selection UX to text-input-first with refreshed current model suggestions and clearer placeholders.
- Channel integrations: expanded slash-command control/visibility directly from Telegram/Discord/Slack:
  - `/run` for active run status
  - `/cancel` (and `/abort`) for run cancellation
  - `/todos` for session todo visibility
  - `/requests` to view pending permission/question requests
  - `/answer <question_id> <text>` to reply to pending question prompts
  - `/providers` and `/models [provider]` for provider/model catalog visibility
- Channel integrations: added `/model <model_id>` to switch the active model on the current default provider from chat channels (without adding provider/token switching flows).

## v0.3.14

- AI hotfix: endless update prompt/version skew
  - Desktop now prefers bundled engine binaries when AppData sidecar binaries are stale, preventing false "You have v0.3.0" prompts after upgrading.
  - Engine update overlay version formatting is normalized to avoid duplicated prefixes (for example `vv0.3.12`).

## v0.3.12

- AI hotfix: MCP runtime compatibility
  - Desktop now falls back to `GET /mcp` server `tool_cache` when `GET /mcp/tools` returns `404` on mixed-version engines.
  - Fixes Extensions MCP runtime load failures (`Failed to load MCP runtime`) while older/newer engine components are temporarily out of sync.
- Registry publish hotfix
  - Fixed crates publish ordering/dependency coverage for `tandem-providers`/`tandem-memory` and `tandem-document`/`tandem-tools` dependency chains.

- Issue #14 fix (custom providers + live model lists):
  - Fixed `custom` provider routing so custom endpoint/model selections are honored for chat/automation dispatch.
  - Provider settings now prefer engine-catalog model IDs (OpenAI/Anthropic/OpenCode Zen) when available, instead of static-only dropdown content.
- Updates + release metadata reliability:
  - Settings release notes now fall back to updater `latest.json` when GitHub Releases API is unavailable.
  - Desktop CSP now explicitly allows GitHub release metadata hosts used by updater/release note fetches.
  - Sidecar updater status now reports bundled-engine version from app metadata instead of stale stored beta values.

## v0.3.9 (Unreleased)

- Memory Consolidation: Added opt-in LLM summarization of session memory using the cheapest available configured provider (prioritizing local/free options like Ollama, Groq, OpenRouter). Automatically triggers as a background task when a session completes.
- Channel Tool Policy: Added `channels.tool_policy` config option (`allow_all`, `deny_all`, `require_approval`) and `TANDEM_CHANNEL_TOOL_POLICY` env var to govern agent tool execution in messaging channels.
- Channel Session Metadata: Upgraded channel session tracking to persist detailed `SessionRecord` mapping (timestamps, channel, sender) instead of bare session IDs.
- Headless web admin: Added embedded single-file `/admin` UI served by `tandem-server` (no external runtime assets).
- Realtime admin updates: Added SSE-driven refresh behavior with polling fallback for live admin visibility.
- New channel admin APIs:
  - `GET /channels/status`
  - `PUT /channels/{name}`
  - `DELETE /channels/{name}`
  - `POST /admin/reload-config`
- New memory admin APIs:
  - `GET /memory`
  - `DELETE /memory/{id}`
- Engine CLI: Added `tandem-engine serve --web-ui` and `--web-ui-prefix` (plus env equivalents).
- Runtime wiring: Channel listener lifecycle now integrates with server startup/reload paths for headless operation.
- Security hardening: Embedded admin responses now include strict CSP/security headers.
- Agent Command Center (desktop): Added initial command-center UI in Orchestrator for live agent-team missions/instances/approvals.
- Agent-Team approvals: Added explicit spawn approval decision endpoints (`POST /agent-team/approvals/spawn/{id}/approve|deny`).
- Docs: Updated engine command reference for web admin flags and headless control surface.
- Desktop channels: Fixed a startup race so saved Telegram/Discord/Slack bot-token connections persist correctly across app/engine restarts after vault unlock.
- Model routing: Fixed provider/model dispatch so selected models are used across chat/session/orchestrator flows instead of fallback defaults.
- Model selection persistence: Chat and Command Center now persist explicit `selected_model` routing in provider config.
- Provider runtime behavior: Streaming/completion calls now honor per-request model overrides.
- OpenRouter attribution: Added Tandem-origin headers for provider requests.
- Memory reliability: Added startup backup + self-heal recovery for malformed/incompatible memory vector tables.
- Command Center reliability: Fixed paused/failed status mapping and disabled launch while runs are active.
- Autonomous swarm permissions: Orchestrator/Command Center sessions now auto-allow shell permissions in autonomous mode.
- Shell robustness: Empty shell calls now fail fast with `BASH_COMMAND_MISSING` instead of hanging until timeout.
- Windows compatibility: Added translation for common Unix-style agent shell commands (`ls -la`, `find ... -type f -name ...`) to PowerShell equivalents.
- Stream stability: Reduced false stream watchdog degraded events while tools are still pending.
- Command Center reliability: Added strict `read`/`write` tool-arg validation (JSON object + non-empty `path`) with fail-fast `INVALID_TOOL_ARGS` handling to prevent endless retry loops.
- Orchestrator error clarity: Replaced generic Windows `os error 3` workspace mismatch messaging with structured classification (`WORKSPACE_NOT_FOUND`, path-not-found fail-fast, timeout codes).
- Workspace safety: Task child sessions now pin explicitly to orchestrator workspace path and preflight-check workspace existence before session creation.
- Workspace propagation fix (CC-001): New runs now persist canonical `workspace_root`, and tool executions receive explicit `workspace_root`/`effective_cwd` context so filesystem operations always use the selected workspace.
- Workspace switch hot-reload fix (CC-001): Switching active workspace now invalidates stale orchestrator engines bound to previous roots, preventing agents from reading/writing in old directories.
- Selected Run readability (CC-002): Added objective line-clamp with `Show more` / `Show less` in Command Center Selected Run panel.
- Runs observability (CC-003): Runs list now shows status badges, started/ended timestamps, and failed-run error snippets.
- Tool history integrity: Tool execution IDs now include session/message/part context to avoid cross-session `part_id` collisions in diagnostics.
- File-tool stability: Increased `read`/`write` timeout budget to reduce premature synthetic timeout terminals on larger repos.
- Engine memory tools:
  - Added `memory_store` for persisting agent-learned memory in `session`/`project`/`global` tiers.
  - Added `memory_list` for browsing/auditing stored memory by scope/tier.
- Global memory support:
  - `memory_search` now supports `tier=global` with explicit opt-in (`allow_global=true` or `TANDEM_ENABLE_GLOBAL_MEMORY=1`).
  - Global tier remains gated by default to preserve isolation without explicit enablement.
- Engine memory DB alignment:
  - `tandem-engine` now auto-sets `TANDEM_MEMORY_DB_PATH` to shared Tandem `memory.sqlite` when unset so connected apps/tools use the same knowledge base.
- Engine-native OS awareness:
  - Added canonical engine-detected runtime context (`os`, `arch`, `shell_family`, `path_style`) shared across server APIs/events and session metadata.
  - `session.run.started` and `/global/health` now include `environment` metadata for cross-client diagnostics (Desktop, TUI, HTTP clients).
  - `tandem-core` prompt assembly now injects a deterministic `[Execution Environment]` block by default (`TANDEM_OS_AWARE_PROMPTS` toggle).
- Cross-platform shell hardening:
  - Non-Windows shell execution now uses POSIX shell (`sh -lc`) instead of PowerShell fallback.
  - Windows shell guardrails now translate common Unix command patterns, block unsafe untranslatable Unix-only commands, and return structured metadata (`os_guardrail_applied`, `translated_command`, `guardrail_reason`).
  - Added OS/path mismatch classification (`OS_MISMATCH`) and suppression of repeated identical mismatch-prone shell retries.
- Documentation:
  - Added CLI examples for `memory_store`, `memory_list`, and global memory operations.
  - Updated engine README with global memory enablement and shared DB behavior notes.
- Quality:
  - Added/updated tool tests for global-memory opt-in gating and scope validation.
- MCP Automated Agents:
  - Added dedicated `Agent Automation` desktop page (separate from Command Center) for scheduled bots + MCP connector control.
  - Added Mission Workshop and ready templates (Daily Research, Issue Triage, Release Reporter) with `webfetch_document`-forward defaults.
  - Added run triage UX: event rail chips, run filters, and run-details panel with reason/timeline/output/artifact visibility.
  - Added sidecar compatibility fallback from `/automations` to legacy `/routines` to reduce mixed-version 404 loops.
  - Added automation model-routing controls and presets (OpenRouter/OpenCode Zen examples) and emitted `routine.run.model_selected` events.
  - Hardened automation `model_policy` validation/patch semantics (including clear with `model_policy: {}`).
  - Expanded guide docs for MCP automated agent setup, headless operation, provider onboarding, and release-readiness checklist.
- Contributor thanks:
  - Thanks to [@iridite](https://github.com/iridite) for PR #12 (ProviderCard i18n namespace fix).
  - Thanks for PR #11 (`feat: enhance ReadTool to support document formats`) moving document extraction toward shared engine-side crate usage (`tandem-document`).

## v0.3.7 - 2026-02-18

- Complete Simplified Chinese overwrite: replaced and normalized zh-CN copy across major app surfaces.
- Full localization sweep: converted remaining hardcoded English strings to translation keys on startup, settings, packs, skills, theme picker, provider cards, and About.
- Locale quality pass: completed `en`/`zh-CN` parity validation and stabilized language-switch coverage for desktop UX.

## v0.3.6 - 2026-02-18

- TUI startup reliability: Added stale shared-engine detection at connect time (version-aware).
- TUI auto-recovery: Added `TANDEM_ENGINE_STALE_POLICY` (default `auto_replace`) so stale engines are replaced automatically instead of silently attached.
- TUI port fallback: When stale/default shared port is occupied, TUI now spawns managed engine on an available port.
- TUI diagnostics: `/engine status` now includes required version, active stale policy, and connection source (`shared-attached` or `managed-local`).
- Release alignment: Bumped Rust crates, app manifests, and npm wrapper packages to `0.3.6`.

## v0.3.3 - 2026-02-18

- Agent Teams: Added server-side Agent Teams foundations in `tandem-server` with shared spawn-policy gating across orchestrator/UI/tool entrypoints.
- Agent Teams: Added role-edge enforcement, budget/cap checks, capability scoping, SKILL.md hash validation/audit wiring, and structured SSE event surfaces for instance/mission visibility.
- Docs: Added Agent Teams rollout/spec docs and API/event references in `guide/src/content/docs`.
- Publishing: Fixed Rust crate publish chain/version coupling to unblock sequential publishes after dependency/version changes.
- Windows publishing: Removed dependency on publish `--no-verify` workaround path by hardening memory crate publish-verify behavior.
- Docs quality: Added crate READMEs (`engine/README.md`, `crates/tandem-tui/README.md`) and clarified npm wrapper README scope.

## v0.3.2 - 2026-02-17

- TUI: Fixed startup PIN flow to unlock existing vaults instead of forcing create-PIN when keystore is empty.
- TUI: Fixed first-run provider onboarding to force setup when unlocked keystore has no provider keys.

## v0.3.0 - 2026-02-17

- Core: Added `copilot` and `cohere` providers; updated default Gemini model to `gemini-2.5-flash`.
- Core: Implemented smart session titling to better name sessions based on user intent.
- Frontend: Debounced history refresh calls to improve performance.
- Docs: Added `TANDEM_TUI_GUIDE.md` and initialized a new `guide` mdbook.
- Engine CLI: Added `parallel` command for concurrent prompt execution with structured JSON task input/output.
- Docs: Added `docs/ENGINE_CLI.md` (bash/WSL-first) and `docs/ENGINE_COMMUNICATION.md` with end-to-end serve/API/SSE flows.
- Security: Added engine API token auth hardening with keychain-first token persistence, desktop masked/reveal/copy controls, and TUI `/engine token` commands.
- Security: Fixed provider key drift by routing auth to runtime-only `/auth/{provider}` handling instead of config-secret persistence.
- Security: `PATCH /config` and `PATCH /global/config` now reject `api_key`/`apiKey` fields with `400 CONFIG_SECRET_REJECTED`.
- Security: TUI and desktop now sync provider keys from keystore to runtime auth (`/auth`) instead of writing keys through config patches.
- Security: Fixed a beta regression where provider keys could appear in plaintext in Tandem config files in specific config-patch flows.
- Networking: Added CORS handling to engine HTTP routes for browser clients using custom auth headers (`X-Tandem-Token`).

- Plan Mode: Fixed `todowrite` empty-argument loops (`todo list updated: 0 items`) by normalizing common todo payload shapes and skipping true empty calls.
- Plan Mode: Added structured clarification fallback (`question.asked`) when no concrete task list can be produced, instead of leaving planning in prose-only follow-up.
- Plan Mode: Tightened todo fallback extraction to structured checklist/numbered lines only, preventing plain-text clarification prose from becoming phantom tasks.
- Desktop UX: Restored walkthrough-question overlays when prompts arrive via `permission(tool=question)` by normalizing into the question modal flow.
- Desktop UX: Scoped permission prompts to the active session to prevent cross-session/parallel-client approval bleed.
- TUI Startup: Engine bootstrap now runs before PIN entry, keeping startup on the matrix/connect screen until engine availability is confirmed.
- Engine Networking: Default engine port standardized to `39731` (instead of `3000`) to reduce frontend port conflicts; desktop/TUI honor env overrides for endpoint selection.
- TUI Download UX: Added byte-based download progress, install-phase messaging, and surfaced last download error details in the connect view.
- TUI Reliability: Engine download failures now support retry/backoff in-process instead of requiring a full app restart.
- TUI Debug Flow: Debug builds now fall back to GitHub release download when no local dev engine binary is present.
- TUI Keystore Recovery: Corrupt/unreadable keystore files now route to create/recovery flow rather than repeated unlock failure loops.
- Skills: Expanded discovery to support multiple project/global ecosystem directories with deterministic project-over-global precedence.
- Skills: Added per-agent `skills` activation controls and universal mode-level access for the `skill` tool.
- Memory: Wired `src-tauri` to consume shared `crates/tandem-memory` directly and removed duplicated local memory implementation files.
- Memory: Added strict `memory_search` tool in `tandem-tools` with enforced session/project scoping and blocked global tier access.
- Memory UX: Added embedding health surface (`embedding_status`, `embedding_reason`) to memory retrieval events and settings, with chat/settings badges.
- Memory UX: Persisted memory lifecycle telemetry into tool history (`memory.lookup`, `memory.store`) so chat badges and console events survive session reload.
- Memory UX: Fixed a chat race where memory events could arrive before assistant text, causing missing badges despite console memory events being present.
- Memory Reliability: Added startup SQLite integrity check + auto backup/reset recovery for malformed `memory.sqlite` databases.
- Windows: Fixed `cargo test -p tandem-memory --lib` link-time CRT mismatch (`LNK2038`) between `esaxx-rs` and `ort-sys` via vendored `esaxx-rs` build patch.
- Desktop: Stream watchdog now skips degraded status while idle with no active runs or tool calls.

## v0.2.25 (2026-02-12)

- Skills: Added canonical Core 9 marketing starter templates (`product-marketing-context`, `content-strategy`, `seo-audit`, `social-content`, `copywriting`, `copy-editing`, `email-sequence`, `competitor-alternatives`, `launch-strategy`).
- Skills: Template installer now copies the full template directory (including `references/`, scripts, and assets), not only `SKILL.md`.
- Skills: Fixed starter-template parsing issues caused by UTF-8 BOM in `SKILL.md` files (`missing or malformed frontmatter`).
- Skills: Fixed invalid YAML `tags` in `development-estimation` and `mode-builder`.
- Skills UI: Prioritized canonical marketing skills over legacy/fallback marketing templates in recommendations.
- Marketing workflow: Replaced `.claude/product-marketing-context.md` references with `scripts/marketing/_shared/product-marketing-context.md` and bundled shared context templates.
- Docs: Added canonical no-duplicate routing map at `docs/marketing_skill_canonical_map.md`.
- Release: Bumped version metadata to `0.2.25` across app manifests.

## v0.2.24 (2026-02-12)

- Modes: Added full custom modes MVP across backend + frontend with server-side enforcement and safe fallbacks.
- Modes UI: Added `Extensions -> Modes` with two views:
  - Guided Builder (recommended)
  - Advanced Editor
- Guided Builder: Added step-by-step mode creation for non-technical users, including preview-before-apply.
- AI Assist: Added optional AI-assisted mode creation flow with a bundled `mode-builder` skill template and paste-and-parse JSON preview.
- Mode Icons: Added icon selection for custom modes and icon rendering in the chat mode selector.
- Mode Selector: Switched to dynamic mode list (built-in + custom) with compact custom-mode descriptions.
- Memory: Auto-index on project load now defaults to enabled (`true`) for new settings state.
- Updates: Fixed version metadata mismatches by syncing `tauri.conf.json`, `package.json`, and `Cargo.toml` so auto-updates detect new releases correctly.

## v0.2.22 (2026-02-11)

- Orchestrator: Fixed a cross-project state bug where opening Orchestrator could load an old completed run from another project.
- Orchestrator: Switching projects (or adding/activating a project) now clears stale orchestrator run selection so each workspace starts clean.
- Orchestrator: Auto-selection now resumes only active runs (`planning`, `awaiting_approval`, `executing`, `paused`) and no longer auto-opens terminal history (`completed`, `failed`, `cancelled`).

## v0.2.21 (2026-02-11)

- Model selector UX: Replaced horizontal provider chips with a compact provider dropdown (`All` + visible providers) to scale cleanly when many providers are available.
- Model selector search: Added provider-aware query syntax via `provider:<id-or-name>` (for example `provider:openrouter sonnet`) while keeping normal model name/id search.
- Model selector clarity: Added inline context text ("Showing configured providers + local") so hidden-provider behavior is explicit.
- Model selector reliability: Provider filter now safely resets to `All` if the selected provider disappears after catalog refresh.
- Empty states: Model dropdown now reports provider-specific no-match states (for example "No models found for OpenRouter").
- Files: Fixed fullscreen file preview readability by using a stronger, opaque surface backdrop so text no longer blends into transparent/gradient themes.

## v0.2.20 (2026-02-11)

- Sidecar updates: Switched OpenCode release discovery to paginated GitHub Releases metadata (`per_page=20` + additional pages), avoiding fragile single-endpoint latest behavior.
- Sidecar updates: Selects the newest compatible release for the current platform/arch by filtering release assets, skipping drafts, and excluding prereleases unless beta channel is enabled.
- Sidecar updates: Added API-efficiency protections (ETag/Last-Modified conditional requests, local cache reuse, and debounce window) to reduce rate-limit pressure and improve resilience.
- Sidecar updates: Improved version comparison with semantic version parsing to avoid incorrect prompts caused by string comparison.
- UI/Status: Added compatibility-aware sidecar status fields (`latestOverallVersion`, `compatibilityMessage`) and improved overlay messaging when latest overall and latest compatible differ.
- **Console & Chat UI Fixes**: Resolved an issue where the Console tab would lose history when switching views or restarting the drawer. Also fixed the "Jump to latest" button positioning to ensure it stays pinned to the bottom of the chat.
- **Streaming Architecture Uplift**: Added a global stream hub with a single long-lived sidecar subscription and fanout to chat, orchestrator, and Ralph.
- **Event Envelope v2**: Added additive `sidecar_event_v2` envelopes (`event_id`, `correlation_id`, `ts_ms`, `session_id`, `source`, `payload`) while preserving legacy `sidecar_event`.
- **Stream Health Visibility**: Added explicit stream health signaling (`healthy`, `degraded`, `recovering`) and surfaced status in chat.
- **Duplicate/Race Reduction**: Refactored `send_message_streaming` to send-only and moved event relay responsibility to the global stream hub.
- **Reliable Frontend Reconciliation**: Added frontend stream dedupe keyed by `event_id` and wired missing `memory_retrieval` event handling.
- **Busy-Agent Queue UX**: Added message queue support while generation is active (enqueue on Enter + queue preview with send-next/send-all/remove).
- **Process Summary UX**: Upgraded assistant tool-call summary cards with compact process status, step counts, running/pending/failed counts, and duration.
- **Skills Lifecycle Upgrade**: Added import preview + apply flow for SKILL.md/zip packs with deterministic conflict policies (`skip`, `overwrite`, `rename`).
- **Skills Metadata Expansion**: Surfaced richer skill metadata (`version`, `author`, `tags`, `requires`, `compatibility`, `triggers`) and better invalid-skill parse feedback.

## v0.2.19 (2026-02-11)

- Memory: Chat now runs vector retrieval in both standard and streaming send paths, injects `<memory_context>` when relevant, and emits verifiable retrieval telemetry events.
- Memory: Assistant responses now include a colored memory capsule with a brain icon (`used/not used`, chunk count, latency) so retrieval usage is visible per response.
- Logs: Memory retrieval logs now use a distinct `tandem.memory` signal with structured fields (status, chunk tier counts, latency, score range, short query hash) and no raw prompt/chunk content.
- Logs/Console: Reworked Logs drawer tabs to focus on Tandem logs + Console activity (removed redundant OC sidecar tab in this view).
- UI: Logs drawer fullscreen now uses dynamic height correctly instead of staying constrained to the initial panel height.
- Stability: Sidecar lifecycle start/stop is serialized to prevent duplicate OpenCode/Bun instances from race conditions.
- Theme: Improved Pink Pony readability by increasing contrast and reducing problematic translucency.

## v0.2.18 (2026-02-10)

- Files (WIP): Attempted auto-refresh of the Files tree when tools/AI create new files, but it is still unreliable and needs deeper investigation. For now, you may need to switch away and back to Files to see new items.
- Files: File preview now supports a dock mount + fullscreen toggle.
- Python: Enforce venv-only python/pip usage across tool approval and staged/batch execution paths.
- Python: When Python is blocked by venv policy, Tandem auto-opens the Python Setup (Workspace Venv) wizard.
- Packs (Python): Add `requirements.txt` and update START_HERE docs to install dependencies into the workspace venv (no global `pip install`).
- Dev: Add a "Python Packs Standard" to `CONTRIBUTING.md` and ship pack-level `CONTRIBUTING.md` where relevant.

## v0.2.17 (2026-02-10)

- Backgrounds: Fix opacity slider flashing/disappearing in some packaged builds by keeping the resolved image URL stable and updating only opacity.
- Backgrounds: Render custom background image as a dedicated fixed layer for more reliable stacking across views.

## v0.2.16 (2026-02-10)

- Updates: Fix the in-app update prompt layout being constrained/squished due to theme background layering CSS.

## v0.2.15 (2026-02-10)

- Backgrounds: Fix custom background images failing to load in some packaged builds by falling back to an in-memory `data:` URL when the `asset:` URL fails.

## v0.2.14 (2026-02-10)

- Themes: Cosmic Glass now has a denser starfield + galaxy glow background.
- Themes: Pink Pony now features a thick, arcing rainbow background.
- Themes: Zen Dusk now uses a minimalist ink + sage haze background.
- Backgrounds: Add an optional custom background image overlay (copied into app data) with an opacity slider in Settings.
- UI: Gradient theme backgrounds now render consistently across main views and overlays (fixes occasional overlay "shine through").
- Sessions: Fix restored sessions appearing selected but not opening until reselecting the folder (defer history load until the sidecar is running; allow re-clicking the selected session to reload).
- Files: Add Rust-based text extraction for common document formats (PDF, DOCX, PPTX, XLSX/XLS/ODS/XLSB, RTF) via `read_file_text`, so these attachments can be previewed and included as usable text in skills/chats without requiring Python.
- Python: Add a workspace-scoped venv wizard (creates `.opencode/.venv` and installs requirements into it) and enforce venv-only python/pip usage for AI tool calls to prevent global installs.
- Navigation: Restore Settings/About/Extensions views after a regression where they would not appear.
- Packs: Style runtime requirement pills consistently.

## v0.2.13 (2026-02-10)

- Skills: Add two new bundled starter skills: `brainstorming` and `development-estimation`.
- Skills: Show runtime requirement pills on starter skill cards via optional `requires: [...]` YAML frontmatter.
- Skills: Improve Skills install/manage UX (runtime note, clearer installed-skill counts, and jump-to-installed).
- Packs: Packs page now shows packs only (remove starter skills section) and moves the runtime note to the top.
- Diagnostics: Improve Logs viewer UX (fullscreen + copy feedback); fix an invalid bundled skill template frontmatter that was being skipped.
- Dev: In `tauri dev`, load starter skill templates from `src-tauri/resources/skill-templates/` so newly added templates appear immediately.
- Docs: Add a developer guide for adding skills in `CONTRIBUTING.md`.

## v0.2.12 (2026-02-09)

- Orchestrator: Persist the selected provider/model on runs and prefer it when sending prompts, so runs don't start without an explicit model spec.
- Orchestrator: Prevent empty plans from being treated as "Completed"; make Restart rerun completed plans and re-plan when needed.
- Orchestrator: Allow deleting orchestrator runs from the Sessions sidebar (removes the run from disk and deletes its backing OpenCode session).
- Diagnostics: Improve in-app Logs drawer sharing UX (horizontal scroll for long lines, selected-line preview, and copy helpers).
- Release: Fix Discord release notifications for automated releases (publish via `GITHUB_TOKEN` doesn't trigger `release: published` workflows).

## v0.2.11 (2026-02-09)

- OpenCode: Prevent sessions from getting stuck indefinitely when a tool invocation never reaches a terminal state (ignore heartbeat/diff noise, treat more tool terminal statuses as `ToolEnd`, and add a fail-fast timeout that cancels the request and surfaces an error).
- Diagnostics: Add an on-demand Logs drawer that can tail Tandem app logs and show OpenCode sidecar stdout/stderr (captured into a bounded in-memory buffer). Streaming only runs while the viewer is open.
- Reliability: Ignore OpenCode `server.*` heartbeat SSE events (and downgrade other unknown SSE events) to prevent warning spam in logs.
- Providers: Add Poe as an OpenAI-compatible provider option (endpoint + `POE_API_KEY`). Thanks [@CamNoob](https://github.com/CamNoob).
- Release: Retry GitHub Release asset uploads to reduce flakes during transient GitHub errors.

## v0.2.10 (Failed Release, 2026-02-09)

- Release attempt failed due to GitHub release asset upload errors during a GitHub incident; no assets were published. v0.2.11 re-cuts the same changes.

## v0.2.9 (2026-02-09)

- Memory: Incremental per-project workspace file indexing with percent progress, auto-index toggle, and a "Clear File Index" action to reclaim space.
- Memory: Vector Database Stats now supports All Projects vs Active Project scope.
- OpenCode: Properly handle question prompts (multi-question wizard with multiple-choice + custom answers).
- Sessions: On startup, automatically load session history for the last active folder (fixes empty sidebar until a manual refresh).
- Windows: Prevent orphaned OpenCode sidecar (and Bun) processes during `pnpm tauri dev` rebuilds by attaching the sidecar to a Job Object (kill-on-close).

## v0.2.8 (2026-02-09)

- Support multiple custom OpenCode providers by name: Tandem now lets you select arbitrary providers from the sidecar catalog (not just the built-in list) and persists the selection for routing.

## v0.2.7 (2026-02-08)

- Fix OpenCode config writes so existing `opencode.json` is not deleted if replacement fails (Windows-safe).
- Reduce sidecar idle memory usage with Bun/JSC environment hints.

## v0.2.6 (2026-02-08)

- Fix macOS release builds by disabling signing/notarization by default (can be enabled via `MACOS_SIGNING_ENABLED=true`).

## v0.2.5 (2026-02-08)

- Re-cut release to ensure CI/release builds run with the corrected GitHub Actions workflow.

## v0.2.4 (2026-02-08)

- Fixed Starter Pack installs failing in packaged builds (bundled resource path resolution).
- Fixed onboarding getting stuck for Custom providers (e.g. LM Studio) and bouncing users back to Settings.
- Added Vector DB stats + manual workspace indexing in Settings.
- Improved macOS release workflow with optional signing/notarization inputs and CI Gatekeeper verification.

## v0.2.3 (2026-02-08)

- Fixed Orchestration Mode creating endless new root chat sessions during execution.
