# Tandem TUI Improvement Kanban

Last updated: 2026-02-17

## Backlog

| ID      | Task                             | Notes                                                                    |
| ------- | -------------------------------- | ------------------------------------------------------------------------ |
| TUI-024 | Grid-mode render budget controls | Apply per-pane render budgets/overscan tuning for 4-agent grid sessions. |

## In Progress

| ID      | Task                                                      | Notes                                                                                                                             |
| ------- | --------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| TUI-011 | Flow rendering parity hardening                           | Added snapshot-style flow tests and complex markdown structure coverage; monitor for regressions during future markdown upgrades. |
| TUI-016 | Evaluate transcript virtualization for very long sessions | Performance pass for large histories and multi-agent grids.                                                                       |

## Done

| ID      | Task                                                                   | Notes                                                                                                                                                                     |
| ------- | ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TUI-001 | Introduce multiline composer state                                     | `composer_input.rs` with cursor/edit primitives.                                                                                                                          |
| TUI-002 | Wire composer state into app/agent drafts                              | `AppState::Chat.command_input` and `AgentPane.draft` migrated from `String`.                                                                                              |
| TUI-003 | Add cursor/edit actions to reducer                                     | Left/right/home/end, line up/down, delete-forward, paste.                                                                                                                 |
| TUI-004 | Support native paste events                                            | `main.rs` handles `Event::Paste`.                                                                                                                                         |
| TUI-005 | Dynamic input height + real cursor rendering                           | `ui/mod.rs` now places terminal cursor explicitly.                                                                                                                        |
| TUI-006 | Replace `tui-markdown` with tandem markdown renderer                   | `ui/markdown.rs` integrated into `FlowList`.                                                                                                                              |
| TUI-007 | Preserve whitespace-only stream deltas                                 | `PromptDelta` no longer drops whitespace chunks.                                                                                                                          |
| TUI-008 | Add newline-gated markdown stream collector                            | `ui/markdown_stream.rs` + success/failure flush behavior.                                                                                                                 |
| TUI-009 | Add composer/markdown/stream unit tests                                | Expanded coverage for edit + render + stream behavior.                                                                                                                    |
| TUI-010 | Add explicit codex adaptation attribution and docs updates             | `LICENSING.md`, `RELEASE_NOTES.md`, `TUI_IMPROVEMENT_PLAN.md`, `TANDEM_TUI_GUIDE.md`.                                                                                     |
| TUI-013 | Add markdown line-wrapping parity snapshots (deep nested lists/quotes) | Added complex markdown flow snapshots for wide and narrow layouts.                                                                                                        |
| TUI-014 | Add stream chunk-fuzz tests for reducer equivalence                    | Added varied-boundary and utf8 stream roundtrip tests for collector correctness.                                                                                          |
| TUI-015 | Add markdown style legend to TUI help/docs                             | Added legend to TUI guide and in-app help modal.                                                                                                                          |
| TUI-017 | Add keyboard parity tests for cursor and delete behaviors              | Added app keymap tests for cursor/edit/newline behavior in chat and autocomplete modes.                                                                                   |
| TUI-018 | Add reducer-level stream fuzz tests (collector + transcript merge)     | Added async reducer tests for success/failure tail flush and utf8 chunk roundtrips.                                                                                       |
| TUI-019 | Prevent regressive stream snapshot overwrite on success/failure        | Reducer merge now keeps richer local assistant text when success payload is shorter than finalized stream content.                                                        |
| TUI-020 | Ratatui API modernization cleanup                                      | Replaced deprecated `Buffer::get_mut` usage and migrated status title alignment to `Block::title_top`.                                                                    |
| TUI-021 | Transcript virtualization prototype                                    | `FlowList` now virtualizes line flattening by rendering recent messages up to viewport + scroll offset + overscan budget.                                                 |
| TUI-022 | Transcript virtualization benchmarking                                 | Added long-transcript ignored benchmark and parity checks; local run showed `virtualized_ms=11` vs `naive_ms=768` across 12 runs.                                         |
| TUI-023 | Transcript render cache                                                | Added bounded message render cache (fingerprint + width keyed) for `FlowList` line generation; benchmark improved to `virtualized_ms=7` vs `naive_ms=767` across 12 runs. |
