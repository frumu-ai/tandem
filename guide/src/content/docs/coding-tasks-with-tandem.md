---
title: Coding Tasks With Tandem
description: How agents should inspect, edit, test, review, and report code changes while working inside a Tandem run.
---

Use this page when a Tandem workflow, mission, or coder run turns into a real code change.

This guide is especially important for MCP-backed side apps such as `~/aca-tandem`: the client can be thin, but Tandem still owns the run state, workspace binding, tool policy, and execution contract.

## Core rule

Treat the Tandem run as the source of truth for the task. Treat local git and the workspace as the source of truth for code state.

- Tandem owns the run, approvals, and task lineage.
- The active workspace root is the authorized filesystem boundary for edits.
- Local git handles branches, worktrees, diffs, commits, and cleanup.
- Tandem file tools handle inspection and content edits inside the allowed workspace.

Never assume a chat transcript or issue comment is enough to represent what was actually changed.

## Workspace and worktrees

Before editing code, confirm which workspace the run is bound to.

- If the run already points at the right workspace, use that workspace directly.
- If the task needs branch isolation, create or switch to a worktree for the change.
- Keep each task in one worktree when possible so the diff stays easy to review.
- Do not edit outside the allowed workspace or cross-pollinate unrelated work between worktrees.

If the workspace is wrong, fix the binding first instead of guessing your way through the edit.

## Choose the right editing tool

Use the smallest tool that makes the change clearly.

- `read` for inspection and context gathering.
- `grep` or `glob` for locating the right files.
- `edit` for targeted string replacements in an existing file.
- `write` for new files or a full-file rewrite.
- `apply_patch` for a multi-hunk, reviewable diff.
- local git commands for branch, worktree, diff, status, and commit handling.

A good default is: inspect first, then make the smallest possible edit, then review the diff.

## Recommended coding loop

1. Confirm the workspace root and allowed paths.
2. Read the relevant files before changing anything.
3. Decide whether the task is a small replacement, a rewrite, or a patch.
4. Make the edit inside the active workspace.
5. Run the smallest meaningful verification command.
6. Inspect the diff before you declare success.
7. Summarize the files changed, the tests run, and any remaining risks.
8. Commit or hand off only after the change is defensible.

## Diff and review

Agents should review changes before closing the loop.

- In the TUI, use `/diff` to inspect the current workspace diff.
- In a shell workflow, use `git diff` or equivalent local review tooling.
- If the diff contains unrelated changes, stop and separate them before continuing.
- If verification fails, fix the local issue instead of hiding it behind a generic success message.

## Testing and verification

Run the smallest test that proves the change is real.

- Prefer a focused unit or integration test before a broad suite.
- If the change touches a workflow or runtime boundary, verify the affected path end to end.
- If the change affects file handling, confirm the file landed where the workflow expected it.
- If the change affects code generation, confirm the generated artifact is the one the run asked for.

A coding task is not done until the agent can say what was verified and what was not.

## How workflow authors should describe coding tasks

When a workflow stage or mission step asks an agent to edit code, the prompt should include:

- the workspace root
- any allowed or denied paths
- the files or subsystem being changed
- the expected output or artifact
- the verification command or check
- the review requirement, if any

Example stage contract:

```json
{
  "objective": "Implement the new tenant-scoped audit envelope",
  "workspace_root": "/home/evan/aca-tandem",
  "allowed_paths": ["crates/tandem-server/src/"],
  "expected_outputs": ["updated Rust source", "focused test result", "diff summary"],
  "verification": "cargo test -p tandem-server provider_auth_set_writes_protected_audit_record -- --nocapture"
}
```

## What not to do

- Do not edit without first confirming the workspace.
- Do not use a wide rewrite when a narrow edit or patch is enough.
- Do not move between worktrees mid-task unless the run explicitly requires it.
- Do not commit before reviewing the diff and running verification.
- Do not present a code change as complete without saying which files changed.

## Related docs

- [Agent Workflow Operating Manual](./agent-workflow-operating-manual/)
- [Agent Workflow And Mission Quickstart](./agent-workflow-mission-quickstart/)
- [Autonomous Coding Agents with GitHub Projects](./autonomous-coding-agents-github-projects/)
- [Creating And Running Workflows And Missions](./creating-and-running-workflows-and-missions/)
- [Tools Reference](./reference/tools/)
