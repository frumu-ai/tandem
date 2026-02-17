# Tandem v0.3.0 Release Notes

## Unreleased

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
