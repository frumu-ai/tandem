# Tandem Planning Modes Architecture

## Executive Summary

Tandem implements a sophisticated planning system that enables users to plan AI operations safely before execution. The architecture uses a **state machine pattern** (consistent with LangGraph, LangChain, and industry best practices) to manage the lifecycle of plans from creation through execution.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           PLAN MODE STATE MACHINE                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│    ┌─────────┐     ┌───────────┐     ┌──────────────┐     ┌──────────┐    │
│    │  IDLE   │────▶│ DRAFTING  │────▶│ AWAITING_    │────▶│ EXECUTING│    │
│    │         │     │           │     │ APPROVAL     │     │          │    │
│    └─────────┘     └───────────┘     └──────────────┘     └──────────┘    │
│         ▲              │                    │                    │          │
│         │              │                    │                    │          │
│         │              ▼                    ▼                    ▼          │
│         │        ┌───────────┐     ┌──────────────┐     ┌──────────┐      │
│         │        │ CANCELLED │     │ REVIEWING    │────▶│ COMPLETED│      │
│         │        │           │     │              │     │          │      │
│         │        └───────────┘     └──────────────┘     └──────────┘      │
│         │                                                                   │
│         │              ┌─────────────────────────────────┐                 │
│         └──────────────│ REVISION_LOOP (DRAFTING ↔ REVIEW)│◀────────────┘ │
│                        └─────────────────────────────────┘                 │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## State Machine Definition

### States

| State               | Description                                         | Entry Actions           | Exit Actions                |
| ------------------- | --------------------------------------------------- | ----------------------- | --------------------------- |
| `IDLE`              | No active plan, user can start new planning session | Clear staging area      | Initialize plan context     |
| `DRAFTING`          | AI is creating/iterating on a plan                  | Create plan document    | Finalize plan for review    |
| `AWAITING_APPROVAL` | Plan is ready, waiting for user review              | Show ExecutionPlanPanel | User made a decision        |
| `REVIEWING`         | User is actively reviewing diffs and operations     | Enable diff viewer      | User clicked execute/reject |
| `EXECUTING`         | Operations are being applied to filesystem          | Set executing flag      | Clear staging on completion |
| `COMPLETED`         | All operations executed successfully                | Show success state      | Reset to IDLE               |
| `CANCELLED`         | Plan was rejected or abandoned                      | Clear staging area      | Reset to IDLE               |

### Valid Transitions

