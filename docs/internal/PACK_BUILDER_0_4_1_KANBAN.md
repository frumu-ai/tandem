# Pack Builder 0.4.1 Kanban

Last updated: 2026-03-03
Owner: Tandem Engine

## In Progress

- [x] Add MCP-first `pack_builder` engine tool (preview/apply)
- [x] Register `pack_builder` tool in tandem-server runtime startup
- [x] Generate packs with explicit MCP tool invocations in `missions/` and `agents/`
- [x] Connector discovery preview with candidate MCP servers + selection
- [x] Apply flow: MCP register/connect + pack install + paused routine registration
- [x] Add `pack_builder` built-in agent profile
- [x] Add channel heuristic routing to `pack_builder`
- [x] Add `pack_presets` registry support and persist connector requirements
- [x] Add SDK/client/control-panel compatibility updates for new preset shape
- [x] Add tests for MCP-required external goals and connector invocation
- [x] Update `CHANGELOG.md` for v0.4.1
- [x] Update `docs/RELEASE_NOTES.md` for v0.4.1
- [x] Harden provider tool-schema normalization for MCP tuple `items` / nested object `properties` compatibility
- [x] Fix pack-builder preview UX to return user-readable summary instead of raw JSON dump
- [x] Fix connector-selection gating for built-in satisfied external needs
- [x] Add safe preview auto-apply path (install + paused routine) when no manual setup is required
- [x] Add regression tests for built-in-only connector gating + safe auto-apply
- [x] Add chat confirmation bridge (`confirm` -> apply last preview plan_id) to support control panel, Tauri, and channel threads
- [x] Add pack-builder session-local confirmation fallback to prevent accidental `pack-builder-ok` installs when model emits preview+short-goal
- [x] Add server-owned workflow persistence for pack-builder plans/workflows (`pack_builder_plans.json`, `pack_builder_workflows.json`)
- [x] Add API-first parity endpoints: `POST /pack-builder/preview|apply|cancel` and `GET /pack-builder/pending`
- [x] Route `/tool/execute pack_builder` and API endpoints through same workflow behavior (preview/apply/cancel/pending)
- [x] Add blocked apply statuses for missing secrets/auth with deterministic next actions
- [x] Add thread-scoped pending-plan resolution to prevent cross-thread confirm/apply collisions
- [x] Add channel API-first pack-builder flow:
  - [x] intent preview via `/pack-builder/preview`
  - [x] `confirm` -> `/pack-builder/apply`
  - [x] `cancel` -> `/pack-builder/cancel`
  - [x] `use connectors: ...` -> `/pack-builder/apply` with connector override
- [x] Add control-panel chat API-first pack-builder flow for preview/apply/cancel commands
- [x] Add parity regression tests for API endpoints + thread-scoped apply selection
- [x] Restore LLM-led initial chat flow for pack creation (remove hard terminal tool-cycle for `pack_builder`)
- [x] Add duplicate-call guard to prevent repeated `pack_builder` executions in the same run cycle
- [x] Render Pack Builder preview/apply cards inline in chat thread (not only in side rail)
- [x] Remove channel auto-preview interception for initial intent (LLM/tool-driven initial pass; deterministic confirm/cancel preserved)

## Completed

- [x] Create implementation kanban for Pack Builder v0.4.1

## Notes

- MCP connectors are default for external data/actions.
- Built-ins are fallback only if no viable MCP catalog match exists, and must emit warnings.
- Routines from generated packs are installed paused/disabled by default.
- Delivery commits:
  - `73e0759` (pack builder implementation landed earlier in branch)
  - `08a9c81` (agent routing, preset registry, HTTP coverage, control panel compatibility)
  - `e872c8d` (TUI preset index compatibility for `pack_presets`)
  - `830cec6` (OpenAI provider schema hardening for MCP tool dispatch)
  - `da0d07f` (pack-builder preview/apply UX hardening + safe auto-apply + tests)
  - `62f1442` (engine confirmation bridge for apply-by-chat across surfaces)
  - `28796f8` (pack-builder session-local confirmation fallback + tests)
  - `1d4f579` (pack-builder API-first parity endpoints + channel/control-panel direct apply path)
  - `6001205` (restore LLM-led chat flow + inline in-thread pack-builder cards + channel interception rollback)
