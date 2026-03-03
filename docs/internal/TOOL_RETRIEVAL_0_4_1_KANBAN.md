# Semantic Tool Retrieval 0.4.1 Kanban

Last updated: 2026-03-03
Owner: Tandem Engine

## To Do

- [x] Add v0.4.1 changelog entry details for semantic retrieval rollout
- [x] Add v0.4.1 release notes entry for semantic retrieval + MCP context reduction
- [x] Final validation pass (`tandem-tools`, `tandem-core` targeted tests, engine compile)

## In Progress

- [x] Track delivery commits in this file

## Done

- [x] Create implementation kanban for semantic tool retrieval (`v0.4.1`)
- [x] Extend `ToolRegistry` with vector index storage and semantic `retrieve(query, k)`
- [x] Add startup indexing hook (`tools.index_all().await`) in engine runtime build
- [x] Keep runtime MCP dynamic registration covered via `register_tool` indexing
- [x] Add MCP server catalog extraction (`mcp_server_names`) to `ToolRegistry`
- [x] Ensure `unregister_by_prefix` removes vector index entries (MCP disconnect path)
- [x] Integrate semantic retrieval into `EngineLoop` candidate selection with env controls
- [x] Keep explicit allowlist/policy tools always included by unioning policy matches from full tool list
- [x] Add compact MCP integration catalog to runtime system prompt (names only)
- [x] Add/adjust tests for new prompt behavior and tool-registry MCP/vector lifecycle

## Notes

- Retrieval defaults:
  - `TANDEM_SEMANTIC_TOOL_RETRIEVAL=1` (default enabled)
  - `TANDEM_SEMANTIC_TOOL_RETRIEVAL_K=24` (aligned to existing expanded tool cap)
  - `TANDEM_MCP_CATALOG_IN_SYSTEM_PROMPT=1`
- Existing `TANDEM_TOOL_ROUTER_ENABLED` default is unchanged in this phase.
- `K=24` is intentionally aligned with the existing `max_tools_per_call_expanded()` default.
- Post-start MCP servers are indexed through updated `register_tool` flows (`connect/refresh/add`).

## Delivery Commits

- `ff2a64b` — semantic retrieval runtime integration + MCP catalog prompt + kanban board
- `TBD` — release docs + kanban completion snapshot
