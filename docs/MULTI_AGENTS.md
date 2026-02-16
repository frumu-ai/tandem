# AI Agent Task: Research + Spec "Agent Teams / Control Center" for Tandem

## Objective

Research "team agents" UX + architecture patterns (Claude Agent Teams as primary reference) and spec out **Tandem Agent Control Center**: a unified interface (Desktop & TUI) where users can manage teams, assign roles, and observe orchestration.

**Critical Requirement**: This feature must work for **both** the Desktop app (`tandem/src`) and the TUI (`tandem/crates/tandem-tui`). This implies a significant architectural refactor to move logic out of `src-tauri` and into shared crates.

## Current State Analysis

Currently, the orchestration logic is embedded in the Tauri application:

- **Orchestrator Engine**: `tandem/src-tauri/src/orchestrator/engine.rs` contains the main loop and logic.
- **Sidecar Management**: `tandem/src-tauri/src/sidecar_manager.rs` is coupled to `tauri::AppHandle`.
- **Event Streaming**: `tandem/src-tauri/src/stream_hub.rs` relies on Tauri's event emitter.
- **Agent Definitions**: `tandem/crates/tandem-core/src/agents.rs` contains basic definitions but lacks the full runtime context.

## Codebase Reality Check (Verified)

This section reflects what exists today and what is still aspirational.

### Exists Today

- **Engine event model**: `EngineEvent` is a typed envelope with `type` and `properties` in `tandem/crates/tandem-types/src/event.rs`.
- **SSE streams**: Engine SSE endpoints exist in `tandem/crates/tandem-server/src/http.rs` (`/event` plus run-specific streaming helpers).
- **Session/run lifecycle**: Sessions + run status are persisted in `tandem/crates/tandem-core/src/storage.rs` and emitted through `tandem/crates/tandem-core/src/engine_loop.rs`.
- **Tool system**: Tool schemas + execution live in `tandem/crates/tandem-tools/src/lib.rs`, with permission gates in `tandem/crates/tandem-core/src/engine_loop.rs`.
- **Skills registry**: Skill storage/import/listing is implemented in `tandem/crates/tandem-skills/src/lib.rs` and exposed via `/skills` endpoints in `tandem/crates/tandem-server/src/http.rs`.
- **Memory store**: Vector memory uses SQLite + sqlite-vec in `tandem/crates/tandem-memory/src/db.rs` with tiers for `session`, `project`, and `global` (no team/curated tiers yet).
- **Leases**: Engine lease endpoints exist in `tandem/crates/tandem-server/src/http.rs` and are used by TUI in `tandem/crates/tandem-tui/src/net/client.rs`.
- **Multi-agent orchestration**: A full orchestrator exists in `tandem/src-tauri/src/orchestrator/*` and is Desktop-specific today.

### Not Implemented Yet (Only Proposed)

- **Shared resources/blackboard**: No `/resource` API or shared resource store exists today.
- **Routines/cron**: No scheduler or routine endpoints exist today.
- **Mission abstraction in engine**: The orchestrator is not in a shared crate and is not engine-native yet.
- **Team/curated memory tiers**: Only session/project/global are implemented.

## Control Center Dashboard Spec ("Spaceship" Aesthetic)

The UI should feel like the cockpit of a sci-fi ship, giving the user a sense of command and visibility.

### 1. The "Bridge" (Main View)

- **Visuals**: Dark mode, high contrast, "HUD" style overlays.
- **Team Roster**:
  - Display agents as "Crew Members" with status lights (Green=Idle, Yellow=Working, Red=Error).
  - "Pilot" (Leader) front and center.
  - "Specialists" (Workers) flanking the leader.
- **Mission Log (The "Matrix")**:
  - A scrolling, monospace feed of `OrchestratorEvent`s.
  - Color-coded by severity/source (e.g., Tool calls in Blue, Agent thoughts in Green, Errors in Red).
  - _Interactive_: Click an event to inspect the full JSON payload or "pause" the universe at that moment.

### 2. "Systems" Panel (Skill Assignment)

Agents are composed of two parts:

- **Directives (System Prompt)**: The "training" or "orders" given to the agent.
- **Modules (Skills/Tools)**: The "equipment" assigned to them.

