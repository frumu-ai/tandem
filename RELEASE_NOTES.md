# Release Notes

Canonical release notes live in `docs/RELEASE_NOTES.md`.

## v0.2.20 (2026-02-11)

- Sidecar updates: Switched OpenCode release discovery to paginated GitHub Releases metadata (`per_page=20` + additional pages), avoiding fragile single-endpoint latest behavior.
- Sidecar updates: Selects the newest compatible release for the current platform/arch by filtering release assets, skipping drafts, and excluding prereleases unless beta channel is enabled.
- Sidecar updates: Added API-efficiency protections (ETag/Last-Modified conditional requests, local cache reuse, and debounce window) to reduce rate-limit pressure and improve resilience.
- Sidecar updates: Improved version comparison with semantic version parsing to avoid incorrect prompts caused by string comparison.
- UI/Status: Added compatibility-aware sidecar status fields (`latestOverallVersion`, `compatibilityMessage`) and improved overlay messaging when latest overall and latest compatible differ.
- **Console & Chat UI Fixes**: Resolved an issue where the Console tab would lose history when switching views or restarting the drawer. Also fixed the "Jump to latest" button positioning to ensure it stays pinned to the bottom of the chat.

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
