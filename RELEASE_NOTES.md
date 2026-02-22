# Release Notes

Canonical release notes live in `docs/RELEASE_NOTES.md`.

## v0.3.9 (Unreleased)

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
