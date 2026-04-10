---
title: Scheduling Workflows And Automations
description: Create interval- and cron-based routines, automations, and workflow-plan-backed automations with the Tandem SDKs.
---

The control panel is not using a separate scheduler. It sends schedule payloads to the same engine APIs your SDK code can call directly.

## The Two Schedule Shapes

Tandem currently has two scheduling families:

### 1. Routines and legacy automations

Used by:

- `client.routines`
- `client.automations`

Accepted schedule shapes:

- Cron string: `"0 8 * * *"`
- Cron object: `{ type: "cron", cron: "0 8 * * *" }`
- Interval object: `{ type: "interval", intervalMs: 3600000 }`
- Manual: `{ type: "manual" }`

Use this family for simple scheduled jobs or older automation definitions.

### 2. Workflow plans and V2 automations

Used by:

- `client.workflowPlans` / `client.workflow_plans`
- `client.automationsV2` / `client.automations_v2`

Accepted schedule shape:

```json
{
  "type": "interval",
  "interval_seconds": 3600,
  "timezone": "UTC",
  "misfire_policy": "run_once"
}
```

Cron uses the same shape with `type: "cron"` and `cron_expression`.

This is the same payload family the control panel sends for planner-created and V2 automations.

Automation plans and V2 DAG workflows can also carry `knowledge` bindings, which lets the engine preflight project-scoped reusable knowledge before a node runs. In practice, that means the workflow can start from promoted knowledge, keep raw working notes local, and only refresh prior work when the configured freshness policy says it is stale.

If you import a workflow bundle that already contains schedule information, keep that schedule staged inside the imported planner session until the user applies the plan. Import is not the same as arming the automation.

## Default Recommendation

For new scheduled automation work, prefer `automationsV2` / `automations_v2`.

Reasons:

- It matches the control panel's current automation model.
- It supports explicit multi-agent DAG flows.
- It can carry knowledge reuse bindings and preflight guidance per workflow or node.
- It uses the richer schedule object the planner and UI already produce.
- It is the best fit when another agent will generate or revise automations from requirements.

Use `workflowPlans` / `workflow_plans` when you want the engine to generate the V2 automation for you from natural-language requirements.

Keep using `routines` for simple recurring single-agent jobs, and keep `automations` only for legacy compatibility or existing installs that already depend on it.

## TypeScript Examples

### Planner-generated automation with an interval schedule

```ts
const draft = await client.workflowPlans.chatStart({
  prompt: "Create an automation that reviews the repo and opens a markdown report.",
  schedule: {
    type: "interval",
    interval_seconds: 6 * 60 * 60,
    timezone: "UTC",
    misfire_policy: "run_once",
  },
  planSource: "chat",
});

await client.workflowPlans.chatMessage({
  planId: draft.plan.plan_id!,
  message: "Keep it single-agent and write output to reports/repo-review.md",
});

await client.workflowPlans.apply({
  planId: draft.plan.plan_id,
  creatorId: "docker-agent",
});
```

### V2 automation every 15 minutes

```ts
await client.automationsV2.create({
  name: "incident-watch",
  status: "active",
  schedule: {
    type: "interval",
    interval_seconds: 15 * 60,
    timezone: "UTC",
    misfire_policy: "run_once",
  },
  agents: [
    {
      agent_id: "watcher",
      display_name: "Watcher",
      model_policy: {
        default_model: {
          provider_id: "openrouter",
          model_id: "openai/gpt-4o-mini",
        },
      },
      tool_policy: { allowlist: ["read", "websearch"], denylist: [] },
      mcp_policy: { allowed_servers: [] },
    },
  ],
  flow: {
    nodes: [
      {
        node_id: "scan",
        agent_id: "watcher",
        objective: "Check for new incidents and summarize urgent changes.",
      },
    ],
  },
});
```

### Routine every hour

