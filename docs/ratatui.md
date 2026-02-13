# AI Agent Task: Ratatui Feature Notes + Reference Links for Tandem TUI

## Goal

Create a **Ratatui feature reference** we can paste into the Tandem repo as a living doc. It must:

- Identify the **highest-impact Ratatui features** for an agentic TUI (chat + sessions + tools + approvals).
- Include **direct reference links** to the official Ratatui docs (docs.rs) for each feature.
- Provide short “how we’ll use it in Tandem” notes and a suggested v1 adoption order.

## Context

We are building:

- `tandem-engine` (Rust headless backend)
- A new **TUI app** using Ratatui that talks to the engine over HTTP/SSE.
- The TUI must support: streaming chat output, session list, mode switching, provider/model switching, key entry (PIN to decrypt keystore), approvals/questions popups, and status footer.

Primary docs index:

- https://docs.rs/ratatui/latest/ratatui/all.html

## Deliverable

Create a single markdown file at:

- `tandem/docs/tui/ratatui-feature-map.md`

### Format requirements

Use this structure:

1. **Executive Summary (10–15 bullets max)**
   - The “top features we should use first” with links.

2. **Feature Map Table**
   A table with columns:
   - Feature / Module
   - Why it matters
   - Tandem use case
   - Docs link(s)
   - V1 / V2 priority (P0/P1/P2)

3. **Recommended Widget Set for Tandem v1**
   - List the exact Ratatui widgets/modules we should start with.
   - Include links per item.

4. **Animation + Matrix Splash Plan**
   - Which primitives to use (`Canvas`, `Text`, tick loop, etc.)
   - Include links.

5. **UI Patterns**
   - Modal overlays (PIN prompt, permission approvals) using `Clear + Block + Paragraph/List`
   - Stateful scrolling (`ListState`, `ScrollbarState`)
   - Tabs for modes
   - Include links.

6. **Implementation Notes**
   - Any gotchas: performance (allocations), redraw frequency, handling long transcripts.
   - How to pair Ratatui with an event/input backend (mention `crossterm` as typical, but keep focus on Ratatui).

## Must-cover Ratatui areas (include each with links)

Prioritize these and cite each:

### Layout

- `ratatui::layout::{Layout, Constraint, Direction, Rect}`

### Core widgets

- `ratatui::widgets::{Paragraph, List, Table, Tabs, Block, Borders, Clear}`
- Stateful pieces: `ListState`, `Scrollbar`, `ScrollbarState`
- Progress/telemetry: `Gauge`, `LineGauge`, `Sparkline`

### Text/Style

- `ratatui::text::{Span, Line, Text}`
- `ratatui::style::{Style, Modifier}`
- `ratatui::prelude::Color`
- `ratatui::symbols`

### Custom drawing / animation

- `ratatui::widgets::Canvas`
- (Optional) `ratatui::widgets::Chart` as a future feature

## Priority Guidance

When assigning priorities, assume:

- P0: required for a usable v1 TUI (chat, lists, modals, scrolling, status bar)
- P1: polish (gauges, sparklines, nicer tabs)
- P2: advanced (charts, complex canvas visualizations)

## Quality bar

- Every item must include a **clickable docs.rs link** (module or type page).
- Keep descriptions short and practical.
- Do not invent APIs: verify names against docs.rs pages you link.

## Output

Write the file content only (markdown), and ensure all links are correct and point to docs.rs.

# AI Agent Task: Upgrade Tandem TUI with Ratatui (Design + Implementation Plan)

## Objective

Improve the Tandem TUI using Ratatui so it feels **fast, visual, animated, and “alive”** while staying pragmatic. Produce a concrete plan + file-level implementation steps for where/how to use Ratatui tools/components.

You must reference Ratatui docs in your notes (module/type names, links):

- https://docs.rs/ratatui/latest/ratatui/all.html
  Also consider our product direction: Team/Agents control center, modes, providers/models, approvals, token dashboard, logs, and PTY.

## Repo context

- Rust engine exists (tandem-engine) with SSE events and session-based agent loop.
- TUI CLI exists or is being created to control the engine (start engine, connect, stream events, run prompts, approvals).
- We want a console UI similar in vibe to: Codex CLI / opencode-cli, but with our own “Tandem” identity.
- Startup should show a **Matrix-like animation** with **TANDEM** centered (short, tasteful), then prompt for **PIN** to decrypt keystore and load provider keys.

## Deliverables (write as files)

1. `tandem/docs/tui/ratatui-upgrade-plan.md`
   - UI architecture, components, event loop design, and which Ratatui widgets we use where.
2. `tandem/docs/tui/ratatui-widget-map.md`
   - A mapping table: “Feature → Ratatui widget/module → why → implementation notes”.
3. `tandem/docs/tui/keybindings.md`
   - Key map for navigation, modes, providers, approvals, team/agents, logs, PTY.

## Hard constraints

- Must run on Windows/macOS/Linux.
- Must remain responsive under high event load (lots of SSE).
- Must not require external API keys (web search keys) to function.
- Must support our modes + provider/model switching + key management + approval prompts.
- The TUI must work even if engine isn’t running (it should start/download engine, or show helpful status).

## What to implement/design (scope)

### A) Core screens (minimum viable)

Design these screens/views (even if P0 is subset):

1. **Home / Dashboard**
   - engine status (running/not), current session, current mode, provider/model, token usage summary
