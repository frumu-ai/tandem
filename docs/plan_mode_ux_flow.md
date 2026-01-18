# Plan Mode: Complete UX Flow

## Visual Overview

```
┌─────────────────────────────────────────────────────────────┐
│ Tandem                                    🔵 Plan Mode  ✅ Connected │
│ 📁 C:\Users\evang\work\project                              │
├─────────────────────────────────────────────────────────────┤
│ ℹ️ Plan Mode Active                                         │
│ The AI will propose file changes for your review.          │
│ When changes are proposed, they'll appear in the           │
│ Execution Plan panel for batch approval.                   │
└─────────────────────────────────────────────────────────────┘
```

## Step-by-Step User Flow

### 1. Enable Plan Mode

**User Action:** Click "Immediate" → "Plan Mode" toggle in header

**Visual Feedback:**
- ✅ Button changes color (blue highlight)
- ✅ Badge shows "Plan Mode"
- ✅ Info banner appears below header explaining the mode
- ✅ AI switches to OpenCode's Plan agent

```
┌─────────────────────────────────────────┐
│ 🔵 Plan Mode     ⚪ Immediate            │ ← Toggle button
└─────────────────────────────────────────┘

┌─────────────────────────────────────────┐
│ ℹ️ Plan Mode Active                     │ ← Info banner
│ The AI will propose file changes...     │
└─────────────────────────────────────────┘
```

### 2. Request Changes

**User Action:** Send a message like "Refactor the auth system"

**What Happens:**
- Message sent to OpenCode's **Plan agent**
- Plan agent analyzes and proposes file operations
- Each operation requires approval (Plan agent's default behavior)

### 3. Operations Get Staged

**When AI Proposes Changes:**
- OpenCode sends `permission_asked` events
- Tandem **intercepts** these instead of showing toast popups
- Operations are **staged** in the StagingStore

**Visual Feedback:**
```
┌─────────────────────────────────────────────┐
│ 🔵 Plan Mode  🟡 3 changes pending          │ ← Counter appears
└─────────────────────────────────────────────┘

┌─────────────────────────────────────────────┐
│ ℹ️ Plan Mode Active                         │
│ 3 changes staged. Review them in the        │ ← Banner updates
│ Execution Plan panel (bottom-right)...      │
└─────────────────────────────────────────────┘
```

### 4. Execution Plan Panel Appears

**Location:** Bottom-right corner (floating panel)

**Shows:**
- List of all staged operations
- File paths and operation types (write/edit/delete)
- Expandable diffs for each file change
- Command preview for bash operations
- Individual "Remove" buttons
- "Execute Plan" button (green, prominent)
- "Clear All" button

```
┌─────────────────────────────────────────────┐
│ 📋 Execution Plan (3 operations)            │
├─────────────────────────────────────────────┤
│ 📄 auth/login.ts (write)                ❌  │
│ 📄 middleware/auth.ts (edit)            ❌  │
│ 💻 npm install jsonwebtoken (bash)     ❌  │
├─────────────────────────────────────────────┤
│ [▶️ Execute Plan]  [🗑️ Clear All]          │
└─────────────────────────────────────────────┘
```

### 5. Review Changes (OPTIONAL)

**User Action:** Click on any operation to expand

**Visual Feedback:**
- Shows side-by-side diff for file changes
- Shows full command for bash operations
- Can remove individual operations

```
┌─────────────────────────────────────────────┐
│ ▼ 📄 auth/login.ts (write)              ❌  │
│ ┌─────────────────────────────────────────┐ │
│ │ OLD                  │ NEW              │ │
│ │                      │ import jwt...    │ │
│ │ export function...   │ export function..│ │
│ │                      │   jwt.sign(...   │ │
│ └─────────────────────────────────────────┘ │
└─────────────────────────────────────────────┘
```

### 6. Execute Plan

**User Action:** Click "Execute Plan" button

**What Happens:**
1. All staged operations sent to OpenCode
2. OpenCode executes file writes/edits/commands
3. Operations journaled for undo
4. Staging area cleared
5. Confirmation sent to AI

**Visual Feedback:**
- ✅ Button shows loading spinner
- ✅ Panel closes after success
- ✅ "X changes pending" badge disappears
- ✅ Banner reverts to waiting state
- ✅ AI receives confirmation and continues

```
┌─────────────────────────────────────────────┐
│ [⏳ Executing...]                           │ ← Loading state
└─────────────────────────────────────────────┘

→ Operations executed

┌─────────────────────────────────────────────┐
│ 🔵 Plan Mode                                │ ← Back to clean state
└─────────────────────────────────────────────┘

AI: "The authentication system has been updated 
     with JWT support. The changes include..."
```

## Key UX Elements

### Header Indicators

1. **Mode Toggle**
   - Always visible in header
   - Shows current mode (Plan / Immediate)
   - Tooltip explains each mode

2. **Staged Counter** (only when operations staged)
   - Shows number of pending changes
   - Pulsing amber dot for attention
   - Only visible in Plan Mode

3. **Connection Status**
   - Green dot = Connected
   - Shows connection state

### Info Banner

- **Always visible in Plan Mode**
- Dynamic text based on state:
  - No operations → Explains how it works
  - Has operations → Tells you to review panel
- Dismissible? No - important context

### Execution Plan Panel

- **Appears automatically** when operations are staged
- **Fixed position** bottom-right
- **Floating** over content (z-50)
- **Scrollable** if many operations
- **Animated** entry/exit

## UX States Summary

| State | Mode Toggle | Counter | Banner Text | Panel Visible |
|-------|-------------|---------|-------------|---------------|
| Immediate Mode | "Immediate" | Hidden | Hidden | No |
| Plan Mode (waiting) | "Plan Mode" | Hidden | Explains mode | No |
| Plan Mode (staged) | "Plan Mode" | Shows count | "Review in panel" | Yes |
| Executing | "Plan Mode" | Shows count | "Review in panel" | Yes (loading) |
| After execution | "Plan Mode" | Hidden | Explains mode | No |

## User Mental Model

```
Enable Plan Mode
    ↓
Ask AI for changes
    ↓
AI analyzes and proposes operations
    ↓
Operations appear in panel (bottom-right)
    ↓
Review diffs and operations
    ↓
Click "Execute Plan" button in panel
    ↓
Changes applied + AI continues
```

## Common Scenarios

### Scenario 1: Simple refactor
```
User: "Add error handling to api/fetch.ts"
→ 1 operation staged (edit file)
→ Review diff
→ Execute plan
→ Done
```

### Scenario 2: Multi-file feature
```
User: "Add user authentication"
→ 5+ operations staged
→ Review each change
→ Remove operation #3 (not needed)
→ Execute plan
→ Done
```

### Scenario 3: Planning only (like your screenshot)
```
User: "Create a documentation plan"
→ AI creates text plan
→ No file operations = no staging
→ Panel never appears
→ Just conversation
```

**Note:** The panel only appears for **destructive operations** (write, edit, delete, bash). Read-only analysis doesn't trigger staging.

## Future Enhancements

- [ ] Keyboard shortcut to execute plan (Ctrl+Enter)
- [ ] Drag to reorder operations
- [ ] Save/load plans for later
- [ ] Diff highlighting themes
- [ ] Operation dependencies visualization
