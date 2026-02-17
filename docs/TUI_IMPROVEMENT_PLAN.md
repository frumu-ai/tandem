# Tandem TUI Improvement Plan (Codex Parity Track)

Tracking board: `docs/TUI_IMPROVEMENT_KANBAN.md`

## Goal

Push `tandem-tui` toward codex-grade TUI behavior and reliability by adapting high-value interaction and rendering patterns from `codex-rs/tui` while preserving Tandem UX conventions.

## Phase 1 (Implemented)

### Input/Composer Foundation

- Added `crates/tandem-tui/src/ui/components/composer_input.rs`:
  - multiline draft state
  - cursor movement (left/right/home/end + line up/down)
  - insert/backspace/delete-forward
  - paste support
  - dynamic desired input height and cursor position helpers

### App/Event Wiring

- Updated `crates/tandem-tui/src/app.rs`:
  - `AppState::Chat.command_input` now uses composer state instead of `String`
  - `AgentPane.draft` now uses composer state per agent
  - added actions for cursor movement, delete-forward, and paste input
  - key handling now supports richer editor interactions
  - slash command routing remains single-line; multiline input routes to prompt path
  - streaming delta path now preserves whitespace-only chunks

### Event Loop + UI

- Updated `crates/tandem-tui/src/main.rs` to handle `Event::Paste`.
- Updated `crates/tandem-tui/src/ui/mod.rs`:
  - dynamic input row height
  - real terminal cursor placement
  - removed synthetic `|` cursor marker rendering

### Markdown Rendering

- Added `crates/tandem-tui/src/ui/markdown.rs` (pulldown-cmark based renderer adapted from codex patterns).
- Updated `crates/tandem-tui/src/ui/components/flow.rs` to use tandem-local markdown renderer.
- Removed `tui-markdown` dependency from `crates/tandem-tui/Cargo.toml`.

### Streaming Delta Stability

- Added `crates/tandem-tui/src/ui/markdown_stream.rs` newline-gated stream collector.
- Updated prompt stream reducers to commit only complete lines during streaming and flush pending tail on success/failure.
- Hardened success-merge behavior to ignore regressive assistant snapshots that are shorter than locally finalized stream content.

### Long Transcript Performance

- Added first-pass transcript virtualization in `crates/tandem-tui/src/ui/components/flow.rs`:
  - render pipeline now materializes only enough message lines to satisfy `viewport + scroll offset + overscan`
  - avoids flattening full chat history on every frame in large sessions
- Added long-transcript benchmarking and parity validation in `flow.rs` tests:
  - `virtualization_scans_subset_for_recent_view`
  - `virtualized_and_naive_visible_output_match`
  - ignored benchmark: `benchmark_virtualized_vs_naive_long_transcript`
- Added bounded per-message render cache in `flow.rs`:
  - cache key includes message fingerprint + width
  - cache stores wrapped/rendered lines to avoid repeated markdown + wrapping work across frames
  - cache capped at 1024 entries to bound memory growth

### Tests Added

- composer unit tests (editing + movement + height clamp)
- markdown renderer smoke test
- markdown renderer parity tests (ordered lists, links, fenced code)
- flow renderer tests for assistant markdown and user raw text behavior
- markdown stream collector roundtrip tests for chunk-splitting correctness
- additional flow snapshot-style tests for heading/code and narrow-width wrapping
- complex markdown flow snapshots (nested list/quote/code structures)
- collector boundary tests across varied split patterns and utf8 content
- keyboard parity tests for cursor/edit/newline keymaps in chat/autocomplete
- markdown style legend added to TUI guide and help modal
- reducer-level stream integration tests (success/failure tail flush + utf8 chunk paths)
- reducer regression coverage for stream-tail preservation across success/failure merge paths
- full regression suite kept green after flow virtualization refactor

## Verification Commands

- `cargo check -p tandem-tui`
- `cargo test -p tandem-tui`
- `cargo test -p tandem-tui benchmark_virtualized_vs_naive_long_transcript -- --ignored --nocapture`

## Latest Benchmark Snapshot

- Local run (2026-02-17, Windows debug profile) from ignored benchmark:
  - before cache: `flow benchmark runs=12 virtualized_ms=11 naive_ms=768`
  - after cache: `flow benchmark runs=12 virtualized_ms=7 naive_ms=767`

## Manual Runtime Checks

1. Start TUI and verify multiline input (`Shift+Enter` / `Alt+Enter`).
2. Verify cursor navigation (`Left/Right/Home/End`, `Ctrl+Up/Down`).
3. Paste multi-line text and verify integrity.
4. Ask for markdown-heavy output (lists, blockquotes, fenced code) and verify readable rendering.
5. Confirm slash command autocomplete and execution still work on single-line input.

## Attribution and Compliance

- Adapted from codex interaction/rendering patterns:
  - `codex/codex-rs/tui/src/public_widgets/composer_input.rs`
  - `codex/codex-rs/tui/src/bottom_pane/textarea.rs`
  - `codex/codex-rs/tui/src/markdown_render.rs`
- Implemented as tandem-local rewrites, not line-by-line copy.
