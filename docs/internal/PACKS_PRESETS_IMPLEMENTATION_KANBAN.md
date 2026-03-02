# Packs + Presets Implementation Kanban (v0.4.0)

Last updated: 2026-03-02
Owner: Core Runtime + Product Architecture

## In Progress

- [ ] PackManager API surface (server)
  - [x] Add routes: `GET /packs`, `GET /packs/{selector}`, `POST /packs/install`, `POST /packs/uninstall`, `POST /packs/export`
  - [x] Add attachment-aware install route: `POST /packs/install_from_attachment`
  - [x] Add marker detection route: `POST /packs/detect`
  - [x] Add update check/apply routes (`GET /packs/{id}/updates`, `POST /packs/{id}/update`) as no-op stubs for now
  - [x] Add TS/Python client methods for new pack routes

- [ ] Pack installer runtime hardening
  - [x] Root marker rule enforced (`tandempack.yaml` at zip root)
  - [x] Safe unzip checks (path traversal + file count + bytes + depth limits)
  - [x] Deterministic install path (`TANDEM_HOME/packs/<name>/<version>/`)
  - [x] Atomic index update (tmp write then rename)
  - [x] `current` pointer per pack name
  - [x] Per-pack lock granularity (install/uninstall serialize per pack name; index writes remain atomic)
  - [x] Zip-bomb ratio heuristic and compressed/uncompressed ratio checks

- [ ] Chat attachment ingestion flow
  - [x] Backend detect/install endpoints for attachment paths
  - [x] Connector dispatch ingestion hook for `.zip` attachment path detection/install
  - [x] Auto-render pack card in control-panel feed/chat surfaces on `pack.detected`
  - [x] Wire Install/Open actions from feed/chat cards to PackManager endpoints
  - [x] Trusted-source auto-install policy checks in ingestion path

## Backlog

- [ ] Capability abstraction plumbing
  - [x] Provider discovery adapter contract: `list_tools()` + schema metadata (MCP + local tool registry discovery endpoint)
  - [x] Capability bindings registry (data files, no code changes required)
  - [x] Resolver selection order: user/org/pack preference + provider health (preference-order MVP)
  - [x] Structured `missing_capability` error contract end-to-end in workflow runtime (resolver API returns structured conflict payload)
  - [ ] Initial spine bindings for GitHub + Slack across Composio/MCP/custom

- [ ] Preset registry implementation
  - [ ] Build layered registry: built-ins + installed packs + project overrides
  - [ ] Deterministic prompt composition engine (core->domain->style->safety)
  - [ ] Fork/edit/save flow for immutable installed presets
  - [ ] Permission/capability summary computation at agent + automation levels
  - [ ] Export composed project overrides as pack content

- [ ] UI parity (Desktop + Control Panel)
  - [ ] Pack Library view: install/inspect/uninstall/export/trust status
  - [ ] Skill Module library with capability + publisher filters
  - [ ] Agent Preset builder with prompt preview + capability summary
  - [ ] Automation Preset builder with step-agent binding swaps
  - [ ] Upgrade flow with permissions diff + re-approval

- [ ] Trust/signing + marketplace readiness
  - [ ] Parse and expose `tandempack.sig` status in inspect endpoint
  - [ ] Verification badges (`unverified`, `verified`, `official`) in API payloads
  - [ ] Permission/risk sheet generation API for pre-install UX
  - [ ] Secret scanning hooks and reject reason taxonomy integration

## Done

- [x] Added marketplace pack specs under `specs/packs/`
- [x] Added modular preset specs under `specs/presets/`
- [x] Added marketplace-ready pack examples under `examples/packs/*_marketplace/`
- [x] Added v0.4.0 release-note/changelog sections

## Exit Criteria for v0.4.0

- [ ] Valid zip without root marker does not auto-install and returns `is_pack=false`
- [ ] Valid pack zip installs to deterministic path and updates index/current atomically
- [ ] Pack install emits lifecycle events (`pack.detected`, `pack.install.*`, `registry.updated`)
- [ ] Workflow capability request can resolve `github.create_pull_request` via at least one non-hardcoded binding
- [ ] Missing required capability returns structured error consumable by UI/chat
