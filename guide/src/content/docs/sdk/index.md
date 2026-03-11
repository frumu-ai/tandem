---
title: Tandem SDK
description: Connect to the Tandem engine from TypeScript or Python.
---

import { LinkCard, CardGrid } from "@astrojs/starlight/components";

The Tandem engine exposes a full HTTP + SSE API. The official SDKs wrap this API with typed, ergonomic interfaces for TypeScript (Node.js) and Python.

Both SDKs cover the main engine namespaces, including sessions, streaming, memory, skills, channels, MCP, workflows, workflow plans, routines, automations, agent teams, and missions.

## Prerequisites

A running Tandem engine and an API token:

```bash
tandem-engine serve --api-token $(tandem-engine token generate)
# or via desktop Settings → API Tokens
```

## Packages

<CardGrid>
  <LinkCard title="TypeScript SDK" href="./typescript/" description="@frumu/tandem-client — Node.js 18+" />
  <LinkCard title="Python SDK" href="./python/" description="tandem-client — Python 3.10+" />
</CardGrid>

## Guides

<CardGrid>
  <LinkCard
    title="Scheduling Automations"
    href="./scheduling-automations/"
    description="Create recurring routines, legacy automations, V2 automations, and planner-backed schedules."
  />
</CardGrid>

## Common event types

Both SDKs expose the same SSE event stream. These are the most common types you will handle:

| `event.type`           | Description                    | Key property                         |
| ---------------------- | ------------------------------ | ------------------------------------ |
| `session.response`     | Text delta from the model      | `properties.delta`                   |
| `session.tool_call`    | Tool invocation                | `properties.tool`, `properties.args` |
| `session.tool_result`  | Tool result                    | `properties.output`                  |
| `run.complete`         | Run finished successfully      | `properties.runID`                   |
| `run.completed`        | Alternate success event name   | `properties.runID`                   |
| `session.run.finished` | Session-scoped terminal event  | `properties.status`                  |
| `run.failed`           | Run failed                     | `properties.error`                   |
| `permission.request`   | Approval required              | `properties.permission`              |
| `question.pending`     | Structured question from agent | `properties.text`                    |

## Namespace overview

All namespaces exist on both the TypeScript and Python clients.

| Namespace                          | What it covers                                                     |
| ---------------------------------- | ------------------------------------------------------------------ |
| `sessions`                         | Create, list, message, fork, abort, cancel, diff, revert sessions  |
| `permissions`                      | List and reply to permission requests                              |
| `questions`                        | List/reply/reject AI-generated approval questions                  |
| `providers`                        | Catalog, config, set API keys and defaults                         |
| `channels`                         | Telegram, Discord, Slack integration config                        |
| `mcp`                              | Register, connect, refresh MCP servers and tools                   |
| `browser`                          | Browser sidecar status, install, and smoke testing                 |
| `memory`                           | Global memory: put, search, list, promote, demote, delete, audit   |
| `skills`                           | Agent skill packs: list, import, preview, install templates        |
| `resources`                        | Key-value resource store (shared agent state)                      |
| `workflows`                        | Workflow registry, runs, hooks, and event streams                  |
| `workflowPlans` / `workflow_plans` | Planner chat, preview, and apply flows for automation generation   |
| `routines`                         | Scheduled routines: create, run, approve/deny/pause/resume runs    |
| `automations`                      | Legacy mission-scoped automations (compatibility path)             |
| `automationsV2` / `automations_v2` | Persistent multi-agent DAG automations with per-agent model policy |
| `bugMonitor` / `bug_monitor`       | Incident triage, drafts, approval, and publishing helpers          |
| `coder`                            | Coder runs, artifacts, review summaries, and memory candidates     |
| `agentTeams`                       | Spawn and manage multi-agent teams                                 |
| `missions`                         | Multi-agent goals and work item tracking                           |
