---
title: Creating And Running Workflows And Missions
description: Choose the right Tandem abstraction, create it through the right surface, and run it reliably over time.
---

Use this guide when an agent or operator needs to answer:

- should this be a workflow, a mission, or an automation?
- which UI, SDK, or HTTP path should I use?
- how do I go from human intent to a running recurring system?

This page is operational. For prompt-writing guidance, see [Prompting Workflows And Missions](./prompting-workflows-and-missions/).

## The short decision map

### Use a workflow plan when

- the user has natural-language intent
- Tandem should generate the automation shape for you
- you want a planner chat loop before applying the result

Primary surfaces:

- control panel planner flows
- `client.workflowPlans` / `client.workflow_plans`
- `POST /workflow-plans/preview`
- `POST /workflow-plans/chat/start`
- `POST /workflow-plans/chat/message`
- `POST /workflow-plans/apply`

### Use a V2 automation when

- you already know the DAG or stage structure
- you want a persistent scheduled workflow
- you want agent-specific policies, checkpoints, retries, and run inspection

Primary surfaces:

- Studio and automation builder flows in the control panel
- `client.automationsV2` / `client.automations_v2`
- `POST /automations/v2`
- `POST /automations/v2/{id}/run_now`
- `GET /automations/v2/{id}/runs`

### Use the mission builder when

- the goal spans several dependent workstreams
- you want one larger staged plan with explicit handoffs
- later work should begin only after earlier work completes
- you want recurring multi-stage operations over days, weeks, or months

Primary surfaces:

- Advanced Swarm Builder / mission builder in the control panel
- mission builder preview/apply engine routes
- `POST /mission-builder/compile-preview`
- `POST /mission-builder/apply`

### Use missions runtime directly when

- the mission object already exists
- you are tracking higher-level work items and state transitions
- you want to push mission events as work progresses

Primary surfaces:

- `client.missions`
- mission runtime endpoints exposed through the SDK and desktop bridge

## Recommended authoring path

For most agent-authored systems, use this sequence:

1. decide whether the user wants a generated workflow, a direct V2 automation, or a staged mission
2. write or generate the workflow or mission definition
3. preview it before applying
4. apply it into the engine
5. schedule it if it should recur
6. inspect runs and repair only the failing stage instead of rebuilding everything

## What to choose for common situations

### “Take this goal and figure out the automation for me”

Use:

- workflow plans
- or mission builder if the goal clearly spans several staged workstreams

This is the best fit for:

- intent-to-automation generation
- iterative planner chat
- shaping a new automation from vague human input

### “I know the agents and stages I want”

Use:

- V2 automation

This is the best fit for:

- deterministic DAG authoring
- repeated scheduled runs
- explicit policies per agent or per node

### “I need a larger coordinated operating loop with stages that gate each other”

Use:

- mission builder
- then apply the compiled result into an automation if the system should run on a schedule

This is the best fit for:

- long-running operational loops
- monitor -> analyze -> decide -> handoff
- intake -> plan -> execute -> verify -> review
- collect -> consolidate -> update state -> notify

## Control panel path

### Workflow-plan path

Use the planner flows when the human intent is still loose and you want Tandem to draft the automation:

- start planner chat
- refine with follow-up messages
- preview the plan
- apply the plan

### Studio / automation path

Use this when the workflow is already understood and should become a direct V2 automation:

- build the DAG
- configure schedule and policies
- save
- run now
- inspect the run timeline

### Advanced Swarm Builder / mission path

Use this when the overall system is a staged mission:

- start from an archetype or authoring prompt
- paste or import a generated mission blueprint
- compile preview
- apply
- then run or schedule the resulting automation/multi-stage workflow

## SDK and HTTP path

### Workflow plans

TypeScript:

```ts
const started = await client.workflowPlans.chatStart({
  prompt:
    "Create a recurring automation that inspects inbound work and produces a verified handoff.",
});

const revised = await client.workflowPlans.chatMessage({
  plan_id: started.plan.plan_id,
  message: "Make it project-scoped, staged, and write explicit artifacts between steps.",
});

await client.workflowPlans.apply({
  planId: revised.plan.plan_id!,
  creatorId: "agent-operator",
});
```

HTTP:

- `POST /workflow-plans/preview`
- `POST /workflow-plans/chat/start`
- `POST /workflow-plans/chat/message`
- `POST /workflow-plans/apply`

### Mission builder

Use the mission builder when you want the engine to compile a structured mission blueprint into a runnable artifact:

- `POST /mission-builder/compile-preview`
- `POST /mission-builder/apply`

This is the right place for staged, dependent workstreams with explicit handoffs.

### V2 automations

Use V2 automations when the structure is already known:

- `POST /automations/v2`
- `POST /automations/v2/{id}/run_now`
- `GET /automations/v2/{id}/runs`
- `GET /automations/v2/runs/{run_id}`

Use the run-level repair surfaces when needed instead of recreating the automation:

- `POST /automations/v2/runs/{run_id}/repair`
- `POST /automations/v2/runs/{run_id}/recover`
- task-level retry/continue/requeue endpoints under `/automations/v2/runs/{run_id}/tasks/...`

### Missions runtime

TypeScript:

```ts
const { mission } = await client.missions.create({
  title: "Operations Handoff Loop",
  goal: "Collect inputs, update state, verify outputs, and publish a reviewed handoff",
  work_items: [{ title: "Initial work item" }],
});

await client.missions.applyEvent(mission!.id!, {
  type: "work_item.completed",
  work_item_id: "work-item-1",
});
```

Use missions runtime when you already have a mission object and need to move its work state forward.

## How this fits long-running systems

For recurring systems that run at 8am every day or weekly over months:

1. create the workflow or mission once
2. give it explicit stage outputs and durable handoffs
3. schedule the resulting automation
4. let later runs reuse promoted project knowledge
5. inspect only the affected stage when a run needs repair

This is better than rebuilding the workflow every day.

## Knowledge reuse defaults

Generated workflows and missions should now default to:

- project-scoped knowledge
- promoted-trust reuse
- preflight reuse checks before recomputation

In practice, that means:

- raw run notes stay local
- validated outputs become reusable
- recurring runs can start from prior promoted knowledge instead of redoing everything

## Good operational pattern for agents

When an agent is asked to “set up an autonomous system,” it should usually do this:

1. identify the right Tandem abstraction
2. generate or author the staged definition
3. preview before applying
4. apply to the engine
5. configure recurrence
6. verify the first run
7. inspect run status and repair failing stages instead of redesigning the whole system

## What not to do

Avoid these common mistakes:

- using flat session chat as a replacement for a workflow or mission
- building one huge stage instead of several bounded stages
- relying on global memory instead of project-scoped promoted reuse
- skipping preview and applying a weak generated plan directly
- recreating automations when a targeted repair or retry would do

## If you are an MCP-driven agent

The most useful combination is:

- [Engine Authentication For Agents](./engine-authentication-for-agents/)
- [Prompting Workflows And Missions](./prompting-workflows-and-missions/)
- [Scheduling Workflows And Automations](./sdk/scheduling-automations/)

That gives you:

- the token and auth path
- the authoring rules
- the runtime scheduling path

## See also

- [Agent Workflow And Mission Quickstart](./agent-workflow-mission-quickstart/)
- [Prompting Workflows And Missions](./prompting-workflows-and-missions/)
- [Control Panel (Web Admin)](./control-panel/)
- [MCP Automated Agents](./mcp-automated-agents/)
- [Scheduling Workflows And Automations](./sdk/scheduling-automations/)
- [Engine Authentication For Agents](./engine-authentication-for-agents/)
