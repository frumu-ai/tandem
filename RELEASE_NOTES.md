# Release Notes

Canonical release notes live in `docs/RELEASE_NOTES.md`.

## v0.2.13 (Unreleased)

- Skills: Add two new bundled starter skills: `brainstorming` and `development-estimation`.
- Skills: Show runtime requirement pills on starter skill cards via optional `requires: [...]` YAML frontmatter.
- Skills: Improve Skills install/manage UX (runtime note, clearer installed-skill counts, and jump-to-installed).
- Packs: Packs page now shows packs only (remove starter skills section) and moves the runtime note to the top.
- Diagnostics: Improve Logs viewer UX (fullscreen + copy feedback); fix an invalid bundled skill template frontmatter that was being skipped.
- Files: Folder entries no longer show `NaN`/`undefined` sizes in the file browser (directory size display removed).
- Files: Add Rust-based text extraction for common document formats (PDF, DOCX, PPTX, XLSX/XLS/ODS/XLSB, RTF) via `read_file_text`, so these attachments can be previewed and included as usable text in skills/chats without requiring Python.
- Python: Add a workspace-scoped venv wizard (creates `.opencode/.venv` and installs requirements into it) and enforce venv-only python/pip usage for AI tool calls to prevent global installs.
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
