# Tandem v0.4.2 Release Notes (Unreleased)

### Highlights

- **Tandem TUI coding-agent workflow upgrades**:
  - added coding-first keyboard shortcuts:
    - `Alt+P` opens workspace file search and inserts `@path` references into the active composer
    - `Alt+D` opens a scrollable git diff overlay for reviewing local changes in-place
    - `Alt+E` opens the active composer text in `$VISUAL` / `$EDITOR` and writes edited content back into the TUI
  - added matching slash commands:
    - `/files [query]`
    - `/diff`
    - `/edit`
  - added dedicated coding workflow overlays:
    - file-search modal with keyboard navigation and quick insert
    - pager modal with line/page scrolling for long content
  - improved tool-call and tool-result transcript rendering to show clearer multi-line execution cells during coding sessions
  - updated Tandem TUI docs/help surfaces with the new coding workflow keys and commands

- **Desktop orchestrator + command center stabilization**:
  - fixed orchestrator resume so runs with no tasks re-enter planning instead of getting stuck trying to execute an empty plan
  - restored run-list visibility across mixed storage by merging context runs with legacy local orchestrator runs
  - hardened run deletion for context runs by removing shared `data/context_runs/<run_id>` state and surfacing real delete failures
  - replaced native desktop confirm prompts in orchestrator controls with in-app confirmation dialogs
  - added in-app toast surfacing for payment/quota failures (`payment required` and credit-limit provider failures)
  - tuned planner guidance so non-trivial report/objective requests avoid collapsing into a single task
  - reduced terminal log spam by suppressing duplicate in-flight `tool.lifecycle.start` events for the same tool part even when provider args stream updates
  - fixed command-center action visibility so selected runs reliably expose pause/cancel/continue/delete controls
  - fixed validator/retry mismatch where write-intended tasks could be treated as non-writing and loop into `Max retries exceeded` with `no changed-file evidence`; retries now escalate to strict-write when validator feedback proves no workspace changes

- **Bug Monitor settings foundation and runtime config surface**:
  - added persisted bug-monitor config/state in `tandem-server` with explicit repo, MCP server, provider preference, and dedicated `model_policy.default_model` routing for the reporter agent
  - added fail-closed readiness evaluation for:
    - selected provider/model availability
    - selected MCP server presence/connectivity
    - required GitHub read/write capability coverage
  - fixed the control-panel Bug Monitor settings-page initialization crash caused by early query access
  - changed reporter model selection to allow typed/manual model IDs with provider suggestions and fixed model persistence across reloads
  - generalized GitHub MCP capability readiness so arbitrary MCP server instance names can satisfy reporter issue capabilities
  - added new reporter endpoints:
    - `GET /config/bug-monitor`
    - `PATCH /config/bug-monitor`
    - `GET /bug-monitor/status`
    - `GET /bug-monitor/drafts`
    - `GET /bug-monitor/drafts/{id}`
  - control-panel Settings now includes a dedicated `Bug Monitor` tab with:
    - enable/disable control
    - target repo input
    - reuse of existing MCP server config
    - dedicated provider/model selection for a cheaper reporter route
    - readiness and capability coverage summaries
  - added a direct `#/bug-monitor` route as the canonical entry for opening the Settings Bug Monitor tab
  - desktop Settings now includes a matching engine-backed `Bug Monitor` surface with runtime readiness, recent draft visibility, and a deep link into `Extensions -> MCP`
  - added a Tauri bridge for Bug Monitor config, status, draft listing, draft lookup, and manual draft submission
  - added `POST /bug-monitor/report` so desktop logs and failed orchestrator runs can create deduped local failure drafts through the engine
  - fixed the desktop sidecar reporter config path to use the canonical `GET/PATCH /config/bug-monitor` route
  - added engine-backed draft approval/deny actions at `POST /bug-monitor/drafts/{id}/approve` and `POST /bug-monitor/drafts/{id}/deny`, and surfaced those actions in desktop Settings
  - control-panel Settings now uses those same draft approval endpoints, keeping Bug Monitor decisions consistent across desktop and web surfaces
  - added `POST /bug-monitor/drafts/{id}/triage-run`, which promotes an approved draft into a minimal engine-owned `bug_monitor_triage` context run with seeded inspection and validation tasks
  - desktop and control-panel Settings can now create those triage runs directly from approved Bug Monitor drafts
  - control-panel Dashboard now includes those `bug_monitor_triage` context runs in the existing context-run visibility drawer
  - added `POST /bug-monitor/drafts/{id}/issue-draft`, which renders a template-aware issue draft artifact from the repo bug template before GitHub publish
  - Bug Monitor GitHub publish now uses that rendered issue-draft artifact instead of opening issues directly from raw incident details
  - auto-publish now defers with `triage_pending` until a triage-backed issue draft exists, preventing premature low-signal issue creation
  - fixed Bug Monitor incident persistence so draft-creation failures leave a visible incident error instead of a half-created tracker row
  - approving a Bug Monitor draft no longer fails the operator action just because the follow-up GitHub publish step is blocked
  - split Bug Monitor readiness into local ingest vs GitHub publish readiness so live tracker surfaces can show `watching locally` when incident capture is healthy but GitHub posting is blocked
  - added `POST /bug-monitor/drafts/{id}/triage-summary` so Bug Monitor triage can persist a structured summary artifact for issue drafting
  - Bug Monitor issue-draft generation now prefers that structured triage summary over raw incident detail when rendering the repo issue template
  - Bug Monitor now suppresses duplicate incidents earlier in both runtime ingest and manual `POST /bug-monitor/report` flows by consulting stored `failure_pattern` memory before opening a fresh draft
  - Bug Monitor incidents now persist a compact duplicate summary when suppression happens so tracker UIs can explain duplicate suppression after reload/reconnect without overloading the raw source-event payload
  - Bug Monitor triage summaries now persist governed `failure_pattern` memory for subject `bug_monitor`, so structured triage can suppress later matching reports even without a prior coder-run artifact
  - Approving a Bug Monitor draft without triage now also persists governed `failure_pattern` memory from the approved draft itself, so operator-approved issues still teach duplicate suppression
  - `failure_pattern` memory now carries recurrence metadata and stronger issue-linkage metadata, and duplicate ranking uses recurrence as a tie-breaker after exact fingerprint matches
  - duplicate-suppressed Bug Monitor incidents now persist a normalized `duplicate_summary` envelope with match count, best-match details, recurrence metadata, and linked-issue unions so tracker UIs can explain suppression deterministically after reload/reconnect
  - manual `POST /bug-monitor/report` suppression now returns that same normalized `duplicate_summary` envelope, and failure-pattern matching reuses the exact-fingerprint -> recurrence -> score ordering so the reported best match stays aligned with runtime suppression
  - Bug Monitor failure-pattern reuse responses now attach the same normalized `duplicate_summary` envelope alongside any raw `duplicate_matches`, and coder-originated duplicate matches now emit a stable `match_reason` so exact-fingerprint priority survives through shared ranking and summary shaping

- **Initial Tandem Coder engine API foundation**:
  - added the first engine-owned coder endpoints:
    - `POST /coder/runs`
    - `GET /coder/runs`
    - `GET /coder/runs/{id}`
    - `GET /coder/runs/{id}/artifacts`
    - `POST /coder/runs/{id}/execute-next`
  - coder runs now persist as thin metadata records linked to engine context runs instead of a frontend-owned workflow store
  - creating an `issue_triage` coder run now seeds a deterministic context-run task graph for issue normalization, memory retrieval, repo inspection, reproduction, and triage artifact writing
  - added initial `coder.run.created` engine event emission and backend regression coverage for coder create/get/list/artifact behavior
  - `issue_triage` now has a first real worker bridge: `execute-next` claims the next runnable context task through the shared lease/claim runtime and dispatches deterministic inspection, reproduction, and final summary actions so the run can complete end to end without frontend-owned orchestration
  - `issue_fix` now uses that same `execute-next` worker bridge: the engine claims fix tasks through the shared task runtime, advances inspection and preparation nodes through workflow progression, and dispatches validation plus final summary handlers to complete the run end to end
  - `pr_review` now also uses `execute-next`: the engine claims review tasks through the same task runtime, advances the initial inspection node through workflow progression, and dispatches review-evidence plus final summary handlers to complete the run end to end
  - `merge_recommendation` now uses `execute-next` too: the engine claims merge-readiness tasks through the same task runtime, advances the initial inspection node through workflow progression, and dispatches readiness plus final recommendation handlers to complete the run end to end
  - added `POST /coder/runs/{id}/execute-all`, which loops that same engine-owned task runtime until a coder run completes, fails, cancels, exhausts runnable tasks, or hits a configured step cap
  - added an initial fail-closed readiness gate for `issue_triage`: required GitHub issue capability bindings must exist, and explicitly requested MCP servers must be configured and connected
  - added `POST /coder/runs/{id}/memory-candidates` so `issue_triage` runs can persist engine-owned memory candidate payloads and attach them to the linked context run as `coder_memory_candidate` artifacts
  - new `issue_triage` runs now seed their retrieval task with prior repo/issue memory candidate hints from earlier coder runs
  - added `POST /coder/runs/{id}/triage-summary` so the engine can write a concrete `triage.summary.json` artifact and attach it as `coder_triage_summary`
  - added `GET /coder/runs/{id}/memory-hits` so clients can inspect ranked triage retrieval hits for the current coder run
  - `issue_triage` bootstrap now combines prior `coder_memory_candidate` payloads with project semantic memory search and writes a `coder_memory_hits` artifact into the linked context run
  - triage summary writes now auto-generate reusable `triage_memory` and `run_outcome` memory candidates so later coder runs can reuse structured triage conclusions without a second manual write step
  - `issue_triage` memory retrieval now also ranks governed/shared memory hits from the existing engine memory database alongside project semantic memory and prior coder-local candidates
  - added `POST /coder/runs/{id}/memory-candidates/{candidate_id}/promote` so reviewed coder memory candidates can be stored in governed memory and optionally promoted to shared visibility with reviewer metadata
  - added `POST /coder/runs/{id}/approve` and `POST /coder/runs/{id}/cancel` as thin coder control endpoints over the existing context-run transition model
  - those control endpoints now emit `coder.run.phase_changed`, and cancelled coder runs now project a dedicated `cancelled` phase
  - `issue_triage` readiness now reuses the shared engine capability-readiness evaluator, so coder run creation blocks on the same missing/unbound/disconnected/auth-pending conditions surfaced by `/capabilities/readiness`
  - explicit `mcp_servers` requested by coder runs still remain hard requirements on top of that shared readiness check
  - coder memory promotion now reuses the generic governed-memory `memory_put` / `memory_promote` path instead of a coder-specific direct DB bridge
  - run-scoped governed-memory capability issuance is now shared through `skills_memory.rs` helpers, so coder workflows derive subject and tier policy through the same helper path as the generic memory routes
  - fixed cold-start global memory initialization so `/memory/*` routes create the memory DB parent directory before opening SQLite
  - coder lifecycle and artifact events now share a normalized payload shape, and `coder.artifact.added` includes explicit `kind` metadata so desktop and other clients can consume coder events without per-event special casing
  - added `POST /coder/runs/{id}/pr-review-summary` so `pr_review` runs can write a structured `coder_pr_review_summary` artifact and emit a first `run_outcome` memory candidate
  - added the first `pr_review` coder workflow skeleton with GitHub PR readiness checks, seeded review task graphs, and direct MCP GitHub pull-request capability bindings
  - `pr_review` now defaults to pull-request-specific memory queries, bootstraps a `coder_memory_hits` artifact at run creation, and reuses prior review `run_outcome` memory during later reviews of the same repo/PR
  - `pr_review` summary writes now also emit reusable `review_memory` candidates, and follow-on PR reviews can retrieve that review-specific memory through the shared coder memory-hits path
  - `pr_review` summary writes now also emit `regression_signal` candidates when review input includes historical regression signals, and later PR reviews can retrieve those signals through the same repo/PR memory-hits path
  - added the first `merge_recommendation` coder workflow skeleton with PR-backed readiness checks, seeded merge-assessment tasks, bootstrapped `coder_memory_hits`, and `POST /coder/runs/{id}/merge-recommendation-summary` for structured merge recommendation artifacts
  - merge recommendation summaries now emit reusable `merge_recommendation_memory` and `run_outcome` candidates so later runs can reuse prior merge guidance without needing a separate manual candidate write
  - added a dedicated `merge_recommendation_memory` candidate kind so merge guidance is stored as reusable recommendation knowledge instead of only a generic run outcome
  - added the first `issue_fix` coder workflow skeleton with issue-backed readiness checks, seeded fix and validation tasks, bootstrapped `coder_memory_hits`, and `POST /coder/runs/{id}/issue-fix-summary` for structured fix summary artifacts with reusable `run_outcome` memory
  - `issue_fix` summary writes now also emit reusable `fix_pattern` memory so later fix runs can reuse prior patch strategies and validation context
  - `issue_fix` memory retrieval now ranks same-issue `fix_pattern` and issue-fix `run_outcome` hits ahead of generic triage memory so fix runs surface prior patch strategy first
  - `merge_recommendation` memory retrieval now ranks same-PR `merge_recommendation_memory`, merge run outcomes, and regression signals ahead of generic review memory so merge runs surface prior merge guidance first
  - `merge_recommendation` ranking now also prefers policy-rich same-PR memories that carry blockers, required checks, or required approvals over generic merge summaries, so readiness-specific history surfaces first
  - `pr_review` memory retrieval now ranks same-PR `review_memory`, `regression_signal`, and PR review outcomes ahead of generic triage memory so review runs surface prior review guidance first
  - `pr_review` ranking now also prefers richer same-PR review memories that carry blockers, requested changes, or regression signals over generic review summaries, so actionable review history surfaces first
  - `issue_triage` memory retrieval now ranks same-issue `failure_pattern`, triage-memory, and triage run outcomes ahead of generic hits so duplicate/root-cause history surfaces first during new triage runs
  - `issue_fix` summary writes now also emit a dedicated `coder_validation_report` artifact when validation steps or results are provided, so validation evidence is consumable without parsing the fix summary
  - `issue_fix` summary writes now also emit reusable `validation_memory` candidates, and same-issue fix retrieval ranks that validation-specific memory ahead of generic triage memory so later fix runs can reuse prior validation evidence directly
  - `issue_fix` now uses a real coder-owned worker session during `prepare_fix`: the engine resolves a worker model, creates a scoped repo session, runs a real prompt through `run_prompt_async_with_context`, and persists the transcript as a `coder_issue_fix_worker_session` artifact before validation continues
  - `prepare_fix` now also derives a deterministic `coder_issue_fix_plan` artifact from that worker transcript so later validation and summary steps have a stable, engine-owned fix-plan record to consume
  - `validate_fix` now also launches a real coder-owned validation session and persists a `coder_issue_fix_validation_session` artifact, so validation evidence comes from the same engine session/runtime path instead of a synthetic placeholder step
  - `prepare_fix` now also harvests concrete changed-file evidence from worker tool invocations and persists it as `coder_changed_file_evidence`, giving later fix validation and UI surfaces an engine-owned record of touched paths when the worker actually edits files
  - changed-file evidence now captures per-file tool provenance plus short content previews when worker tool args include editable payloads, and final `coder_patch_summary` artifacts now carry those harvested entries forward for downstream review surfaces
  - `issue_fix` patch evidence now also snapshots the touched workspace files from the engine side, attaching lightweight file-existence, size, line-count, and preview metadata to both `coder_changed_file_evidence` and `coder_patch_summary`
  - final issue-fix summaries now also emit a dedicated `coder_patch_summary` artifact that ties the structured fix summary to changed files plus the linked worker and validation session IDs, giving desktop and future UIs a stable engine-owned patch-summary surface before full diff harvesting is added
  - added `POST /coder/runs/{id}/pr-draft` for `issue_fix`, which builds an engine-owned `coder_pr_draft` artifact from the latest fix summary, validation, and patch evidence and emits `coder.approval.required` for human review before submission
  - added `POST /coder/runs/{id}/pr-submit`, which reuses that `coder_pr_draft`, enforces fail-closed `github.create_pull_request` readiness, and writes a `coder_pr_submission` artifact for dry-run or approved submission flows
  - `issue_fix` validation and final summary generation now reuse those worker-session, validation-session, and issue-fix-plan artifacts, attaching session IDs, transcript excerpts, and plan-derived fields instead of only generic inline placeholders
  - fixed a small set of ownership bugs in `skills_memory.rs` that were blocking `tandem-server` validation for the shared governed-memory path used by coder promotion and worker-backed issue-fix execution
  - `issue_triage` memory retrieval now ranks same-issue `failure_pattern`, `triage_memory`, and issue-triage `run_outcome` hits above generic project/governed matches so triage runs surface prior failure signatures and conclusions first
  - repo-scoped coder memory retrieval is now GitHub-ref-aware, so `pr_review` and `merge_recommendation` get a true same-PR boost instead of only issue-number or recency bias
  - promoted coder memory now stores richer searchable governed-memory content from workflow payloads, including fix strategy, root cause, blockers, required checks, approvals, validation details, and regression summaries instead of only a bare summary string
  - merge recommendation summaries now also write a dedicated `coder_merge_readiness_report` artifact whenever blockers, required checks, or required approvals are present so merge readiness state can be consumed directly without reparsing the summary artifact

