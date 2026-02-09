# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.9] - Unreleased

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

[Unreleased]: https://github.com/frumu-ai/tandem/compare/v0.1.13...HEAD
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