**Assignment Flow**:

1.  **Select Crew Member**: Pick an agent from the roster.
2.  **Equip Modules**: Drag-and-drop skills from the Skills registry (`tandem/crates/tandem-skills/src/lib.rs`) surfaced via `/skills` endpoints.
    - _Example_: Equip "Filesystem" skill to allow reading/writing files.
    - _Example_: Equip "GitHub" skill to allow making PRs.
3.  **Set Directives**: Edit the system prompt to define their behavior (e.g., "You are a cautious security officer. Verification is your top priority.").

### 3. "Intervention" Console

- **Pause/Resume**: Big, tactile toggle.
- **Override**: Text input to send a "God Mode" message/instruction directly to the active agent, bypassing the plan.
- **Emergency Stop**: Instantly kills all active tool processes and sub-agents.

## Collaboration Model: "The Mission"

Agents don't just "chat"; they embark on a **Mission**.

1.  **Briefing**: User sets the high-level goal.
2.  **Flight Plan**: Leader agent generates a `Task` DAG (Directed Acyclic Graph).
3.  **Sortie**: Leader dispatches tasks to specific crew members based on their equipped Modules (Skills).
    - _Example_: Leader needs a file read. It checks who has the "Filesystem" module and dispatches the task to the "Engineer" agent.
4.  **Debrief**: Agents report results back to the Leader's "Mailbox".

## For Non-Developers: The "Autopilot" Experience

To make this "dead simple," we hide the graph/wiring complexity behind **Team Templates**.

### 1. The "Hiring Hall" (Template Store)

Instead of building a team, the user just "Hires" a pre-configured team for a specific job.

- **"The Startup Team"**: Product Manager (Leader) + Coder + Designer.
  - _Goal_: "Build a landing page."
- **"The Research Team"**: Lead Researcher + Browser Agent + Writer.
  - _Goal_: "Find me 5 cheap flight options to Tokyo."
- **"The Editor Team"**: Chief Editor + Grammar Geek + Fact Checker.
  - _Goal_: "Review my blog post."

### 2. "Magic Onboarding" (Natural Language Config)

When a user selects a team, they don't see JSON configs. They chat with the **Team Lead**.

- _User_: "I want a personal site."
- _Team Lead_: "Sure. Do you want a dark mode? What's your bio?"
- _System_: Automatically populates the `Directives` based on this chat.

### 3. "Outcome-First" UI

For these users, the detailed "Matrix" log is hidden. They see a simple progress bar:

- "Researching..." (30%)
- "Drafting..." (60%)
- "Polishing..." (90%)
- **DONE**: "Here is your website." [Open Folder]

## Reference: Claude Agent Teams Patterns

**Source**: `https://code.claude.com/docs/en/agent-teams`

- **Mental Model**: "Lead" agent orchestrates "Teammates".
- **Coordination**: Uses a **Shared Task List** (file-based in `~/.claude/tasks/`) which all agents watch.
- **Communication**: Asynchronous "mailbox" (messages delivered automatically).
- **Verbs**: `spawn` (create agent), `dispatch` (assign task), `wait` (synchronize).

**Tandem Adaptation**:

- We will adopt the **Shared Task List** pattern but back it with the `OrchestratorStore` (SQLite/JSON) instead of raw files for better concurrency in Rust.
- We will adopt the **Lead/Teammate** terminology.
- We will implement `spawn` and `dispatch` as tools available to the Leader agent.

## Core Architecture & Refactoring Plan (P0)

Before building the UI, we must unify the engine.

1.  **Extract Orchestrator**: Move `src-tauri/src/orchestrator` to `tandem/crates/tandem-orchestrator` (or `tandem-core`).
    - _Reference_: `OrchestratorEngine` struct in `engine.rs` needs to be generic over the event bus.
2.  **Abstract Dependencies**:
    - Create `SidecarProvider` trait in `tandem-core` to abstract `SidecarManager`.
    - Create `EventBus` trait to abstract `StreamHub` and Tauri events.
3.  **Implement Adapters**:
    - `TauriSidecarProvider` (in `src-tauri`): Existing logic wrapping the Tauri sidecar.
    - `HeadlessSidecarProvider` (in `tandem-tui`): New implementation for TUI/Text-only modes to spawn processes directly (or connect to a daemon).

