# Tandem Multi-Agent Orchestration Mode v1

## Overview

This document defines the Multi-Agent Orchestration Mode for Tandem—a NEW mode that coordinates specialized sub-agents (Planner, Builder, Validator, Researcher) to accomplish complex objectives through a DAG-based task execution system with strict security and cost controls.

> **Mode Relationship**:
>
> - **Plan Mode** → Single-agent planning with approval gating
> - **Ralph Loop** → Iterative single-agent execution until completion
> - **Orchestrate Mode** → Multi-agent coordination with task DAG, budgets, and validation

Ralph loop is already implemented. Orchestration can optionally use Ralph internally for "retry until verified" as a fix phase.

**Model Note**: Model choice is end-user controlled in Tandem. Do NOT hardcode or recommend specific models. Enforce budgets and safety regardless of which model a user selects.

---

## References (Patterns, Not Code)

1. **Modern agentic coding tools** — Looping + state directory concepts
   https://github.com/anthropics/claude-code/tree/main/plugins/ralph-wiggum

2. **Open Ralph Wiggum (TypeScript)** — Loop controller patterns + persisted run state
   https://github.com/Th0rgal/open-ralph-wiggum

3. **OpenCode Primitives** — Agents, tools, permissions, plugins, server/SSE
   - https://opencode.ai/docs/agents/
   - https://opencode.ai/docs/tools/
   - https://opencode.ai/docs/permissions/

4. **opencode-orchestrator** — Orchestration UX inspiration (no dependency)
   https://github.com/agnusdei1207/opencode-orchestrator

---

## Goals

Implement a v1 Multi-Agent Orchestration feature that:

- **Planning Stage**: Creates and manages a task list (with dependencies) for a user objective
- **Execution Stage**: Dispatches runnable tasks to sub-agents concurrently (DAG-aware) with configurable concurrency limits
- **Real-time Progress**: Task board + logs + budget meter in UI
- **Security**: Strict file/tool gating with tiered approvals
- **Cost Control**: Hard token/time/iteration budgets with clean stop
- **Artifacts**: Deterministic run folders for debugging and resumption

---

## Security (Non-Negotiable)

### Tool/Action Tiering

Implemented in Rust-side policy engine (`orchestrator/policy.rs`):

| Tier                   | Actions                                     | Default Behavior                             |
| ---------------------- | ------------------------------------------- | -------------------------------------------- |
| **Tier 0 (Safe)**      | `read_file`, `search`, `list`, `diff`       | Auto-allow                                   |
| **Tier 1 (Write)**     | `write_file`, `apply_patch` in workspace    | Require approval (per-run or per-action)     |
| **Tier 2 (Dangerous)** | `shell`, `install`, `web_fetch`, `git_push` | Blocked by default; explicit enable required |

### File Access Scope

- Read/write only inside workspace
- Deny access to paths outside project root

### Secret Redaction

- Never include `.env`, keys, tokens, credential files in model context
- Redact common patterns (API keys, tokens) in logs/prompts

### Approval UX

- Show exact action: command, path, or diff preview
- Show reason from agent
- Options: "Approve once" | "Approve for run" | "Deny"

---

## Cost Control (Hard Limits)

Enforced by `orchestrator/budget.rs` independent of model selection:

| Limit                 | Default | Notes                             |
| --------------------- | ------- | --------------------------------- |
| `max_iterations`      | 200     | Total planning + execution cycles |
| `max_total_tokens`    | 400k    | Estimated if metering unavailable |
| `max_tokens_per_step` | 60k     | Per sub-agent call                |
| `max_wall_time`       | 60 min  | Hard time limit                   |
| `max_subagent_runs`   | 500     | Total agent invocations           |
| `max_web_sources`     | 30      | When research enabled             |
| `max_task_retries`    | 3       | Per-task retry before fail-block  |

**Behavior**: Stop BEFORE exceeding cap. Persist partial artifacts + clear "budget exceeded" status.

**UI Display**: Show "estimated spend" vs "measured spend" with color-coded progress bars.

---

## Concurrency & Coordination

### Global and Resource Limits

| Limit                | Default | Notes                                                      |
| -------------------- | ------- | ---------------------------------------------------------- |
| `max_parallel_tasks` | 4       | Maximum number of runnable tasks executed concurrently     |
| `llm_parallel`       | 3       | Maximum concurrent sub-agent generations                   |
| `fs_write_parallel`  | 1       | Safe default; combine with per-path locking for file tools |
| `shell_parallel`     | 1       | Safe default for shell-like tools                          |
| `network_parallel`   | 2       | Optional limit for network tools                           |

