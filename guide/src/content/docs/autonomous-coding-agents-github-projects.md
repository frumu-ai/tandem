---
title: Autonomous Coding Agents with GitHub Projects
---

Use this guide when you want an agent running on `tandem-engine` to pull work from GitHub Projects and execute coding tasks without inventing a second GitHub integration layer.

## Core Rule

Treat Tandem as the execution authority.

- GitHub Projects is an intake and visibility surface.
- Tandem owns planning, run state, tool policy, execution, and artifacts.
- GitHub MCP is the GitHub API path for project reads, PRs, issue comments, and remote sync.
- Local `git` remains the path for clone, checkout, worktrees, commits, diffs, and testable workspace changes.

Do not build a parallel client-side GitHub adapter or require a separate `gh` login flow for the GitHub Projects path.

For the concrete edit/diff/worktree loop, use [Coding Tasks With Tandem](./coding-tasks-with-tandem/).

## Expected Bootstrap Path

For GitHub Projects work, prefer Tandem's built-in GitHub MCP bootstrap:

- Tandem auto-registers the official GitHub MCP server when a GitHub PAT is available.
- The PAT can come from `GITHUB_PERSONAL_ACCESS_TOKEN`, `GITHUB_TOKEN`, or Tandem's persisted provider auth store.
- Manual `mcp add` should usually not be required for the default GitHub Projects flow.

Before starting autonomous work, verify:

1. a GitHub-capable MCP server is connected
2. GitHub project tools are visible through Tandem
3. the coder project is bound to the intended GitHub Project

## What The Agent Should Do

The agent should follow this operating loop:

1. Read the assigned GitHub Project inbox through Tandem's engine APIs.
2. Select actionable issue-backed TODO items and ignore unsupported or ambiguous items.
3. Intake one item into a Tandem-native coder run.
4. Treat the resulting Tandem run as the source of truth for execution state.
5. Inspect the local repository, plan the change, and work inside the allowed workspace.
6. Use local tools for editing, testing, and git state changes.
7. Use GitHub MCP for remote GitHub actions such as PR creation, issue/project updates, and review metadata.
8. Report status back through Tandem so remote sync state stays inspectable.

## Division Of Responsibility

### Tandem engine

- owns durable run state
- enforces tool policy and approvals
- binds local repo context to the run
- manages GitHub Project intake and remote sync metadata

### GitHub MCP

- reads GitHub Project data
- reads and writes GitHub issue, PR, and review state
- provides the remote GitHub action surface to the agent

### Local git and filesystem tools

- create branches and worktrees
- edit files
- run tests and linters
- inspect diffs
- prepare commits

## Recommended Agent Contract

Use language close to this in the agent's system prompt:

```md
You are an autonomous coding agent running on Tandem.

Use Tandem as the system of record for execution state. Treat GitHub Projects as an intake and visibility layer, not as the scheduler or runtime authority.

When GitHub access is needed, use Tandem's connected GitHub MCP path rather than a separate gh-based adapter. Use local git tools for repository changes, branches, commits, diffs, and tests.

For each task:

1. Confirm the GitHub Project item is actionable.
2. Intake it into a Tandem-native coder run.
3. Inspect the codebase before changing anything.
4. Make the smallest coherent change that satisfies the task.
5. Run relevant verification.
6. Publish remote GitHub updates through GitHub MCP.
7. Leave clear completion or blocker notes with enough detail for the next agent or operator.

Never assume GitHub Project state alone is the source of truth for execution progress. Never introduce a parallel client-side GitHub adapter if Tandem's built-in MCP path can handle the job.
```

## Good Agent Behavior

- Prefer one project item at a time unless the workflow explicitly supports batching.
- Convert vague project items into concrete implementation tasks before editing code.
- Inspect the repo before proposing changes.
- Keep changes scoped to the assigned issue or TODO.
- Run the smallest meaningful verification step before declaring success.
- Update GitHub state only after local work has reached a defensible checkpoint.
- Surface schema drift, auth failures, or missing MCP tools as operator-visible blockers.

## Bad Agent Behavior

- Treating GitHub Projects as the runtime scheduler
- depending on `gh` auth when Tandem MCP auth is already available
- opening PRs before local verification
- writing status only to GitHub while Tandem run state stays stale
- bypassing Tandem tool policy with ad hoc side channels
- assuming stale cached project schema is still valid after drift is detected

## Minimal Operator Validation

Check the engine contract first:

```bash
TOKEN="tk_test_token"
HOST="http://127.0.0.1:39731"
PROJECT_ID="repo-123"

curl -s "$HOST/coder/projects/$PROJECT_ID/bindings" \
  -H "X-Agent-Token: $TOKEN" | jq .

curl -s "$HOST/coder/projects/$PROJECT_ID/github-project/inbox" \
  -H "X-Agent-Token: $TOKEN" | jq .
```

Then intake a project item:

```bash
curl -s -X POST "$HOST/coder/projects/$PROJECT_ID/github-project/intake" \
  -H "X-Agent-Token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "project_item_id": "PVT_ITEM_123",
    "source_client": "autonomous_coding_agent"
  }' | jq .
```

The returned run should expose GitHub Project linkage and remote sync fields. If that contract is unhealthy, fix the engine-side path before adding more agent logic.

## Design Guardrails

- Keep GitHub secrets out of plain config files and logs.
- Prefer explicit tool allowlists for autonomous runs.
- Keep GitHub MCP server usage engine-first and namespaced through Tandem.
- Treat project schema drift and MCP auth challenges as first-class runtime states, not silent failures.

## See Also

- [MCP Automated Agents](./mcp-automated-agents/)
- [Engine Testing](./engine-testing/)
- [TypeScript SDK](./sdk/typescript/)
- [Python SDK](./sdk/python/)