- **Setup understanding now routes setup asks instead of treating them as ordinary chat**:
  - added a shared backend setup-understanding endpoint at `POST /setup/understand`
  - setup messages are now classified into:
    - provider setup
    - MCP / integration setup
    - automation creation
    - channel setup help
    - broad setup help / clarification
    - normal chat pass-through
  - the resolver is deterministic and state-aware: it uses setup verbs, provider/model names, integration targets, MCP catalog hits, schedule/delivery patterns, and current missing configuration to decide whether to intercept, clarify, or pass through

- **Channels now understand setup requests structurally**:
  - Telegram, Discord, and Slack channel dispatch now call setup understanding before normal LLM prompt routing
  - clear automation requests such as “Monitor GitHub issues and post a daily digest to Slack” now go straight to Pack Builder preview instead of relying on the older narrow pack-intent string matcher
  - provider and integration setup requests now return deterministic setup guidance in-channel instead of wasting a normal assistant turn
  - ambiguous setup asks now use a scoped clarification flow so the follow-up answer is resolved inside the same room/thread/topic conversation
  - existing slash commands and Pack Builder `confirm` / `cancel` shortcuts still take precedence

- **Desktop and control-panel chat now surface setup cards**:
  - desktop chat now preflights outgoing prompts through setup understanding and shows setup cards that open the right surface for the detected task:
    - Settings for provider setup
    - Extensions / MCP for tool connections
    - Pack Builder preview for automation creation
  - control-panel chat now uses the same preflight and presents setup cards that route into:
    - Settings
    - MCP
    - Automations
  - added a Tauri bridge for setup understanding so desktop chat consumes the same backend contract as channels and the web control panel

- **Regression coverage added for the new interpretation layer**:
  - added backend tests for provider/integration/automation interception, broad-setup clarification, and pass-through chat
  - added channel dispatcher validation to keep the new interception layer compatible with existing scoped session and Pack Builder reply behavior

# Tandem v0.4.1 Release Notes (2026-03-07)

### Highlights

- **Scoped channel sessions and trigger-aware adapter metadata**:
  - Channel adapters now emit structured trigger metadata (`direct_message`, `mention`, `reply_to_bot`, `ambient`) plus stable conversation scope metadata (`direct`, `room`, `thread`, `topic`) instead of relying only on pre-stripped message text.
  - Channel session routing now scopes by conversation as well as sender, preventing the same user from sharing one active session across unrelated Discord channels/threads, Slack threads, and Telegram private/topic contexts.
  - Channel slash-command/session resolution now follows the scoped conversation key and transparently migrates legacy `{channel}:{sender}` mappings on first use.
  - Slack now supports `mention_only` across environment config, persisted channel config, and `/channels/config`, bringing it into parity with the existing Telegram/Discord gating model.
  - Added targeted regression coverage for scoped key generation, legacy channel-session migration, Slack mention parsing, and the server channel-config surface.

- **Strict swarm write reliability and cross-client engine retries**:
  - Fixed streamed OpenAI/OpenRouter tool-call parsing so multi-chunk `write` calls keep the correct tool-call identity and no longer lose follow-up argument chunks when later deltas omit the tool name.
  - Hardened write-argument recovery so truncated/malformed JSON can still recover `content` even when `path` is omitted, and raised the default provider output budget from `2048` to `16384` to reduce clipped single-file artifact responses.
  - Session/tool history persistence now preserves write args/results through the verifier path, eliminating false strict-write verifier failures where tools ran but persisted session history looked empty.
  - Swarm planner and worker prompts now prefer single-pass implementation for single-file objectives instead of splitting creation/refinement into fragile multi-task chains.
  - Added consistent local-engine retry handling across the control-panel orchestrator, Tauri desktop sidecar client, and Rust TUI for transient transport failures and `ENGINE_STARTING` startup responses.

- **Orchestration now reports real planner/provider failures and persists real tool history**:
  - Swarm planning now surfaces upstream provider failures directly when LLM planning is required, instead of reducing quota/auth problems to vague `no valid tasks` planner errors.
  - Backend session dispatch now writes explicit engine error markers like `ENGINE_ERROR: AUTHENTICATION_ERROR: ...` into session history so control-panel orchestration can present the actual cause to the user.
  - Control-panel planner startup now detects and bubbles provider quota/auth failures such as OpenRouter `403 Key limit exceeded (monthly limit)`.
  - Runtime tool invocation/result events now persist correctly into session history using the actual `WireMessagePart` event shape emitted by `tandem-engine`.
  - This fixes a major false-negative verifier path where swarm tasks were marked `NO_TOOL_ACTIVITY_NO_WORKSPACE_CHANGE` even though backend tools like `glob` and `read` had executed.

- **Provider catalog honesty in Settings and `/provider`**:
  - `GET /provider` now returns explicit catalog metadata (`catalog_source`, `catalog_status`, `catalog_message`) so clients can tell the difference between live remote catalogs, config-defined catalogs, and manual-entry-only providers.
  - Removed synthetic single-model fallback catalog entries for built-in providers, which previously made most non-OpenRouter providers look like they had exactly one available model.
  - Added live remote catalog discovery for the first supported provider set:
    - `openrouter`
    - `openai`
    - `groq`
    - `mistral`
    - `together`
    - `anthropic`
    - `cohere`
  - Remote catalog discovery now also reads runtime-auth and persisted provider-auth stores, so authenticated providers can surface real model lists without forcing secrets into config files.
  - Providers without reliable generic discovery in this pass now remain configurable but are shown as manual-entry providers instead of fake one-model catalogs:
    - `azure`
    - `vertex`
    - `bedrock`
    - `copilot`
    - `ollama`
  - Control-panel Settings now displays provider catalog state honestly with remote counts, `configured models`, or `manual entry` labels as appropriate.

- **Declarative workflows and pack extensions**:
  - Added a new `tandem-workflows` workspace crate for workflow schema definitions, YAML loading, merge rules, and validation.
  - Added engine-owned workflow runtime execution, hook dispatch, simulation, run persistence, and event streaming in `tandem-server`.
  - Added workflow HTTP APIs:
    - `GET /workflows`
    - `GET /workflows/{id}`
    - `POST /workflows/validate`
    - `POST /workflows/simulate`
    - `POST /workflows/{id}/run`
    - `GET /workflows/runs`
    - `GET /workflows/runs/{id}`
    - `GET /workflow-hooks`
    - `PATCH /workflow-hooks/{id}`
    - `GET /workflows/events`
  - Installed packs can now declare workflow entrypoints, workflow files, and workflow hooks as first-class manifest content; pack inspection now exposes those extensions in risk, permission-sheet, and workflow-extension views.
  - Added workflow runtime coverage for manual runs, hook dispatch/dedupe behavior, and context-run blackboard projection of workflow actions and artifacts.

- **Control-panel visual system and workflow operations refresh**:
  - Added a shared `tandem-theme-contract` package and refreshed the control-panel theme system with richer curated themes, shared CSS variables, and a dedicated theme picker.
  - Introduced reusable motion/UI primitives and refreshed page shells/cards/layouts across dashboard, login, packs, chat, feed, settings, and orchestrator surfaces.
  - Added a workflow operations view in Packs so users can inspect workflow definitions, toggle hooks, simulate events, run workflows, and watch live workflow events from the control panel.
  - Swarm/control-panel server routing now tracks per-run controller state more explicitly for status, run switching, and revision requests instead of relying only on a single active run snapshot.

- **Headless-first Chromium browser automation with readiness diagnostics**:
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

- **Blackboard as central coordination layer + control panel parity**:
  - Extended engine blackboard with first-class task state (`blackboard.tasks`) including workflow IDs, task lineage references, lease ownership/token/expiry, retries, and optimistic task revision (`task_rev`).
  - Added append-only task patch operations:
    - `add_task`
    - `update_task_lease`
    - `update_task_state`
  - Added task coordination endpoints:
    - `POST /context/runs/{run_id}/tasks`
    - `POST /context/runs/{run_id}/tasks/claim`
    - `POST /context/runs/{run_id}/tasks/{task_id}/transition`
    - `GET /context/runs/{run_id}/blackboard/patches`
  - Added optional `context_run_id` on pack-builder API endpoints (`preview`/`apply`/`cancel`/`pending`) so pack-builder lifecycle changes can be projected into context-run blackboard tasks.
  - Added automation-v2 run projection into blackboard: node status is synced as blackboard tasks and `GET /automations/v2/runs/{run_id}` now returns `contextRunID`.
  - Added optional `context_run_id` support on skill-router endpoints (`/skills/router/match`, `/skills/compile`) to materialize routing outcomes as blackboard tasks.
  - Desktop blackboard convergence step: `orchestrator_get_blackboard` and `orchestrator_get_blackboard_patches` now prefer engine context-run blackboard APIs and use local orchestrator store only as compatibility fallback.
  - Desktop legacy read path convergence: `orchestrator_get_events`, `orchestrator_list_runs`, and `orchestrator_load_run` now prefer engine context-run APIs first, retaining local fallback for older orchestrator snapshots.
  - Added backward compatibility coverage for legacy blackboard payloads that do not include `tasks`.
  - Control-panel Swarm SSE stream now includes `blackboard_patch` deltas in addition to run events for live blackboard parity.
  - Swarm task card rendering now clamps/wraps large prompt-backed titles with `More/Less` expand controls to avoid horizontal overflow.
  - Task lifecycle transitions now emit context-run events (`context.task.created`, `context.task.claimed`, `context.task.started`, `context.task.completed`, `context.task.failed`, etc.) carrying `patch_seq` + `task_rev`.
  - Replay now reports blackboard drift for task parity (revision/count/status) and returns replayed and persisted blackboard payloads for debug comparison.
  - Control panel swarm API shim now forwards blackboard patches and task state from engine context runs.
  - `packages/tandem-control-panel` Swarm view now includes blackboard modes:
    - Docked
    - Expanded
    - Fullscreen debug
  - Blackboard UI now renders run status/current step/why-next-step, decision lineage, agent activity lanes, workflow progress, artifact lineage, drift alerts, and patch feed.
  - Added regression tests covering claim race single-winner behavior, `command_id` idempotency, task revision conflicts, monotonic patch sequence, and replay/task compatibility.
  - Fixed Swarm continue/resume executor no-op in control panel:
    - executor now runs an existing `in_progress` step when context driver returns no new `selected_step_id`
    - `/api/swarm/continue` and `/api/swarm/resume` now return `started`, `requeued`, `selectedStepId`, and `whyNextStep` for operator visibility
    - Swarm page now renders `lastError` inline for immediate failure diagnosis
    - execution sessions now fall back to configured swarm provider/model for legacy runs that do not include persisted provider/model fields
  - Added swarm fail-closed execution safeguards:
    - model resolution now uses strict precedence (`run -> swarm state -> engine default`) and hard-fails when no model is resolvable
    - empty/no-op `prompt_sync` responses now fail step execution with explicit diagnostics instead of silently passing
    - added executor loop guard that stops repeated same-step replay when step state does not advance after completion
    - added compatibility reconcile path that marks stale completed steps as `done` via engine API when transition events do not materialize immediately
    - `/api/swarm/status` now includes resolved model metadata and executor state/reason for diagnostics
  - Added run list cleanup controls in control panel:
    - per-run `Hide` and bulk `Hide Completed` actions in Swarm
    - hidden runs are user-scoped and stored at `~/.tandem/control-panel/swarm-hidden-runs.json`
    - run cleanup is non-destructive (filters/hides from UI list by default, no engine run deletion)
  - Fixed completed-run false error surfacing and improved output discoverability:
    - successful completion no longer writes completion text into swarm `lastError`
    - added Swarm `Run Output` panel with latest completed step + session ID + assistant output preview
    - task `Open Session` now resolves from `step_completed` event payload `session_id`
    - run/task status badges now map to semantic success/warn/error styles
  - Increased duplicate-signature retry headroom for write/edit loops:
    - `write`/`edit`/`multi_edit`/`apply_patch` duplicate-signature limit now defaults to `200` (previously `3`)
    - `pack_builder` remains fixed at `1`; shell tools remain strict at `2`
    - global override through `TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT` is unchanged