```typescript
// src/machines/planStateMachine.ts

export type PlanState =
  | "IDLE"
  | "DRAFTING"
  | "AWAITING_APPROVAL"
  | "REVIEWING"
  | "EXECUTING"
  | "COMPLETED"
  | "CANCELLED";

export type PlanEvent =
  | { type: "START_PLANNING"; task: string }
  | { type: "PLAN_CREATED"; planId: string }
  | { type: "PLAN_UPDATED"; revision: number }
  | { type: "FINISH_DRAFTING" }
  | { type: "OPERATION_STAGED"; operation: StagedOperation }
  | { type: "USER_REVIEWING" }
  | { type: "EXECUTE_PLAN" }
  | { type: "PLAN_EXECUTED" }
  | { type: "REJECT_PLAN" }
  | { type: "REQUEST_REVISION"; feedback: string }
  | { type: "CANCEL" };

export const planStateMachine: StateMachine<PlanState, PlanEvent, PlanContext> = {
  initial: "IDLE",

  context: {
    planId: undefined,
    revision: 0,
    operations: [],
    executionResults: [],
  },

  states: {
    IDLE: {
      on: {
        START_PLANNING: {
          target: "DRAFTING",
          actions: ["initializePlan", "setTask"],
        },
      },
    },

    DRAFTING: {
      on: {
        PLAN_CREATED: {
          target: "AWAITING_APPROVAL",
          actions: ["setPlanId", "incrementRevision"],
        },
        PLAN_UPDATED: {
          target: "DRAFTING",
          actions: ["updatePlan", "incrementRevision"],
        },
        FINISH_DRAFTING: {
          target: "AWAITING_APPROVAL",
          actions: ["finalizePlan"],
        },
        CANCEL: {
          target: "CANCELLED",
          actions: ["clearPlan"],
        },
      },
    },

    AWAITING_APPROVAL: {
      on: {
        OPERATION_STAGED: {
          target: "REVIEWING",
          actions: ["addOperation"],
        },
        USER_REVIEWING: "REVIEWING",
        EXECUTE_PLAN: {
          target: "EXECUTING",
          actions: ["setExecuting"],
        },
        REQUEST_REVISION: {
          target: "DRAFTING",
          actions: ["queueRevision", "incrementRevision"],
        },
        REJECT_PLAN: {
          target: "CANCELLED",
          actions: ["clearStaging"],
        },
        CANCEL: {
          target: "CANCELLED",
          actions: ["clearPlan"],
        },
      },
    },

    REVIEWING: {
      on: {
        OPERATION_STAGED: {
          actions: ["addOperation"],
        },
        EXECUTE_PLAN: {
          target: "EXECUTING",
          actions: ["setExecuting"],
        },
        REQUEST_REVISION: {
          target: "DRAFTING",
          actions: ["queueRevision", "incrementRevision"],
        },
        REJECT_PLAN: {
          target: "CANCELLED",
          actions: ["clearStaging"],
        },
        CANCEL: {
          target: "CANCELLED",
          actions: ["clearPlan"],
        },
      },
    },

    EXECUTING: {
      entry: ["executeAllOperations"],
      on: {
        PLAN_EXECUTED: {
          target: "COMPLETED",
          actions: ["recordResults", "clearStaging"],
        },
        // Execution can fail for individual ops but continue
      },
    },

    COMPLETED: {
      entry: ["showSuccess", "notifyAI"],
      on: {
        START_PLANNING: {
          target: "DRAFTING",
          actions: ["initializePlan"],
        },
      },
    },

    CANCELLED: {
      entry: ["clearStaging"],
      on: {
        START_PLANNING: {
          target: "DRAFTING",
          actions: ["initializePlan"],
        },
      },
    },
  },
};
```

## Current Implementation

### What's Already Built

#### 1. Plan Mode Toggle (src/App.tsx:85-86, Chat.tsx:103-114)

```typescript
const [usePlanMode, setUsePlanMode] = useState(false);
const [selectedAgent, setSelectedAgent] = useState<string | undefined>(undefined);

// When Plan Mode is enabled
const selectedAgent =
  propSelectedAgent !== undefined ? propSelectedAgent : propUsePlanMode ? "plan" : undefined;
```

#### 2. Staging Area (src-tauri/src/tool_proxy.rs:195+)

```rust
pub struct StagingStore {
    operations: RwLock<HashMap<String, StagedOperation>>,
}

impl StagingStore {
    pub fn new() -> Self {
        Self {
            operations: RwLock::new(HashMap::new()),
        }
    }

    pub fn stage(&self, operation: StagedOperation) {
        let mut ops = self.operations.write().unwrap();
        ops.insert(operation.id.clone(), operation);
    }

    pub fn get_all(&self) -> Vec<StagedOperation> {
        let ops = self.operations.read().unwrap();
        ops.values().cloned().collect()
    }

    pub fn clear(&self) -> Vec<StagedOperation> {
        let mut ops = self.operations.write().unwrap();
        ops.drain().map(|(_, v)| v).collect()
    }
}
```

#### 3. Permission Interception (src/components/chat/Chat.tsx:977-990)

```typescript
} else if (event.type === "permission_asked") {
  console.log("[Chat] Permission asked:", event);

  // Stage the operation if in plan mode
  if (usePlanMode) {
    await stageOperation(
      event.request_id,
      sessionId,
      event.tool || "unknown",
      event.args || {},
      currentMessageIdRef.current
    );
    setStagedCount((prev) => prev + 1);
  } else {
    // Immediate mode: show notification
    // ... existing toast logic
  }
}
```

