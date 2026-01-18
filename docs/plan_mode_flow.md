# Plan Mode: Using OpenCode's Native Plan Agent

## Overview

Tandem leverages **OpenCode's built-in Plan agent** for execution planning. This provides a native, well-tested planning experience without hacks or workarounds.

## OpenCode's Plan Agent

OpenCode comes with a Plan agent that has:
- **Permission-based restrictions** - File edits and bash commands require approval
- **Analysis focus** - Designed for planning and code review
- **Read-only tools** - Can read and search but not modify by default

Learn more: https://opencode.ai/docs/agents/

## How Tandem Integrates It

### 1. Mode Toggle

When user toggles to "Plan Mode" in Tandem:
- All messages are sent using OpenCode's `plan` agent
- The agent parameter is set: `agent: "plan"`
- OpenCode's Plan agent handles the conversation

### 2. Permission Interception

OpenCode's Plan agent sets file operations to "ask":
- When Plan agent wants to edit files, it asks for permission
- Tandem intercepts these `permission_asked` events
- Instead of showing individual toasts, operations are **staged**
- All staged operations appear in ExecutionPlanPanel

### 3. Batch Execution

When user clicks "Execute Plan":
1. All staged operations are approved in sequence
2. OpenCode executes the file changes
3. Operations are journaled for undo
4. Confirmation is sent back to the AI

### 4. AI Awareness

The Plan agent knows:
- ✅ It should propose operations for approval
- ✅ Operations will be reviewed before execution
- ✅ User controls when changes are applied
- ✅ It's designed for planning workflows

## Implementation Details

### Backend (Rust)

Added `agent` field to `SendMessageRequest`:
```rust
pub struct SendMessageRequest {
    pub parts: Vec<MessagePartInput>,
    pub model: Option<ModelSpec>,
    pub agent: Option<String>,  // ← New field
}
```

### Frontend (TypeScript)

```typescript
// When Plan Mode is enabled
const selectedAgent = usePlanMode ? "plan" : undefined;

// Send message with agent specified
await sendMessageStreaming(sessionId, content, attachments, selectedAgent);
```

### User Flow

```
User: [Enables Plan Mode] "Refactor the auth system"

AI (Plan Agent): "I'll analyze the authentication system and propose changes.
                  Let me examine the current implementation...
                  
                  I propose the following changes:
                  1. Update auth/login.ts - Add JWT support
                  2. Modify middleware/auth.ts - JWT verification
                  3. Update utils/crypto.ts - Token signing
                  
                  [Operations staged in Execution Plan panel]"

[User reviews diffs in ExecutionPlanPanel]
[User clicks "Execute Plan"]

AI: "The changes have been applied successfully. The authentication
     system now uses JWT tokens with proper verification..."
```

## Benefits Over Custom Implementation

✅ **Native OpenCode feature** - Uses tested, supported functionality  
✅ **Proper permissions** - Leverages OpenCode's permission system  
✅ **Agent-specific prompts** - Plan agent has specialized system prompts  
✅ **Consistent behavior** - Works like Plan mode in OpenCode TUI  
✅ **Future-proof** - Benefits from OpenCode updates  
✅ **No hacks** - No string injection or workarounds  

## Configuration

You can customize the Plan agent's behavior via `.opencode/agents/plan.md` in your workspace:

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
When proposing changes, the user will review them in a visual panel before execution.
Focus on clear descriptions of what each change accomplishes.
```

## Technical Notes

- Plan agent is specified per-message, not per-session
- Agent switching happens automatically based on Plan Mode toggle
- Confirmation messages also use the Plan agent to maintain context
- OpenCode handles all agent-specific logic internally
