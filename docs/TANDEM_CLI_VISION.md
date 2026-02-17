You are implementing a Rust TUI using ratatui + crossterm for tandem-cli.

REFERENCE (CURRENT UI MUST BE PRESERVED)
The current UI elements must remain visible and recognizable:

- Top-left: “New session”
- Top-right: green dot + “Online” (or Offline)
- Main transcript area shows “you:” + messages
- Input box labeled “Input” with placeholder:
  “Type prompt or /command... (Tab for autocomplete)”
- Bottom status strip ALWAYS visible showing selected:
  MODE | PROVIDER | MODEL | SESSION/ENGINE ID
  Example: “Ask | openrouter | z-ai/glm-5 | dcad20d4”
  Do NOT remove/replace these indicators. You may add an “Active: Agent X” segment, but keep the core strip.

PRIMARY GOAL
Make it feel like a developer CLI (opencode-cli / codex-cli / gemini-cli / claude-cli):

- One canonical transcript per agent
- Smooth streaming output
- Keyboard-first navigation
- Commands + palette later, but core interaction must be clean

NEW FEATURE GOAL: MULTI-AGENT IN ONE WINDOW (NO OS WINDOWS REQUIRED)
Support running multiple agents concurrently via event streams WITHOUT interleaving their tokens into one transcript.
Users can either:
A) Focus a single agent (default)
B) Toggle a GRID view (2–4 panes) showing multiple agents at once

IMPORTANT UX PRINCIPLES

- Never stream multiple agents into the same transcript at the same time.
- In Focus mode, only the active agent’s transcript + streaming row is shown.
- Other agents can show summarized “activity” badges/lines (optional), but not full token spam.
- In Grid mode, each pane shows that agent’s transcript (trimmed/scrollable) and optionally its streaming row inside the pane.

UI MODES

1. FOCUS MODE (default)
   ┌─ New session ────────────────────────────────────────────────● Online ───────┐
   │ MAIN VIEWPORT (scroll): Active agent transcript + tool events + streaming │
   ├─ Input ─────────────────────────────────────────────────────────────────────┤
   │ > composer (active agent) │
   ├─────────────────────────────────────────────────────────────────────────────┤
   │ Ask | openrouter | z-ai/glm-5 | dcad20d4 | Active: A2 │
   └─────────────────────────────────────────────────────────────────────────────┘

2. GRID MODE (optional toggle, for 2–4 agents)
   The viewport becomes a container grid:

- 2 agents: 1 row, 2 cols
- 3 agents: 2 top, 1 bottom spanning full width (2x1 layout)
- 4 agents: 2x2
  Each pane has a small title bar:
  “Agent A1 ● Running” / “Agent A2 ✓ Done” / “Agent A3 ! Error”
  Highlight the ACTIVE pane (border style or title marker).

INPUT MODEL (CRITICAL)
We keep ONE visible input box at the bottom (developer CLI feel),
but typing must apply to the currently selected/active agent.

RECOMMENDED: per-agent draft buffers

- Each agent has its own composer buffer (draft text).
- When switching active agent, the input box shows that agent’s draft.
  This avoids losing typed text when hopping between agents.

FOCUS / SELECTION
Users must be able to switch which agent receives input easily:

- Tab / Shift+Tab: cycle active agent (in Focus mode cycles through agents; in Grid mode cycles panes)
- Alt+1..Alt+9: jump directly to agent N
- Optional: Ctrl+P palette later for “Switch agent…”

SPAWNING AGENTS
Add ability to spawn a new agent run/session:

- Ctrl+N: create new agent (A{n}), start in idle state
- /agent new [name] command (optional)
  When created:
- appears in agent list/state
- in Focus mode, optionally auto-switch to it
- in Grid mode, reflow layout if <= 4 agents shown

CLOSING AGENTS (CLEAN SHUTDOWN)
Must handle closure safely:

- Ctrl+W: close active agent
  Rules:

1. If agent is streaming/running → send Cancel, wait for cancel ack OR immediately mark “Cancelling…”
2. Then transition to Closed state and remove from grid focus set
3. If it has unsent draft text, show confirm modal:
   “Discard draft and close agent? (Y/N)”
4. Always keep at least one agent alive; if closing last agent, create a fresh “A1” session.

EVENT STREAM ROUTING
Every event/chunk MUST have:

- agent_id
- run_id (optional, but helpful)
- channel: assistant | tool | system | log
  Store outputs per-agent, never global.

STATE MODEL (MUST FOLLOW)
Global:

- connection_status: Online/Offline
- selection_status: { mode, provider, model, engine_or_session_id } // render in bottom strip
- ui_mode: Focus | Grid
- active_agent_id
- agent_order: Vec<AgentId> // for cycling + grid placement
- modal: Option<ModalState>

Per AgentState:

- status: Idle | Running | Streaming | Cancelling | Done | Error | Closed
- transcript: Vec<Message> (User/Assistant/ToolEvent/System)
- streaming: Option<StreamingMessage>
- draft: TextArea buffer (per-agent)
- viewport_scroll: u16

RENDER MODEL (MUST FOLLOW)

- Focus mode renders ONLY active agent transcript + streaming row in the main viewport.
- Grid mode renders up to 4 agents in panes:
  each pane renders that agent transcript + streaming row (if any).
- Input box always visible at bottom, showing active agent draft.

KEYBINDINGS (MUST IMPLEMENT)
Core:

- Enter: submit draft to active agent
- Shift+Enter (or Alt+Enter): newline
- Ctrl+C: cancel active agent streaming
- Ctrl+N: new agent
- Ctrl+W: close active agent (with confirm if needed)
- Tab / Shift+Tab: cycle active agent
- Alt+1..Alt+9: select agent
- G: toggle Grid/Focus mode
- PgUp/PgDn or Ctrl+U/Ctrl+D: scroll active agent viewport (or active pane in grid)
- F1: help modal (include multi-agent keys)
- Esc: close modal / return focus to input

IMPLEMENTATION CONSTRAINTS

- Unidirectional architecture:
  handle_events -> Action -> update(state, action) -> view(frame, state)
- No state mutation in view()
- Layout code must not destroy the existing header/footer semantics.

STREAMING DEMO (REQUIRED BEFORE REAL LLM WIRING)
Implement a simulation per agent:

- Press 'S': start streaming demo tokens into active agent’s streaming row
- Press 'B': spawn a background agent and start its streaming too
  In Focus mode, background agent must NOT spam the active viewport; it just updates its status badge.
  In Grid mode, you can see both streaming simultaneously in their own panes.

DELIVERABLES

1. Working TUI preserving “New session”, “Online”, “Input”, bottom status strip (mode/provider/model/id).
2. Multi-agent state with spawn/switch/close.
3. Focus mode + Grid mode toggle with auto layouts for 2, 3, 4 agents.
4. Per-agent draft buffers so switching agents preserves what you were typing.
5. Streaming simulation for multiple agents routed correctly (no interleaving).