2. **Chat / Session View (Flowing Terminal)**
   - **Dynamic Content**: Handle mixed content types (Markdown text, code blocks, tables, tool calls).
   - **Streaming**: Visualize text appearing in real-time.
   - **Paragraphs**: Use distinct paragraph spacing for readability.
   - **Tool Output**: Render tool inputs/outputs as distinct, collapsible blocks.
   - **Auto-scroll**: Smart scrolling that sticks to bottom while streaming but allows manual review.
3. **Approvals Queue**
   - pending permissions (allow once/always/deny) + show diff/preview when relevant
4. **Modes & Provider/Model Switcher**
   - list modes, select mode, list providers/models, set defaults per session
5. **Agents / Team Control (lightweight)**
   - list roles/agents, status, start/stop/cancel, assign goal (basic)
6. **Task List & Plan View (Animated)**
   - **Live Tasks**: Show active agent tasks in a list.
   - **Animations**:
     - "Working" state: Spinner/loader animation (e.g., dots, rotating chars).
     - "Done" state: Green checkmark or dimmed text.
   - **Pinning**: Ability to "pin" active or important tasks to the top or side.
   - **Structure**: Hierarchical view if tasks have sub-steps.

7. **Logs & Events**
   - raw SSE events, filtered by session/agent, export-to-file

8. **PTY / Terminal View (if enabled)**
   - connect to PTY websocket and display terminal buffer (P1 if too big)

### B) Ratatui: where + how to use it

You must propose specific Ratatui building blocks and how we’ll use them:

#### Layout & structure

- `ratatui::layout::{Layout, Constraint, Direction, Rect}`
- `ratatui::widgets::{Block, Borders, Padding}`
- `ratatui::style::{Style, Modifier}`
- Make a clean layout system: header / main / footer, plus modal overlays.

#### Widgets to use (examples)

- Streaming chat: Custom `FlowWidget` or `List` of `Paragraph`s to handle mixed content (Markdown, Code).
- **Task List**: Custom `TaskWidget` that renders:
  - Icon/Status (spinner if active, check if done).
  - Description (text).
  - Pinned state (visual indicator/separation).
- Lists: `List` + `ListState` for sessions, agents, modes, providers, models
- Tabs: `Tabs` for major sections
- Tables: `Table` for providers/models and token metrics
- Progress: `Gauge` for token budgets, task progress
- Charts (optional): `Sparkline` or `Chart` for tokens over time
- Scroll: keep scroll state for log panes and chat panes
- Popups/modals: use centered `Rect` + `Clear` overlay pattern

#### Interactivity / input

- Use a central event loop handling:
  - keyboard input
  - engine SSE events
  - timers (for animations/refresh)
- **Task Interaction**: Keybindings to pin/unpin tasks, expand/collapse details.
- Suggest crates (if needed) such as `crossterm` backend and `tokio` integration.

#### Animation

- Startup “Matrix”:
  - make it short (1–2 seconds), deterministic, no CPU burn
  - implement via timer tick + pseudo-random glyph trail in a buffer
- Loading spinners / subtle animations:
  - spinner widget or custom `Paragraph` that cycles frames
  - animated status in header while streaming

### C) State management & performance

Propose architecture:

- `AppState` holding:
  - current view/tab
  - selected session/agent
  - provider/model selection
  - pending approvals/questions
  - logs ring buffer
  - token metrics time series
- Event routing:
  - SSE → parsed events → bounded channel → reducer updates state
  - avoid unbounded growth (ring buffers, truncation policies)
- Backpressure strategy:
  - drop or coalesce high-frequency events (e.g., text deltas) into frame updates
- Rendering frequency:
  - fixed tick (e.g., 30–60 FPS max) or adaptive (only redraw when state dirty)

### D) Key features we must support

- Switch modes (build/plan/orchestrator/etc.)
- Switch providers and models
- Add/update encrypted provider keys (PIN unlock)
- Approval prompts (allow once / always / deny)
- Cancel current run/session
- View token usage per session + total
- View tool calls & tool results as structured blocks
- Optional: spawn/attach PTY

### E) Implementation plan with file paths

You must propose:

- new module layout for the TUI crate, e.g.
  - `src/app.rs` (AppState + reducer)
  - `src/ui/mod.rs` (render entry)
  - `src/ui/views/*.rs` (dashboard/chat/approvals/etc.)
  - `src/input.rs` (keybinding dispatch)
  - `src/engine_client.rs` (HTTP + SSE + ws)
  - `src/anim/*.rs` (matrix intro, spinners)
- incremental steps that can be implemented in small PRs
- quick smoke tests for each milestone

## Output requirements

- Be opinionated: pick widget choices and layout defaults.
- Include a “Widget Map” section listing Ratatui modules/types to use.
- Include Mermaid diagrams for:
  1. TUI event loop flow
  2. State update flow (SSE → reducer → render)
- Include a prioritized roadmap:
  - P0 (must ship)
  - P1 (PTY, teams richer)
  - P2 (charts, tmux integration, advanced)

## Acceptance criteria

- After following your plan, the TUI should:
  - feel polished and animated
  - support our modes/providers/models/keys/approvals
  - remain stable under streaming and high event volume
  - be straightforward to extend with “Agent Teams” later

## Notes

- Do NOT implement a complex 3-pane layout immediately. Start with a clean tabbed UI + modals.
- Keep the Matrix intro tasteful and fast.
- Favor reliable UX on Windows terminal environments.