### Workspace Safety

- Prefer parallel planning/analysis, then coordinate any filesystem mutations at the approval/apply boundary.
- Use per-path locking for file write/delete tool approvals to prevent concurrent writes to the same file.

---

## Deterministic Artifacts

Every run produces a folder at `.tandem/orchestrator/<run_id>/`:

```
.tandem/orchestrator/<run_id>/
├── tasks.json           # Current task DAG
├── events.log           # Append-only event log
├── budget.json          # Spent/remaining budgets
├── latest_summary.md    # Human-readable summary
└── artifacts/           # Sub-agent outputs
    └── <task_id>/
        ├── patch.diff
        └── notes.md
```

**Artifact-First Communication**: Sub-agents write to `artifacts/<task_id>/`; orchestrator reads these to form next prompt (minimizes tokens).

---

## UI Requirements

### Mode Placement

"Orchestrate" appears as a top-level mode alongside Ask/Plan in the bottom bar.

### Components

| Component           | Description                                    |
| ------------------- | ---------------------------------------------- |
| **Objective Input** | Text field for user's goal                     |
| **Config Panel**    | Budget limits, toggle settings                 |
| **Task Board**      | Kanban: Pending / In Progress / Blocked / Done |
| **Budget Meter**    | Progress bars for tokens, time, iterations     |
| **Event Log**       | Live log + last agent output                   |
| **Controls**        | Start / Pause / Cancel / Resume                |

### Default Settings

- "Require approval before writing" → **ON**
- "Enable research (web)" → **OFF**
- `max_parallel_tasks` → **4**

---

## Backend Architecture (Rust)

Create module `src-tauri/src/orchestrator/` with:

| File           | Purpose                                         |
| -------------- | ----------------------------------------------- |
| `mod.rs`       | Module entry point                              |
| `types.rs`     | `Run`, `Task`, `Budget`, `PolicyDecision`, etc. |
| `policy.rs`    | Tool/file gating + secret redaction             |
| `budget.rs`    | Cost tracking and enforcement                   |
| `scheduler.rs` | DAG scheduler (select next runnable task)       |
| `agents.rs`    | Sub-agent prompt templates                      |
| `engine.rs`    | Main orchestration loop                         |
| `store.rs`     | Persistence (JSON + append-only log)            |

### Tauri Commands

```rust
orchestrator_create_run(objective, config) -> run_id
orchestrator_start(run_id)
orchestrator_pause(run_id)
orchestrator_cancel(run_id)
orchestrator_get_run(run_id) -> RunSnapshot
orchestrator_list_tasks(run_id) -> Vec<Task>
orchestrator_approve(run_id, approval_token, scope)
orchestrator_stream_events(run_id) // SSE via events
```

### Integration with Existing Systems

| Component          | Reuse Strategy                             |
| ------------------ | ------------------------------------------ |
| `SidecarManager`   | Sub-agent calls via existing sidecar       |
| `ToolProxy`        | Approval flow reuses permission staging    |
| `RalphLoopManager` | Optional "fix loop" for failed validations |

---

## Sub-Agent Roles (v1)

Implemented as prompt templates in `agents.rs`, not hard-coded logic.

### 1. Planner

- **Input**: Objective + workspace summary + constraints
- **Output**: `tasks.json` array with deps + acceptance criteria
- **Max tasks**: 12
- **Style**: Concise, no essays

### 2. Builder

- **Input**: One task + only necessary file context
- **Output**: `patch.diff` + short notes + verification hint
- **Constraints**: No dangerous tools unless approved

### 3. Validator

- **Input**: Diff + logs + acceptance criteria
- **Output**: PASS/FAIL + required fixes
- **On FAIL**: Propose fix tasks or trigger Ralph fix loop

### 4. Researcher (Optional, OFF by default)

- **Input**: Research question + constraints
- **Output**: `sources.json` + `fact_cards.md` with citations
- **Constraints**: Obey `max_sources`, dedupe, block prohibited domains

---

## Orchestration Algorithm

### Run Lifecycle

