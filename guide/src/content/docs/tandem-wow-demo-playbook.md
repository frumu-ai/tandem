---
title: Tandem Wow Demo Playbook
description: How agents should turn Tandem docs and engine contracts into showcase workflow and automation payloads.
---

Use this page when you want an agent to generate a demo payload that clearly shows what Tandem can do.

This is not a general product overview. It is a build recipe for agents that need to turn docs into a working showcase.

## Human intent to engine payload

If an agent starts from a natural-language goal, the safest path is:

1. Ask the docs MCP for the best starting page or how-to answer.
2. Read the Tandem docs pages for the right abstraction.
3. Choose the smallest Tandem shape that fits the goal.
4. If the goal is recurring, choose the schedule shape first.
5. Turn the chosen shape into JSON.
6. Hand that JSON directly to the matching SDK method.
7. Run the created automation once and inspect the result.

Use these handoffs:

- raw automation JSON -> `client.automationsV2.create(...)` or `client.automations_v2.create(...)`
- workflow-plan bundle JSON -> `client.workflowPlans.importPreview(...)` / `client.workflowPlans.importPlan(...)` or the Python equivalents
- planner conversation -> `client.workflowPlans.chatStart(...)` -> `chatMessage(...)` -> `apply(...)`

If the payload is already a full automation object, do not route it through planner chat first.

## Human intent to recurring schedule

If the goal says things like:

- run every day at 8am
- check every 15 minutes
- summarize weekly
- repeat until a condition changes

have the agent choose the schedule shape before it writes the rest of the payload.

Use this mapping:

- one-off run -> `{"type": "manual"}`
- every N minutes or hours -> `{"type": "interval", ...}`
- specific time on the calendar -> `{"type": "cron", ...}`

The schedule should live in the same JSON object as the workflow or automation definition.

For direct automation creation, the schedule travels with the payload:

```ts
const created = await client.automationsV2.create({
  name: "Daily repo summary",
  status: "active",
  schedule: {
    type: "interval",
    interval_seconds: 24 * 60 * 60,
    timezone: "UTC",
    misfire_policy: { type: "run_once" },
  },
  // agents + flow here
});
```

```python
created = await client.automations_v2.create({
    "name": "Daily repo summary",
    "status": "active",
    "schedule": {
        "type": "interval",
        "interval_seconds": 24 * 60 * 60,
        "timezone": "UTC",
        "misfire_policy": {"type": "run_once"},
    },
    # agents + flow here
})
```

That lets an agent convert "run this every day" into a runnable recurring payload without guessing where the schedule lives.

## Start here

Before writing anything, have the agent read:

- [Prompting Workflows And Missions](./prompting-workflows-and-missions/)
- [Creating And Running Workflows And Missions](./creating-and-running-workflows-and-missions/)
- [Agent Workflow And Mission Quickstart](./agent-workflow-mission-quickstart/)
- [MCP Automated Agents](./mcp-automated-agents/)
- [Connected-Agent Handoffs](./connected-agent-handoffs/)
- [Automation Examples For Teams](./automation-examples-for-teams/)

Those pages cover the public contract. This playbook shows how to combine them into a demo shape that is easy to understand on camera and easy for an agent to reproduce.

## The learning loop

When an agent is asked to create a demo payload, it should follow this sequence:

1. Read the relevant Tandem docs for the target abstraction.
2. Confirm the current engine surface, schema, or control-panel path.
3. Choose the smallest demo that proves one idea.
4. Write the payload with explicit handoffs, artifacts, and tool limits.
5. Preview or validate the payload before applying it.
6. Run it once and inspect the resulting artifact or run history.
7. Save the final payload somewhere reusable so future agents do not start from scratch.

The important habit is to begin with the docs and the engine contract, not with a free-form guess.

## Three demo shapes that work well

### 1. Smart heartbeat skip path

Use this when you want to show that Tandem can poll intelligently and skip work when there is nothing to do.

What to include:

- a fast triage stage
- `metadata.triage_gate: true` on the checking node
- structured output that includes `has_work`
- downstream nodes that only run when work exists

Why it works:

- it proves that Tandem can save compute on empty cycles
- it makes the skip behavior visible and concrete
- it is easy to explain in one sentence

### 2. MCP-guardrailed triage

Use this when you want to show that Tandem can connect to MCP tools without giving an agent the whole universe.

What to include:

