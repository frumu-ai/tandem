# Tandem Agent Swarm Architecture

This example demonstrates how to build a robust, multi-agent AI Swarm using **only** Tandem's core primitives. No external orchestration frameworks (like LangChain or AutoGen) are required.

By utilizing Tandem's native `Shared Resources` for state, `Routines` for continuous monitoring, and the `Server-Sent Events (SSE)` bus for deterministic triggers, we can build highly observable, strictly sandboxed multi-agent systems.

## The Swarm Topology

This example provisions four distinct specialized agents:

1. **Manager Agent**: Parses user instructions, breaks them into features, and uses local shell tools to spawn isolated Git Worktrees (`.swarm/worktrees/<task_id>`).
2. **Worker Agent**: Placed strictly inside a generated worktree via Tandem's `fs_allowlist`. Modifies code, and uses the GitHub MCP to open Pull Requests.
3. **Tester Agent**: Activated inside the worktree to run linters and test suites. Updates shared state on failure to trigger re-work.
4. **Reviewer Agent**: Fetches PR diffs via GitHub MCP, analyzes for architectural/security flaws, and submits an official GitHub Review.

## How It Works

We use an external Node.js orchestrator (`orchestrator.ts`) to wire these primitives together:

1. **Shared State:** The orchestrator creates a Tandem Shared Resource called `swarm.active_tasks`. This acts as the single source of truth for the distributed system.
2. **Event Subscription:** The orchestrator subscribes to Tandem's global event stream (`/api/events`).
3. **Triggering:** When the Manager Agent successfully executes the `create_worktree.sh` tool, the orchestrator detects the `tool.call.completed` event.
4. **Spawning Defaults:** The orchestrator immediately spawns a new transient Session for the **Worker Agent**, dynamically injecting the absolute path of the new worktree into the Worker's prompt and filesystem boundaries.

### MCP Token Context Limit Solution in Action

Unlike "Tool RAG" frameworks, this swarm scales infinitely without risking MCP token context bloat.

Tandem ensures:

- The Worker receives _only_ the GitHub PR creation tools.
- The Tester receives _only_ the shell execution tools.
- The Reviewer receives _only_ the GitHub diffing and commenting tools.

Because Tandem implements strict `allowed_tools` policies per session instead of guessing auto-truncation, the agents remain hyper-focused and token-efficient.

## Quickstart

### 1. Prerequisites

Ensure the Tandem Engine is running locally and you have the GitHub MCP server connected via the Tandem UI.

### 2. Start the Orchestrator

```bash
npm install -g ts-node
ts-node orchestrator.ts
```

### 3. Deploy the Swarm

Navigate to the Tandem UI, open a standard chat session, paste in the `agents/manager.md` prompt, and assign it a task (e.g., "Build a new landing page route").

Watch the Orchestrator detect the worktree creation and seamlessly fan-out the workload to the Worker, Tester, and Reviewer!

### 4. Setup Health Monitors

To ensure tasks don't get stuck indefinitely, open the Tandem UI -> Routines, and import `routines/check_swarm_health.json`. This cron job will run every 10 minutes, query the `swarm.active_tasks` shared resource, and send an alert via the native Telegram integration if a pull request stalls.
