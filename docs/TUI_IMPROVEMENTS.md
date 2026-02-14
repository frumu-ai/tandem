You are implementing a Rust TUI using ratatui + crossterm for tandem-cli.

PROJECT CONTEXT (CURRENT CODEBASE)
- TUI entrypoint and event loop: tandem/crates/tandem-tui/src/main.rs (draw -> handle events -> update).
- App state + Actions: tandem/crates/tandem-tui/src/app.rs.
- Chat view layout: tandem/crates/tandem-tui/src/ui/mod.rs.
- Transcript rendering (role prefixes): tandem/crates/tandem-tui/src/ui/components/flow.rs.
- Network streaming: tandem/crates/tandem-tui/src/net/client.rs uses SSE on /session/{id}/prompt_sync.

CURRENT UI REALITY (MUST BE PRESERVED)
- Chat header uses the current session title (defaults to “New session”).
- Online/Offline indicator is right-aligned in the chat header with a colored dot glyph.
- Transcript lines are labeled with role prefixes: “you:”, “ai:”, “sys:”.
- Input box label is “Input” and placeholder is “Type prompt or /command... (Tab for autocomplete)”.
- Bottom status strip exists today, but currently shows:
  “Tandem TUI | MODE | PROVIDER | MODEL | Sessions: N”.
  This strip must be preserved and updated to show MODE | PROVIDER | MODEL | SESSION/ENGINE ID
  (do not remove the label, only swap Sessions: N for session/engine identifier).

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

IMPORTANT: In the current code, the header title is the session title, the dot indicator lives in the header
right-aligned, and the role prefixes are “you: / ai: / sys:”. Preserve these affordances even when adding
multi-agent panes.

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
- Preserve the existing optional task list panel behavior (today it only renders when tasks exist).

UI MODES
1) FOCUS MODE (default)
┌─ New session ────────────────────────────────────────────────● Online ───────┐
│ MAIN VIEWPORT (scroll): Active agent transcript + tool events + streaming    │
├─ Input ─────────────────────────────────────────────────────────────────────┤
│ > composer (active agent)                                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│ Ask | openrouter | z-ai/glm-5 | dcad20d4 | Active: A2                        │
└─────────────────────────────────────────────────────────────────────────────┘

2) GRID MODE (optional toggle, for 2–4 agents)
The viewport becomes a container grid:
- 2 agents: 1 row, 2 cols
- 3 agents: 2 top, 1 bottom spanning full width (2x1 layout)
- 4 agents: 2x2
Each pane has a small title bar:
“Agent A1  ● Running” / “Agent A2  ✓ Done” / “Agent A3  ! Error”
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
1) If agent is streaming/running → send Cancel, wait for cancel ack OR immediately mark “Cancelling…”
2) Then transition to Closed state and remove from grid focus set
3) If it has unsent draft text, show confirm modal:
   “Discard draft and close agent? (Y/N)”
4) Always keep at least one agent alive; if closing last agent, create a fresh “A1” session.

EVENT STREAM ROUTING
Every event/chunk MUST have:
- agent_id
- run_id (optional, but helpful)
- channel: assistant | tool | system | log
Store outputs per-agent, never global.

PROJECT-SPECIFIC STREAMING NOTES
- Current streaming path is SSE from /session/{id}/prompt_sync and is handled in tandem-tui net/client.rs.
- The current UI directly appends PromptDelta into the last assistant message in the transcript.
- For multi-agent, keep per-agent streaming buffers separate and only project them into the active pane.

CURRENT COMMANDS (FROM /HELP)
- /help
- /engine status | /engine restart
- /sessions | /new [title...] | /use <session_id> | /title <new title>
- /prompt <text> | /messages [limit]
- /modes | /mode <name>
- /providers | /provider <id> | /models [provider] | /model <model_id>
- /keys | /key set <provider> | /key remove <provider> | /key test <provider>
- /task add <desc> | /task done <id> | /task fail <id> | /task work <id> | /task pin <id> | /task list
- /approve <id> [once|always] | /deny <id> [message...] | /answer <id> <text>
- /config
- /cancel exists but currently returns “not implemented yet”.

STATE MODEL (MUST FOLLOW)
Global:
- connection_status: Online/Offline
- selection_status: { mode, provider, model, engine_or_session_id }  // render in bottom strip
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

CURRENT KEYBINDINGS (TODAY)
Global:
- Ctrl+C or Ctrl+X: quit
Main menu:
- q: quit
- n: new session
- j/k or Up/Down: move selection
- Enter: open selected session
Chat:
- Enter: submit command/prompt
- Esc: back to main menu
- Tab: open autocomplete
- Up/Down: scroll transcript
- PageUp/PageDown: scroll faster
- Backspace: delete input
- Type: append to input
Autocomplete popup:
- Enter/Tab: accept
- Esc: dismiss
- Up/Down: move selection
- Ctrl+J/Ctrl+K: move selection
Setup wizard:
- Enter: next step
- Up/Down: move selection
- Backspace: delete input
- Type: append to input
- Esc: quit
Mouse:
- Scroll: moves selection in menus and scrolls chat

TARGET MULTI-AGENT KEYBINDINGS (MUST IMPLEMENT)
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
- Align with current tandem-tui structure: Action enum + update() in app.rs, draw() in ui/mod.rs, main loop in main.rs.

STREAMING DEMO (REQUIRED BEFORE REAL LLM WIRING)
Implement a simulation per agent:
- Press 'S': start streaming demo tokens into active agent’s streaming row
- Press 'B': spawn a background agent and start its streaming too
In Focus mode, background agent must NOT spam the active viewport; it just updates its status badge.
In Grid mode, you can see both streaming simultaneously in their own panes.

DELIVERABLES
1) Working TUI preserving “New session”, “Online”, “Input”, bottom status strip (mode/provider/model/id).
2) Multi-agent state with spawn/switch/close.
3) Focus mode + Grid mode toggle with auto layouts for 2, 3, 4 agents.
4) Per-agent draft buffers so switching agents preserves what you were typing.
5) Streaming simulation for multiple agents routed correctly (no interleaving).