- **Context runs now commit through a single Run Engine boundary**:
  - Added `ContextRunEngine` with per-run locking so context-run mutations apply in one deterministic order: load snapshot, apply mutation, append the authoritative event, persist the updated run snapshot, then emit compatibility blackboard projection output.
  - Task lifecycle endpoints (`/context/runs/{run_id}/tasks`, `/tasks/claim`, `/tasks/{task_id}/transition`) and run event mutation paths now route through that shared commit helper instead of writing `run_state.json`, `events.jsonl`, and blackboard projection files independently.
  - `events.jsonl` is now treated as the authoritative ordered per-run mutation history; `run_state.json`, `blackboard.json`, and `blackboard_patches.jsonl` remain compatibility projections and caches.
  - Authoritative task events now include stable sequencing and mutation metadata such as `event_seq`, `revision`, `task_id`, and `command_id` for replay/debug flows.
  - Read paths now repair stale `run_state.json` from the event log and stale blackboard projection data from the patch log, improving crash recovery and replay determinism.
  - `POST /context/runs/{run_id}/events` now rejects `context.task.*` writes so task authority cannot bypass the run engine.
  - Added backend regression coverage for lifecycle transition validation, claim-race single-winner behavior, idempotent command handling, snapshot repair, blackboard repair, and concurrent multi-run isolation.
- **MCP-first Pack Builder in the engine**:
  - Added built-in `pack_builder` tool with two-phase execution:
    - `preview`: parse goal, resolve external capabilities to MCP catalog servers, generate pack artifacts, and return approval summary
    - `apply`: register/connect selected MCP servers, install the generated zip, register routines paused-by-default, and persist preset metadata
  - Pack generation now emits explicit MCP tool invocations (`action: mcp.*`) in `missions/default.yaml` and discovered tool IDs in `agents/default.md`.
  - Preview now exposes connector candidates (name, description, docs URL, transport URL, auth/setup requirements, score), selected MCP mappings, required secrets, and fallback warnings where no connector exists.
  - Connector-first behavior is now default for external data/actions; built-ins are fallback-only when no MCP match exists.
- **Pack preset persistence + cross-surface compatibility**:
  - Added `pack_presets` to preset registry indexing and override export/copy flows.
  - Pack apply flow now writes `presets/overrides/pack_presets/<pack_id>.yaml` capturing:
    - registered connector servers
    - required credentials/env refs
    - selected MCP tool IDs
  - Updated control panel packs view to handle and surface `pack_presets`.
  - Updated TUI preset index contract/output to include `pack_presets`.
- **Routing and test hardening for Pack Builder**:
  - Added channel-dispatcher routing for natural-language “create/build automation pack” requests to `pack_builder` with restricted allowlist.
  - Added HTTP and unit test coverage ensuring:
    - external goals map to at least one `mcp.*` tool in preview
    - generated mission files contain explicit MCP action calls
    - apply mode blocks without explicit approvals
    - preset registry indexes/saves/deletes `pack_preset`
- **Engine startup health stability fix**:
  - Fixed a startup race where background workers could access runtime state before startup completion and panic (`runtime accessed before startup completion`).
  - Startup workers now wait for runtime readiness/failure state before subscribing to runtime-backed event streams.
- **Pack Builder first-run approval UX fix**:
  - Added baseline permission allow for `pack_builder` so pack-generation prompts from control panel and channel integrations do not timeout waiting for initial tool approval.
  - `pack_builder` still enforces explicit apply-time approvals for connector registration/install/enable actions.
- **Pack Builder token-burn guardrail**:
  - Duplicate-signature retry limit for `pack_builder` is now `1` to stop repeated identical calls quickly.
  - Added same-run duplicate-call guard so repeated `pack_builder` execution attempts are skipped deterministically.
- **OpenAI tool-schema compatibility hardening for MCP connectors**:
  - Added recursive provider-side schema normalization before tool dispatch so MCP schemas that use tuple `items` or omit nested object `properties` are transformed into OpenAI-compatible function parameter schemas.
  - Fixes provider 400 `invalid_function_parameters` failures seen on models such as `openai/gpt-5.3-codex` when MCP tools (for example Airtable list-records) are present in the toolset.
- **Pack Builder preview/apply UX hardening**:
  - Preview output now renders a concise user-facing summary instead of raw tool JSON blobs.
  - Fixed connector-selection gating so built-in satisfied external capabilities (for example headline lookup via built-in web tools) no longer incorrectly require connector selection.
  - Added goal parsing support for email-address targets (e.g. `user@example.com`) so email-send capabilities are inferred even without the literal word “email”.
  - Apply now uses plan-selected connectors by default (not a single arbitrary candidate).
  - Safe previews now auto-apply by default when no connector choice, manual auth/setup, or secret input is required; routines are still registered paused unless explicitly enabled.
  - Added conversational confirmation bridging: a follow-up chat reply like `confirm` after preview now maps to `pack_builder` apply with recovered `plan_id`, avoiding accidental creation of a new pack from the word “confirm”.
  - Added tool-level session fallback: `pack_builder` tracks last preview plan per session and upgrades short confirmation goals (`ok`, `confirm`, `apply`) to apply for that plan, ensuring consistent behavior across control panel, desktop, and channel adapters.
- **Pack Builder API-first parity across surfaces (web/desktop/channels)**:
  - Added deterministic workflow endpoints:
    - `POST /pack-builder/preview`
    - `POST /pack-builder/apply`
    - `POST /pack-builder/cancel`
    - `GET /pack-builder/pending`
  - Added server-owned workflow persistence for pending and terminal states (`pack_builder_workflows.json`) plus persisted prepared plans for restart resilience (`pack_builder_plans.json`).
  - Added session+thread scoped pending-plan resolution to prevent cross-thread mis-apply when users reply `confirm` from different channel threads.
  - Added channel adapter command mapping (Telegram/Discord/Slack dispatcher):
    - pack intent -> direct preview endpoint
    - `confirm`/`ok` -> direct apply endpoint for pending plan
    - `cancel` -> direct cancel endpoint
    - `use connectors: ...` -> apply with explicit connector override
  - Control panel chat now uses the same API-first pack-builder flow for pack intents and confirmation replies, reducing provider calls and avoiding opaque/truncated JSON tool dumps in assistant output.
  - Added endpoint regression tests covering preview/pending/cancel roundtrip, thread-scoped apply correctness, and missing-secret apply blocking semantics.
- **Tauri desktop Pack Builder parity**:
  - Added Tauri sidecar/command bridge support for:
    - `pack_builder_preview`
    - `pack_builder_apply`
    - `pack_builder_cancel`
    - `pack_builder_pending`
  - Desktop chat now renders inline Pack Builder state cards and uses direct apply/cancel endpoint actions for deterministic transitions.
  - Added Tauri unit tests to verify preview/apply/cancel/pending endpoint routing.
- **Pack Builder chat flow parity and in-thread UX**:
  - Restored LLM-led initial pack creation flow in engine loop so pack requests can continue with assistant-guided clarification when needed.
  - Control panel chat now renders Pack Builder preview/apply states inline in the conversation thread (with deterministic apply/cancel actions), not only in side-rail event views.
  - Channel dispatcher no longer short-circuits initial pack-intent messages into immediate canned previews; deterministic `confirm`/`cancel` command mapping remains for apply/cancel.
- **Pack Builder observability metrics**:
  - Added metric events for preview/apply/success/blocked/cancelled/wrong-plan outcomes (`pack_builder.metric`) with per-surface tagging for web, Tauri, Telegram, Discord, and Slack.
- **Routine MCP tool-picker in control panel**:
  - Added a routine-editor MCP picker so users can search discovered `mcp.*` tools and add them directly into the routine allowlist.
  - Added connected-server filtering in the routine form to quickly target tools from a specific MCP integration.
  - Reduces routine setup failures caused by manually typing long MCP tool IDs.

---

# Tandem v0.4.0 Release Notes (Unreleased)

### Highlights

- **Engine-embedded MCP catalog for all frontends**:
  - Added generated MCP catalog assets in engine resources (`index.json` + per-server TOMLs).
  - Added server endpoints:
    - `GET /mcp/catalog`
    - `GET /mcp/catalog/{slug}/toml`
  - Added generator pipeline (`scripts/generate-mcp-catalog.mjs`) and control-panel refresh command (`npm run mcp:catalog:refresh`).
  - Added curated official remote MCP entries:
    - GitHub: `https://api.githubcopilot.com/mcp/`
    - Jira (Atlassian): `https://mcp.atlassian.com/v1/mcp`
    - Notion: `https://mcp.notion.com/mcp`
  - Removed Docker-first GitHub default from curated flow; official remote endpoint is now the default.

- **Capability readiness preflight (fail-closed) across engine and clients**:
  - Added `POST /capabilities/readiness` to validate required capabilities before saving/running automations.
  - Added structured blocking issue output for:
    - missing capability bindings
    - unbound required capabilities
    - missing/disconnected required MCP servers
    - auth-pending MCP tools
  - Added TypeScript SDK readiness API (`client.capabilities.readiness(...)`) and exported types.
  - Added desktop/Tauri readiness command + wrapper integration (`capability_readiness`, `capabilityReadiness(...)`).

- **Control panel MCP integration improvements**:
  - MCP page now renders a searchable “Remote MCP Packs” list sourced from engine catalog.
  - Added quick actions for pack apply (prefill transport/name) and TOML open.
  - Added MCP settings readiness check UI with structured result display.
  - Pack builder save flows now enforce capability-readiness checks before writing agent/automation overrides.
- **Desktop (Tauri) MCP searchable catalog view**:
  - Added Tauri bridge command/wrapper for engine MCP catalog retrieval (`mcp_catalog`).
  - Extensions/Integrations now includes searchable remote MCP catalog listings.
  - Added one-click Apply (prefill remote name/URL) and Docs quick-open actions per catalog row.

---

# Tandem v0.3.28 Release Notes (Unreleased)

### Highlights

- **Control panel UX and workflow hotfixes**:
  - Replaced the control-panel login hero animation with a more uniform silicon-chip/data-flow visual.
  - Chat `New` action now auto-collapses the history sidebar to prioritize active conversation space.
  - Added dashboard charts/summary cards for run status and automation activity, improving at-a-glance runtime insight.
- **Chat runtime reliability + approvals consistency**:
  - Fixed delayed user message appearance; messages now render immediately on send (optimistic render path).
  - Fixed right-rail tool activity not populating by expanding accepted event families (`session.tool_call`, `session.tool_result`, and tool-part update variants).
  - Fixed stale approval request entries persisting after resolution by tightening pending-status filtering and refresh behavior, including one-shot `once` approval semantics.
- **MCP/Composio connectivity fixes**:
  - Control panel MCP add/connect flow now supports auth modes (`auto`, `x-api-key`, `bearer`, `custom`, `none`) with Composio-aware auto-header behavior.
  - MCP add/connect failure toasts now include surfaced server-side `last_error` context after refresh.
  - Fixed runtime MCP parser to handle streamable/SSE JSON-RPC response envelopes for remote discovery (`initialize`, `tools/list`), resolving Composio errors such as:
    - `Invalid MCP JSON response: expected value at line 1 column 1`
- **Persistent Automations V2 backend (additive)**:
  - Added `automations/v2` API family for lifecycle management, run controls (`run_now`, pause/resume/cancel), run listing/inspection, and V2 SSE events.
  - Added durable V2 state files: `automations_v2.json` and `automation_v2_runs.json`.
  - Added V2 scheduler/executor loops with DAG node checkpoint state (`completed_nodes`, `pending_nodes`, `node_outputs`) and resumable run metadata.
- **Per-agent model routing for cost control**:
  - Added per-agent `model_policy` in V2 agent profiles and node-level model selection at execution time.
  - Enables mixed model fleets per automation (for example, lower-cost models for simple nodes and higher-capability models for complex nodes).
- **Cron + policy runtime hardening**:
  - Replaced cron no-op scheduling path with real timezone-aware cron next-fire + misfire planning behavior.
  - Tool policy checks now support wildcard/prefix patterns (`*`, `mcp.github.*`, `mcp.composio.*`) across session and capability enforcement.
- **Hard pause semantics for active runs**:
  - Routine run pause now cancels active tracked session IDs immediately and includes canceled sessions in pause responses/events.
- **Agent template + SDK parity**:
  - Added agent template write APIs: `POST/PATCH/DELETE /agent-team/templates`.
  - Added TypeScript client support for new `automationsV2` namespace and template lifecycle helpers.
  - Added Python SDK parity with `client.automations_v2` and template create/update/delete helpers.
- **Control panel V2 builder + run operations**:
  - Added `Automation Builder V2` in the Agents view (mission metadata, schedule, dynamic agent count, per-agent model/skills/MCP/tool policy fields, and DAG node editor).
  - Added V2 automation operations in UI: run-now, pause/resume automation, list runs for automation, and per-run pause/resume/cancel actions from run inspector.
  - Added preset packs for fast setup:
    - Engineering: `GitHub bug hunter`, `Code generation pipeline`, `Release notes + changelog`
    - Marketing/productivity: `Marketing content engine`, `Sales lead outreach`, `Inbox to tasks`
