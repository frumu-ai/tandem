# Packs + Presets Implementation Kanban (v0.4.0)

Last updated: 2026-03-02
Owner: Core Runtime + Product Architecture

## Completed Workstreams

- [x] PackManager API surface (server)
  - [x] Add routes: `GET /packs`, `GET /packs/{selector}`, `POST /packs/install`, `POST /packs/uninstall`, `POST /packs/export`
  - [x] Add attachment-aware install route: `POST /packs/install_from_attachment`
  - [x] Add marker detection route: `POST /packs/detect`
  - [x] Add update check/apply routes (`GET /packs/{id}/updates`, `POST /packs/{id}/update`) as no-op stubs for now
  - [x] Add TS/Python client methods for new pack routes

- [x] Pack installer runtime hardening
  - [x] Root marker rule enforced (`tandempack.yaml` at zip root)
  - [x] Safe unzip checks (path traversal + file count + bytes + depth limits)
  - [x] Deterministic install path (`TANDEM_HOME/packs/<name>/<version>/`)
  - [x] Atomic index update (tmp write then rename)
  - [x] `current` pointer per pack name
  - [x] Per-pack lock granularity (install/uninstall serialize per pack name; index writes remain atomic)
  - [x] Zip-bomb ratio heuristic and compressed/uncompressed ratio checks

- [x] Chat attachment ingestion flow
  - [x] Backend detect/install endpoints for attachment paths
  - [x] Connector dispatch ingestion hook for `.zip` attachment path detection/install
  - [x] Auto-render pack card in control-panel feed/chat surfaces on `pack.detected`
  - [x] Wire Install/Open actions from feed/chat cards to PackManager endpoints
  - [x] Trusted-source auto-install policy checks in ingestion path

- [x] Capability abstraction plumbing
  - [x] Provider discovery adapter contract: `list_tools()` + schema metadata (MCP + local tool registry discovery endpoint)
  - [x] Capability bindings registry (data files, no code changes required)
  - [x] Resolver selection order: user/org/pack preference + provider health (preference-order MVP)
  - [x] Structured `missing_capability` error contract end-to-end in workflow runtime (resolver API returns structured conflict payload)
  - [x] Initial spine bindings for GitHub + Slack across Composio/Arcade/MCP/custom (+ alias-aware matching)

- [x] Trust/signing + marketplace readiness
  - [x] Parse and expose `tandempack.sig` status in inspect endpoint
  - [x] Verification badges (`unverified`, `verified`, `official`) in API payloads
  - [x] Permission/risk sheet generation API for pre-install UX (`pack.inspect.permission_sheet`)
  - [x] Secret scanning hooks integrated (`TANDEM_PACK_SECRET_SCAN_STRICT` for local strict reject)

- [x] Preset registry implementation
  - [x] Build layered registry: built-ins + installed packs + project overrides
  - [x] Deterministic prompt composition engine (core->domain->style->safety)
  - [x] Fork/edit/save flow for immutable installed presets
  - [x] Permission/capability summary computation at agent + automation levels
  - [x] Export composed project overrides as pack content

- [x] UI parity (Desktop + Control Panel)
  - [x] Pack Library view: install/inspect/uninstall/export/trust status
  - [x] Skill Module library with capability + publisher filters
  - [x] Agent Preset builder with prompt preview + capability summary
  - [x] Automation Preset builder with step-agent binding swaps
  - [x] Upgrade flow with permissions diff + re-approval (stub-backed API/UI signaling)

- [x] Control Panel IA alignment under Settings
  - [x] Move `Packs`, `Channels`, `MCP`, and `Files` out of primary sidebar nav
  - [x] Convert Settings integration launchers into tabs (`General`, `Packs`, `Channels`, `MCP`, `Files`) and keep users on `#/settings`
  - [x] Add migration prompts in Automations and legacy surfaces to direct users to Settings

- [x] Control Panel visual polish + desktop theme parity
  - [x] Add shared control-panel theme registry + runtime apply hooks (`tandem.themeId`)
  - [x] Add Settings > Appearance theme selector for cross-surface parity
  - [x] Refactor shell styles (cards/nav/buttons/inputs/settings tabs) to token-driven theme variables
  - [x] Replace route-loading placeholders with themed skeleton surfaces for first-paint polish
  - [x] Expand icon registry for packs/presets/settings surfaces to prevent missing icons across rerenders
  - [x] Fix Settings tab icon hydration scope so tab icons persist across section switches
  - [x] Tokenize chat UI color system so Settings theme selection applies to chat rails/messages/composer/dynamic cards
  - [x] Raise Porcelain contrast (theme tokens + markdown light-mode overrides) to fix washed-out text
  - [x] Apply theme-token consistency fixes to Automations page (wizard/cards/tabs residual slate-zinc classes)
  - [x] Harden Settings tab icon hydration for async subview updates
  - [x] Align `#/agents` top tab treatment with Settings underline tab style
  - [x] Restore multi-theme selector + full theme catalog in Settings Appearance
  - [x] Fix provider test flow in Settings to use single async probe (remove sync/async double-run stall path)
  - [x] Hide internal provider-test sessions from chat list and run tests outside workspace-scoped session surface

## Active Backlog

- [ ] None (current v0.4.0 packs/presets scope complete)

## Done

- [x] Added marketplace pack specs under `specs/packs/`
- [x] Added modular preset specs under `specs/presets/`
- [x] Added marketplace-ready pack examples under `examples/packs/*_marketplace/`
- [x] Added hands-on starter packs under `examples/packs/` (`daily_github_pr_reviewer`, `slack_release_notes_writer`, `customer_support_drafter`)
- [x] Added personal first-pack walkthrough at `specs/packs/PERSONAL_TUTORIAL_FIRST_PACK.md`
- [x] Added v0.4.0 release-note/changelog sections

## Exit Criteria for v0.4.0

- [x] Valid zip without root marker does not auto-install and returns `is_pack=false`
- [x] Valid pack zip installs to deterministic path and updates index/current atomically
- [x] Pack install emits lifecycle events (`pack.detected`, `pack.install.*`, `registry.updated`)
- [x] Workflow capability request can resolve `github.create_pull_request` via at least one non-hardcoded binding
- [x] Missing required capability returns structured error consumable by UI/chat
