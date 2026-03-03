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
