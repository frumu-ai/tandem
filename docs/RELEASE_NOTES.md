# Tandem v0.2.13 Release Notes

## Highlights

- **New Starter Skills**: Add two new bundled starter skills: `brainstorming` and `development-estimation`.
- **Runtime Requirement Pills**: Starter skill cards can show optional runtime hints (Python/Node/Bash) via `requires: [...]` YAML frontmatter.
- **Skills UX Improvements**: Clearer install/manage experience (runtime note, installed-skill counts, and better discoverability of deletion).
- **Packs Page Cleanup**: Packs page now shows packs only (no starter skills section) and surfaces the runtime note at the top.
- **Diagnostics Polishing**: Logs viewer improvements (fullscreen + copy feedback) and fix invalid bundled skill template frontmatter so templates aren’t skipped.
- **Document Text Extraction (Rust)**: PDF/DOCX/PPTX/XLSX (and more) can now be extracted to plain text for preview and for attaching to skills/chats, without requiring Python.
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
