---
title: Agent Workflow And Mission Quickstart
description: Minimal checklist for agents that need to create, run, and maintain Tandem workflows or missions.
---

Use this page as the shortest reliable path for an agent that needs to turn human intent into a running Tandem system.

This quickstart assumes:

- the agent can reach a running `tandem-engine`
- the agent is allowed to use an engine token
- the agent may need to create or update workflows, missions, or scheduled automations

## Quick checklist

### 1. Authenticate to the engine

- obtain the token from an explicitly provided source
- confirm the engine URL
- verify access with a health check before doing anything else

Example:

```bash
curl -s http://127.0.0.1:39731/global/health \
  -H "X-Agent-Token: tk_your_token"
```

If you do not have a valid token yet, stop and use [Engine Authentication For Agents](./engine-authentication-for-agents/).

### 2. Choose the right Tandem path

Use:

- **workflow plans** when the user wants Tandem to generate the automation from intent
- **mission builder** when the goal spans several dependent staged workstreams
- **V2 automations** when the DAG is already known and should be scheduled or run directly
- **missions runtime** when a mission already exists and work state must be updated

If you are not sure which path applies, use [Creating And Running Workflows And Missions](./creating-and-running-workflows-and-missions/).

### 3. Prompt for structure, not vibes

When authoring a workflow node or mission stage, always define:

- the stage objective
- allowed inputs
- required outputs
- constraints
- completion criteria

Do not treat Tandem like one long chat prompt.

Use [Prompting Workflows And Missions](./prompting-workflows-and-missions/) for the full authoring pattern.

### 4. Prefer project-scoped reusable knowledge

Default behavior should be:

- raw working notes stay local to the run
- validated outputs are promoted for reuse
- later runs preflight project knowledge before recomputing

Do not rely on flat global memory for normal workflow execution.

### 5. Preview before apply

Before creating a live automation or mission-backed system:

- preview the workflow plan
- or compile-preview the mission blueprint

Do not apply a weak generated definition directly if the user depends on it.

### 6. Apply through the correct engine surface

Common paths:

- `POST /workflow-plans/apply`
- `POST /mission-builder/apply`
- `POST /automations/v2`

The SDKs wrap these for you if you are not using raw HTTP.

### 7. Schedule only after the structure is sound

For recurring systems:

- verify the first run manually
- confirm handoffs and artifacts are correct
- then enable the recurring schedule

Recurring work should reuse promoted project knowledge where appropriate instead of starting from scratch every time.

### 8. Repair runs, do not rebuild everything

When a run fails:

- inspect the run details
- repair or recover the failing run
- retry or continue only the affected node or stage when possible

Avoid recreating the whole automation unless the structure itself is wrong.

## Minimum safe operating pattern

If an agent is asked to “set this up for me,” the default safe sequence is:

1. authenticate
2. choose workflow plan, mission builder, V2 automation, or mission runtime
3. author or generate the staged definition
4. preview
5. apply
6. run once
7. inspect the result
8. enable recurrence if needed

## Best starting docs

- [Engine Authentication For Agents](./engine-authentication-for-agents/)
- [Choosing Providers And Models For Agents](./choosing-providers-and-models-for-agents/)
- [Creating And Running Workflows And Missions](./creating-and-running-workflows-and-missions/)
- [Coding Tasks With Tandem](./coding-tasks-with-tandem/)
- [Prompting Workflows And Missions](./prompting-workflows-and-missions/)
- [Scheduling Workflows And Automations](./sdk/scheduling-automations/)
- [Control Panel (Web Admin)](./control-panel/)