- a tiny `tool_policy.allowlist`
- a restricted `mcp_policy.allowed_servers`
- one triage node and one follow-up node
- a narrow output target that shows exactly what the agent decided

Why it works:

- it proves the tool isolation story
- it keeps prompts short and readable
- it demonstrates that agent power can be constrained instead of widened by default

### 3. Approval-gated multi-stage flow

Use this when you want to show human control over a high-impact workflow.

What to include:

- a handoff or approval boundary
- `requires_approval: true` for routine-based flows, or an Automation V2 approval node before the only mutation-capable step
- a review step before anything downstream consumes the artifact
- separate nodes for draft/create, approval, and post-approval execution
- node-level tool/MCP policies so only the post-approval node has send/publish tools
- a final output path that makes the result visible

`handoff_config.auto_approve: false` only stages an envelope in the handoff inbox. It does not create a governed approval gate or a public approve action.

Why it works:

- it shows governance instead of just automation
- it makes Tandem feel safe enough for real work
- it gives the user a clear pause point before promotion

Do not give the approval gate external mutation tools. The gate should collect the decision only. A downstream execution node should read the upstream draft/artifact id and call the concrete approved tool, such as a namespaced `mcp.*.gmail_send_draft` action.

## What agents should optimize for

Ask the agent to keep the payload:

- small enough to understand at a glance
- explicit enough to run repeatedly
- narrow enough to show policy boundaries
- strong enough to produce a real artifact

In practice, that usually means:

- 1 to 3 nodes or stages
- one responsibility per node
- `depends_on` only for real handoffs
- explicit artifact paths
- minimal tool allowlists
- a clear run/preview/apply story

## What to look for in the docs

Tell the agent to extract these facts from the docs before it drafts a payload:

- which Tandem abstraction fits the request
- whether the run should be a workflow plan, a V2 automation, a mission builder output, or a scheduled routine
- how the stage boundaries should be written
- what the approval or handoff boundary should be
- which MCP servers or tools are actually required
- which artifact path should hold the result

If the answer is not in the docs, the agent should stop and narrow the scope instead of inventing a schema.

## JSON handoff examples

For a raw automation payload generated from human intent:

```ts
import { readFile } from "node:fs/promises";

const payload = JSON.parse(await readFile("./demo-automation.json", "utf8"));
const created = await client.automationsV2.create(payload as any);
await client.automationsV2.runNow(created.automation?.automation_id ?? created.automation_id);
```

```python
import json
from pathlib import Path

payload = json.loads(Path("./demo-automation.json").read_text())
created = await client.automations_v2.create(payload)
await client.automations_v2.run_now(created.automation_id or "")
```

For a workflow-plan bundle:

```ts
const preview = await client.workflowPlans.importPreview({ bundle: planBundle });
const imported = await client.workflowPlans.importPlan({
  bundle: preview.bundle ?? planBundle,
});
```

This is the bridge that lets an agent move from intent to a runnable Tandem object without guessing which API surface to use.

## Reusable prompt template

Use this when instructing another agent to create a showcase payload:

```text
Create a Tandem showcase payload for the following goal.

Requirements:
- Use the smallest design that proves the feature.
- Keep the graph to 1-3 nodes unless a larger graph is required.
- Use explicit depends_on only for real handoffs.
- Keep tool allowlists and MCP access as small as possible.
- Include a visible artifact path for every meaningful stage.
- If the flow can skip work, add a triage gate that can return has_work: false.
- If the flow needs human oversight, add an approval or handoff boundary.
- Return valid JSON only.

Goal:
[insert goal]

Workspace root:
[insert workspace root]

Allowed MCP servers:
[insert allowed servers or none]

Output artifact:
[insert target path]
```

## Good review questions

Before applying a generated payload, check:

- Does each stage have one clear job?
- Can a human explain the whole run in one breath?
- Is the tool surface smaller than it needs to be?
- Is the skip path visible when there is no work?
- Is the approval boundary in the right place?
- Would the output still make sense if a new agent read it tomorrow?

If the answer to any of those is no, simplify the payload before publishing it.

## See also

- [Agent Workflow And Mission Quickstart](./agent-workflow-mission-quickstart/)
- [Creating And Running Workflows And Missions](./creating-and-running-workflows-and-missions/)
- [Prompting Workflows And Missions](./prompting-workflows-and-missions/)
- [MCP Automated Agents](./mcp-automated-agents/)
- [Connected-Agent Handoffs](./connected-agent-handoffs/)