#### 4. Execution Plan Panel (src/components/plan/ExecutionPlanPanel.tsx)

Shows staged operations with:

- Expandable diffs for file changes
- Command preview for bash operations
- Individual remove buttons
- Execute Plan button
- Clear All button

#### 5. Plan Action Buttons (src/components/chat/PlanActionButtons.tsx)

- Implement this (executes staged operations)
- Rework (prompts for feedback)
- Cancel (clears staging area)
- View Tasks (opens TaskSidebar)

## Planned Features

### 1. Plan Versioning System

**Purpose:** Track plan revisions and enable rollback to previous versions.

**Implementation:**

```rust
// src-tauri/src/plan_store.rs

pub struct PlanVersion {
    pub id: String,
    pub revision: u32,
    pub parent_revision: Option<u32>,
    pub plan_content: String,
    pub operations: Vec<StagedOperation>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub approval_hash: String,  // SHA-256 hash of approved state
}

pub struct PlanStore {
    versions: RwLock<HashMap<String, Vec<PlanVersion>>>,
    active_plan: RwLock<Option<ActivePlan>>,
}

impl PlanStore {
    /// Create a new plan revision
    pub fn create_revision(
        &self,
        session_id: &str,
        content: String,
        operations: Vec<StagedOperation>,
    ) -> Result<PlanVersion> {
        let mut versions = self.versions.write().unwrap();
        let session_versions = versions.entry(session_id.to_string()).or_default();

        let revision = session_versions.len() as u32 + 1;
        let parent = session_versions.last();

        // Compute approval hash for this revision
        let approval_hash = self.compute_hash(&content, &operations);

        let version = PlanVersion {
            id: uuid::Uuid::new_v4().to_string(),
            revision,
            parent_revision: parent.map(|p| p.revision),
            plan_content: content,
            operations,
            created_at: chrono::Utc::now(),
            approval_hash,
        };

        session_versions.push(version.clone());
        Ok(version)
    }

    /// Get all revisions for a session
    pub fn get_revisions(&self, session_id: &str) -> Vec<PlanVersion> {
        self.versions.read().unwrap()
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Get a specific revision
    pub fn get_revision(&self, session_id: &str, revision: u32) -> Option<PlanVersion> {
        self.versions.read().unwrap()
            .get(session_id)
            .and_then(|revs| revs.iter().find(|v| v.revision == revision))
            .cloned()
    }

    fn compute_hash(&self, content: &str, operations: &[StagedOperation]) -> String {
        let mut hasher = sha2::Sha256::new();
        hasher.update(content.as_bytes());
        for op in operations {
            hasher.update(op.id.as_bytes());
            hasher.update(op.tool.as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }
}
```

**File Structure:**

```
.opencode/tandem/
├── plans/
│   ├── index.json              # Plan index with metadata
│   └── {sessionId}/
│       ├── r001.md             # Revision 1 (original plan)
│       ├── r002.md             # Revision 2 (user feedback)
│       ├── r003.md             # Revision 3 (AI iteration)
│       └── meta.json           # Revision metadata
└── runs/
    └── {runId}/
        ├── meta.json           # Execution metadata
        ├── operations.json     # Staged operations
        └── results.json        # Execution results
```

### 2. Managed Tools Bundle

**Purpose:** Provide curated, context-aware tools for planning mode that guide the AI toward best practices.

**Implementation:**