- **Guide docs refresh (`guide/src`)**:
  - Updated SDK documentation for V2 automation namespaces (`automationsV2` / `automations_v2`) and per-agent model-policy examples.
  - Added agent template CRUD examples in SDK docs.
  - Updated MCP automation guide with `/automations/v2` DAG creation/run examples.

---

# Tandem v0.3.27 Release Notes (Unreleased)

### Highlights

- **Discord channel reliability and operator UX uplift**:
  - Control panel Channels view now loads and pre-fills saved channel settings from `/channels/config`.
  - Added editable Discord controls for `mention_only` and optional `guild_id` (plus Slack `channel_id` parity in the same surface).
  - Added channel-level `last_error` visibility in the control panel so channel failures are diagnosable without backend log inspection.
  - Added inline Discord usage guidance clarifying this integration's message flow (`@bot /help`) and that Discord app slash commands are not auto-registered.
- **Desktop Connections parity + first-run clarity**:
  - Added desktop-side channel verification command and wiring (`verify_channel_connection`) so Tauri Settings can run the same Discord setup checks as the web/control-panel flow.
  - Added a `Verify Discord` action in desktop Settings -> Connections with actionable failure summaries (token auth, gateway reachability, Message Content intent).
  - Added explicit runtime guidance in desktop Settings that channel listeners run only while the app is open and the machine is awake.
  - Added always-on guidance pointing users to deploy Tandem Control Panel/engine on an always-on host for 24/7 channel availability.
- **Discord allowlist matching compatibility fix**:
  - Discord adapter allowlist matching now accepts multiple identity forms: user ID, username, global name, and mention-style entries (`<@id>`, `<@!id>`, `@name`).
  - Resolves common “bot appears connected/online but ignores messages” failures when allowlists were configured with names rather than raw user IDs.

---

# Tandem v0.3.25 Release Notes (Unreleased)

### Highlights

- **Global Memory promoted to always-on runtime primitive**:
  - Added durable global memory record storage in `memory.sqlite` (`memory_records` + `memory_records_fts`) with FTS5 retrieval as baseline.
  - Memory API operations now persist to DB-backed global records rather than in-process server maps.
  - Added memory demotion path via `POST /memory/demote`.
- **Automatic memory ingestion across run/event lifecycle**:
  - Added server ingestion subscriber capturing memory candidates from:
    - user messages
    - assistant final messages
    - tool inputs/results
    - permission asks/replies
    - MCP auth challenge events (`mcp.auth.required` / `mcp.auth.pending`)
    - plan/todo/question events (`todo.updated`, `question.asked`)
  - Added write-path secret/PII safety gate with scrub/block behavior and redaction metadata tracking.
- **Automatic memory retrieval in planning loops**:
  - Added engine-loop prompt-context hook so memory retrieval/injection runs per provider planning iteration, not just at run start.
  - Injected memory context is bounded and scored, with retrieval telemetry emitted for each iteration.
- **Memory observability event family**:
  - Added memory write lifecycle events:
    - `memory.write.attempted`
    - `memory.write.succeeded`
    - `memory.write.skipped`
  - Added retrieval/injection events:
    - `memory.search.performed` (score distribution, sources, latency)
    - `memory.context.injected` (count + token-size estimate)
- **SDK parity for memory APIs**:
  - TypeScript and Python client memory surfaces now tolerate durable-memory response variants and support explicit demotion calls.
- **Contract/OpenAPI parity for global memory**:
  - Synced `contracts/http.md` and `contracts/events.json` with the new global-memory API/event shapes.
  - Updated server OpenAPI route summaries for `/memory/demote`, `/memory`, and `/memory/{id}`.
- **Docs parity for global memory UX/API**:
  - Updated root README (EN/zh-CN), SDK READMEs, and guide pages for SDK memory usage, headless endpoints, and tools/CLI examples to match global always-on memory behavior.

---

# Tandem v0.3.22 Release Notes (Unreleased)

### Highlights

- **Engine-first context-driving runtime expansion**:
  - Extended context-run wiring used by Desktop + TUI for sequenced event consumption, replay/checkpoint visibility, deterministic next-step selection, and todo->step synchronization.
  - Preserves the engine-as-source-of-truth contract for run status/progress/decision context.
- **Premium Blackboard UX (Orchestrator + Command Center parity)**:
  - Added shared Blackboard panel behaviors across both operator surfaces with docked/expanded/fullscreen modes.
  - Added decision spine + lineage rail views for clear decision history and attached context visibility.
  - Added deterministic follow behavior (decision-driven auto-focus only; manual exploration pauses follow).
  - Added drift details drawer with mismatch flags, checkpoint/event sequence markers, and copyable debug bundle payload.
  - Added keyboard controls (`E`, `F`, `Space`, `/`, `Esc`) and fullscreen focus-handling baseline.
- **Refresh/perf/test hardening**:
  - Blackboard materialization refresh now uses relevant event-family gating + debounce + refresh-sequence watermarking to reduce redundant fetches.
  - Added blackboard-focused test target (`pnpm test:blackboard`) covering projection/filtering, follow state invariants, refresh policy, and drift drawer state contracts.
- **Orchestrator execution reliability + continuity**:
  - Planning now uses a two-pass flow (analysis -> planner) to improve task decomposition quality for complex objectives.
  - Builder prompts now include continuation context from context-pack summaries, helping retries/resumes continue from prior rationale.
  - Failed-task retry now preserves task session context by default; run load/restart restores task session bindings from checkpoint snapshots.
  - Budget token usage now records prompt + response estimates across planner analysis, planner, builder, and validator calls.
  - Added explicit fail-fast checks when file-modifying tasks complete recovery with no tools, or only read-only tools.
  - Resume now preserves per-task failure rationale in prompt context to reduce fresh-start retries.
- **Blackboard parity improvements (Orchestrator + Command Center)**:
  - Blackboard projection/refresh now recognizes orchestrator runtime event families (for example `context_pack_built`, planning/task lifecycle, and run failure events), not only context-run `meta_next_step_selected`.
  - Improves live blackboard context visibility during active engine-owned orchestrator runs.
  - Added `task_trace` projection/refresh/filtering support so stage details like `FIRST_TOOL_CALL: glob` are surfaced in blackboard rails.
- **Filesystem/tool-path reliability hardening**:
  - File-path normalization now rejects synthetic placeholders (`files/directories`, `tool/policy`) and recognizes document extensions (`.pdf/.docx/.pptx/.xlsx/.rtf`) for path recovery.
  - `read` now returns explicit failure categories (`path_not_found`, `path_is_directory`, `read_text_failed`) instead of empty output on failure.
  - Sandbox-denied path responses now include actionable diagnostics (`workspace_root`, `effective_cwd`, and suggested in-workspace path).
  - Windows verbatim paths (`\\?\...`) are accepted when they remain in-workspace, reducing false sandbox denials.

---

# Tandem v0.3.20 Release Notes (Unreleased)

### Highlights

- **Tandem TUI reliability + UX upgrade**:
  - Small pastes (1-2 lines) now insert directly without `[Pasted ...]` markers; CRLF payloads are normalized to avoid line-overlap rendering artifacts.
  - Fixed multiline composer height growth for explicit newlines, preventing second-line overlap/cropping in the input box.
  - `/agent fanout` now auto-switches mode from `plan` to `orchestrate` before delegation to reduce plan-mode approval/clarification blockers during team runs.
  - Expanded agent-team fanout integration: coordinated `TeamCreate` + delegated `task` routing, local mailbox/session binding, and teammate alias normalization (`A2`/`a2`/`agent-2`).

---

# Tandem v0.3.19 Release Notes (Released)

### Highlights

- **Stress benchmark parity and accuracy uplift (VPS portal)**:
  - Server-side Stress Lab prompt scenarios now execute async runs and wait for completion, so latency reflects true end-to-end provider/tool execution.
  - Server-side stress runner now resolves and passes explicit provider/model payloads for prompt runs, preventing accidental non-LLM timing paths.
  - Stress chart rendering was hardened for empty/all-zero samples to avoid NaN polyline failures in browser.
- **Tandem vs OpenCode comparison surface (portal)**:
  - Added OpenCode benchmark read integration for:
    - `GET /results/latest`
    - `GET /results/history?days=30`
    - `GET /results/by-date/{yyyy-mm-dd}`
    - `GET /health` (with compatibility handling)
  - Added scenario-mapped comparison panel showing Tandem vs OpenCode avg/p95 deltas and recent error context.
- **Engine performance diagnostics improvements**:
  - Added request-latency instrumentation for core server routes under load:
    - `session.command`
    - `session.get`
    - `session.list`
  - Improves bottleneck visibility for providerless and mixed endpoint soak analysis.
- **Tandem TUI reliability + UX upgrade**:
  - Upgraded TUI terminal stack to `ratatui 0.30` and `crossterm 0.29`, with local spinner rendering replacing third-party throbber dependency.
  - Added safer Windows paste semantics using paste-token placeholders to avoid line-by-line replay/auto-submit failures on large clipboard input.
  - Fixed plan-mode request/question handoff loops that could repeatedly trigger `409 session has active run` conflicts by queueing busy-run follow-ups safely.
  - Improved question request handling (selection/confirm behavior) and added explicit confirmation output that shows submitted answers.
  - Restored plan task-pane persistence when reopening historical sessions by broadening tool-call history parsing (`tool`, `tool_call`, `tool_use`).
  - Added sessions-list delete shortcut (`d`/`Delete`) and `/agent fanout [n]` command for explicit multi-agent grid fanout (default 4).

---

# Tandem v0.3.18 Release Notes (Unreleased)

### Highlights

- **OpenRouter model persistence hotfix**:
  - Fixed env-layer provider bootstrap so `OPENROUTER_API_KEY` no longer forces `openai/gpt-4o-mini` as the effective default model.
  - Preserves saved/configured OpenRouter model selections (for example `z-ai/glm-5`) in web/VPS deployments.
  - Env-driven model override is now explicit-only (requires a model env var), preventing silent model drift.

---

# Tandem v0.3.17 Release Notes (Unreleased)

### Highlights

- **Channel runtime reliability updates**:
  - Channel-created sessions now start with practical default permission rules to avoid hidden permission deadlocks in connector workflows.
  - Channel dispatcher now attaches SSE at session scope and parses `message.part.updated` text deltas plus additional terminal run lifecycle variants.
  - Improves reply reliability for Telegram/Discord/Slack message handling in long-lived sessions.
- **Telegram production diagnostics improvements**:
  - Telegram poll failures now emit richer diagnostics (`{e:?}` transport context and non-success status/body preview) to reduce blind debugging.
- **Portal debugging/observability uplift (minimal UX scope)**:
  - Added global pending-approval visibility/action in portal shell.
  - Improved run watchdog trace messaging and session-level SSE attach behavior for web examples to reduce `connected/ready but no deltas` confusion.

---

# Tandem v0.3.16 Release Notes (Unreleased)

### Highlights

- **What's New release-note alignment hotfix**:
  - The desktop What's New overlay now fetches release notes for the installed app tag from GitHub at runtime.
  - If release-note fetch fails or the matched release has no body text, the overlay avoids stale local note content and links users to the latest release page.
- **Plan execution task-state integrity**:
  - `Execute Pending Tasks` now requires real todo state transitions (`todowrite`) before Tandem considers execution complete.
  - Assistant-only completion claims no longer mark execution as successful when todo statuses remain pending.
  - Execution now targets pending-only tasks, keeping prompt payloads in sync with the Tasks sidebar.

---

# Tandem v0.3.15 Release Notes

### Highlights

- **Breaking web tool migration**:
  - Removed `webfetch_document`.
  - `webfetch` now returns structured markdown-first JSON output by default.
  - Added `webfetch_html` for explicit raw HTML fallback.
  - Migration examples:
    - Old: `{"tool":"webfetch_document","args":{"url":"https://example.com","return":"both","mode":"auto"}}`
    - New: `{"tool":"webfetch","args":{"url":"https://example.com","return":"both","mode":"auto"}}`
- **Custom provider + llama-swap reliability uplift**:
  - Fixed custom provider runtime registration so enabled custom endpoint/model selections are persisted into engine provider config (`providers.custom`) and selected consistently.
  - Hardened OpenAI-compatible endpoint normalization for custom providers (handles trailing `/v1`, duplicated `/v1/v1`, and accidental full-path endpoints like `/v1/chat/completions`).
  - Added engine support for custom/non-built-in provider IDs from config, preventing `configured providers: local` fallback when custom is selected.
  - Added short retry behavior on transient connection/timeout failures for OpenAI-compatible provider calls.
  - Improved provider error diagnostics to include endpoint + failure category (`connection error` / `timeout`) for faster local-gateway troubleshooting.
- **Provider settings UX improvements**:
  - Saving **Custom Provider** now surfaces explicit success/error feedback in Settings.
  - Anthropic/OpenAI settings now use text-input-first model selection with refreshed current model suggestions and clearer provider-specific placeholders.

---

# Tandem v0.3.14 Release Notes

### Highlights

- **Endless update prompt/version skew hotfix**:
  - Desktop now prefers bundled engine binaries when AppData sidecar binaries are stale, preventing false old-version update prompts after app upgrade.
  - Update overlay engine version labels are normalized to avoid duplicated `v` prefixes.

---

# Tandem v0.3.12 Release Notes

### Highlights

- **MCP runtime compatibility hotfix**:
  - Desktop now falls back to MCP server `tool_cache` from `GET /mcp` when `GET /mcp/tools` is unavailable (404) on mixed-version engines.
  - Prevents Extensions MCP tab runtime-load failures during app/engine version skew.
- **Registry publish workflow hotfix**:
  - Corrected crate publish ordering and dependency coverage for tandem workspace crates (`tandem-providers` before `tandem-memory`, plus `tandem-document` before `tandem-tools`).

### Highlights

