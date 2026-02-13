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
2.  **Equip Modules**: Drag-and-drop skills from the `tandem/crates/tandem-core/src/skills.rs` registry.
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
  - _Proposed_: Shared `MemoryBank` struct passed to all agents in a team, referencing `tandem/src-tauri/src/memory/` (to be moved to `tandem-core`).

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