```rust
// src-tauri/src/tool_bundle.rs

pub struct ToolBundle {
    pub category: String,
    pub instructions: String,
    pub json_schema: Value,
    pub example: String,
}

pub const PLANNING_TOOLS: &[ToolBundle] = &[
    ToolBundle {
        category: "planning",
        instructions: r#"
You are in Plan Mode. Your role is to create detailed, actionable plans for code changes.

When creating a plan:
1. Break down complex tasks into atomic steps
2. Consider file dependencies and execution order
3. Identify potential risks and mitigation strategies
4. Suggest validation steps (tests, builds, etc.)

For file operations:
- Use 'read' to understand existing code structure
- Use 'write' to propose complete file replacements
- Use 'edit' for targeted modifications
- Use 'bash' for shell commands (tests, builds, etc.)

Always explain the rationale behind each step.
"#,
        json_schema: json!({
            "type": "object",
            "properties": {
                "steps": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "description": { "type": "string" },
                            "tool": { "type": "string", "enum": ["read", "write", "edit", "bash"] },
                            "args": { "type": "object" },
                            "rationale": { "type": "string" },
                            "risk": { "type": "string" }
                        },
                        "required": ["description", "tool", "args"]
                    }
                }
            }
        }),
        example: r#"
I'll create a plan to add authentication to the API:

## Step 1: Read existing files
- Read `src/middleware/auth.ts` to understand current structure
- Read `src/routes/api.ts` to understand routing

## Step 2: Create JWT utility
- Write `src/utils/jwt.ts` with token creation/verification

## Step 3: Add auth middleware
- Write `src/middleware/auth.ts` with JWT verification

## Step 4: Update routes
- Edit `src/routes/api.ts` to use new middleware

## Step 5: Test
- Run `npm test` to verify changes
"#,
    },
];

/// Inject planning tools into OpenCode
pub async fn inject_planning_tools(
    sidecar: &SidecarManager,
    session_id: &str,
) -> Result<()> {
    let tools = PLANNING_TOOLS
        .iter()
        .map(|t| ToolGuidance {
            category: t.category.clone(),
            instructions: t.instructions.clone(),
            json_schema: t.json_schema.clone(),
            example: t.example.clone(),
        })
        .collect::<Vec<_>>();

    sidecar.send_tool_definitions(session_id, &tools).await?;
    Ok(())
}
```

**Rust Command:**

```rust
#[tauri::command]
pub async fn inject_planning_tools(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<()> {
    tool_bundle::inject_planning_tools(&state.sidecar, &session_id).await?;
    Ok(())
}
```

**Tauri API:**

```typescript
// src/lib/tauri.ts

export async function injectPlanningTools(sessionId: string): Promise<void> {
  return invoke("inject_planning_tools", { sessionId });
}
```

### 3. Approval Hash Enforcement

**Purpose:** Ensure that the exact plan reviewed by the user is the one that gets executed, preventing drift between review and execution.

**Implementation:**

```rust
// src-tauri/src/approval_enforcer.rs

pub struct ApprovalHash {
    pub plan_id: String,
    pub revision: u32,
    pub content_hash: String,
    pub operations_hash: String,
    pub combined_hash: String,
    pub approved_at: chrono::DateTime<chrono::Utc>,
    pub approved_by: String,  // "user" or "system"
}

pub struct ApprovalEnforcer {
    pending_approvals: RwLock<HashMap<String, ApprovalHash>>,
    execution_history: RwLock<Vec<ExecutionRecord>>,
}

impl ApprovalEnforcer {
    /// Generate approval hash for a plan revision
    pub fn generate_approval_hash(
        &self,
        plan_id: &str,
        revision: u32,
        content: &str,
        operations: &[StagedOperation],
    ) -> ApprovalHash {
        let content_hash = self.hash_content(content);
        let operations_hash = self.hash_operations(operations);
        let combined = format!("{}{}", content_hash, operations_hash);
        let combined_hash = self.hash_content(&combined);

        ApprovalHash {
            plan_id: plan_id.to_string(),
            revision,
            content_hash,
            operations_hash,
            combined_hash,
            approved_at: chrono::Utc::now(),
            approved_by: "user".to_string(),
        }
    }

    /// Verify approval hash before execution
    pub fn verify_approval(&self, plan_id: &str, revision: u32) -> Result<ApprovalHash> {
        let approvals = self.pending_approvals.read().unwrap();

        let key = format!("{}_{}", plan_id, revision);
        approvals.get(&key)
            .cloned()
            .ok_or_else(|| Error::msg("No approval found for this plan revision"))
    }

    /// Enforce that current plan matches approved hash
    pub fn enforce_execution(
        &self,
        plan_id: &str,
        revision: u32,
        current_content: &str,
        current_operations: &[StagedOperation],
    ) -> Result<()> {
        let approved = self.verify_approval(plan_id, revision)?;

        let current_content_hash = self.hash_content(current_content);
        let current_ops_hash = self.hash_operations(current_operations);

        if current_content_hash != approved.content_hash {
            return Err(Error::msg(
                "Plan content has changed since approval. Please re-review."
            ));
        }

        if current_ops_hash != approved.operations_hash {
            return Err(Error::msg(
                "Operations have changed since approval. Please re-review."
            ));
        }

        Ok(())
    }

    /// Record execution for audit trail
    pub fn record_execution(
        &self,
        approval: ApprovalHash,
        results: &[ExecutionResult],
    ) {
        let mut history = self.execution_history.write().unwrap();
        history.push(ExecutionRecord {
            timestamp: chrono::Utc::now(),
            approval,
            results: results.to_vec(),
        });
    }
}
```

