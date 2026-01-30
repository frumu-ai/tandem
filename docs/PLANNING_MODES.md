# Tandem Planning Modes Architecture

## Executive Summary

Tandem implements a **Plan-First** coding workflow designed for safety and clarity. The architecture ensures that AI-proposed changes are visually planned, strictly gated, and deterministic in execution.

**Core Principles:**

1.  **Visual Planning**: The AI writes a detailed Markdown plan to a file in `.opencode/plans/`.
2.  **Single Source of Truth**: The plan file is updated **in-place** (no clutter of `r001`, `r002` files).
3.  **Dynamic Tool Injection**: Planning capabilities are injected into OpenCode via `.opencode/agents/` on project load.
4.  **Strict Gating**: Execution is blocked until the user approves a specific hash of the (Markdown Content + Staged Operations).

---

## 1. Architecture Overview

### 1.1 State Machine

Tandem manages the planning lifecycle via a simplified state machine:

```
┌─────────┐    START      ┌──────────┐    PLAN_READY    ┌────────────┐
│  IDLE   │ ────────────▶ │ DRAFTING │ ───────────────▶ │ AWAITING_  │
│         │               │          │                  │ APPROVAL   │
└─────────┘               └──────────┘                  └────────────┘
     ▲                         ▲                              │
     │                         │                              │ REVISE
     │                         │ REQUEST_REVISION             │
     │                         │ (Update Plan File)           │
     │                         └──────────────────────────────┤
     │                                                        │
     │      ┌───────────┐     PLAN_EXECUTED     ┌───────────┐ │ BUILD (Hash Match)
     └──────│ COMPLETED │ ◀─────────────────────│ EXECUTING │◀┘
            │           │                       │           │
            └───────────┘                       └───────────┘
```

| State                 | Role                                | Input Gating                                    |
| --------------------- | ----------------------------------- | ----------------------------------------------- |
| **IDLE**              | No active plan.                     | -                                               |
| **DRAFTING**          | AI is writing/editing `PLAN_*.md`.  | **Auto-Approved**: Writes to `.opencode/plans/` |
| **AWAITING_APPROVAL** | Plan is stable. User reviews diffs. | **Blocked**: Writes to source code              |
| **EXECUTING**         | Applying approved operations.       | **Strict**: Only runs if hash matches approval  |
| **COMPLETED**         | Operations finished.                | -                                               |

---

## 2. Filesystem Schema

Tandem uses a hybrid persisted/dynamic filesystem approach:

```
.opencode/
├── agents/
│   └── plan.md                       # Tandem-managed agent definition
├── plans/                            # Plans organized by session
│   ├── auth-implementation/
│   │   ├── PLAN_add_jwt.md
│   │   └── PLAN_add_middleware.md
│   └── database-refactor/
│       └── PLAN_migrate_schema.md
└── tandem/
    └── plans/
        └── {plan_id}/
            ├── approvals.json        # Hash binding records
            ├── taskmap.json          # Linked todos
            └── backups/              # Internal snapshots (for undo)
```

### 2.1 Plan Organization

Plans are organized hierarchically by session for easy management:

**Session Folders**:

- Created automatically when the AI starts a new planning session
- Named with kebab-case based on the overall goal (e.g., `auth-implementation`, `payment-integration`)
- Can be deleted entirely when the work is complete

**Plan Files**:

- Format: `.opencode/plans/{session-name}/PLAN_{descriptive-name}.md`
- Examples: `auth-implementation/PLAN_add_jwt.md`, `refactor/PLAN_split_routes.md`
- Multiple plans can exist within the same session folder

**Benefits**:

- ✅ Easy cleanup (delete session folder when done)
- ✅ No clutter (prevents thousands of files in one directory)
- ✅ Context grouping (related plans stay together)

### 2.2 The Visual Plan (`.opencode/plans/{session}/PLAN_*.md`)

This file is the **Single Source of Truth** for the user.

- **Updates**: The AI updates this file in-place using standard `write_file` tools.
- **Visibility**: Tandem's "Plan Panel" watches this file and renders it in real-time.
- **Auto-Approval**: The `ToolProxy` is configured to automatically allow writes to `.opencode/plans/*.md` without intercepting them, creating a fluid "Cursor-like" experience.

---

## 3. Dynamic Agent Injection

To avoid needing app restarts or global plugin installs, Tandem uses OpenCode's workspace scanning capability.

### 3.1 Injection Logic (Rust)

On `set_active_project`:

1.  Check for `.opencode/agents/plan.md`.
2.  If missing or outdated (hash check), update it with the built-in Plan Agent definition.

### 3.2 Plan Agent Definition

```yaml
---
name: Plan
description: AI Architect for planning and reviewing code changes.
model: anthropic/claude-3-5-sonnet
tools:
  - name: todo
  - name: read_file
  - name: search_codebase
system: >
  You are the Planning Agent.
  RULES:
  1. WRITE YOUR PLAN to `.opencode/plans/PLAN_{session_id}.md`.
  2. DO NOT EDIT CODE yet. You are in PLANNING mode.
---
```

---

## 4. Execution Gating & Safety

### 4.1 Approval Hash

Execution safety relies on a **Content+Operation Hash**. When a user approves a plan:

```rust
struct ApprovalHash {
    plan_id: String,
    content_hash: String,     // SHA-256 of PLAN_*.md content
    operations_hash: String,  // SHA-256 of staged operation IDs list
    approved_at: DateTime<Utc>,
}
```

### 4.2 The "Build" Action

When the user clicks "Build" (Execute):

1.  Tandem re-computes the current hash of the plan file and staged operations.
2.  If it matches the `ApprovalHash`, execution proceeds.
3.  If it differs (drift detected), execution is rejected, and the user must re-approve.

---

## 5. UI Components

### 5.1 Plan Selector (Hamburger)

Located in the chat header, allowing users to switch contexts.

- **Action**: When switching plans, Tandem sends a **System Message Override** to OpenCode:
  > `[SYSTEM]: User switched context to Plan: "Refactor Auth". Current file: .opencode/plans/PLAN_auth.md. Please ignore previous context.`

### 5.2 Plan Visualization

- **Markdown Viewer**: Renders the active plan file.
- **Todo List**: Renders tasks captured from the `todo` tool, linked to the plan via `taskmap.json`.

---

## 6. Implementation Roadmap

### Phase 1: Foundation (Days 1-3)

- [ ] Implement `sync_plan_agent()` in Rust.
- [ ] Update `ToolProxy` to auto-approve `.opencode/plans/*.md`.
- [ ] Watch file changes in `.opencode/plans/` for the UI.

### Phase 2: Plan Logic (Days 4-5)

- [ ] Implement `TaskMapper` to capture `todo` tool calls.
- [ ] Build the **Plan Selector** and system message injection logic.

### Phase 3: Safety & Final Polish (Days 6-8)

- [ ] Implement `ApprovalEnforcer` and hash logic.
- [ ] Build the "Execution Plan" Staging Panel.