## Deliverables

### 1. Refactoring Plan (`tandem/docs/agent-teams/refactor-plan.md`)

- Detailed steps to move `OrchestratorEngine` to a shared crate.
- Definition of the `SidecarProvider` and `EventBus` traits.
- Strategy for shared state management (Rust `RwLock` vs DB).

### 2. Control Center Spec (`tandem/docs/agent-teams/control-center-spec.md`)

- **Data Model**:
  - **Team**: Collection of Agents + Shared Context (Files/Memory).
  - **Member**: Reference to an Agent (`tandem-core/src/agents.rs`) + Role (Leader/Worker).
  - **Mission**: A high-level goal that instantiates a `Run` (ID, Budget, Status).
- **UX Flows (Desktop & TUI)**:
  - _Creation_: "New Team" -> Select Agents -> Assign Roles.
  - _Execution_: "New Mission" -> Team Lead plans -> Workers execute.
  - _Observation_: Live view of the `OrchestratorEvent` stream.

### 3. TUI Implementation Plan

- Reference `tandem/crates/tandem-tui/src/ui` modules.
- New `TeamTab` in TUI.
- Log stream view for active missions.

## Hard Constraints

- **Local-First**: All state in `.tandem/` or `app_data`.
- **Headless Capable**: The engine must run without a GUI (for TUI/CLI).
- **No User Keys Required**: Use the existing Sidecar for inference.

## Research Questions to Answer

### Core Mental Model

- **Team vs Session**: Is a Team valid only for a session, or is it a persistent configuration?
  - _Proposed Decision_: Persistent Configuration stored in `.tandem/teams.json`.
- **Leader Agent**: Does the Leader reuse the existing `plan` agent prompt, or a new "Manager" prompt?
  - _Proposed Decision_: Extend `plan` agent with "Manager" capabilities (delegation tools).
- **Communication**: How do agents share context?
  - _Current_: Shared `file_context` in `OrchestratorEngine::get_task_file_context`.
  - _Proposed_: Shared `MemoryBank` struct passed to all agents in a team, referencing the new `crates/tandem-memory` crate (shared "Brain").

### UX + Workflows

1.  **Assignments**: User assigns high-level goal to Leader. Leader breaks it down (Planner Phase) and assigns to Workers (Executor Phase).
2.  **Intervention**: User can pause and edit the plan (Tasks) in `tandem/src-tauri/src/orchestrator/scheduler.rs` via the UI.

## Engine API & Events (Shared)

New Endpoints (exposed via Tauri Cmds AND logic in TUI):

- `create_team(name, members)`
- `start_mission(team_id, goal)`
- `get_mission_status(run_id)`

Events (SSE/Channel):

- `TeamStateChanged`
- `MissionProgress` (Task constraints)
- `AgentAction` (Tool call)

## SUPER_ORCHESTRATOR Merge (Filtered for Current Reality)

The following ideas are compatible with the repo but are not implemented yet. They should be treated as a roadmap, not current behavior.

### Engine vs UI Boundary (Proposed)

- **Engine owns**: durable state (missions/work items/approvals), orchestration runtime, capability enforcement, SSE streams, skills + MCP + provider routing, audit events.
- **Desktop/TUI owns**: command center UX, approval workflows, filters/search, device integrations.

### Orchestrator Abstraction (Proposed)

- Reducer model: `init(spec) -> state` and `on_event(state, event) -> Vec<Command>`.
- Event types should mirror existing `EngineEvent` envelopes and reuse `sessionID/runID` naming.
- Commands should map to existing engine APIs (start run, call tool, request approval), not invent new side channels.

### Shared Resources / Blackboard (Not Yet Built)

- No `/resource` API exists today; would need new server endpoints and a durable store.
- Existing leases in `/global/lease/*` can be extended for locks/TTL if a resource store is added.

### Memory Tiers (Partially Implemented)

- Implemented: `session`, `project`, `global` in `tandem-memory`.
- Proposed: `team` and `curated` tiers, with explicit promotion and review gates.

### Routines / Cron (Not Yet Built)

- No scheduler or routine endpoints exist today; would require new state, persistence, and event emission.