- **Issue #14 fix (custom providers + live model lists)**:
  - Fixed `custom` provider routing so custom endpoint/model selections are honored for chat/automation dispatch.
  - Provider settings now prefer engine-catalog model IDs (OpenAI/Anthropic/OpenCode Zen) when available, instead of static-only dropdown content.
- **Update + release metadata reliability**:
  - Settings release notes now fall back to updater `latest.json` when GitHub Releases API fetches fail.
  - Desktop CSP now allows GitHub release metadata hosts used by updater/release notes fetch paths.
  - Sidecar updater status now reports bundled-engine version from app metadata to avoid stale beta-version prompts.

---

# Tandem v0.3.9 Release Notes

### Highlights

- **External Messaging Channels (Phases 1+2)**: New `tandem-channels` crate - `Channel` trait, session dispatcher, slash commands, and fully working adapters:
  - **Telegram**: long-poll, 4096-char chunking, typing indicators.
  - **Discord**: WebSocket gateway (op 2 Identify, heartbeat, op 7/9 reconnect), 2000-char chunking, typing, mention-only mode.
  - **Slack**: `conversations.history` poll, `last_ts` dedup, `auth.test` self-filter.
- **In-channel slash commands**: `/new`, `/sessions`, `/resume`, `/rename`, `/status`, `/help` - manage Tandem sessions directly from Telegram/Discord/Slack messages.
- **Persistent session mapping**: Each `{channel}:{sender}` pair maps to a named Tandem session, surviving server restarts.
- **Supervisor with backoff**: Channel listeners auto-restart on failure with exponential backoff (1s -> 60s cap).
- **Headless Web Admin UI**: Added an embedded, single-file `/admin` interface served directly by `tandem-server`.
- **Realtime admin refresh**: Added SSE-first updates with polling fallback for channel/session/memory visibility.
- **Headless control APIs**: Added:
  - `GET /channels/status`
  - `PUT /channels/{name}`
  - `DELETE /channels/{name}`
  - `POST /admin/reload-config`
  - `GET /memory`
  - `DELETE /memory/{id}`
- **Agent Command Center (Desktop)**: Added an orchestrator-embedded command center surface for Agent Teams with live mission/instance/approval visibility.
- **Agent Automation IA split (Desktop)**: Added a dedicated `Agent Automation` page (robot icon) for MCP connector operations, scheduled bot wiring, and routine run monitoring; `Command Center` now stays focused on swarm/orchestrator runs.
- **Agent-Team spawn approval decisions**: Added dedicated decision endpoints:
  - `POST /agent-team/approvals/spawn/{id}/approve`
  - `POST /agent-team/approvals/spawn/{id}/deny`
- **Control Center role routing migration**: Orchestrator model routing now supports canonical role-map keys (`orchestrator`, `delegator`, `worker`, `watcher`, `reviewer`, `tester`) with compatibility aliases from legacy keys (`planner`, `builder`, `validator`, `researcher`).
- **Model/provider routing fix**: Fixed request routing so selected provider/model is used consistently across chat, queue, command center, and orchestrator dispatch paths.
- **Model selection persistence fix**: Chat and Command Center selectors now persist `providers_config.selected_model`.
- **Provider runtime model-override fix**: Streaming and completion provider calls now honor explicit per-request model overrides.
- **OpenRouter attribution fix**: Added Tandem-origin request headers for OpenRouter calls.
- **Memory startup self-heal**: Added backup + auto-recovery for malformed/incompatible memory vector DB state during initialization.
- **Command Center status/launch reliability**: Fixed paused/failed state mapping and disabled launch while runs are active to prevent duplicate swarm starts.
- **Command Center retry-loop fix (Windows/path failures)**: Added strict `read`/`write` argument validation (`JSON object` + non-empty `path`) with fail-fast `INVALID_TOOL_ARGS` handling to stop endless task retries.
- **Structured orchestrator error taxonomy**: Replaced generic `os error 3` workspace mismatch messaging with explicit categories (`WORKSPACE_NOT_FOUND`, path-not-found fail-fast, timeout codes).
- **Workspace pinning + preflight**: Child task sessions now pin to the orchestrator workspace and validate workspace existence before session creation.
- **Workspace propagation correctness (CC-001)**: Orchestrator runs now persist canonical `workspace_root`, and tool calls execute with explicit `workspace_root`/`effective_cwd` context to keep all filesystem actions rooted to the selected workspace.
- **Workspace switch hot-switch hardening (CC-001)**: Active-project switches now invalidate stale in-memory orchestrator engines tied to old workspace roots, preventing cross-workspace bleed.
- **Selected Run prompt UX (CC-002)**: Added inline objective line-clamp with `Show more` / `Show less` toggle in the Command Center Selected Run panel.
- **Runs status visibility (CC-003)**: Command Center run list now shows status badges, started/ended timestamps, and failed-run error snippets.
- **Tool-history correlation integrity**: Tool execution IDs now include session/message/part context to prevent cross-session `part_id` collision overwrite.
- **File-tool timeout tuning**: Increased `read`/`write` timeout budget to reduce premature synthetic timeout terminal events.
- **Autonomous swarm approvals**: Command Center/orchestrator sessions now auto-allow shell permissions in autonomous mode instead of repeatedly prompting.
- **Shell timeout/hang prevention**: Empty shell calls now fail immediately with explicit `BASH_COMMAND_MISSING` rather than stalling until watchdog timeout.
- **Windows shell translation**: Added automatic translation for common Unix-style agent commands (`ls -la`, `find ... -type f -name ...`) into PowerShell equivalents on Windows.
- **Watchdog signal quality**: Reduced false stream watchdog degradation events while tool executions are still pending.
- **Command Center failed-task retry**: Added a one-click `Retry Task` action for failed tasks that re-queues the task and re-evaluates dependency blocks without requiring full run restart.
- **Command Center failure visibility**: Failed task cards now show clearer validator/error context directly on the task card.
- **Command Center live debugging UX**: Added an inline run-scoped Console panel and promoted workspace file browser visibility for in-context swarm troubleshooting.
- **Startup view safety default**: Desktop startup now defaults to Chat view instead of restoring Command Center first (pending a dedicated starter page flow).
- **Engine memory learning tools**: Added `memory_store` and `memory_list` to the engine tool registry so agents can write and inspect memory directly through tool calls.
- **Global knowledge-base search (opt-in)**: Extended `memory_search` to support `tier=global` when explicitly enabled via `allow_global=true` or `TANDEM_ENABLE_GLOBAL_MEMORY=1`.
- **Shared memory DB routing**: `tandem-engine` now auto-configures `TANDEM_MEMORY_DB_PATH` to the shared Tandem `memory.sqlite` path when unset, improving cross-app memory consistency.
- **Engine-canonical OS context**: Added shared `HostRuntimeContext` (`os`, `arch`, `shell_family`, `path_style`) and exposed it via `/global/health`, session metadata, and `session.run.started` events.
- **OS-aware prompt injection**: `tandem-core` now prepends deterministic `[Execution Environment]` context to model runs (default on, controlled by `TANDEM_OS_AWARE_PROMPTS`).
- **Cross-platform shell guardrails**: Added stronger Windows Unix-command translation/blocking with structured guardrail metadata, plus POSIX-native shell execution path on non-Windows hosts.
- **OS mismatch diagnostics and loop suppression**: Added `OS_MISMATCH` error classification and retry suppression for repeated identical shell mismatch patterns.
- **Docs and examples refresh**: Added engine CLI examples for memory write/list/global flows and documented global-memory startup configuration.
- **Safety and coverage**: Added/updated tests to enforce explicit global-memory gating and avoid accidental unrestricted global recall.
- **MCP Automated Agents (Desktop IA)**: Added a dedicated `Agent Automation` page (robot icon) for scheduled bots and MCP connector operations, separate from Command Center swarm workflows.
- **Mission Workshop + templates**: Added mission drafting helper and ready templates (Daily Research, Issue Triage, Release Reporter) with default `webfetch_document`-first workflows.
- **Automation run triage UX**: Added run event rail, run filters (`All`, `Pending`, `Blocked`, `Failed`), and run details panel with reason/timeline/output/artifact visibility.
- **Automations API compatibility**: Desktop sidecar now falls back to legacy `/routines` endpoints when `/automations` is unavailable, reducing mixed-version 404 failures.
- **Automation model routing**: Added provider/model routing controls and presets (OpenRouter/OpenCode Zen examples), plus orchestrated role model hints.
- **Model selection observability**: Runs now emit `routine.run.model_selected` events so selected provider/model and source are visible in event streams.
- **Server model policy hardening**: Added strict `model_policy` validation in automation create/patch handlers and explicit clear semantics (`model_policy: {}`).
- **Docs rollout for automated agents**: Expanded MCP automated agent guide with headless setup, provider onboarding (Arcade/Composio), mission quality guidance, model-policy examples, and release test checklist.

### Contributor Thanks

