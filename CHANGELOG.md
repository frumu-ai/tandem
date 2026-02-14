# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.26] - 2026-02-14

### Added

- **Internationalization (i18n) Foundation**: Added i18n initialization, translation namespaces, and locale bundles for English and Simplified Chinese.
- **Language Settings UI**: Added a dedicated language selection section in Settings with persisted language preference.
- **I18N Documentation**: Added `docs/I18N_GUIDE.md` for localization architecture and usage.
- **Provider Base URL Editing**: Added editable provider `Base URL` controls in Settings cards with save/cancel flow and keyboard support.
- **Endpoint Reset Control**: Added reset-to-default endpoint action for providers to quickly recover known defaults.

### Changed

- **Provider Endpoint Source of Truth**: Provider cards now render endpoint values from persisted provider config rather than static literals.
- **Version Metadata Sync**: Bumped version to `0.2.26` across `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml`.
- **Credits**: Thanks [@iridite](https://github.com/iridite) for the full `0.2.26` contribution set, including i18n and provider base URL improvements.

## [0.2.25] - 2026-02-12

### Added

- **Canonical Marketing Skills (Core 9)**: Added starter skill templates for `product-marketing-context`, `content-strategy`, `seo-audit`, `social-content`, `copywriting`, `copy-editing`, `email-sequence`, `competitor-alternatives`, and `launch-strategy`.
- **Marketing Skills Canonical Map**: Added `docs/marketing_skill_canonical_map.md` to document no-duplicate routing and fallback strategy.

### Changed

- **Skill Template Install Behavior**: `skills_install_template` now installs the full template directory recursively (not just `SKILL.md`), so bundled `references/`, scripts, and assets ship with installs.
- **Marketing Starter Ordering**: Updated `SkillsPanel` recommendations to prioritize canonical marketing skills over legacy/fallback templates for marketing-intent discovery.
- **Shared Marketing Context Path**: Replaced `.claude/product-marketing-context.md` references with `scripts/marketing/_shared/product-marketing-context.md` and included shared context template references.

### Fixed

- **Skill Template Parsing Reliability**: Re-saved template `SKILL.md` files in UTF-8 without BOM to prevent false `missing or malformed frontmatter (---...---)` parser failures.
- **Template Frontmatter YAML**: Fixed invalid `tags` format in `development-estimation` and `mode-builder` (`string` -> YAML sequence).
- **Legacy Marketing Template Labeling**: Updated overlapping bundled marketing templates to clearly indicate legacy/fallback usage.
- **Version Metadata Sync**: Bumped version to `0.2.25` across app metadata for release consistency.

## [0.2.24] - 2026-02-12

### Added

- **Custom Modes (Phased MVP Complete)**: Added end-to-end custom mode support with backend-authoritative enforcement, including mode listing, create/update/delete, import/export, deterministic precedence (`builtin < user < project`), and safe fallback behavior.
- **Guided Mode Builder**: Added a non-technical, step-by-step mode creation wizard in `Extensions -> Modes`.
- **Mode Management in Extensions**: Added a dedicated `Modes` area under `Extensions` with `Guided Builder` and `Advanced Editor` views.
- **AI-Assisted Mode Builder**: Added optional AI assist flow in Guided Builder with:
  - `Start AI Builder Chat`
  - paste-and-parse JSON preview before apply
  - new bundled skill template: `mode-builder`
- **Mode Icons**: Added selectable mode icons that render in chat mode selector UI.

### Changed

- **Chat Mode Selector**: Mode selector now loads built-in + custom modes dynamically and uses compact descriptions for custom entries.
- **Memory Indexing Default**: `auto_index_on_project_load` now defaults to `true` for new users/devices.

### Fixed

- **Version Metadata Sync**: Updated `tauri.conf.json`, `package.json`, and `Cargo.toml` so auto-updates detect new releases correctly.

## [0.2.23] - 2026-02-12

### Added

- **Global Activity Indicators**: Added top-right runtime badges for concurrent background work (`CHATTING` and `ORCHESTRATING` counts) so active work remains visible while navigating between sessions/views.
- **Session List Running State UX**: Added explicit running status indicators in the Sessions sidebar for active chat sessions and orchestrator runs.
- **Orchestrator Budget Controls**: Added in-panel budget actions so users can extend run limits (`Add Budget Headroom`) or relax caps for long-running orchestrations (`Relax Max Caps`) without starting over.

### Changed

- **Session Selection Behavior**: Selecting a normal chat session now exits Orchestrator panel mode and clears stale selected run context.
- **Sidebar Status Presentation**: Refined running indicators to avoid duplicate spinners and keep status signal in a consistent location (`RUNNING` on the metadata line).
- **Chat Activity Accounting**: Chat running counts now derive from global sidecar stream events (session-scoped), not only the currently mounted chat component state.

### Fixed

- **Orchestrator Console Persistence**: Fixed orchestrator Console tab history clearing on drawer reopen by scoping logs to run sessions and loading persisted tool events across base + task child sessions.
- **Orchestrator Console Live Scope**: Fixed Console stream bleed by filtering live tool events to only the orchestrator run's related session IDs.
- **Orchestrator Retry Error Visibility**: Fixed retry/restart failures being visible only in logs by surfacing run failure reasons directly in Orchestrator UI alerts.
- **Orchestrator Failure Context**: Improved terminal failure messaging to include concrete failed-task error details (e.g. provider/model-not-found) instead of generic max-retry text.
- **Orchestrator Budget Recovery**: Fixed budget-limit dead ends by allowing failed budget runs to move back to resumable state after caps are increased.
- **Concurrent Chat Session Indicators**: Fixed sidebar/chat-header indicators dropping when switching selection by tracking running sessions globally and rendering status per session ID.
- **Budget Warning Log Spam**: Throttled repetitive orchestrator budget warning logs (e.g. `wall_time at 80%`) to log on meaningful threshold progression/cooldown instead of every loop tick.

## [0.2.22] - 2026-02-11

### Fixed

- **Orchestrator Run Isolation by Project**: Prevented Orchestrator mode from reusing a stale run across projects by clearing selected run state when switching/adding projects and scoping run selection to the active workspace.
- **Orchestrator Auto-Resume Behavior**: Opening Orchestrator with no explicit run now auto-resumes only active runs (`planning`, `awaiting_approval`, `executing`, `paused`) instead of reopening terminal/completed history by default.

## [0.2.21] - 2026-02-11

### Added

- **Model Selector Provider Filter**: Added an explicit provider selector inside the chat model dropdown (`All` + visible providers) so users can narrow large catalogs without horizontal scrolling.
- **Provider-Aware Search Token**: Added `provider:<id-or-name>` support in model search (for example `provider:openrouter sonnet`) to quickly scope results from the keyboard.

### Changed

- **Model Selector UX**: Replaced horizontal provider chips with a compact full-width provider dropdown for better scalability with many providers.
- **Model Selector Clarity**: Added helper copy ("Showing configured providers + local") to explain why some providers are hidden by default.
- **Provider Filter Behavior**: Provider filters now reset safely to `All` when a previously selected provider is no longer available after model reload.

### Fixed

- **Provider-Scoped Empty State**: Empty states in model selection now explain when no matches exist for the active provider filter.
- **Fullscreen File Preview Readability**: Increased fullscreen preview opacity and backdrop strength so file content remains readable on highly transparent/gradient themes (e.g. Pink Pony) instead of blending into the app background.

## [0.2.20] - 2026-02-11

### Added

- **Sidecar Update Compatibility Metadata**: Sidecar status now exposes `latestOverallVersion` and `compatibilityMessage` so the UI can clearly explain when newest overall and newest compatible releases differ.
- **Global Stream Hub**: Added a single long-lived sidecar stream substrate (`stream_hub`) that fans out events to chat, orchestrator, and Ralph, reducing duplicate subscriptions and race-prone stream wiring.
- **Event Envelope v2 (Additive)**: Added `sidecar_event_v2` with envelope metadata (`event_id`, `correlation_id`, `ts_ms`, `session_id`, `source`, `payload`) while keeping legacy `sidecar_event` for compatibility.
- **Stream Health Signaling**: Added explicit stream health events (`healthy`, `degraded`, `recovering`) emitted from the backend and surfaced in chat UI.
- **Chat Message Queue IPC**: Added queue APIs for busy-agent workflows: `queue_message`, `queue_list`, `queue_remove`, `queue_send_next`, `queue_send_all`.
- **Skills Import Preview + Conflict Policies**: Added `skills_import_preview` and `skills_import` with deterministic conflict strategies: `skip`, `overwrite`, `rename`.
- **Skills Pack/Zip Import Support**: Added multi-skill zip import parsing (`SKILL.md` discovery) with pre-apply preview summary.
- **Richer Skill Metadata Surface**: Expanded skill metadata handling to include `version`, `author`, `tags`, `requires`, `compatibility`, and `triggers`.

### Fixed

- **OpenCode Sidecar Release Discovery**: Sidecar update checks now query GitHub Releases with pagination (`per_page=20`, multi-page) instead of relying on a single latest path.
- **Update Target Selection**: Sidecar updater now selects the newest compatible release for the current platform/architecture by filtering assets from release metadata and skipping drafts (and prereleases unless beta channel is enabled).
- **Rate Limit Resilience**: Added conditional GitHub requests (`If-None-Match`, `If-Modified-Since`), local release-cache reuse, and check debouncing to reduce API pressure and improve reliability when offline/rate-limited.
- **Version Comparison Correctness**: Updater now uses semantic version comparison (with fallback parsing) to prevent incorrect update prompts from string-based version checks.
- **Sidecar Update Messaging**: Improved update overlay messaging to surface compatibility context instead of always presenting newest-tag text.
- **Console History Persistence**: Fixed historical tool executions not loading in the Console tab by correctly parsing persisted `type: "tool"` messages (which differ from live streaming format) and simplifying part-ID resolution.
- **Chat Jump Button**: Fixed "Jump to latest" button floating in the middle of the view by positioning it as an absolute overlay at the bottom of the message area, independent of scroll content height.
- **Streaming Subscription Duplication**: Eliminated per-request stream subscription in `send_message_streaming`; message streaming now uses shared stream bus events, reducing duplicate event emission risks.
- **Memory Retrieval Event Handling in Chat**: Wired frontend handling for `memory_retrieval` stream events so retrieval telemetry is now visible in the active chat flow.
- **Orchestrator/Ralph Stream Contention**: Migrated orchestrator and Ralph loop event consumption to stream-hub fanout instead of opening independent sidecar event feeds.
- **Chat Event Duplication Under Load**: Added deterministic frontend dedupe keyed by `event_id` for v2 stream envelopes.

### Changed

- **Streaming Architecture**: Shifted Tandem to a hub-first streaming model with additive v2 envelopes and backward-compatible legacy event emission during migration.
- **Chat UX During Generation**: Pressing Enter while generation is active now queues messages (FIFO) with inline queue controls for send-next/send-all/remove.
- **Tool Activity Presentation**: Updated inline assistant tool summary to show compact process-oriented status (step count, running/pending/failed counts, duration) with detail drill-down retained.

## [0.2.19] - 2026-02-11

### Added

- **Memory Retrieval Telemetry**: Chat requests now run memory retrieval before sending prompts, emit a `memory_retrieval` stream event, and include balanced telemetry (usage, chunk counts, latency, score range, short query hash) without logging raw query text or chunk contents.
- **Chat Memory Badge**: Assistant responses now show a memory capsule with a brain icon and retrieval status (used/not used, chunks, latency) for verifiable retrieval visibility per response.
- **Console Tab (Logs Drawer)**: Added a dedicated Console tab for tool-execution events and approvals in the Logs drawer workflow.

### Fixed

- **Memory Retrieval Coverage**: Wired retrieval context injection into both `send_message` and `send_message_streaming` so normal chat requests can actually use indexed vector memory.
- **Sidecar Duplicate Spawn Race**: Prevented duplicate OpenCode/Bun sidecar launches by serializing sidecar start/stop lifecycle transitions with a lifecycle lock.
- **Logs Drawer Fullscreen Height**: Fixed logs panel sizing so height is fully dynamic in fullscreen instead of staying at the smaller constrained height.
- **Logs Redundancy**: Removed the redundant OpenCode sidecar log tab from the logs viewer and consolidated command activity under the Console tab.
- **Pink Pony Readability**: Tuned Pink Pony theme contrast, surface opacity, borders, and text colors to improve legibility on bright backgrounds.
- **Chat Performance**: Significantly improved rendering performance for long chat sessions by implementing list virtualization and component memoization.
- **Production Build**: Fixed a TypeScript error in the Logs Drawer component (`ResizeObserver` type mismatch) that was blocking production builds.

### Changed

- **Memory Log Signal**: Memory retrieval logging now uses a distinct `tandem.memory` target and a brain marker for easier scanning in logs.
- **Production Frontend Build**: Production Vite builds now drop `console.*` and `debugger` statements.

## [0.2.18] - 2026-02-10

### Added

- **Python**: Auto-open the Python Setup (Workspace Venv) wizard when Python is blocked by venv-only policy enforcement (helps LLM-triggered Python attempts recover quickly).
- **Python**: Extend venv-only enforcement to staged/batch execution (preflight staged operations before approving any tool calls).
- **Python**: Add a shared policy helper + tests for consistent enforcement across approval paths.
- **Packs (Python)**: Add `requirements.txt` to the Data Visualization and Finance Analysis packs; update their docs to install via the workspace venv.
- **Packs**: Install pack-level `CONTRIBUTING.md` when present (copied alongside `START_HERE.md`).
- **Files**: Add a dock mount + fullscreen toggle for file previews.

### Fixed

- **Skills/Templates**: Fix bundled starter skill templates with missing YAML frontmatter fields so they no longer get skipped on startup.
- **Python**: Improve the requirements install UX by defaulting to the workspace and auto-detecting `requirements*.txt` when present.

### Known Issues

- **Files Auto-Refresh (WIP)**: The Files tree does not reliably refresh when tools/AI create new files in the workspace. Deeper investigation needed; workaround is to navigate away and back to Files.

## [0.2.17] - 2026-02-10

### Fixed

- **Custom Background Opacity Slider (Packaged Builds)**: Fix opacity changes causing the background image to flash or disappear in bundled builds by keeping the resolved image URL stable and updating only opacity.
- **Background Layering**: Render the custom background image as a dedicated fixed layer so it consistently appears across views without impacting overlay layout.

## [0.2.16] - 2026-02-10

### Fixed

- **Update Overlay Layout**: Fix the in-app update prompt becoming constrained/squished due to theme background layering CSS.

## [0.2.15] - 2026-02-10

### Fixed

- **Custom Background Image Loading (Packaged Builds)**: Fix custom background images failing to load after updating by falling back to an in-memory `data:` URL when the `asset:` URL fails.

## [0.2.14] - 2026-02-10

### Added

- **Themes: Background Art Pass**: Add richer background art for Cosmic Glass (starfield + galaxy glow), Pink Pony (thick arcing rainbow), and Zen Dusk (minimalist ink + sage haze).
- **Theme Background Support**: Add an `app-background` utility class so gradient theme backgrounds render correctly throughout the app (not just as a solid `background-color`).
- **Custom Background Image Overlay**: Allow users to choose a background image (copied into app data) and overlay it on top of the active theme, with an opacity slider in Settings.
- **File Text Extraction (Rust)**: Add best-effort, cross-platform text extraction for common document formats (PDF, DOCX, PPTX, XLSX/XLS/ODS/XLSB, RTF) via the `read_file_text` command so attachments can be used by skills without requiring Python.
- **Python Workspace Venv Wizard**: Add a cross-platform in-app Python setup wizard to create a workspace-scoped venv at `.opencode/.venv` and install dependencies into it (never global).
- **Docs: Theme Contribution Guide**: Add guidance for creating and iterating on theme backgrounds.

### Fixed

- **Settings/About/Extensions Navigation**: Restore Settings/About/Extensions panels after a regression where these views would not appear.
- **Overlay Layering**: Ensure theme/background layers render consistently across main views (chat + settings) without unintended translucency.
- **Startup Session Restore**: Fix restored sessions appearing selected but not opening until reselecting the folder (defer history load until the sidecar is running; allow re-clicking the selected session to reload).

### Changed

- **Packs UI**: Style runtime requirement pills consistently.

## [0.2.13] - 2026-02-10

### Added

- **Skill Templates: New Starter Skills**: Add two new bundled starter skills: `brainstorming` and `development-estimation`.
- **Skill Templates: Runtime Pills**: Starter skill cards now show optional runtime hints (e.g. Python/Node/Bash) via `requires: [...]` YAML frontmatter.
- **Skills UI: Installed Skill Discoverability**: Add clearer install/manage UX (runtime note, counts for folder vs global installs, and a jump-to-installed action).

### Fixed

- **Dev Skill Template Discovery**: In `tauri dev`, load starter skill templates from `src-tauri/resources/skill-templates/` so newly added templates appear immediately (avoids stale `target/**/resources/**` copies).
- **Logs Viewer UX**: Improve log viewer usability (fullscreen mode, and copy feedback).
- **Skill Template Parsing**: Fix invalid bundled skill template frontmatter (missing `name`) so it is not skipped.

### Changed

- **Packs UI**: Show packs only (remove starter skills section) and move the runtime note to the top of the Packs page.
- **Docs**: Expand contributor documentation with a developer guide for adding skills.

## [0.2.12] - 2026-02-09

### Fixed

- **Orchestrator Model Routing**: Persist the selected provider/model on orchestrator runs and prefer it when sending prompts so runs don't start with an "unknown" model or send messages without an explicit model spec.
- **Orchestrator Restart/Retries**: Prevent "restart" from instantly reporting success without doing any work (guard against empty plans; allow restarting completed runs to rerun the full plan).
- **Logs Viewer Copy/Scroll**: Make long log lines easy to inspect and share (horizontal scroll + selected-line preview + copy helpers).
- **Orchestrator Run Deletion**: Allow deleting orchestrator runs from the Sessions sidebar (removes the run from disk and deletes its backing OpenCode session).
- **Release to Discord**: Automated releases now post to Discord via the release workflow (release:published events triggered by `GITHUB_TOKEN` are not delivered to other workflows).
- **Release to Discord**: Ensure Discord notifications fire for automated releases by posting from the release workflow (instead of relying on `release: published`, which doesn't trigger when publishing via `GITHUB_TOKEN`).

## [0.2.11] - 2026-02-09

### Added

- **On-Demand Logs Viewer**: Add a right-side Logs drawer that can tail Tandem app log files (from the app data `logs/` directory) and show OpenCode sidecar stdout/stderr (captured into a bounded in-memory ring buffer). Streaming only runs while the drawer is open/active to avoid baseline performance cost.
- **Poe Provider**: Add Poe as an OpenAI-compatible provider option (endpoint + `POE_API_KEY`). Thanks [@CamNoob](https://github.com/CamNoob).

### Fixed

- **OpenCode Session Hangs**: Prevent sessions from getting stuck indefinitely when a tool invocation never reaches a terminal state by recognizing more terminal tool statuses, ignoring heartbeat/diff noise in the stream, and fail-fast cancelling with a surfaced error after a timeout.
- **Sidecar StdIO Deadlock Risk**: Always drain the OpenCode sidecar stdout/stderr pipes so the sidecar cannot block if it emits high-volume output.
- **Log Noise Reduction**: Ignore OpenCode `server.*` heartbeat SSE events (and downgrade other unknown SSE events) to prevent log spam during long-running sessions.
- **Vault Locked Log Spam**: Avoid warning-level logs when the keystore isn't available because the vault is locked (expected state).
- **Release Pipeline Resilience**: Retry GitHub Release asset uploads to reduce flakes during transient GitHub errors.

## [0.2.10] - 2026-02-09 (Failed Release)

- Release attempt failed due to GitHub release asset upload errors during a GitHub incident; no assets were published. v0.2.11 re-cuts the same changes.

## [0.2.9] - 2026-02-09

### Added

- **Project File Indexing**: Add an incremental, per-project file index for workspace embeddings with total/percent progress reporting.
- **Memory Stats Scope**: Switch Vector Database Stats between All Projects and Active Project views.
- **Auto-Index Toggle**: Optionally auto-index the active project on load (with a short cooldown).
- **Clear File Index**: Clear only file-derived vectors/chunks for a project (optional VACUUM) to reclaim space.

### Fixed

- **Question Prompts**: Properly handle OpenCode `question.asked` events (including multi-question requests) and render an interactive one-at-a-time wizard with multiple-choice + custom answers; replies are sent via the OpenCode `/question/{requestID}/reply` API.
- **Startup Session History**: When restoring the last active project on launch, automatically load its sessions by scoping OpenCode `/session` and `/project` listing calls to the active workspace directory.
- **Windows Dev Reload Sidecar Cleanup**: Prevent orphaned OpenCode sidecar (and Bun) processes when the app is restarted during `tauri dev` rebuilds by attaching the sidecar to a Windows Job Object (kill-on-close).

## [0.2.8] - 2026-02-09

### Added

- **Multi Custom Providers (OpenCode)**: Support selecting any provider from the OpenCode sidecar catalog (including user-defined providers by name in `.opencode/config.json`), not just the built-in set.

### Fixed

- **Model Selection Routing**: Persist the selected `provider_id` + `model_id` and prefer it when sending messages, so switching to non-standard providers actually takes effect.

## [0.2.7] - 2026-02-08

### Fixed

- **OpenCode Config Safety**: Prevent OpenCode config writes from deleting an existing `opencode.json` when replacement fails (e.g. file locked on Windows).
- **Sidecar Idle Memory**: Set Bun/JSC memory env hints to reduce excessive idle memory usage.

## [0.2.6] - 2026-02-08

### Fixed

- **macOS Release Builds**: Disabled codesigning/notarization by default in the release workflow to prevent macOS builds from failing when Apple certificate secrets are missing or misconfigured. (Enable with `MACOS_SIGNING_ENABLED=true` repo variable.)

## [0.2.5] - 2026-02-08

### Fixed

- **Release Build Trigger**: Bumped version/tag to ensure GitHub Releases builds run with the corrected workflow configuration.

## [0.2.4] - 2026-02-08

### Added

- **Vector DB Stats (Settings)**: Added a Memory section in Settings to view vector database stats and manually index the current workspace.
- **macOS Release Verification**: Release/CI now includes Gatekeeper checks (`codesign`, `spctl`, `stapler validate`) for produced DMGs (informational unless Apple signing secrets are configured).

### Fixed

- **Starter Pack Installs (Windows/macOS/Linux)**: Fixed pack/template resolution in packaged builds so Starter Packs and Starter Skills can be installed correctly from bundled resources.
- **Onboarding For Custom Providers**: Custom providers (e.g. LM Studio / OpenAI-compatible endpoints) are now treated as “configured”, preventing onboarding from forcing users back to Settings.
- **Pack Install Errors**: Pack install failures now surface the underlying error message in the UI.

## [0.2.3] - 2026-02-08

### Fixed

- **Orchestration Session Spam**: Orchestration no longer creates endless new root chat sessions during execution.
  - Sub-agent/task sessions are now created as child sessions (so they don't flood the main session list).
  - Session listing now prefers root sessions only, with a fallback for older sidecars.

## [0.2.2] - 2026-02-08

### Added

- **Knowledge Work Skills Migration**: Completed the migration of all legacy knowledge work skills to the Tandem format.
  - **Productivity Pack**: `productivity-memory`, `productivity-tasks`, `productivity-start`, `productivity-update`, `inbox-triage`, `meeting-notes`, `research-synthesis`, `writing-polish`.
  - **Sales Pack**: `sales-account-research`, `sales-call-prep`, `sales-competitive-intelligence`, `sales-create-asset`, `sales-daily-briefing`, `sales-draft-outreach`.
  - **Bio-Informatics Pack**: `bio-instrument-data`, `bio-nextflow-manager`, `bio-research-strategy`, `bio-single-cell`, `bio-strategy`.
  - **Data Science Pack**: `data-analyze`, `data-build-dashboard`, `data-create-viz`, `data-explore-data`, `data-validate`, `data-write-query`.
  - **Enterprise Knowledge Pack**: `enterprise-knowledge-synthesis`, `enterprise-search-knowledge`, `enterprise-search-source`, `enterprise-search-strategy`, `enterprise-source-management`.
  - **Finance Pack**: `finance-income-statement`, `finance-journal-entry`, `finance-reconciliation`, `finance-sox-testing`, `finance-variance-analysis`.
  - **Legal Pack**: `legal-canned-responses`, `legal-compliance`, `legal-contract-review`, `legal-meeting-briefing`, `legal-nda-triage`, `legal-risk-assessment`.
  - **Marketing Pack**: `marketing-brand-voice`, `marketing-campaign-planning`, `marketing-competitive-analysis`, `marketing-content-creation`, `marketing-performance-analytics`.
  - **Product Pack**: `product-competitive-analysis`, `product-feature-spec`, `product-metrics`, `product-roadmap`, `product-stakeholder-comms`, `product-user-research`.
  - **Support Pack**: `support-customer-research`, `support-escalation`, `support-knowledge-management`, `support-response-drafting`, `support-ticket-triage`.
  - **Design & Frontend Pack**: `canvas-design`, `theme-factory`, `frontend-design`, `web-artifacts-builder`, `algorithmic-art`.
  - **Internal Comms**: `internal-comms`.
  - **Utilities**: `cowork-mcp-config-assistant`.
- **Skill Templates**: All migrated skills are now available as offline-compatible templates in the `src-tauri/resources/skill-templates` directory.
- **Brand Neutralization**: All skills have been updated to be model-agnostic, removing dependencies on specific AI providers.
- **Extensions**: New top-level Extensions area with tabs for Skills, Plugins, and Integrations (MCP).
- **MCP Integrations UI**: Add/remove remote HTTP and local stdio MCP servers with scope support (Global vs Folder).
- **MCP Presets**: Added popular remote presets (including Context7 and DeepWiki) for quick setup.
- **Skills Search**: Added a search box to filter both Starter skills and Installed skills.
- **New Skill Template**: `youtube-scriptwriter` starter skill template.

### Improved

- **MCP Test Connection**: Test now performs a protocol-correct MCP `initialize` POST and validates JSON-RPC (including SSE responses) instead of using HEAD/GET.
- **MCP Status UX**: More accurate status mapping and actionable error messages (auth required, wrong URL, incompatible transport, deprecated endpoint).

### Fixed

- MCP connection tests no longer report "Connected" for non-2xx HTTP responses like 405/410.

## [0.2.1] - 2026-02-07

### Added

- **Guided onboarding wizard** to drive a first outcome (choose folder → connect AI → run starter workflow).
- **Starter Packs**: bundled, offline workflow packs you can install into a folder from inside the app.
- **Starter Skills gallery**: bundled, offline skill templates with an “Advanced: paste SKILL.md” option retained.
- **Contributor hygiene**: GitHub issue/PR templates and new product/architecture docs at repo root.

### Improved

- **Orchestration reliability**:
  - Increased default budgets (iterations/sub-agent runs) and auto-upgraded legacy runs with too-low limits.
  - Provider rate-limit/quota errors now **pause** runs (instead of burning retries) so you can switch model/provider and resume.
- **Provider switching**: fixed stale env var propagation by explicitly syncing/removing provider API key env vars and restarting sidecar when provider toggles change.
- **CI confidence**: frontend lint now fails the build instead of being ignored.

### Fixed

- Orchestrator could “explode” sub-agent runs due to tasks not being marked finished on error (leading to endless requeue/recovery loops).
- Model/provider could not be changed after a run failed; model selection is now available to recover and resume.

## [0.2.0] - 2026-02-06

### Added

- **Multi-Agent Orchestration**: Introduced a major new mode for complex task execution.
  - **Task DAG**: Supports dependency-aware task graphs (Planner -> Builder -> Validator).
  - **Sub-Agents**: Orchestrates specialized agents for planning, coding, and verifying.
  - **Cost & Safety**: Implements strict budget controls (tokens, time, iterations) and policy-based tool gating.
  - **Visualize**: New Kanban board and budget meter to track progress in real-time.
- **Unified Session Sidebar**: Completely redesigned the sidebar to merge chat sessions and orchestrator runs into a single, cohesive chronological list.
  - **Project Grouping**: Items are smartly grouped by project with sticky headers.
  - **Status Indicators**: Orchestrator runs show live status (Running, Completed, Failed).

## [0.1.15] - 2026-02-03

### Added

- **Unified Update UI**: Replaced the disparate update experiences for OpenCode (Sidecar) and Tandem (App) with a single, polished, full-screen overlay component.
- **Conflict Resolution**: The new `AppUpdateOverlay` takes precedence over other update screens, ensuring that app updates (which restart the application) are handled cleanly and avoid conflicts with sidecar updates.

## [0.1.14] - 2026-01-31

### Improved

- **Ralph Loop Reliability**: Updated the prompt engineering for both Ralph Loop and Plan Execution modes to explicitly enforce the use of the `todowrite` tool. This ensures that tasks are visually marked as "completed" in the UI as the AI finishes them, preventing the state desync where work was done but tasks remained unchecked.
- **Task Execution Flow**: When executing approved tasks from the Plan sidebar, the system now provides stronger directives to the AI to update task status immediately upon completion.

## [0.1.13] - 2026-01-30

### Added

- **Ralph Loop**: Implemented iterative task execution mode with the following features:
  - New `ralph` Rust module with `RalphLoopManager`, `RalphStorage`, and `RalphRunHandle`
  - Toggle button in chat control bar to enable/disable loop mode
  - Status chip showing current iteration and status (Running/Paused/Completed/Error)
  - Side panel with pause/resume/cancel controls and context injection
  - Completion detection via `<promise>COMPLETE</promise>` token matching
  - Struggle detection after 3 iterations with no file changes or repeated errors
  - Git-based file change tracking between iterations
  - Workspace-local storage at `.opencode/tandem/ralph/` (state.json, history.json, context.md)
  - Seven Tauri commands: `ralph_start`, `ralph_cancel`, `ralph_pause`, `ralph_resume`, `ralph_add_context`, `ralph_status`, `ralph_history`
  - Plan Mode integration - Ralph respects staging and never auto-executes
  - Frontend components: `LoopToggle`, `LoopStatusChip`, `RalphPanel`
- **Memory Context System**: Integrated a semantic memory store using `sqlite-vec`. This allows the AI to store and retrieve context from past sessions and project documentation, enabling long-term memory and smarter context-aware responses.

### Fixed

- **Memory Store Initialization**: Resolved an `unresolved import sqlite_vec::sqlite_vec` error by correctly implementing the `sqlite3_vec_init` C-extension registration via `rusqlite`.

## [0.1.13] - 2026-01-30

### Added

- **Planning Mode**: Introduced a dedicated planning agent that generates comprehensive markdown-based implementation plans before executing code changes. Includes support for real-time plan file synchronization and a specialized UI for plan management.
- **Plan File Watcher**: Backend file watcher for `.opencode/plans/` that automatically updates the UI when plans are modified, ensuring the frontend is always in sync with the AI's latest proposals.
- **Ask Follow-up Question**: Integrated support for the `ask_followup_question` tool in the planning process, allowing the AI to clarify scope and technical preferences with interactive suggestion buttons.

### Fixed

- **Backend Compilation**: Resolved a critical "no method named `get_workspace_path` found" error in `commands.rs` by adding the missing method to `AppState`.
- **Tool Parsing Accuracy**: Improved sidecar communication by strictly enforcing tool name formatting (removing potential leading spaces) and correcting invalid tool examples in the plan skill instructions.

### Changed

- **Planning Flow**: Streamlined the transition from plan to execution. The AI is now instructed to generate plans immediately without conversational filler, using strict system directives.

## [0.1.12] - 2026-01-22

### Fixed

- **File Viewer**: Fixed "Failed to load directory" error by removing overly restrictive path allowlist checks that were causing Windows path normalization issues.
- **Permission Spam**: Prevented repeated approval prompts for the same tool request.
- **Allow All Auto-Approval**: Aligned auto-approval with permission request IDs to stop duplicate prompts.
- **Session Switching**: Cleared pending permission state when switching sessions to avoid stale approvals.

## [0.1.11] - 2026-01-22

### Fixed

- **Version Metadata**: Fixed version numbers in `tauri.conf.json`, `package.json`, and `Cargo.toml` to ensure proper auto-update detection. Previous release (v0.1.10) had mismatched version metadata (some files were 0.1.8 or 0.1.9 while the built version was 0.1.10), causing update failures.
- **File Access Guardrails**: Enforced workspace allowlist checks for file browsing, text reads, and binary reads to prevent unintended access outside the active workspace.
- **Windows Path Denylist**: Normalized Windows path separators so deny patterns like `.env` and key files reliably block access.
- **Binary Read Limits**: Added size limits for binary reads to avoid large base64 payloads.
- **Log Noise**: Removed verbose streaming and provider debug logs to reduce UI overhead during active sessions.

## [0.1.10] - 2026-01-22

### Added

- **Skills Management UI**: Added a complete skills management interface in Settings, allowing users to import, view, and manage OpenCode-compatible skills (both project-specific and global).
- **Skill Discovery**: Implemented automatic discovery of installed skills from both project (`.opencode/skill/`) and global (`~/.config/opencode/skills/`) directories.
- **Smart Project Selection**: Skills panel now displays the active project name and automatically disables project-specific installation when no project is selected.
- **Skill Resource Links**: Added clickable links to popular skill repositories (open skills library, SkillHub, GitHub) using Tauri's native URL opener.
- **Automatic Sidecar Restart**: Implemented seamless AI engine restart after skill import with a full-screen overlay matching the app's aesthetic. Features animated rotating icon, pulsing progress bars, and backdrop blur.

### Fixed

- **Skills Import Reliability**: Fixed critical bug where SKILL.md files with YAML frontmatter containing colons (e.g., "for: (1)") would fail to parse. The parser now automatically quotes descriptions with special characters.
- **Skills Save Format**: Fixed issue where imported skills were being reconstructed incorrectly, causing frontmatter corruption. Skills are now saved with their original content preserved.
- **TypeScript Errors**: Resolved missing `projectPath` prop type in SkillsPanel component.
- **External Links**: Fixed broken external links in Skills panel to use Tauri's `openUrl()` instead of non-functional `href` attributes.

### Changed

- **Button Styling**: Cleaned up Save button appearance by removing emoji for a more professional look.
- **Project Name Display**: Improved visual hierarchy in project selection with bold primary-colored project names and muted path indicators.
- **Error Handling**: Added comprehensive debug logging for skill discovery and YAML parsing to improve troubleshooting.
- **Auto-Refresh**: Skills list now properly refreshes after importing new skills by awaiting the refresh callback.

## [0.1.9] - 2026-01-21

### Fixed

- **macOS Styling:** Refined the glass effect styling and other UI polish to improve the overall look and feel on macOS.
- **BaseHref Support:** Added support for `baseHref` in HTML previews to correctly resolve relative paths for images and stylesheets.

## [0.1.7] - 2026-01-21

### Fixed

- **Slides Workflow Feedback Loop:** Refined the presentation guidance to be more flexible, ensuring the AI acknowledges user feedback/improvements during the planning phase instead of jumping immediately to execution.
- **"Add to Chat" Reliability:** Fixed a state management bug in `ChatInput` that prevented HTML files and other external attachments from being correctly added to the chat context.
- **Blur Obstruction:** Removed the `blur(6px)` transition from the `Message` component and streaming indicator, preventing the chat from becoming unreadable during active AI generation.
- **High-Fidelity PDF Export:** Added `@page { margin: 0; size: landscape; }` and `color-adjust` CSS to the HTML slide template to suppress browser headers/footers and preserve professional aesthetics during PDF export.
- **File Link Detection (Chat UI):** Refined the file path detection regex to only match explicit paths (containing slashes or drive letters), preventing normal text from being incorrectly rendered as "jarbled" clickable links.
- **Dynamic Ollama Discovery:** Implemented automatic model discovery for local Ollama instances. The application now dynamically generates the sidecar configuration based on actually installed local models, ensuring a seamless zero-config experience across all platforms.
- **Cross-Platform Config Reliability:** Updated the sidecar manager to correctly handle OpenCode configuration paths on Linux, macOS, and Windows, and bundled a default template in the installer for improved auto-update reliability.
- **Settings Synchronization:** Fixed a bug where changing the model/provider in settings was not immediately reflected in the Chat interface.
- **Model Selector Refinement:** Cleaned up the model dropdown to prioritize OpenCode Zen/Ollama and hide unconfigured providers, reducing clutter.
- **"Allow All" Logic:** Fixed a critical issue where the "Allow All" toggle was ignored by the event handler, implementing robust auto-approval logic for permissions.
- **Chat History Visibility:** Improved session list filtering to strictly handle project path normalization, ensuring only relevant project chats are shown while preventing history loss.

## [0.1.6] - 2026-01-20

### Added

- **High-Fidelity HTML Slides:** Replaced legacy PPTX generation with an interactive 16:9 HTML slideshow system featuring Chart.js integration, keyboard navigation, optimized PDF export via a dedicated Print button, content overflow protection, and strict density limits (max 6 items per slide).
- **Collapsible Tool Outputs:** Large tool outputs (like `todowrite` or file operations) are now collapsed by default in the chat view, reducing visual noise. Users can expand them to see full details.
- **Chart Generation Capabilities:** Updated internal marketing documentation to highlight the new capability of generating interactive visual dashboards directly from research data.
- HTML Canvas/Report feature: render interactive HTML files in a sandboxed iframe with Tailwind, Chart.js, and Font Awesome support.
- "Research" tool category with dedicated instructions for a robust "Search → Select → Fetch" workflow.
- Visibility of AI reasoning/thinking parts in both live streaming and chat history.
- Automatic persistence of the current active session across reloads/refreshes.
- Default "allow" rules for safe tools (`ls`, `read`, `todowrite`, `websearch`, `webfetch`) to reduce permission prompts.

### Fixed

- **[REDACTED] Filtering:** Removed spurious `[REDACTED]` markers that were leaking from OpenCode's internal reasoning output into the chat UI.
- **File Link Detection (Critical Fix):** Completely rewrote the file path regex to reliably detect Unix absolute paths like `/home/user/file.html` in chat messages, making them clickable.
- **Slide Layout & Scaling:** Fixed vertical stacking of slides in the HTML generator and added auto-scaling to fit the viewer's viewport dimensions.
- **Chat Error Handling:** Implemented deduplication for session error messages to prevent repeated bubbles during stream failures.
- **Linux UI Transparency:** Fixed an issue where the project switcher dropdown was unreadable on Linux due to incorrect glass effect rendering.
- **Session Loading:** Resolved a bug where the application would start with a blank screen instead of loading the previously selected chat session.
- **External Link Handling:** Fixed permission issues preventing "Open in Browser" from working for generated files.
- **HTML Preview:** Links within generated HTML reports now correctly open in the system default browser.
- **Tool Selector Cleanup:** Temporarily disabled the unimplemented "Diagrams" and "Tables" categories from the specialized tools selector to improve UX.
- Robust cancellation: Stop button now reliably terminates backend AI processes using a fallback API mechanism.
- Tool visibility: All tool calls (including technical ones) are now visible throughout the session per user request.
- Fixed chat "freezing" by ensuring intermediate reasoning and tool steps are always streamed to the UI.
- Replaced hardcoded version numbers with dynamic values in `MatrixLoader`, `Settings`, and the initial splash screen.
- Improved error handling in the sidecar manager when primary cancellation endpoints are unavailable.
- Resolved ESLint warnings in `Message.tsx` and `Chat.tsx`.

### Changed

- Updated `create_session` and `rewind_to_message` to include default safe-tool permissions.
- Modified `sidecar.rs` to treat "reasoning" parts as visible content.

## [0.1.5] - 2026-01-20

### Added

- Compact theme selector dropdown with theme details and color swatches.
- Active provider/model badge next to the tool selector in chat.
- Allow-all toggle for tool permissions on new chats.

### Fixed

- Linux `.deb` auto-update now downloads the `.deb` artifact (instead of the AppImage), preventing `update is not a valid deb package`.
- Taskbar no longer overlays the About and Settings screens.
- OpenCode sidecar API compatibility after upstream route changes (provider/model listing and prompt submission).
- Streaming event parsing for newer OpenCode SSE payload shapes.
- Structured provider errors now surface in the chat UI instead of failing silently; improved extraction of specific reasons (e.g., credit limits) from nested responses.
- Permission prompts now render correctly for updated tool event payloads.
- Provider key status refreshes immediately after saving or deleting API keys.
- Technical tool calls (edit, write, ls, etc.) are now handled as transient background tasks and auto-cleanup from chat on success.
- Final AI responses now render reliably at the end of a session, with an automatic backfill mechanism if the stream is interrupted.
- Reduced terminal log spam by downgrading verbose background activity and summarizing large event payloads.
- Fixed a TypeScript error where the `tool` property was missing from the `tool_end` event payload.

### Changed

- Update checking and install progress is now shown at the top of Settings.

## [0.1.4] - 2026-01-20

### Added

- Auto-update functionality with GitHub releases
- Sidecar binary management and updates
- Vault encryption for API keys
- About page with update checker

### Changed

- Improved sidecar process management on Windows
- Enhanced error handling for file operations

### Fixed

- File locking issues during sidecar updates on Windows
- ESLint warnings in React components

## [0.1.0] - 2026-01-18

### Added

- Initial release
- Chat interface with OpenCode AI engine
- Session management and history
- Multi-provider support (Anthropic, OpenAI, OpenRouter)
- Zero-trust security with local encryption
- Project-based organization
- Real-time streaming responses

[Unreleased]: https://github.com/frumu-ai/tandem/compare/v0.2.25...HEAD
[0.2.25]: https://github.com/frumu-ai/tandem/compare/v0.2.24...v0.2.25
[0.2.24]: https://github.com/frumu-ai/tandem/compare/v0.2.23...v0.2.24
[0.2.23]: https://github.com/frumu-ai/tandem/compare/v0.2.22...v0.2.23
[0.2.22]: https://github.com/frumu-ai/tandem/compare/v0.2.21...v0.2.22
[0.2.21]: https://github.com/frumu-ai/tandem/compare/v0.2.20...v0.2.21
[0.2.19]: https://github.com/frumu-ai/tandem/compare/v0.2.18...v0.2.19
[0.2.18]: https://github.com/frumu-ai/tandem/compare/v0.2.17...v0.2.18
[0.2.17]: https://github.com/frumu-ai/tandem/compare/v0.2.16...v0.2.17
[0.2.16]: https://github.com/frumu-ai/tandem/compare/v0.2.15...v0.2.16
[0.2.15]: https://github.com/frumu-ai/tandem/compare/v0.2.14...v0.2.15
[0.2.14]: https://github.com/frumu-ai/tandem/compare/v0.2.13...v0.2.14
[0.2.13]: https://github.com/frumu-ai/tandem/compare/v0.2.12...v0.2.13
[0.2.12]: https://github.com/frumu-ai/tandem/compare/v0.2.11...v0.2.12
[0.2.11]: https://github.com/frumu-ai/tandem/compare/v0.2.10...v0.2.11
[0.2.10]: https://github.com/frumu-ai/tandem/compare/v0.2.9...v0.2.10
[0.2.9]: https://github.com/frumu-ai/tandem/compare/v0.2.8...v0.2.9
[0.2.8]: https://github.com/frumu-ai/tandem/compare/v0.2.7...v0.2.8
[0.1.13]: https://github.com/frumu-ai/tandem/compare/v0.1.12...v0.1.13
[0.1.12]: https://github.com/frumu-ai/tandem/compare/v0.1.11...v0.1.12
[0.1.11]: https://github.com/frumu-ai/tandem/compare/v0.1.10...v0.1.11
[0.1.10]: https://github.com/frumu-ai/tandem/compare/v0.1.9...v0.1.10
[0.1.9]: https://github.com/frumu-ai/tandem/compare/v0.1.7...v0.1.9
[0.1.8]: https://github.com/frumu-ai/tandem/compare/v0.1.7...v0.1.8
[0.1.7]: https://github.com/frumu-ai/tandem/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/frumu-ai/tandem/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/frumu-ai/tandem/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/frumu-ai/tandem/compare/v0.1.0...v0.1.4
[0.1.0]: https://github.com/frumu-ai/tandem/releases/tag/v0.1.0
