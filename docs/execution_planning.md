# PRD: Execution Planning & Staging Area

## Overview

Tandem aims to provide a "safety-first" AI workspace. While current AI agents often perform operations one-by-one, a more advanced workflow involves the AI proposing a **complete execution plan** which the user can review, modify, and "commit" to the real filesystem as a single atomic-like batch.

## Problem

- **Single-tool approval is tedious**: Approving 10 file writes one-by-one is a poor UX.
- **Lack of context**: Seeing one file change without seeing the subsequent test run makes it hard to judge if the change is correct.
- **Accidental mess**: If an AI fails halfway through a multi-step task, the workspace is left in an inconsistent state.

## Proposed Solution: The Staging Area

Tandem will implement a "Staging Area" (similar to Git's `git add` phase) where AI operations are buffered before being applied to the real filesystem.

### 1. Interception Layer

- Intercept `permission.asked` events from the OpenCode sidecar.
- Instead of executing the tool immediately upon approval, Tandem will store the proposed change in a `DraftStore`.

### 2. Multi-Step Planning

- Tandem will encourage the AI to emit a "Task Plan" at the start of a session.
- This plan is rendered as a checklist in the UI, showing the intended sequence of `read`, `write`, and `bash` operations.

### 3. Visual Staging UI

- **Pending Changes Sidebar**: A dedicated view showing all proposed file creations, modifications, and deletions.
- **Diff Preview**: Clicking a pending change opens a rich side-by-side diff viewer.
- **Batch Actions**: "Approve All", "Reject All", or "Edit Proposal".

## Technical Implementation

### Backend (Rust/Tauri)

- **`JournalEntry` Extension**: Update the `OperationJournal` to support a `Staged` status.
- **Tool Proxy**: Modify `approve_tool` to allow an "Apply Later" flag.

### Frontend (React/TypeScript)

- **`useStagingArea` Hook**: A global state manager for pending tool calls.
- **`DiffComponent`**: Integration of a diffing library (e.g., `react-diff-viewer-continued`) to show changes between the `before_state` (real file) and `after_state` (AI proposal).

## User Flow

1. **AI Proposes**: AI wants to refactor a component. It sends 3 `write` requests.
2. **Buffer**: Tandem intercepts these and adds them to the "Pending Changes" list.
3. **Review**: User sees the 3 files marked as "To be modified".
4. **Commit**: User reviews the diffs and clicks **"Execute Plan"**.
5. **Execution**: Tandem loops through the tool IDs and sends `approve_tool` calls to the OpenCode sidecar in the correct sequence.

## Success Metrics

- **Reduction in "Oops" moments**: Fewer user reverts needed after AI operations.
- **Higher Trust**: Users feel more comfortable giving AI broad permissions if they can see the full plan first.
- **Efficiency**: Users approve complex refactors in a single click rather than 10.