## new ideas

# Mission: Tandem Autonomous Missions + Orchestrator + Shared Resources + Memory + Routines (Cron) + Packaging

You are a senior Rust systems architect working inside the Tandem monorepo. Your job is to propose a concrete, implementation-ready architecture for **autonomous missions** (long-living agent workflows) that are:

- **engine-first** (headless, reusable by other developers)
- **multi-client** (Desktop + TUI + optional future web)
- **watchable** (command & control center UX backed by SSE/WS streams)
- **safe** (capabilities, approvals, auditability)
- **durable** (survives restarts, replayable)
- **team/LAN ready** (multi-tenant guardrails)

You must base your proposal on the actual code and existing patterns in this repo (event bus, SSE, tools runtime, memory, leases, MCP, etc). Prefer minimal changes that reuse existing primitives.

---

## 0) Constraints / Non-goals

- Do NOT redesign the entire engine. Extend existing systems.
- Do NOT put “truth” into vector memory. Vector store is for semantic recall only.
- The engine must remain usable headlessly, and frontends should remain clients.
- Assume the engine may run on a LAN server used by multiple team members.

---

## 1) First: Repo reconnaissance (required)

Scan the repo to locate and summarize:

- Engine event types / SSE streaming payload shape (e.g., EngineEvent)
- Session + run lifecycle endpoints
- Existing “leases” mechanism (if any)
- Tool system + tool schemas + any existing policy/permission model
- Memory / vector store APIs (e.g., memory_search) and storage tiers
- Skills registry (import/export)
- MCP registry/connect code paths
- TUI integration points (if it already talks to engine via HTTP/SSE)

Output: a short “Current State Map” section referencing file paths.

---

## 2) Deliverable A: Engine vs UI boundary (platform split)

Produce a definitive boundary table and rationale.

### Engine MUST own (platform)

- durable state: missions, work items, approvals, shared resources
- execution: tool runtime + agent runs + orchestration runtime
- enforcement: capabilities + policy + sandbox boundaries
- streaming: typed event streams for runs + missions + resources
- extensibility: skills + MCP + provider routing
- auditability: who did what, from where, when

### Desktop/TUI MUST own (experience)

- Command & Control Center UX (kanban view, swarm view, timeline, diff viewer)
- Approve/reject UI, filters, search, layouts, hotkeys, onboarding
- device integrations (notifications, clipboard, file pickers)

Include “litmus tests” for future decisions.

Output: `docs/design/ENGINE_VS_UI.md`

---

## 3) Deliverable B: Orchestrator abstraction (handles ANY mission type)

Design the orchestrator as an **Event → State → Command** reducer loop (deterministic, replayable).

### Requirements

- Orchestrator does not “do” work directly.
- Orchestrator consumes events, updates mission state, emits commands the engine executes.
- Must support multiple mission styles (kanban, research, coding, ops routines).

Define:

- `MissionSpec` (JSON/struct): goal, success criteria, budgets, capabilities, entrypoint
- `MissionState` (durable)
- `WorkItem` (generic task node; kanban is just a derived view)
- `Event` types (mission/run/tool/approval/timer/resource events)
- `Command` types (start run, call tool, request approval, schedule timer, persist artifact, emit notice)

Provide a minimal trait interface:

- `init(spec) -> state`
- `on_event(state, event) -> Vec<Command>`

Output: `docs/design/ORCHESTRATOR.md`

---

## 4) Deliverable C: The “Pal + Nerd → Board → Specialists → Reviewer → Tester” default orchestrator

Design the default mission workflow as an orchestrator implementation, but keep it configurable.

### Planning stage

Two agents:

- Pal (PM/coach): clarify intent, define done, risk, milestones
- Nerd (architect): file mapping, constraints, task breakdown

Planning outputs structured JSON which becomes WorkItems.

### Execution stage

- Assign WorkItems to specialist agents by skills tags.
- Parallel execution supported (swarm), but bounded by concurrency + budgets.
- Every WorkItem binds to a run_id and produces artifacts.

### Gates

- Reviewer must approve diffs/patches before applying.
- Tester must run checks before marking done.

Output: `docs/design/DEFAULT_MISSION_FLOW.md`

---

