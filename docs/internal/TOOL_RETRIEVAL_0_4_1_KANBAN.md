# Semantic Tool Retrieval 0.4.1 Kanban

Last updated: 2026-03-03
Owner: Tandem Engine

## To Do

- [x] Add v0.4.1 changelog entry details for semantic retrieval rollout
- [x] Add v0.4.1 release notes entry for semantic retrieval + MCP context reduction
- [x] Final validation pass (`tandem-tools`, `tandem-core` targeted tests, engine compile)
- [x] Add semantic-retrieval reliability hardening for action-heavy prompts (web/email)
- [x] Add guardrails for non-offered tool calls and false email-send claims
- [x] Default Pack Builder-created routines to autonomous execution (`active`, no manual approval)
- [x] Increase default provider stream connect timeout for routine reliability (30s -> 90s)

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
- [x] Add retrieval fallback-to-full-tools guard when top-K omits required web/email tool families
- [x] Add `tool.call.rejected_unoffered` event + available-tool hinting for unsupported per-turn calls
- [x] Add final-response guard preventing “email sent” claims without successful email tool execution
- [x] Change pack-builder apply defaults to `approve_* = true` for API-first unattended workflows
- [x] Change generated routine YAML/output defaults to no-approval autonomous behavior

## Notes

- Retrieval defaults:
  - `TANDEM_SEMANTIC_TOOL_RETRIEVAL=1` (default enabled)
  - `TANDEM_SEMANTIC_TOOL_RETRIEVAL_K=24` (aligned to existing expanded tool cap)
  - `TANDEM_MCP_CATALOG_IN_SYSTEM_PROMPT=1`
- Existing `TANDEM_TOOL_ROUTER_ENABLED` default is unchanged in this phase.
- `K=24` is intentionally aligned with the existing `max_tools_per_call_expanded()` default.
- Post-start MCP servers are indexed through updated `register_tool` flows (`connect/refresh/add`).
- New reliability behavior:
  - for web-research/email-delivery prompts, if semantic retrieval omits required families, engine falls back to full list for that turn
  - out-of-offer tool calls are rejected deterministically (no silent unknown-tool execution)
  - email-send success claims require actual successful email-like tool evidence in-run
- Automation defaults:
  - pack-builder apply now enables routines by default and writes `requires_approval=false`
  - provider stream connect timeout default raised to 90s for slower providers/scheduled runs

## Delivery Commits

- `ff2a64b` — semantic retrieval runtime integration + MCP catalog prompt + kanban board
- `e6d564f` — v0.4.1 changelog/release notes updates + kanban completion snapshot
- `9c5ed20` — reliability hotfix for retrieval fallback, unoffered-tool rejection, and email-claim evidence guard
- `TBD` — autonomous pack-builder defaults + provider connect-timeout uplift