```
┌─────────────┐    CREATE     ┌─────────────┐    PLANNER    ┌─────────────┐
│    IDLE     │ ───────────▶  │  PLANNING   │ ───────────▶  │  AWAITING   │
│             │               │             │               │  APPROVAL   │
└─────────────┘               └─────────────┘               └─────────────┘
                                                                   │
                  ┌────────────────────────────────────────────────┤
                  │                                                │
                  ▼                                                ▼ APPROVE
            ┌─────────────┐                                  ┌─────────────┐
            │  REVISION   │ ◀──────────────────────────────  │  EXECUTING  │
            │  REQUESTED  │     REVISE                       │             │
            └─────────────┘                                  └─────────────┘
                  │                                                │
                  │ PLANNER                                        │
                  └─────────────────────▶ AWAITING_APPROVAL ◀──────┘
                                                                   │
                                         COMPLETE/BUDGET           │
                                               ▼                   │
                                         ┌─────────────┐           │
                                         │  COMPLETED  │ ◀─────────┘
                                         │  / FAILED   │
                                         └─────────────┘
```

### Execution Loop

1. **Create** run folder + initialize budgets
2. **Planner** generates task DAG → store `tasks.json`
3. **User reviews** and approves or requests revision
4. **Loop** until done/cancel/budget:
   a. Scheduler selects next runnable task (deps done, not blocked)
   b. Dispatch Builder with minimal context
   c. Collect artifacts (patch + notes); apply patch if approved
   d. Dispatch Validator to confirm acceptance criteria
   e. Update task state; record events + budget deltas
5. **Completion** when all tasks done AND validator says PASS
6. **Persist** final summary + artifacts

### Ralph Fix Loop Integration

If Validator returns FAIL on same task N times (`max_task_retries`), trigger mini Ralph-style fix loop:

- `max_fix_iterations`: 3
- Separate micro-budget
- Return to orchestration loop on success

---

## Context Minimization (Critical)

Do NOT give agents entire repo context. Instead provide:

- File manifest (top N relevant paths)
- Only files touched by task or selected by heuristics (grep on symbols)
- Diffs since last step
- Validator failure output only

This prevents token blowups.

---

## Frontend Components

Create `src/components/orchestrate/`:

| Component                   | Purpose                      |
| --------------------------- | ---------------------------- |
| `OrchestrateMode.tsx`       | Main container with controls |
| `TaskBoard.tsx`             | Kanban task visualization    |
| `BudgetMeter.tsx`           | Real-time budget display     |
| `OrchestrateApproval.tsx`   | Approval request panel       |
| `OrchestratePlanReview.tsx` | Plan review before execution |
| `types.ts`                  | TypeScript type definitions  |
| `index.ts`                  | Module exports               |

---

## Acceptance Criteria

- [ ] Orchestrate mode appears in mode selector
- [ ] Works end-to-end on demo: "Add a small feature + test; pass tests"
- [ ] Budget caps stop run cleanly and persist state
- [ ] Policy blocks unsafe actions by default
- [ ] Writes require approval (default ON)
- [ ] Secrets not included in prompts/logs
- [ ] UI shows task status + live events + spend
- [ ] Planning stage requires user approval before execution

---

## Deliverables

1. **Implementation**: Rust module + Tauri commands + React components
2. **Developer doc**: `docs/orchestrate-v1.md` explaining:
   - Run lifecycle
   - Budgets + approvals
   - How to add new agent role
   - How to add templates for non-dev users
3. **Starter templates**:
   - "Build: implement feature + tests"
   - "Write: landing page copy"
   - "Research: market snapshot" (research toggle required)

---

## Implementation Phases

### Phase 1: Foundation (3-4 days)

- [ ] Create `orchestrator/` module structure
- [ ] Implement types and policy engine
- [ ] Implement budget tracking
- [ ] Add basic Tauri commands

### Phase 2: Core Logic (4-5 days)

- [ ] Implement scheduler with DAG resolution
- [ ] Implement agent prompt templates
- [ ] Build main orchestration engine
- [ ] Add persistence layer

### Phase 3: UI Integration (3-4 days)

- [ ] Add mode selector entry
- [ ] Build TaskBoard component
- [ ] Build BudgetMeter component
- [ ] Wire approval flow

### Phase 4: Polish & Testing (2-3 days)

- [ ] Write unit tests for policy/budget/scheduler
- [ ] Integration testing
- [ ] Documentation
- [ ] Edge case handling

---

## Working Style

- Prefer smallest safe implementation
- Avoid external orchestrator dependencies/binaries
- Add tests for policy/budget/scheduler
- Keep defaults conservative (safe + cheap)
- Ensure deterministic persisted artifacts for debugging