## 5) Deliverable D: Shared Resources (“blackboard”) for multi-agent coordination

Design a shared resource subsystem with:

- namespaces: `run/*`, `mission/*`, `project/*`, `team/*`
- resource types: `status`, `board`, `artifacts`, `notes`, `decisions`, `locks`
- revisioning (optimistic concurrency): `rev`, `if_match_rev`
- watchability: SSE stream for resource events by prefix
- leases/locks with TTL (avoid conflicting edits / card assignment collisions)

Key rule:

- Agent status should be mostly **engine-derived** from run/tool events (not self-reported).

Define minimal API surface (HTTP):

- `GET /resource?prefix=...`
- `GET /resource/{key}`
- `PUT/PATCH /resource/{key}` (with rev constraints)
- `GET /resource/events?prefix=...` (SSE)

Output: `docs/design/SHARED_RESOURCES.md`

---

## 6) Deliverable E: Vector store tiers for “learning over time” (local + team LAN guardrails)

Design memory tiers:

- `session` (ephemeral, default, no leakage)
- `project` (persistent per repo/workspace)
- `team` (shared across projects on a LAN server; opt-in + guardrails)
- `curated` (admin/reviewed; safe to auto-use)

Rules:

- KV/resources/event log = truth layer (deterministic)
- vector store = knowledge layer (semantic recall)
- promotion pipeline: session → project/team/curated must be explicit, scrubbed, reviewed
- strict partitioning by `{org_id, workspace_id, project_id, tier}`
- capability tokens restrict which tiers can read/write/promote

Define:

- what to store: “solution capsules” (small structured summaries + artifact refs)
- what NOT to store: secrets, live status, raw sensitive logs
- scrubber requirements + audit logging

Output: `docs/design/MEMORY_TIERS.md`

---

## 7) Deliverable F: Internal cron (“Routines”) for long-living agents

Design an engine-native scheduler for routines:

- persisted RoutineSpec: cron/interval, timezone, misfire policy, caps, entrypoint, args
- next_fire_at computed and stored durably
- on fire: create a MissionRun (or Run) with caps + budgets
- leases to prevent double execution (future multi-instance / team LAN)
- routine events emitted into event bus / SSE

Define:

- misfire policies: skip / run_once / catch_up(n)
- API:
  - `POST /routines`
  - `GET /routines`
  - `PATCH /routines/{id}`
  - `POST /routines/{id}/run_now`
  - `GET /routines/events` (SSE)
  - `GET /routines/{id}/history`

Output: `docs/design/ROUTINES_CRON.md`

---

## 8) Packaging: ecosystem-friendly distribution (NPM + Cargo)

Summarize a recommended distribution plan:

- NPM scoped packages: `@frumu/tandem`, `@frumu/tandem-tui`, plus optional per-OS binary packages (optionalDependencies pattern).
- Cargo: crates.io has no scopes; propose naming scheme that avoids `tandem` collision but stays coherent (e.g., `tandem-engine`, `tandem-client`, or `frumu-tandem-*` if needed).
- Ensure: other devs can build their own clients by targeting stable HTTP/SSE event contracts.

Output: `docs/design/PACKAGING.md`

---

## 9) Implementation plan (phased)

Provide a practical phased plan aligned to minimal risk:

- Phase 1: shared resources store + SSE + status indexer
- Phase 2: orchestrator abstraction + default mission flow producing a board
- Phase 3: approvals/test gates + artifact linking
- Phase 4: memory tier promotion + team guardrails
- Phase 5: routines scheduler (cron) + misfire + leases
- Phase 6: polish + SDK ergonomics

Include:

- acceptance criteria per phase
- what endpoints/events are “stabilized” for third-party builders
- minimal schema snapshot tests (avoid tool schema regressions)

Output: `docs/design/IMPLEMENTATION_PLAN.md`

---

## Output requirements

- Write the docs to `tandem/docs/design/` (create folder if needed).
- Be concrete: include JSON schemas or Rust struct sketches where helpful.
- Reference real files/paths found in the repo during reconnaissance.
- Prefer reusing existing naming conventions (sessionID/runID/etc).
- Keep the proposal compatible with current Desktop + TUI clients.

End with a short “Decisions summary” list: 10 bullets max.
