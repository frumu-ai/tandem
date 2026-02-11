# Terminal / TUI Integration Investigation — Tandem

> **Date:** 2026-02-11  
> **Status:** Investigation complete — decision memo  
> **Author:** AI Agent (prompted by product team)

---

## 1. Current State Summary

### How OpenCode Is Launched & Managed

| Aspect                | Mechanism                                                                                                    | File                                                                                      |
| --------------------- | ------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------- |
| **Binary dispatch**   | `Command::new(path).args(["serve","--hostname","127.0.0.1","--port",<port>])`                                | [sidecar.rs](file:///c:/Users/evang/work/tandem/src-tauri/src/sidecar.rs#L853-L871)       |
| **Stdio handling**    | `stdin(null)`, `stdout(piped)`, `stderr(piped)` — drained by background threads into a `LogRingBuffer(2000)` | [sidecar.rs](file:///c:/Users/evang/work/tandem/src-tauri/src/sidecar.rs#L948-L981)       |
| **Crash recovery**    | `CircuitBreaker` with max 3 failures → 30 s cooldown → half-open retry                                       | [sidecar.rs](file:///c:/Users/evang/work/tandem/src-tauri/src/sidecar.rs#L152-L203)       |
| **Orphan prevention** | Windows: `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` job object. Unix: `child.kill()`                               | [sidecar.rs](file:///c:/Users/evang/work/tandem/src-tauri/src/sidecar.rs#L987-L1011)      |
| **Binary updates**    | `sidecar_manager.rs` checks GitHub releases, downloads, and stores in AppData                                | [sidecar_manager.rs](file:///c:/Users/evang/work/tandem/src-tauri/src/sidecar_manager.rs) |
| **Health check**      | `GET /global/health` polled every 500 ms up to 60 s on startup                                               | [sidecar.rs](file:///c:/Users/evang/work/tandem/src-tauri/src/sidecar.rs#L1196-L1229)     |

### How Logs Are Streamed Today

```
OpenCode (Bun)  ─stdio→  [Rust drain threads]  ─push→  LogRingBuffer(2000 lines)
                                                            │
                Tauri "start_log_stream" command ───────────┘
                                                            │
            Tauri event "log_stream_event" ←─── batched lines ──→  LogsDrawer.tsx (frontend)
```

- **Two sources:** `"tandem"` (rolling file logs) and `"sidecar"` (ring buffer)
- **Frontend binding:** `startLogStream()` / `stopLogStream()` / `onLogStreamEvent()` in [tauri.ts](file:///c:/Users/evang/work/tandem/src/lib/tauri.ts#L934-L956)
- **Sidecar events (SSE):** `subscribe_events()` → `GET /event` returns `StreamEvent` enum with variants: `Content`, `ToolStart`, `ToolEnd`, `SessionStatus`, `SessionIdle`, `PermissionAsked`, `QuestionAsked`, `TodoUpdated`, `FileEdited`, `Raw`

### Existing UI Components (Reusable for Console)

| Component                                                                                                    | Purpose                                                                                                                                                                | Reusability                                                               |
| ------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| [LogsDrawer.tsx](file:///c:/Users/evang/work/tandem/src/components/logs/LogsDrawer.tsx)                      | Virtualized log viewer, two tabs (Tandem/Sidecar), search, level filter, pause/play, copy, **fullscreen toggle** (`expanded` state → `inset-0` vs `right-0 w-[560px]`) | ★★★ — can be extended with a "Console" tab or forked into a ConsoleDrawer |
| [ActivityDrawer.tsx](file:///c:/Users/evang/work/tandem/src/components/chat/ActivityDrawer.tsx)              | Collapsible bottom bar showing recent tool executions with status                                                                                                      | ★★ — model for "command history" UI                                       |
| [ActivityPanel.tsx](file:///c:/Users/evang/work/tandem/src/components/chat/ActivityPanel.tsx)                | Right-side panel variant of activity view with expand/collapse per item                                                                                                | ★★ — right-panel pattern                                                  |
| [PermissionToast.tsx](file:///c:/Users/evang/work/tandem/src/components/permissions/PermissionToast.tsx)     | Floating approval toast: "Allow Once / Allow / Deny Always" with diff preview                                                                                          | ★★★ — exact pattern for "approve & run command"                           |
| [QuestionDialog.tsx](file:///c:/Users/evang/work/tandem/src/components/chat/QuestionDialog.tsx)              | Multi-choice question prompt from OpenCode                                                                                                                             | ★★ — interactive decision UI                                              |
| [OrchestratorPanel.tsx](file:///c:/Users/evang/work/tandem/src/components/orchestrate/OrchestratorPanel.tsx) | Multi-agent task dashboard with embedded LogsDrawer                                                                                                                    | ★★ — "control room" precedent                                             |

---

## 2. Proposed Use-Cases (Ranked)

### Rank 1: "Approve & Run Command" Console

| Aspect            | Detail                                                                                                                                                                                         |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Who**           | All users (non-dev friendly)                                                                                                                                                                   |
| **What**          | Agent proposes shell commands → user sees preview + risk level → clicks "Run". Output streams inline. Replaces the current `PermissionToast` for `run_command` with a richer, persistent view. |
| **Value vs logs** | Logs are read-only history. This adds **interactive control** with provenance (who ran what, when, why).                                                                                       |
| **Risks**         | Low — default is still approval-gated; no arbitrary shell access.                                                                                                                              |

### Rank 2: "Session Transcript & Artifacts" View

| Aspect            | Detail                                                                                                                                          |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| **Who**           | All users                                                                                                                                       |
| **What**          | Searchable, filterable timeline of the entire AI conversation: messages, tool calls, file diffs, command outputs. Export to clipboard/markdown. |
| **Value vs logs** | Logs show raw text. This shows **structured, semantic history** — "what did the AI change and why?"                                             |
| **Risks**         | UX complexity; need clear scoping to avoid overwhelming non-devs.                                                                               |

### Rank 3: "System Diagnostics" Panel

| Aspect            | Detail                                                                                                                                          |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| **Who**           | Power users, support cases                                                                                                                      |
| **What**          | One-click environment report: sidecar version, port, health, API key status, workspace path, memory stats, MCP connections, Python venv status. |
| **Value vs logs** | Currently requires reading raw sidecar logs + checking settings. This provides **structured health at a glance**.                               |
| **Risks**         | Low. Pure read-only.                                                                                                                            |

### Rank 4: "Task Control Room" (Multi-Agent Dashboard)

| Aspect            | Detail                                                                                                                                     |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| **Who**           | Power users using orchestration mode                                                                                                       |
| **What**          | Enhanced `OrchestratorPanel` with: live agent status per task, queue visualization, step-by-step progress, ability to re-run/skip/reorder. |
| **Value vs logs** | Orchestrator already has a basic panel. This adds **operational control** beyond "watch it run."                                           |
| **Risks**         | Medium complexity. Requires orchestrator API extensions.                                                                                   |

### Rank 5: "Interactive Troubleshooting" Mode

| Aspect            | Detail                                                                                                                                                      |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Who**           | Power users / advanced support                                                                                                                              |
| **What**          | User types OpenCode API commands directly (e.g., `list-sessions`, `health`, `get-session <id>`). Guardrails: whitelisted command set, read-only by default. |
| **Value vs logs** | Direct introspection into OpenCode internals without restarting.                                                                                            |
| **Risks**         | Medium — must never expose arbitrary shell; needs strict allowlist.                                                                                         |

### Rank 6: "Command Output History" (Persistent)

| Aspect            | Detail                                                                                                                               |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| **Who**           | All users                                                                                                                            |
| **What**          | Every command the agent ran (via `run_command` tool), with its full stdout/stderr, exit code, and timestamp. Searchable, exportable. |
| **Value vs logs** | Agent command outputs are currently embedded in chat messages and truncated. This gives **full, persistent output**.                 |
| **Risks**         | Low. Read-only view of already-captured data.                                                                                        |

### Rank 7: "Local Project Shell" (Expert Mode)

| Aspect            | Detail                                                                                                                       |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| **Who**           | Developers only                                                                                                              |
| **What**          | Real terminal (PTY) in the workspace directory. Opt-in, behind "Advanced → Enable Terminal".                                 |
| **Value vs logs** | Enables git commands, npm scripts, manual fixes without leaving Tandem.                                                      |
| **Risks**         | **High security concern.** Arbitrary execution. Must be fully opt-in, clearly labeled, and potentially require confirmation. |

---

## 3. Integration Options Analysis

### Option A: GUI "Console Panel" Inside Tandem

**Description:** Add a new panel/drawer component (extending the existing `LogsDrawer` architecture) that presents a console-like UI. NOT a real shell/PTY — instead a structured console connected to OpenCode's event stream.

**Architecture:**

```
OpenCode SSE ──→ Tauri Rust ──emit→ Tauri events ──→ ConsolePanel.tsx
                                                        │
User clicks "Run" ──→ Tauri command ──→ Rust ──HTTP→ OpenCode API
                                                  (approve_tool / custom endpoint)
```

| Aspect              | Detail                                                                                                                                                                                 |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Real shell/PTY?** | No. Console UI rendering structured events. Commands go through OpenCode's tool approval pipeline.                                                                                     |
| **Safety**          | Default safe — all commands go through approval flow. No direct shell access.                                                                                                          |
| **Cross-platform**  | ✅ Pure web UI + Tauri — works on Windows/macOS/Linux identically.                                                                                                                     |
| **Complexity**      | **Small–Medium.** Extends existing `LogsDrawer` pattern. Most plumbing (SSE, Tauri events, approval) already exists.                                                                   |
| **v1 scope**        | 1. Add "Console" tab to LogsDrawer. 2. Show command proposals from `ToolStart(tool="bash"/"shell")` events. 3. "Run" button triggers `approveTool()`. 4. Stream command output inline. |

### Option B: External TUI Client (ratatui/Rust)

**Description:** A separate Rust binary using [ratatui](https://ratatui.rs/) that connects to the running OpenCode sidecar over its local HTTP API.

| Aspect             | Detail                                                                                                              |
| ------------------ | ------------------------------------------------------------------------------------------------------------------- |
| **Discovery**      | Must discover the sidecar port. Options: (a) read a lockfile written by Tandem, (b) CLI flag `--port`, (c) env var. |
| **Auth**           | Currently none — OpenCode binds to `127.0.0.1` (localhost-only). A local token could be added for defense-in-depth. |
| **Cross-platform** | ✅ ratatui works on Windows (conpty), macOS (termios), Linux.                                                       |
| **Complexity**     | **Large.** New binary, new build target, new install/update pipeline, new UX to maintain.                           |
| **Value**          | Great for SSH workflows, headless servers, power-user dev setups. Not useful for Tandem's primary non-dev audience. |
| **v1 scope**       | Separate repo. MVP: subscribe to SSE, display events, approve/deny tools via keyboard.                              |

### Option C: "Open System Terminal" Button

**Description:** Tandem spawns the user's native terminal (cmd/PowerShell on Windows, Terminal.app on macOS, xterm on Linux) and runs a pre-configured command.

| Aspect             | Detail                                                                                                   |
| ------------------ | -------------------------------------------------------------------------------------------------------- |
| **Implementation** | Tauri's `shell::open` or `Command::new("cmd")` etc. Can run: `tail -f <log_file>`, or a custom CLI tool. |
| **Cross-platform** | ⚠️ Fragile. Different terminal emulators, PATH issues, admin permissions vary wildly.                    |
| **Safety**         | ⚠️ Opens a real shell. User could run anything.                                                          |
| **Complexity**     | **Small** to open a terminal. **Medium** to make it useful and cross-platform.                           |
| **Value**          | Quick escape hatch for devs who just want a shell in the project dir. Not useful for non-devs.           |
| **v1 scope**       | Button in settings/advanced menu. Spawns terminal in workspace dir.                                      |

### Comparison Table

| Criteria                    | A: GUI Console Panel | B: External TUI (ratatui) | C: System Terminal |
| --------------------------- | :------------------: | :-----------------------: | :----------------: |
| **Non-dev friendly**        |        ✅ Yes        |           ❌ No           |       ❌ No        |
| **Security (default)**      |  ✅ Approval-gated   |    ⚠️ Needs auth layer    |   ⚠️ Full shell    |
| **Cross-platform**          |     ✅ Identical     |          ✅ Good          |     ⚠️ Fragile     |
| **Complexity**              |         S–M          |             L             |         S          |
| **Leverages existing code** |    ✅ Heavy reuse    |      ❌ New codebase      |     ❌ Minimal     |
| **Fullscreen support**      |  ✅ Already exists   |    ✅ Native terminal     |  ✅ Native window  |
| **SSH/headless use**        |        ❌ No         |          ✅ Yes           |       ❌ No        |
| **Maintenance burden**      |         Low          |           High            |        Low         |

### ✅ Recommended v1: **Option A — GUI Console Panel**

**Rationale:** It serves the primary audience (non-devs), reuses existing architecture (LogsDrawer, PermissionToast, SSE events), adds zero new binaries, and is secure by default. Option B can be explored later as an "advanced tools" add-on.

---

## 4. UX Recommendation

### Placement Options

#### Placement 1: "Console" Tab in LogsDrawer ⭐ RECOMMENDED

```
┌─────────────────────────────────────────────┐
│ Logs                              [⊞] [✕]  │
│ ┌──────────┬──────────┬──────────┐          │
│ │ Tandem   │ Sidecar  │ Console  │          │
│ └──────────┴──────────┴──────────┘          │
│                                             │
│  ▶ npm install (approved, exit 0)      3s   │
│  ▶ git status (pending approval)            │
│    [Run] [Deny] [Details ▾]                 │
│                                             │
│  ◆ Agent: "I'll run these commands to..."   │
│                                             │
│  $ npm install                              │
│  added 127 packages in 4.2s                 │
│                                             │
└─────────────────────────────────────────────┘
```

- **Why discoverable:** Users already know the Logs button. Adding a tab is zero-friction.
- **Why safe for non-devs:** It's next to familiar UI. No "Terminal" label that implies raw shell.
- **Advanced unlock:** The "Console" tab only appears when there are command events. Or it's always visible but empty state says "Commands run by the AI will appear here."
- **Fullscreen:** Already supported — `expanded` state toggles `inset-0` (fullscreen) vs side panel.

#### Placement 2: Bottom Bar near Activity Drawer

```
┌─ Chat ──────────────────────────────────────┐
│                                             │
│  ... chat messages ...                      │
│                                             │
├─────────────────────────────────────────────┤
│ 🔧 AI Activity  │  📟 Console  │  2 running │
│  > npm install ................................. ✓  │
│  > git status .................................. ⏳ │
└─────────────────────────────────────────────┘
```

- **Why discoverable:** Always visible at bottom, like VS Code's terminal panel.
- **Risk:** Competes with `ActivityDrawer` for space. Confusing to have two bottom bars.

#### Placement 3: Settings → Advanced → "Terminal (Expert)"

- **Why discoverable:** It isn't, which is the point. Hidden for expert users.
- **Risk:** Too hidden for use-case ranks 1–3. Only suitable for the real-shell use case (rank 7).

### **Recommended: Placement 1** — Console Tab in LogsDrawer

The LogsDrawer already has the right architecture (two tabs, fullscreen toggle, virtualized list). Adding a third tab `Console` is the smallest change with the highest discoverability. The `expanded` state already provides fullscreen behavior.

### Fullscreen Behavior

| Mode                         | Implementation                                                                  | Effort                         |
| ---------------------------- | ------------------------------------------------------------------------------- | ------------------------------ |
| **Fullscreen within Tandem** | Already works — `LogsDrawer` `expanded` state toggles `inset-0`                 | ✅ Zero effort                 |
| **Separate Tauri window**    | `tauri::WebviewWindowBuilder::new()` opens a second window with console content | Medium — new window management |
| **Easiest path**             | Fullscreen within Tandem (existing `expanded` toggle)                           | ✅ Recommended for v1          |

---

## 5. Security Model for v1

### Default Safe Mode (Non-Negotiable)

1. **No raw shell access.** The console is NOT a PTY. It renders structured events.
2. **All commands go through OpenCode's tool approval pipeline.** The `PermissionAsked` event triggers an inline approve/deny UI within the console tab.
3. **Command allowlist (optional upgrade).** Future: let users define patterns (e.g., "always allow `npm test`") via the existing tool policy system in [tool_policy.rs](file:///c:/Users/evang/work/tandem/src-tauri/src/tool_policy.rs).
4. **No network-exposed endpoints.** Sidecar binds to `127.0.0.1` only. No remote access.
5. **Audit trail.** Every approved/denied command is logged with timestamp and user decision.

### Expert Mode (Future, Opt-In)

If a real shell is ever added (Option C / Rank 7):

- Gated behind `Settings → Advanced → Enable Terminal`
- Requires explicit opt-in toggle (not just a button click)
- Limited to workspace directory
- Clear warning banner: "This is a real shell. Commands run directly on your computer."

---

## 6. Suggested Next Steps (Smallest Implementable Slice)

### v1 MVP: "Console" Tab in LogsDrawer

**Scope:** ~2–3 days of focused work.

1. **Add a "Console" tab** to `LogsDrawer.tsx` (alongside "Tandem" and "Sidecar").
2. **Filter `StreamEvent`s** to show only `ToolStart`/`ToolEnd` events where `tool` matches command-execution tools (e.g., `bash`, `shell`, `run_command`).
3. **Render command cards** with: proposed command, args, status (pending/running/completed/failed), and output.
4. **Inline approve/deny** buttons for `PermissionAsked` events (reusing `PermissionToast` logic but rendered inline in the console timeline instead of as a floating toast).
5. **"Copy output"** button per command block.
6. **Fullscreen** via existing `expanded` toggle.

### v1.1: Diagnostics Panel

7. Add a "Diagnostics" sub-tab showing: sidecar version, port, health status, API key status, workspace info, memory stats. Pure read-only, calling existing Tauri commands (`getSidecarStatus`, `getMemoryStats`, `pythonGetStatus`, etc.).

### v1.2: Searchable Session Transcript

8. Structured view of the entire session: messages, tool calls (with expandable args/results), file diffs. Export as markdown.

### Future: Option B (External TUI)

9. Only if there's validated demand from SSH/headless users. Separate repo, separate release cycle.

---

## 7. Risks, Unknowns, and What to Test First

### Risks

| Risk                                                                  | Severity | Mitigation                                                                      |
| --------------------------------------------------------------------- | -------- | ------------------------------------------------------------------------------- |
| Users confuse "Console" with a real terminal and try to type commands | Medium   | Label it "AI Commands" or "Run History". Empty state explains what it does.     |
| Console tab adds clutter for users who never use commands             | Low      | Tab only appears when command events exist, or always-visible with empty state. |
| Performance with many command outputs                                 | Low      | Already using virtualized list (`react-window`). Same approach.                 |
| Approval flow latency (user must click before command runs)           | Low      | Already the case with `PermissionToast`. Console just relocates the UX.         |

### Unknowns

- **Does OpenCode's `run_command` tool output stream in real-time via SSE, or only after completion?** Test by running a long command (e.g., `npm install`) and checking if `ToolEnd` arrives with progressive output or all-at-once.
- **Can we distinguish different command types** (bash vs Python vs custom MCP tools) from the `tool` field in `ToolStart`?
- **Is there demand for a separate window** (multi-monitor UX) or is fullscreen-in-app sufficient?

### What to Test First

1. **Prototype the Console tab** — even a static mock that shows hardcoded command cards, to validate UX with real users.
2. **Log a real session** and inspect the `ToolStart`/`ToolEnd` event payloads to confirm the data shape matches what the console needs.
3. **User test** the label: "Console" vs "Commands" vs "Run History" vs "AI Actions."

---

## 8. No-Go Criteria

Do **NOT** build this feature if:

1. **OpenCode's SSE stream does not expose command outputs.** The console would have no data to show.
2. **User research shows non-devs are confused** by ANY terminal-like UI, even a structured one.
3. **The approval pipeline cannot be inlined** in a different UI location (currently tied to `PermissionToast` floating position).
4. **Performance tests show** that adding a third event consumer (alongside Chat and ActivityDrawer) degrades responsiveness.
5. **Product direction shifts** away from "power user" features entirely.

---

## 9. Relevant Files Reference

| File                                                                                                         | Role                                                    |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------- |
| [sidecar.rs](file:///c:/Users/evang/work/tandem/src-tauri/src/sidecar.rs)                                    | OpenCode spawn, lifecycle, SSE stream, API calls        |
| [sidecar_manager.rs](file:///c:/Users/evang/work/tandem/src-tauri/src/sidecar_manager.rs)                    | Binary version management, downloads                    |
| [logs.rs](file:///c:/Users/evang/work/tandem/src-tauri/src/logs.rs)                                          | `LogRingBuffer`, file listing, tail                     |
| [lib.rs](file:///c:/Users/evang/work/tandem/src-tauri/src/lib.rs)                                            | App initialization, tracing, Tauri command registration |
| [tool_policy.rs](file:///c:/Users/evang/work/tandem/src-tauri/src/tool_policy.rs)                            | Tool permission policies                                |
| [tauri.ts](file:///c:/Users/evang/work/tandem/src/lib/tauri.ts)                                              | Frontend Tauri command bindings (100+ commands)         |
| [LogsDrawer.tsx](file:///c:/Users/evang/work/tandem/src/components/logs/LogsDrawer.tsx)                      | Virtualized log viewer with fullscreen                  |
| [ActivityDrawer.tsx](file:///c:/Users/evang/work/tandem/src/components/chat/ActivityDrawer.tsx)              | Bottom bar tool activity tracker                        |
| [ActivityPanel.tsx](file:///c:/Users/evang/work/tandem/src/components/chat/ActivityPanel.tsx)                | Right-panel activity view                               |
| [PermissionToast.tsx](file:///c:/Users/evang/work/tandem/src/components/permissions/PermissionToast.tsx)     | Tool approval toast with risk levels                    |
| [Chat.tsx](file:///c:/Users/evang/work/tandem/src/components/chat/Chat.tsx)                                  | Main chat component, SSE event handling                 |
| [OrchestratorPanel.tsx](file:///c:/Users/evang/work/tandem/src/components/orchestrate/OrchestratorPanel.tsx) | Multi-agent orchestrator with embedded logs             |

---

## 10. Recommendation Summary

> **Build Option A ("Console" tab in LogsDrawer) as v1.**
>
> It's the smallest, safest, most reusable path. It serves 80% of users. It requires no new binaries, no PTY, no shell access. It leverages existing SSE events + approval flow + virtualized panel architecture. Fullscreen already works. Ship in 2–3 days.
>
> Defer Option B (ratatui TUI) until there's validated headless/SSH demand. Skip Option C (system terminal) unless explicit developer-mode is prioritized.