```ts
await client.routines.create({
  name: "hourly-repo-summary",
  schedule: { type: "interval", intervalMs: 60 * 60 * 1000 },
  entrypoint: "Summarize changes in the repo from the last hour.",
});
```

### Legacy automation every day at 08:00 UTC

```ts
await client.automations.create({
  name: "daily-security-scan",
  schedule: "0 8 * * *",
  mission: {
    objective: "Review the repo for security-sensitive changes.",
    successCriteria: ["Write findings to reports/security-daily.md"],
  },
  policy: {
    tool: { externalIntegrationsAllowed: false },
    approval: { requiresApproval: false },
  },
  outputTargets: ["file://reports/security-daily.md"],
});
```

## Python Examples

### Planner-generated automation with an interval schedule

```python
draft = await client.workflow_plans.chat_start(
    prompt="Create an automation that reviews the repo and opens a markdown report.",
    schedule={
        "type": "interval",
        "interval_seconds": 6 * 60 * 60,
        "timezone": "UTC",
        "misfire_policy": "run_once",
    },
    plan_source="chat",
)

await client.workflow_plans.chat_message(
    plan_id=draft.plan.plan_id or "",
    message="Keep it single-agent and write output to reports/repo-review.md",
)

await client.workflow_plans.apply(
    plan_id=draft.plan.plan_id,
    creator_id="docker-agent",
)
```

### V2 automation every 15 minutes

```python
await client.automations_v2.create(
    {
        "name": "incident-watch",
        "status": "active",
        "schedule": {
            "type": "interval",
            "interval_seconds": 15 * 60,
            "timezone": "UTC",
            "misfire_policy": "run_once",
        },
        "agents": [
            {
                "agent_id": "watcher",
                "display_name": "Watcher",
                "model_policy": {
                    "default_model": {
                        "provider_id": "openrouter",
                        "model_id": "openai/gpt-4o-mini",
                    }
                },
                "tool_policy": {"allowlist": ["read", "websearch"], "denylist": []},
                "mcp_policy": {"allowed_servers": []},
            }
        ],
        "flow": {
            "nodes": [
                {
                    "node_id": "scan",
                    "agent_id": "watcher",
                    "objective": "Check for new incidents and summarize urgent changes.",
                }
            ]
        },
    }
)
```

### Routine every hour

```python
await client.routines.create(
    {
        "name": "hourly-repo-summary",
        "schedule": {"type": "interval", "intervalMs": 60 * 60 * 1000},
        "entrypoint": "Summarize changes in the repo from the last hour.",
    }
)
```

### Legacy automation every day at 08:00 UTC

```python
await client.automations.create(
    {
        "name": "daily-security-scan",
        "schedule": "0 8 * * *",
        "mission": {
            "objective": "Review the repo for security-sensitive changes.",
            "successCriteria": ["Write findings to reports/security-daily.md"],
        },
        "policy": {
            "tool": {"externalIntegrationsAllowed": False},
            "approval": {"requiresApproval": False},
        },
        "outputTargets": ["file://reports/security-daily.md"],
    }
)
```

## Important Distinction

`client.workflows` is for registered workflow definitions and runs. It is not the scheduling surface.

If you want a recurring job, schedule one of these:

- `routines` for simpler scheduled jobs
- `automations` for legacy mission-based automations
- `automationsV2` / `automations_v2` for the recommended persistent DAG automation model
- `workflowPlans` / `workflow_plans` when you want the engine planner to generate the automation first, then apply it

## Practical Recommendation

For new work:

- Use `automationsV2` / `automations_v2` if you already know the automation shape you want to create programmatically.
- Use `workflowPlans` / `workflow_plans` if another agent will describe requirements in natural language and let Tandem generate the automation definition.
- Use `routines` only when you want a much simpler recurring job and do not need the V2 automation model.

## See Also

- [TypeScript SDK](./typescript/)
- [Python SDK](./python/)
- [MCP Automated Agents](../mcp-automated-agents/)