- Thanks to [@iridite](https://github.com/iridite) for:
  - **PR #12**: Provider settings i18n namespace fix (`ProviderCard` translation resolution).
  - **PR #11**: [`feat: enhance ReadTool to support document formats`](https://github.com/frumu-ai/tandem/pull/11), moving document/file-reading extraction toward shared engine-side crate usage (`tandem-document`).

### Orchestrator Routing Migration Notes

- Legacy run payloads and API requests using `planner`/`builder`/`validator` continue to load and are normalized automatically.
- New routing payloads can be sent as direct role-key maps; unknown roles are preserved but execution falls back to worker behavior when needed.
- Task schema now supports role metadata:
  - `assigned_role` (default: `worker`)
  - `template_id` (optional hint)
  - `gate` (`review` or `test`, optional)

### Complete Feature List - tandem-channels v0.3.9

#### New Crate: `tandem-channels`

- Added `Channel` trait (`send`, `listen`, `health_check`, `start_typing`, `stop_typing`, `supports_draft_updates`).
- Added `ChannelMessage` and `SendMessage` data types.
- Added `ChannelsConfig` with env-var loading (`TANDEM_TELEGRAM_BOT_TOKEN`, `TANDEM_DISCORD_BOT_TOKEN`, `TANDEM_SLACK_BOT_TOKEN`, and related vars).
- Added `is_user_allowed` helper with `["*"]` wildcard support.
- Added `TelegramChannel`: `getUpdates` long-poll, `sendMessage`, chunking, typing-indicator loop.
- Added `DiscordChannel`: WebSocket gateway, Identify (op 2), heartbeat, Reconnect/InvalidSession handling (op 7/9), 2000-char chunking, mention normalization, bot self-filter, typing indicator loop.
- Added `SlackChannel`: `conversations.history` poll every 3s, `last_ts` dedup, `auth.test` self-filter, `chat.postMessage` with `ok`-field validation.
- Added session dispatcher: `start_channel_listeners`, supervised listeners, `get_or_create_session`, `run_in_session` (HTTP poll).
- Added in-channel slash commands: `/new [name]`, `/sessions`, `/resume <query>`, `/rename <name>`, `/status`, `/help`.
- Added unit tests: allowlist wildcards, comma-separated user lists, message splitting for Telegram (4096) and Discord (2000).

### Headless Web Admin + Server Integration

- Added embedded admin UI module and baked HTML shell:
  - `crates/tandem-server/src/webui/mod.rs`
  - `crates/tandem-server/src/webui/admin.html`
- Added secure admin response headers:
  - `Content-Security-Policy`
  - `X-Frame-Options: DENY`
  - `X-Content-Type-Options: nosniff`
  - `Referrer-Policy: no-referrer`
- Added channel runtime lifecycle wiring into server startup/reload path with status publication events.
- Added server-side memory listing/deletion routes for admin operations.
- Added memory event aliases consumed by admin UX:
  - `memory.updated`
  - `memory.deleted`
- Added CLI + env control for web admin serving:
  - `tandem-engine serve --web-ui --web-ui-prefix /admin`
  - `TANDEM_WEB_UI`, `TANDEM_WEB_UI_PREFIX`
- Updated engine command docs to include web-admin serve options.

---

# Tandem v0.3.7 Release Notes

## Release Date: 2026-02-18

### Highlights

- **Complete Simplified Chinese overwrite**: Replaced and normalized Simplified Chinese copy across major app surfaces for a consistent zh-CN experience.
- **Full localization sweep**: Converted remaining high-traffic hardcoded English strings to translation keys and completed `en`/`zh-CN` parity for startup, settings, packs, skills, and About.
- **Language-switch consistency**: Improved in-app language coverage so zh-CN selection now updates key shells and settings surfaces immediately and persistently.

---

# Tandem v0.3.6 Release Notes

## Release Date: 2026-02-18

### Highlights

- **TUI stale-engine guard at connect time**: TUI now validates connected engine version before attaching to shared endpoints.
- **Automatic stale replacement by default**: New `TANDEM_ENGINE_STALE_POLICY` defaults to `auto_replace`, so stale engines are replaced with a fresh managed runtime.
- **Managed-port fallback**: If shared/default port is occupied by stale runtime, TUI selects an available local port and continues.
- **Deterministic runtime diagnostics**: `/engine status` now includes required version, stale policy, and connection source (`shared-attached` vs `managed-local`).
- **Release line alignment**: Rust crates, app manifests, and npm wrappers are all bumped to `0.3.6`.

---

# Tandem v0.3.3 Release Notes

## Release Date: 2026-02-18

### Highlights

- **Agent Teams MVP foundation shipped**: Engine/server now includes Agent Teams primitives with unified server-enforced spawn policy gates across orchestrator, UI, and tool-triggered spawns.
- **Safety + audit contract strengthened**: Added role-edge enforcement, budget/cap controls, capability scoping, SKILL.md hash validation/audit wiring, and richer Agent Teams SSE event surfaces.
- **Publish/release pipeline stabilization**: Fixed crate dependency/version publish chain coupling and removed dependence on the Windows `--no-verify` workaround path for release flow continuity.
- **Docs + packaging clarity pass**: Added crate-level README guides and clarified npm wrapper package docs.

---

# Tandem v0.3.2 Release Notes

## Release Date: 2026-02-17

### Highlights

- **TUI PIN persistence fix**: Existing users with a valid `vault.key` now correctly enter unlock flow on startup, instead of being forced back into create-PIN flow when the keystore is empty or missing.
- **First-run provider onboarding fix**: After unlock, TUI now opens setup when no provider keys are configured in the unlocked keystore, ensuring API key setup is not skipped.
- **TUI startup flow hardening**: PIN-state detection now keys off vault existence, while provider onboarding keys off actual stored provider keys.

---

# Tandem v0.3.1 Release Notes

## Release Date: 2026-02-17

### Highlights

- **TUI provider onboarding hotfix**: TUI now requires a real configured provider before entering normal chat/session flow, and excludes fallback `local` provider from configured-provider checks.
- **Provider setup consistency**: Startup gating, provider checks, and `/key test` now share the same sanitized provider catalog behavior.
- **Desktop stream recovery fix**: Sidecar event-subscription failures during sidecar restart/startup are treated as transient for circuit-breaker accounting, preventing restart loops from tripping a long-lived open breaker.
- **StreamHub log/telemetry noise reduction**: Transition-state subscription failures now report as recovering retries instead of repeated hard `STREAM_SUBSCRIBE_FAILED` spam.

---

# Tandem v0.3.0 Release Notes

## Release Date: 2026-02-17

### Highlights

- **Massive orchestration platform expansion**: Added engine-native mission runtime, shared resources blackboard, tiered governed memory promotion, routine scheduler + policy gates, and Desktop/TUI parity controls.
- **Mission runtime now first-class**: New engine mission APIs support create/list/get/apply-event flows backed by shared reducer state and reviewer/tester rework gates.
- **Shared resource coordination layer**: Added revisioned blackboard APIs + SSE streams and status indexer updates derived from run/tool lifecycle events.
- **Memory learning with guardrails**: Implemented scoped `session/project/team/curated` governance with capability gating, scrub pipeline, and audit trails (`/memory/audit`).
- **Routine automation safety model**: Added user-creatable routines with scheduler durability, misfire behavior, manual `run_now`, lifecycle SSE, and explicit connector side-effect policy enforcement.
- **Phase 6 contract hardening complete (`W-019`)**: Mission/routine event families are now stable SDK contracts after server snapshot tests and Desktop/TUI parity consumption checks.
- **Control-plane discipline added**: Build flow now tracks work IDs, decision log, and progress cadence in `docs/design` to keep multi-file pushes coordinated.

- **Keystore/runtime auth realignment**: Provider keys now flow through runtime auth (`PUT /auth/{provider}`) instead of config-secret patch paths.
- **Config secret write blocking**: `PATCH /config` and `PATCH /global/config` now reject `api_key`/`apiKey` with `400 CONFIG_SECRET_REJECTED`.
- **Desktop/TUI transport update**: Desktop and TUI now push provider keys from keystore to runtime auth on connect/start rather than persisting keys via config patch payloads.
- **Plaintext persistence regression fixed**: Closed a beta gap where provider keys could appear in Tandem config files under certain flows.
- **Browser API compatibility**: Added CORS support on engine HTTP endpoints so browser examples with `X-Tandem-Token` can pass preflight.

- **Engine CLI concurrency mode**: Added `tandem-engine parallel` to run multiple prompts concurrently with explicit JSON task batches and bounded concurrency control.
- **Engine CLI docs overhaul**: Added a bash/WSL-first `ENGINE_CLI.md` with practical examples for `run`, `tool`, `parallel`, and full `serve` + HTTP/SSE workflows.
- **Engine communication reference**: Added `ENGINE_COMMUNICATION.md` documenting client/runtime topology, API/run lifecycle contracts, and observability paths.
- **Default engine port hardening**: Standardized default engine endpoint to `127.0.0.1:39731` (from `3000`) to avoid common frontend-dev collisions, with env override support across engine, desktop sidecar, and TUI.
- **Engine API token hardening**: Added token-gated engine API auth with keychain-first token persistence (fallback file), desktop masked/reveal/copy token UX, and TUI `/engine token` commands.

- **Plan mode todo-call recovery**: Fixed repeated `todowrite` no-op loops by normalizing common todo payload aliases and skipping empty todo executions.
- **Plan mode clarification fallback**: Added engine-level structured `question.asked` fallback when planning cannot derive a concrete todo list.
- **Todo fallback precision**: Todo extraction fallback now only accepts structured checklist/numbered lines, preventing prose clarification text from becoming fake tasks.
- **Question modal reliability**: Desktop now normalizes `permission(tool=question)` into the question overlay flow so walkthrough prompts reappear consistently.
- **Permission scope isolation**: Permission prompts are now scoped to the active session to avoid cross-session approval bleed when multiple clients are connected.
- **Memory architecture consolidation**: Desktop now consumes the shared `tandem-memory` crate directly, eliminating duplicated local memory implementation paths.
- **Strict memory search scope guarantees**: Added a dedicated `memory_search` tool with explicit session/project scope requirements and blocked global-tier queries.
- **Embedding health visibility**: Memory retrieval telemetry and settings now expose embedding backend status and reason, surfaced in chat/settings badges.
- **Windows memory-test link fix**: Resolved CRT mismatch (`LNK2038`) in `tandem-memory` test linking by patching vendored `esaxx-rs` CRT behavior.
- **Idle stream health**: Stream watchdog now skips degraded status while the app is idle with no active runs or tool calls.

- **Engine-owned skills system expansion**: Skills are now discovered from multiple ecosystem paths with deterministic priority and exposed through unified engine APIs/tooling for desktop + TUI parity.
- **Per-agent skill activation**: Agents can now optionally define equipped skills (`skills`) to control which discovered skills are active for that agent.
- **Universal skill access at mode level**: Mode allowlists no longer block the `skill` tool, preventing accidental lockout of installed skills.

- **TUI multi-agent reliability pass**: fixed silent/no-response prompt runs by hardening run-scoped SSE handling and stream termination behavior.
- **Auth/key setup flow repaired**: TUI now syncs unlocked local keystore keys into engine provider config on connect, including legacy key-name alias mapping.
- **Key setup wizard in CLI**: added interactive key-setup routing when a provider is selected but not connected.
- **Error visibility improvements**: stream errors (`session.error` and failed run finishes) are surfaced directly in transcript output.
- **Transcript readability improvements**: long lines now wrap in the TUI flow renderer; added `/last_error` for quick full error recall.
- **Working-state UX**: added active-agent spinner/status activity indicators in footer and grid pane titles.
- **Windows dev docs fixes**: added explicit PowerShell equivalents for build/copy/tauri-dev steps and lock-file recovery guidance.
- **Request center in TUI**: added approval/answer modal flow for pending permissions and questions (`Alt+R`, `/requests`) with keyboard-only controls.
- **Permission context clarity**: approval modal now shows mode + tool intent and explains why a permission is needed (especially in Plan mode).
- **Plan/question flow repair**: normalized `permission(tool=question)` events into answerable question flows and added custom-answer support with multiple-choice prompts.
- **Startup/PIN polish**: fullscreen-centered PIN prompt, stricter digit-only PIN entry, and animated connecting screen that waits for full engine readiness before switching views.
- **Shared permission defaults**: desktop and TUI now consume centralized permission rule defaults from `tandem-core`.
- **TUI interaction polish**: moved grid toggle to `Alt+G`, increased scroll speed, and reduced in-transcript request noise in favor of status/request UI.
- **TUI composer/editor upgrade**: chat input now supports multiline editing with cursor navigation, delete-forward, and native paste insertion.
- **Markdown renderer upgrade**: assistant transcript markdown now renders through a tandem-local `pulldown-cmark` pipeline adapted from codex patterns (replacing `tui-markdown` dependency).
- **Streaming text correctness**: whitespace-only prompt deltas are preserved during active streaming instead of being dropped.
- **Long-transcript rendering performance**: TUI transcript rendering now virtualizes line materialization to avoid flattening full history every frame.
- **Render cache optimization**: added bounded per-message render cache (fingerprint + width keyed) to reduce repeated markdown/wrap work for large sessions.
- **Stream merge correctness**: fixed reducer merge paths so shorter success/failure snapshots cannot overwrite richer locally finalized stream tails.

## Highlights

- **Major-version break for runtime API naming**: Hard-renamed legacy `send_message_streaming` naming to split-semantics names aligned with Session-linear execution.
- **Desktop recovery contract coverage**: Added dedicated sidecar tests for reconnect recovery via `GET /session/{id}/run`.
- **Conflict and cancel-by-runID coverage**: Added explicit tests for `409` conflict parsing (`retryAfterMs`, `attachEventStream`) and client cancel-by-runID flows for desktop + CLI.
- **Dual-license rollout for Rust SDK/runtime**: Rust SDK and runtime packages are now `MIT OR Apache-2.0` for broader adoption and clearer patent grant coverage.
- **App/web licensing unchanged**: This pass does not change desktop/web app licensing scope.
- **Webpage -> Markdown extraction**: `webfetch_document` converts HTML into clean Markdown with link extraction and size stats.
- **Tool debugging via CLI**: `tandem-engine tool --json` runs tools directly, and `mcp_debug` returns raw MCP responses for parser validation.
- **Default web tools**: `websearch` (MCP-backed) and `webfetch_document` are now available in default modes with approval gating.
- **Websearch reliability hardening**: Added arg normalization/recovery, query-source observability, and loop-guard protections to prevent empty-args retries.
- **Sidecar build mismatch diagnostics**: Desktop now surfaces stale-engine mismatch details using sidecar `/global/health` build metadata.
- **Better provider auth diagnostics**: Authentication errors now include provider-specific API key guidance.
- **Desktop external links**: Assistant markdown links now open through Tauri opener support.

## Complete Feature List

### Naming and Interfaces

- Renamed server append handler to `post_session_message_append`.
- Renamed Tauri command `send_message_streaming` to `send_message_and_start_run`.
- Renamed frontend bridge export `sendMessageStreaming` to `sendMessageAndStartRun`.
- Renamed sidecar methods to explicit split semantics:
  - `append_message_and_start_run`
  - `append_message_and_start_run_with_context`

### Tests

- Added sidecar tests:
  - `test_parse_prompt_async_response_409_includes_retry_and_attach`
  - `test_parse_prompt_async_response_202_parses_run_payload`
  - `recover_active_run_attach_stream_uses_get_run_endpoint`
  - `cancel_run_by_id_posts_expected_endpoint`
  - `cancel_run_by_id_handles_non_active_run`
- Added `tandem-tui` client tests:
  - `cancel_run_by_id_posts_expected_endpoint`
  - `cancel_run_by_id_returns_false_for_non_active_run`

### Tools

- Added `webfetch_document` for HTML -> Markdown conversion, metadata/link extraction, and size stats.
- Added `mcp_debug` to capture raw MCP responses (status, headers, body, truncation).
- Added `tandem-engine tool --json` for direct CLI tool execution.
- Improved MCP tool calls to accept `text/event-stream` responses.
- Hardened `websearch` call reliability:
  - normalize args before permission + execution
  - infer/recover missing query deterministically
  - emit `query_source` + `query_hash` metadata
  - circuit-break repeated identical search signatures

### Reliability and Diagnostics

- Added additive `/global/health` fields:
  - `build_id`
  - `binary_path` (debug)
- Added desktop stale sidecar binary mismatch warning path in dev workflows.
- Improved provider auth failure hints to map to the selected provider key.
- Added Tauri-backed external link opening for assistant markdown content.

### Modes and Permissions

- Default mode presets now include `websearch` and `webfetch_document`.
- Added permission rule support for `webfetch_document`.

### Docs

- Expanded `ENGINE_TESTING.md` with tool testing examples, size savings, and Windows quickstart.

---

# Tandem v0.2.25 Release Notes

## Highlights

- **Canonical Marketing Core 9**: Added dedicated starter skills for SEO audit, social content, content strategy, copywriting/editing, email sequencing, launch planning, competitor alternatives, and shared product marketing context.
- **Template Install Completeness**: Starter template install now copies full template folders (including `references/`, scripts, and assets), not just `SKILL.md`.
- **Skill Parser Reliability Fixes**: Removed UTF-8 BOM issues in template `SKILL.md` files and fixed YAML `tags` format in affected starter skills.
- **No-Duplicate Marketing Routing**: Added canonical mapping guidance and updated UI recommendations to prioritize canonical marketing starters over legacy fallback templates.
- **Shared Context Path Update**: Migrated marketing context references from `.claude/...` to `scripts/marketing/_shared/...`.
- **Version Sync**: Bumped app metadata to `0.2.25`.

## Complete Feature List

### Skills

- Added starter templates:
  - `product-marketing-context`
  - `content-strategy`
  - `seo-audit`
  - `social-content`
  - `copywriting`
  - `copy-editing`
  - `email-sequence`
  - `competitor-alternatives`
  - `launch-strategy`
- Updated marketing legacy templates to indicate fallback usage.
- Added shared `references/product-marketing-context-template.md` to canonical marketing templates.

### Backend

- Updated `skills_install_template` to recursively copy template directories.
- Added template-directory resolver helper for install path validation.

### UI

- Updated Skills recommendations to rank canonical marketing templates first for marketing-intent discovery.

### Docs

- Added `docs/marketing_skill_canonical_map.md`.

---

# Tandem v0.2.24 Release Notes

## Highlights

- **Custom Modes MVP (complete)**: Added backend-authoritative custom modes plus full frontend management in `Extensions -> Modes`.
- **Guided Builder + Advanced Editor**: Added a beginner-friendly guided wizard and a power-user manual editor with import/export.
- **AI-Assisted Mode Creation**: Added optional AI assist with a bundled `mode-builder` skill template and paste/parse preview-before-apply flow.
- **Mode Icons**: Added icon selection for custom modes and icon rendering in chat mode selector.
- **Mode Selector Refresh**: Chat mode selector now loads built-in + custom modes dynamically with cleaner compact labels.
- **Indexing Default On**: Auto-index on project load now defaults to enabled for new settings state.
- **Update detection fix**: Synced version metadata across `tauri.conf.json`, `package.json`, and `Cargo.toml` so auto-updates detect new releases correctly.

## Complete Feature List

### Updates

- Align version numbers across app metadata to prevent false "up to date" status in the updater.

### Modes

- Added complete custom mode management APIs and backend enforcement path.
- Added deterministic precedence merge: `builtin < user < project`.
- Added safe fallback behavior for missing/invalid selected modes.
- Added mode CRUD + import/export UI in Advanced Editor.
- Added Guided Builder with preset-based configuration and preview-before-apply.
- Added optional AI Assist:
  - start AI builder chat from Modes UI
  - parse a pasted JSON result
  - preview and apply only after explicit user action
- Added custom mode icon support in both creation flows and chat selector rendering.

---

# Tandem v0.2.23 Release Notes

## Highlights

- **Project-scoped orchestrator context**: Fixed Orchestrator mode reopening a stale run from a different project/workspace.
- **Clean project switching behavior**: Switching or adding projects now clears stale orchestrator run selection so each workspace starts from the correct context.
- **Safer default resume logic**: Opening Orchestrator with no explicit run now auto-resumes only active runs and does not auto-open completed/terminal history.
- **Persistent orchestrator console history**: Orchestrator Console logs now reload correctly after closing/reopening the drawer, including task-session tool calls.
- **Clear runtime visibility**: Added global activity badges and per-session running indicators so concurrent chat + orchestrator work is visible at a glance.
- **Actionable failure feedback**: Retry/restart failures now surface directly in Orchestrator UI with specific reason text (for example model/provider resolution errors).
- **User-adjustable orchestrator budgets**: Added in-panel controls to increase budget headroom or relax caps so long-running runs can continue without starting over.
- **Reduced warning noise**: Repetitive orchestrator budget warnings are now throttled to prevent log spam.

## Complete Feature List

### Orchestrator

- Clear `currentOrchestratorRunId` and close stale orchestrator panel state during project switch/add flows so previous-workspace run IDs are not carried forward.
- When entering Orchestrator without an explicit run selection, only auto-select runs in active states:
  - `planning`
  - `awaiting_approval`
  - `executing`
  - `paused`
- Avoid auto-selecting terminal/history states by default:
  - `completed`
  - `failed`
  - `cancelled`
- Added a guard to reset the selected run if it is not present in the active workspace run list.
- Fixed Orchestrator Logs drawer wiring so Console receives orchestrator session scope instead of mounting without session context.
- Console history reconstruction now aggregates run base session + task child sessions for complete persisted tool execution history.
- Console live event handling is scoped to run-related session IDs to avoid unrelated stream noise.
- Retry/restart failure reasons are now surfaced in-panel via `run_failed` handling and persisted snapshot error hydration.
- Backend failure completion now preserves concrete failed-task reason text when transitioning run status to `failed`.
- Added direct budget controls in Orchestrator UI:
  - `Add Budget Headroom` (+100 iterations, +100k tokens, +30 minutes wall time, +500 sub-agent calls)
  - `Relax Max Caps` (sets very high safety limits for extended runs)
- Extending budget limits now updates persisted run config/budget state and clears `Budget exceeded` failure state when limits are no longer exceeded, allowing resume.
- Budget warning logs are now throttled:
  - log on 5% bucket increases (80, 85, 90, ...)
  - or after cooldown (30s)
  - instead of warning every execution loop tick.

### Sessions + Chat Runtime UX

- Selecting a normal chat session now reliably exits Orchestrator view and clears stale run selection.
- Added sidebar running markers for chat sessions (`RUNNING`) and orchestrator items (`EXECUTING`/active spinner).
- Added top-right runtime badges for concurrent activity:
  - `N CHATTING`
  - `N ORCHESTRATING`
- Chat running indicators now use global sidecar stream-derived running session IDs, so indicators remain correct even when switching to a different selected session.
- Removed duplicate/overlapping spinner states in the session row to keep one clear running signal per item.

---

# Tandem v0.2.21 Release Notes

## Highlights

- **Provider filtering that scales**: The model selector now uses a compact provider dropdown (`All` + visible providers) instead of horizontal scrolling chips.
- **Faster provider targeting from keyboard**: Search now supports `provider:<id-or-name>` (for example `provider:openrouter sonnet`).
- **Clearer visibility rules**: Added in-context helper copy ("Showing configured providers + local") so provider filtering behavior is explicit.
- **Better empty-state feedback**: No-result messages now explain when the active provider filter has no matching models.
- **Readable fullscreen file previews**: Fullscreen file preview now uses a stronger surface backdrop so document text stays legible across transparent/gradient themes.

## Complete Feature List

### Model Selector UX

- Replaced horizontal provider chip rail with a full-width provider dropdown to improve usability with many providers.
- Kept existing provider visibility policy (configured providers + local defaults) and surfaced that behavior in the dropdown header.
- Added resilient filter behavior so provider selection resets to `All` if a previously selected provider disappears after model reload.
- Improved no-result messaging to include provider context when filtering within a specific provider.

### Files

- Increased fullscreen file preview backdrop opacity/contrast so file text remains readable instead of blending with theme background layers.

### Search

- Added provider token parsing in model search:
  - `provider:openrouter`
  - `provider:OpenCodeZen`
- Provider token filtering composes with existing model name/id search text.

---

# Tandem v0.2.20 Release Notes

## Highlights

- **Stream reliability foundation**: Tandem now uses a single global stream hub with one long-lived sidecar subscription, then fans events out internally to chat, orchestrator, and Ralph.
- **Modern event envelopes (backward compatible)**: Added additive `sidecar_event_v2` metadata envelopes (`event_id`, `correlation_id`, `ts_ms`, `session_id`, `source`, `payload`) while keeping legacy `sidecar_event`.
- **Smoother busy-chat UX**: Enter while generation is active now queues messages (FIFO) with queue controls to send next/all or remove items.
- **Skills import lifecycle upgrade**: Added SKILL.md/zip import preview and apply workflows with conflict policy control (`skip`, `overwrite`, `rename`) and richer metadata surfacing.

## Complete Feature List

### Streams + Reliability

- Added `stream_hub` as the centralized event substrate for app runtime streaming.
- Refactored `send_message_streaming` to send-only; stream relay now comes from shared hub.
- Migrated orchestrator and Ralph event consumption off independent sidecar subscriptions onto hub fanout.
- Added stream health signaling (`healthy`, `degraded`, `recovering`) and surfaced it in chat.
- Added frontend deterministic event dedupe keyed by `event_id` from v2 envelopes.

### Chat UX

- Added queue IPC + UI integration:
  - `queue_message`
  - `queue_list`
  - `queue_remove`
  - `queue_send_next`
  - `queue_send_all`
- Queue behavior is FIFO and supports enqueueing while the assistant is currently generating.
- Added inline queue preview controls in chat.
- Added missing `memory_retrieval` stream handling path in chat.
- Improved inline process/tool summary cards with compact status and counts.

### Skills Lifecycle

- Added backend APIs:
  - `skills_import_preview(fileOrPath, location, namespace?, conflictPolicy)`
  - `skills_import(fileOrPath, location, namespace?, conflictPolicy)`
- Added zip-pack SKILL discovery and preview summary before apply.
- Added deterministic conflict handling policies: `skip`, `overwrite`, `rename`.
- Added namespaced import-path support for better organization.
- Expanded surfaced skill metadata:
  - `version`
  - `author`
  - `tags`
  - `requires`
  - `compatibility`
  - `triggers`
- Improved invalid-skill parse feedback in installed skills UX.

---

# Tandem v0.2.19 Release Notes

## Highlights

- **Verifiable memory retrieval**: Chat now executes vector retrieval before prompt send, emits telemetry, and shows a per-response memory badge in chat.
- **Cleaner diagnostics UX**: Logs drawer now separates Tandem logs and Console activity, and fullscreen log height scales dynamically.
- **Sidecar lifecycle hardening**: Start/stop transitions are serialized to prevent duplicate OpenCode/Bun process spawns.
- **Pink Pony readability pass**: Theme contrast/surface opacity tuned for better legibility against bright gradients.
- **Chat Performance**: Long chat sessions now render smoothly thanks to new list virtualization and optimization.

## Complete Feature List

### Memory + Chat

- Run memory retrieval in both send paths (`send_message`, `send_message_streaming`) before forwarding to sidecar.
- Emit `memory_retrieval` stream events with:
  - `used`
  - `chunks_total`
  - `session_chunks`
  - `history_chunks`
  - `project_fact_chunks`
  - `latency_ms`
  - `query_hash` (short SHA-256 prefix)
  - `score_min` / `score_max`
- Inject formatted `<memory_context>` above user content only when retrieval returns context.
- Show assistant-side memory capsule with a brain icon:
  - `Memory: X chunks (Yms)` when used
  - `Memory: not used` when skipped or empty

### Logging + Console

- Route memory telemetry through a distinct `tandem.memory` logging target for easier log filtering/scanning.
- Keep telemetry privacy-safe (no raw user query text, no chunk content/body logging).
- Remove redundant OC sidecar tab from the log viewer and use Console tab for command/tool activity.
- Fix log drawer fullscreen sizing so list height re-measures on resize and tab/expand changes.

### Sidecar Reliability

- Add sidecar lifecycle serialization lock to prevent concurrent start/stop races that could spawn duplicate OpenCode/Bun processes.

### Themes

- Improve Pink Pony readability with higher-contrast text, stronger surface opacity, and clearer borders/glass values.

### Performance

- **Chat Virtualization**: Implemented list virtualization for the chat interface, ensuring O(1) rendering performance regardless of message history length.
- **Component Memoization**: Optimized message rendering to prevent unnecessary re-renders during typing and streaming.
- **Build Reliability**: Fixed strict TypeScript errors in the Logs Drawer to ensure clean production builds.

---

# Tandem v0.2.18 Release Notes

## Highlights

- **Workspace Python venv enforcement**: Venv-only python/pip policy now applies consistently, including staged/batch execution, and the Python Setup wizard auto-opens when Python is blocked.
- **Python pack hygiene**: Python packs ship `requirements.txt` and venv-first docs (no more encouraging global `pip install`).
- **Better file previews**: File preview supports a dock mount + fullscreen toggle.

## Work In Progress / Known Issues

- **Files Auto-Refresh (WIP)**: The Files tree does not reliably refresh when tools/AI create new files in the workspace. Deeper investigation needed; workaround is to navigate away and back to Files.

## Complete Feature List

### Python

- Enforce venv-only python/pip usage across approval flows and staged/batch execution.
- Auto-open the Python Setup (Workspace Venv) wizard when Python is blocked by policy.
- Add a shared policy helper + tests for consistent enforcement across tool approval paths.

### Packs

- Data Visualization + Finance Analysis packs ship `requirements.txt`.
- Pack docs are venv-first (install via `.opencode/.venv`).
- Packs can include a pack-level `CONTRIBUTING.md` which is installed alongside `START_HERE.md`.

### Files

- File preview supports a dock mount + fullscreen toggle.

---

# Tandem v0.2.17 Release Notes

## Highlights

- **Custom Background Opacity Slider Fix**: Fix opacity changes causing the background image to flash or disappear in bundled builds by keeping the resolved image URL stable and updating only opacity.
- **Reliable Background Layering**: Render the custom background image as a dedicated fixed layer for consistent stacking across views.

---

# Tandem v0.2.16 Release Notes

## Highlights

- **Update Prompt Layout Fix**: Fix the in-app update prompt becoming constrained/squished due to theme background layering CSS.

---

# Tandem v0.2.15 Release Notes

## Highlights

- **Custom Background Loading Fix**: Fix custom background images failing to load after updating in some packaged builds by falling back to an in-memory `data:` URL when the `asset:` URL fails.

---

# Tandem v0.2.14 Release Notes

## Highlights

- **Theme Background Art Pass**: Cosmic Glass (starfield + galaxy glow), Pink Pony (thick arcing rainbow), and Zen Dusk (minimalist ink + sage haze).
- **Custom Background Image Overlay**: Choose a background image (copied into app data) and overlay it on top of the active theme, with an opacity slider in Settings.
- **Settings/About/Extensions Restored**: Fix a regression where Settings/About/Extensions views would not appear.
- **Document Text Extraction (Rust)**: PDF/DOCX/PPTX/XLSX (and more) can now be extracted to plain text for preview and for attaching to skills/chats, without requiring Python.
- **Python Venv Wizard + Safety Enforcement**: In-app wizard creates `.opencode/.venv` per workspace and installs dependencies into it; AI tool calls are blocked from running global `pip install` or `python` outside the venv.
- **Startup Session Restore Fix**: Restored sessions now open reliably on startup (no need to reselect a session).

---

# Tandem v0.2.13 Release Notes

## Highlights

- **New Starter Skills**: Add two new bundled starter skills: `brainstorming` and `development-estimation`.
- **Runtime Requirement Pills**: Starter skill cards can show optional runtime hints (Python/Node/Bash) via `requires: [...]` YAML frontmatter.
- **Skills UX Improvements**: Clearer install/manage experience (runtime note, installed-skill counts, and better discoverability of deletion).
- **Packs Page Cleanup**: Packs page now shows packs only (no starter skills section) and surfaces the runtime note at the top.
- **Diagnostics Polishing**: Logs viewer improvements (fullscreen + copy feedback) and fix invalid bundled skill template frontmatter so templates aren’t skipped.
- **Dev Quality of Life**: In `tauri dev`, starter skill templates are loaded from `src-tauri/resources/skill-templates/` so newly added templates appear immediately.

---

# Tandem v0.2.12 Release Notes

## Highlights

- **Orchestrator Model Routing Fix**: Orchestrator runs persist the selected provider/model and always send prompts with an explicit model spec, avoiding "unknown" run model and preventing runs that never reach the provider.
- **Orchestrator Restart/Retries**: Prevent Restart/Retry from claiming "Completed" without doing any work by guarding against empty plans and allowing completed runs to rerun the full plan.
- **Delete Orchestrator Runs**: Orchestrator runs can now be deleted from the Sessions sidebar (removes the run from disk and deletes its backing OpenCode session).
- **Better In-App Log Sharing**: The Logs drawer supports horizontal scroll for long lines, plus selected-line preview and one-click copy helpers.
- **Release to Discord Notifications**: Automated releases now post to Discord reliably (publishing via `GITHUB_TOKEN` does not trigger separate `release: published` workflows).

---

# Tandem v0.2.11 Release Notes

## Highlights

- **No More Stuck "Pending Tool" Runs**: Prevent sessions from hanging indefinitely when an OpenCode tool invocation never reaches a terminal state. Tandem now ignores heartbeat/diff noise, recognizes more tool terminal statuses, and fail-fast cancels the request with a visible error after a timeout.
- **On-Demand Log Streaming Viewer**: A new Logs side drawer can tail Tandem's own app logs and show OpenCode sidecar stdout/stderr (captured safely into a bounded in-memory buffer). It only streams while open to avoid baseline performance cost.
- **Orchestrator Model Routing Fix**: Orchestrator runs now persist the selected provider/model and always send prompts with an explicit model spec, avoiding "unknown" run model and preventing runs that never reach the provider.
- **Cleaner Logs**: OpenCode `server.*` heartbeat SSE events are ignored (and other unknown SSE events are downgraded) to prevent warning spam.
- **Poe Provider**: Add Poe as an OpenAI-compatible provider option (endpoint + `POE_API_KEY`). Thanks [@CamNoob](https://github.com/CamNoob).
- **Release Pipeline Resilience**: GitHub Release asset uploads now retry to reduce flakes during transient GitHub errors.

_Note: v0.2.10 was a failed release attempt due to a GitHub incident during asset upload; v0.2.11 is the re-cut._

---

# Tandem v0.2.10 Release Notes

## Highlights

_Release attempt failed on 2026-02-09 due to GitHub release asset upload errors during a GitHub incident; no assets were published._

---

# Tandem v0.2.9 Release Notes

## Highlights

- **Project File Indexing**: Incremental per-project workspace file indexing with total/percent progress, plus All Projects vs Active Project stats scope and a "Clear File Index" action (optional VACUUM) to reclaim space.
- **Question Prompts**: Properly handle OpenCode `question.asked` events (including multi-question requests) and show an interactive one-at-a-time wizard with multiple-choice + custom answers; replies are sent via the OpenCode reply API.
- **Startup Session History**: When Tandem restores the last active folder on launch, it now loads that folder's session history automatically (no more empty sidebar until a manual refresh).
- **Windows Dev Reload Sidecar Cleanup**: Prevent orphaned OpenCode sidecar (and Bun) processes during `pnpm tauri dev` rebuilds by attaching the sidecar to a Windows Job Object (kill-on-close).

---

# Tandem v0.2.8 Release Notes

## Highlights

- **Multi Custom Providers (OpenCode)**: Select any provider from the OpenCode sidecar catalog, including user-defined providers by name in `.opencode/config.json`.
- **Model Selection Routing**: Persist the selected `provider_id` + `model_id` and prefer it when sending messages, so switching to non-standard providers actually takes effect.

---

# Tandem v0.2.7 Release Notes

## Highlights

- **OpenCode Config Safety**: Prevent OpenCode config writes from deleting an existing `opencode.json` when replacement fails (e.g. file locked on Windows).
- **Sidecar Idle Memory**: Set Bun/JSC memory env hints to reduce excessive idle memory usage.

---

# Tandem v0.2.6 Release Notes

## Highlights

- **macOS Release Builds**: Disabled signing/notarization by default to prevent macOS build failures when Apple certificate secrets are missing or misconfigured. (Enable with `MACOS_SIGNING_ENABLED=true`.)

---

# Tandem v0.2.5 Release Notes

## Highlights

- **Release Build Trigger**: Re-cut release so GitHub Actions builds run with the corrected release workflow configuration.

---

# Tandem v0.2.4 Release Notes

## Highlights

- **Starter Pack Installs Fixed**: Starter Packs and Starter Skills now install correctly from packaged builds (bundled resource path resolution).
- **Custom Provider Onboarding**: Custom endpoints (e.g. LM Studio / OpenAI-compatible) are treated as configured, so onboarding no longer forces you back to Settings.
- **Vector DB Stats**: New Settings panel to track vector DB size/chunks and manually index your workspace (with progress).
- **macOS Release Hardening**: Release workflow now supports optional signing/notarization inputs and runs Gatekeeper verification in CI.

## Complete Feature List

### Starter Packs & Skills

- Fix bundled pack/template discovery in production builds so installs work reliably.
- Show more actionable pack install errors in the UI.

### Onboarding

- Treat enabled Custom providers with an endpoint as “configured” to avoid onboarding loops.

### Memory

- Add a Vector DB stats card in Settings.
- Add manual “Index Files” action with progress events and indexing summary.

### Release / CI

- Add Gatekeeper verification of produced macOS DMGs (`codesign`, `spctl`, `stapler validate`).
- Add optional Apple signing/notarization inputs to the GitHub release workflow.

---

# Tandem v0.2.3 Release Notes

## Highlights

- **Orchestration Session Spam Fix**: Orchestration Mode no longer creates an endless stream of new root chat sessions while running tasks. Internal task/resume sessions are now treated as child sessions and the session list prefers root sessions only.

## Complete Feature List

### Orchestration

- Prevent internal orchestration task/resume sessions from flooding the main session history.

---

# Tandem v0.2.2 Release Notes

## Highlights

- **Knowledge Work Plugins Migration**: We've completed the massive effort to migrate all legacy knowledge work plugins into Tandem's native skill format. This brings a wealth of specialized capabilities directly into your local workspace.
- **New Power Packs**:
  - **Productivity**: A complete system for memory and task management with a built-in visual dashboard.
  - **Sales**: A suite of tools for account research, call prep, and asset creation.
  - **Bio-Informatics**: Specialized skills for scientific research and data analysis.
- **Model Agnostic**: All new skills are designed to work seamlessly with any AI model you choose to connect.
- **Extensions + MCP Integrations**: New Extensions area lets you configure OpenCode plugins and MCP servers (remote HTTP + local stdio), test remote connectivity, and use presets (Context7, DeepWiki).
- **Skills Search**: Search Starter skills and Installed skills from one box.

## Complete Feature List

### Skills & Packs

- **Productivity Pack**:
  - `productivity-memory`: Two-tier memory system (Working + Deep Memory) for decoding workplace shorthand.
  - `productivity-tasks`: Markdown-based task tracker with dashboard support.
  - `productivity-start` & `productivity-update`: Workflows to initialize and sync your productivity system.
  - Additional tools: `inbox-triage`, `meeting-notes`, `research-synthesis`, `writing-polish`.
- **Sales Pack**:
  - `sales-account-research`, `sales-call-prep`, `sales-competitive-intelligence`, `sales-create-asset`, `sales-daily-briefing`, `sales-draft-outreach`.
- **Bio-Informatics Pack**:
  - `bio-instrument-data`, `bio-nextflow-manager`, `bio-research-strategy`, `bio-single-cell`, `bio-strategy`.
- **Data Science Pack**:
  - `data-analyze`, `data-build-dashboard`, `data-create-viz`, `data-explore-data`, `data-validate`, `data-write-query`.
- **Enterprise Knowledge Pack**:
  - `enterprise-knowledge-synthesis`, `enterprise-search-knowledge`, `enterprise-search-source`, `enterprise-search-strategy`, `enterprise-source-management`.
- **Finance Pack**:
  - `finance-income-statement`, `finance-journal-entry`, `finance-reconciliation`, `finance-sox-testing`, `finance-variance-analysis`.
- **Legal Pack**:
  - `legal-canned-responses`, `legal-compliance`, `legal-contract-review`, `legal-meeting-briefing`, `legal-nda-triage`, `legal-risk-assessment`.
- **Marketing Pack**:
  - `marketing-brand-voice`, `marketing-campaign-planning`, `marketing-competitive-analysis`, `marketing-content-creation`, `marketing-performance-analytics`.
- **Product Pack**:
  - `product-competitive-analysis`, `product-feature-spec`, `product-metrics`, `product-roadmap`, `product-stakeholder-comms`, `product-user-research`.
- **Support Pack**:
  - `support-customer-research`, `support-escalation`, `support-knowledge-management`, `support-response-drafting`, `support-ticket-triage`.
- **Design & Frontend Pack**:
  - `canvas-design` (includes font library), `theme-factory`, `frontend-design`, `web-artifacts-builder`, `algorithmic-art`.
- **Utilities**:
  - `internal-comms`, `cowork-mcp-config-assistant`.

### Extensions + Integrations (MCP)

- New top-level **Extensions** area with tabs:
  - Skills
  - Plugins
  - Integrations (MCP)
- Configure MCP servers in OpenCode config:
  - Remote HTTP endpoints with optional headers
  - Local stdio servers (command + args)
  - Global vs Folder scope
- Test remote MCP servers using a real MCP `initialize` request:
  - Validates JSON-RPC response
  - Supports servers that respond with JSON or SSE
- Popular presets:
  - Context7: `https://mcp.context7.com/mcp`
  - DeepWiki: `https://mcp.deepwiki.com/mcp`

### Quality / Fixes

- Fixed MCP "Test connection" to stop showing Connected for HTTP errors like 405/410 and to provide actionable error labels.

---

# Tandem v0.2.1 Release Notes

## Highlights

- **First-outcome onboarding**: a guided wizard helps new users pick a folder, connect AI, and run a starter workflow in minutes.
- **Starter Packs + Starter Skills (offline)**: install curated, local-first templates directly from the app—no copy/paste required (advanced SKILL.md paste remains available).
- **More reliable Orchestration**: runs now pause on provider quota/rate-limit errors so you can switch model/provider and resume, instead of failing after max retries.

## Complete Feature List

### UX

- Onboarding wizard to drive a “first successful outcome”.
- Packs panel for browsing and installing bundled workflow packs.
- Starter Skills gallery with a clear separation between templates and “Advanced: paste SKILL.md”.
- Reduced developer-jargon in key surfaces to better match a non-coder-first product.

### Orchestration

- Increased default iteration/sub-agent budgets and auto-upgraded older runs created with too-low limits.
- Provider quota/rate-limit detection now pauses runs (and avoids burning retries), enabling recovery without restarting from scratch.
- Model selection is available even after a run fails to support “switch and resume”.

### Platform / Reliability

- Provider env vars are explicitly synced/removed and sidecar restarts correctly apply config changes.

### Contributors

- Added top-level product docs (`VISION.md`, `PRODUCT.md`, `PRINCIPLES.md`, `ARCHITECTURE.md`, `ROADMAP.md`).
- Added GitHub issue templates and a PR template.
- CI now fails on frontend lint instead of ignoring violations.

---

# Tandem v0.2.0 Release Notes

## Highlights

- **Multi-Agent Orchestration (Phase 1)**: The biggest update to Tandem yet. We've introduced a dedicated Orchestration Mode that coordinates specialized AI agents to solve complex, multi-step problems together. Instead of a single chat loop, Tandem now builds a plan, delegates tasks to "Builder" and "Validator" agents, and manages the entire process with a visual Kanban board.
- **Unified Sidebar**: We've simplified navigation by merging standard Chat Sessions and Orchestrator Runs into a single, unified list. Your entire history is now organized chronologically under smart project headers, so you never lose track of a context.

## Complete Feature List

### Orchestration

- **DAG Execution Engine**: Tasks are no longer linear. The orchestrator manages dependencies, allowing independent tasks to run in parallel.
- **Specialized Sub-Agents**:
  - **Planner**: Breaks down objectives into executable steps.
  - **Builder**: Writes code and applies patches.
  - **Validator**: Runs tests and verifies acceptance criteria.
- **Safety First**: New policy engine enforces distinct permission tiers for reading vs. writing vs. web access.
- **Budget Controls**: Hard limits on tokens, time, and iterations prevent runaway costs.

### UX Improvements

- **Unified Session List**: A cleaner, more organized sidebar that handles both Chat and Orchestrator contexts seamlessly.
- **Real-time Status**: See at a glance which runs are completed, failed, or still in progress directly from the sidebar.

---

# Tandem v0.1.15 Release Notes

## Highlights

- **Unified Update Experience**: We've completely overhauled how updates are handled. Previously, the application and the local AI engine (sidecar) had separate, disjointed update screens. Now, they share a unified, polished full-screen interface that communicates progress clearly and resolves scheduling conflicts.

## Complete Feature List

### UX Improvements

- **Unified Update UI**: A shared visual component now powers the "checking", "downloading", and "installing" states for all types of updates.
- **Blocking Update Overlay**: Critical updates for the app now present a prominent, focused overlay that can't be missed, ensuring you're always on the latest version.
- **Conflict Management**: Smart layering ensures that if both an app update and an AI engine update are available, the app update (which requires a restart) takes priority to prevent corrupted states.

---

# Tandem v0.1.14 Release Notes

## Highlights

- **Task Completion Relaibility**: We've tightened the feedback loop between the AI's work and the UI. Now, when Ralph Loop or Plan Mode executes a task, it is explicitly instructed to mark that task as "completed" in your list using the `todowrite` tool. This fixes the annoying desync where the AI would finish the work but leave the checkbox empty.
- **Smarter Execution Prompts**: The automated prompts used during plan execution have been refined to ensure the AI understands exactly how to report its progress back to the interface.

## Complete Change List

### Core Intelligence

- **Prompt Engineering**: Updated `ralph/service.rs` and `Chat.tsx` to include strict directives for task status updates. The AI is now mandated to call `todowrite` with `status="completed"` immediately after finishing a task item.

- **Ralph Loop (Iterative Task Agent)**: Meet Ralph—a new mode that puts the AI in a robust "do-loop." Give it a complex task, and it will iterate, verify, and refine its work until it meets a strict completion promise. It's like having a tireless junior developer who checks their own work.
- **Long-Term Memory**: Tandem now remembers! We've integrated a semantic memory system using `sqlite-vec` that allows the AI to recall context from previous sessions and project documents. This means smarter, more context-aware assistance that grows with your project.
- **Semantic Context Retrieval**: Questions about your project now tap into a vector database of your codebase, providing accurate, relevant context even for large repositories that don't fit in a standard prompt.

## Complete Feature List

### Core Intelligence

- **Vector Memory Store**: Implemented a local, zero-trust vector database (`sqlite-vec`) to store and retrieve semantic embeddings of your codebase and conversation history.
- **Memory Context Injection**: The AI now automatically receives relevant context snippets based on your current query, reducing hallucinations and "I don't know" responses about your own code.