### 4. Task Mapping

**Purpose:** Link plan operations to structured tasks/todos for better tracking.

**Implementation:**

```rust
// src-tauri/src/task_mapper.rs

pub struct TaskMapping {
    pub todo_id: String,
    pub operation_ids: Vec<String>,
    pub status: TaskStatus,
}

pub struct TaskMapper {
    mappings: RwLock<HashMap<String, TaskMapping>>,
}

impl TaskMapper {
    /// Create a task mapping for an operation
    pub fn map_operation_to_task(
        &self,
        todo_id: &str,
        operation_id: &str,
    ) {
        let mut mappings = self.mappings.write().unwrap();
        let entry = mappings.entry(todo_id.to_string()).or_insert(TaskMapping {
            todo_id: todo_id.to_string(),
            operation_ids: Vec::new(),
            status: TaskStatus::Pending,
        });
        entry.operation_ids.push(operation_id.to_string());
    }

    /// Get all operations for a task
    pub fn get_task_operations(&self, todo_id: &str) -> Vec<String> {
        self.mappings.read().unwrap()
            .get(todo_id)
            .map(|m| m.operation_ids.clone())
            .unwrap_or_default()
    }

    /// Update task status based on operation results
    pub fn update_task_status(
        &self,
        todo_id: &str,
        operation_results: &[String],
    ) -> TaskStatus {
        let mappings = self.mappings.read().unwrap();
        if let Some(mapping) = mappings.get(todo_id) {
            let all_completed = mapping.operation_ids.iter()
                .all(|op_id| operation_results.contains(op_id));

            if all_completed {
                return TaskStatus::Completed;
            }

            let any_failed = mapping.operation_ids.iter()
                .any(|op_id| !operation_results.contains(op_id));

            if any_failed {
                return TaskStatus::Failed;
            }
        }
        TaskStatus::InProgress
    }
}
```

## Integration Points

### OpenCode Sidecar Integration

```rust
// src-tauri/src/sidecar.rs

impl SidecarManager {
    /// Send message with plan agent
    pub async fn send_message_with_agent(
        &self,
        session_id: &str,
        content: &str,
        agent: &str,
    ) -> Result<()> {
        let request = SendMessageRequest {
            parts: vec![MessagePartInput {
                content: content.to_string(),
                role: "user".to_string(),
            }],
            model: None,
            agent: Some(agent.to_string()),
        };

        self.send_request(session_id, &request).await?;
        Ok(())
    }

    /// Approve a staged tool operation
    pub async fn approve_tool(
        &self,
        session_id: &str,
        request_id: &str,
    ) -> Result<()> {
        let request = ApproveToolRequest {
            session_id: session_id.to_string(),
            request_id: request_id.to_string(),
        };

        self.send_request(session_id, &request).await?;
        Ok(())
    }
}
```

### Frontend State Management

