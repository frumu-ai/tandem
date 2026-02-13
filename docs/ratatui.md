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