```typescript
// src/hooks/usePlanMachine.ts

import { createMachine, assign } from "xstate";
import { useMachine } from "@xstate/react";

const planMachine = createMachine({
  id: "plan",
  initial: "idle",
  context: {
    planId: undefined,
    revision: 0,
    operations: [],
    results: [],
  },
  states: {
    idle: {
      on: {
        START_PLANNING: "drafting",
      },
    },
    drafting: {
      on: {
        PLAN_CREATED: {
          target: "awaitingApproval",
          actions: assign({
            planId: (_, event) => event.planId,
            revision: (_, event) => event.revision,
          }),
        },
        CANCEL: "cancelled",
      },
    },
    awaitingApproval: {
      on: {
        OPERATION_STAGED: {
          actions: assign({
            operations: (context, event) => [...context.operations, event.operation],
          }),
        },
        EXECUTE: "executing",
        REQUEST_REVISION: "drafting",
        REJECT: "cancelled",
      },
    },
    executing: {
      invoke: {
        src: "executePlan",
        onDone: "completed",
        onError: "drafting",
      },
    },
    completed: {
      on: {
        START_PLANNING: "drafting",
      },
    },
    cancelled: {
      on: {
        START_PLANNING: "drafting",
      },
    },
  },
});

export function usePlanMachine() {
  return useMachine(planMachine);
}
```

## User Flows

### Basic Planning Flow

```
1. User toggles "Plan Mode" → State: IDLE → DRAFTING
2. User describes task → AI generates plan
3. Plan operations are staged as AI proposes them
4. AI signals "PLAN_CREATED" → State: DRAFTING → AWAITING_APPROVAL
5. ExecutionPlanPanel appears with all operations
6. User reviews diffs → State: AWAITING_APPROVAL → REVIEWING
7. User clicks "Execute Plan" → State: REVIEWING → EXECUTING
8. executeStagedPlan() runs, approving each operation
9. State: EXECUTING → COMPLETED
10. User starts new plan → State: COMPLETED → DRAFTING
```

### Revision Flow

```
1. User reviews plan in REVIEWING state
2. User clicks "Rework" → State: REVIEWING → DRAFTING
3. User provides feedback
4. AI creates revised plan → State: DRAFTING → AWAITING_APPROVAL (revision++)
5. User reviews updated plan
```

### Cancellation Flow

```
1. User clicks "Cancel" at any point → State: * → CANCELLED
2. Staging area is cleared
3. User can start fresh → State: CANCELLED → DRAFTING
```

## Configuration

### Plan Agent Configuration (.opencode/agents/plan.md)

```markdown
---
mode: primary
model: anthropic/claude-sonnet-4-20250514
temperature: 0.1
permission:
  edit: ask
  bash: ask
---

You are in planning mode within Tandem.

## Your Role

1. Analyze the user's request thoroughly
2. Create detailed, step-by-step plans for code changes
3. Break complex tasks into atomic operations
4. Consider file dependencies and execution order
5. Suggest validation steps (tests, builds)

## Output Format

When presenting a plan:

1. Start with a brief summary
2. List each step with:
   - Description of the change
   - Tool to use (read/write/edit/bash)
   - File path
   - Rationale
   - Potential risks

## Special Commands

- Signal "PLAN_CREATED" when your plan is complete
- Signal "PLAN_UPDATED" when revising based on feedback
- The user will review all operations in the ExecutionPlanPanel before execution
```

## Success Metrics

| Metric                 | Target          | Measurement                            |
| ---------------------- | --------------- | -------------------------------------- |
| Plan completion rate   | >80%            | Plans that reach COMPLETED state       |
| Revision frequency     | <2 per plan     | Average revisions per plan             |
| Execution success rate | >95%            | Operations that complete without error |
| User satisfaction      | >4.5/5          | Post-session survey                    |
| Time to execute        | <30s for 10 ops | Execution phase duration               |

## Future Enhancements

- [ ] Keyboard shortcuts (Ctrl+Enter to execute)
- [ ] Drag-to-reorder operations
- [ ] Save/load plans for later
- [ ] Operation dependency visualization
- [ ] AI-suggested execution order
- [ ] Parallel operation execution
- [ ] Plan templates for common tasks
- [ ] Team sharing of plan revisions
