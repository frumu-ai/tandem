---
title: Prompting Workflows And Missions
description: How to turn human intent into strong Tandem workflows and missions that remain reliable across stages, runs, and schedules.
---

Use this guide when you are authoring prompts for:

- workflow nodes
- V2 automations
- mission blueprints
- staged recurring missions
- LLM helpers that generate Tandem workflow or mission specs

The goal is not to write a clever prompt. The goal is to give Tandem enough structure that the engine can run the work repeatedly, hand off cleanly between stages, and reuse prior validated knowledge without turning memory into a garbage dump.

## The core idea

Prompt Tandem around **bounded stages, explicit handoffs, and concrete outputs**.

Do not prompt it like a one-shot chat assistant.

Strong Tandem prompts do four things well:

1. state the exact stage objective
2. name the allowed inputs and scope boundaries
3. define the output contract and artifact expectations
4. explain how the next stage should be able to use the result

## Choose the right abstraction

### Use a workflow when

- the task is one bounded pipeline
- the stages are tightly related
- the whole run is meant to complete as one automation

### Use a mission when

- the overall goal spans multiple workflows or workstreams
- later stages should begin only after earlier stages complete
- you want explicit review or approval gates
- you want recurring or long-running staged operations over days, weeks, or months

As a rule of thumb:

- workflows are the bounded execution pipelines
- missions are the larger staged operating plans

## What good prompting looks like in Tandem

Every stage prompt should make these items obvious:

- **Purpose**: what this stage is responsible for
- **Inputs**: what it may read and what it must ignore
- **Output**: what artifact, structured object, or downstream handoff it must produce
- **Constraints**: what it must preserve, not invent, or not repeat
- **Completion**: what has to be true before the stage may finish

If any of those are vague, downstream runs tend to drift.

## Prompting a workflow node

When writing a node or step prompt, prefer this structure:

```text
Role:
You are the [role] for this Tandem step.

Stage objective:
[one precise sentence about this step only]

Allowed inputs:
- [upstream artifacts]
- [workspace or project scope]
- [specific external sources if permitted]

Required output:
- Create or update [artifact path or output object]
- The output must contain [required sections, fields, or schema]
- The result must be usable by [next step or consumer]

Constraints:
- Preserve relevant upstream evidence and decisions.
- Do not invent unsupported facts or fill gaps silently.
- Do not redo earlier stages unless the current assignment explicitly requires it.
- Stay within this step's scope.

Completion criteria:
- The required output exists.
- The output satisfies the declared contract.
- Any unresolved gaps are listed explicitly instead of guessed.
```

That shape is much stronger than a short instruction like “research this” or “summarize that.”

## Prompting a mission blueprint generator

If you are asking an LLM to produce a Tandem mission blueprint, do **not** ask for “a mission” in one blob.

Ask for:

- one shared mission goal
- several scoped workstreams
- explicit `depends_on` handoffs
- concrete output contracts for every workstream
- review or approval stages only where they materially improve reliability
- a schedule recommendation when the intent implies recurring execution
- project-scoped knowledge reuse by default

The model should be told to optimize for:

- bounded stage scope
- durable handoffs
- predictable scheduling
- project-first knowledge reuse
- validated outputs feeding later stages

## A strong meta-prompt for mission generation

Use this when another LLM is generating a mission blueprint from human intent:

```text
Design a Tandem mission blueprint for the following objective.

Requirements:
- Return one mission blueprint only.
- Use one shared mission goal and 3-7 scoped workstreams.
- Give each workstream one clear responsibility.
- Use explicit depends_on and input_refs only for real handoffs.
- Every workstream must include a concrete prompt and output_contract.
- Add review, test, or approval stages only where they materially improve quality or promotion safety.
- Design the mission for repeated execution if the objective implies daily, weekly, or long-running operation.
- Default to project-scoped promoted knowledge reuse.
- Do not rely on flat global memory.
- Do not treat raw intermediate output as shared truth.
- Keep stage prompts specific about evidence, format, and downstream usability.
- Return valid YAML or JSON only.

Objective:
[insert objective]

Shared constraints:
[insert constraints]

Workspace root:
[insert workspace root]
```

## How to structure long-running staged missions

For recurring missions, prefer a pattern like this:

1. collect or inspect the current state
2. analyze or consolidate the findings
3. make a recommendation, decision, or plan
4. produce the handoff, update, artifact, or execution step
5. review or approve only where needed

The important thing is not the exact labels. It is that each stage has:

- one durable job
- one clear downstream consumer
- one inspectable output

### Smart Heartbeat Monitor Pattern

If your mission or workflow is meant to check something constantly on a schedule, avoid having a single stage that checks and performs the work. Instead, prompt for a separation:

1. **Triage Gate**: An `assess` stage using a fast, cheap model that checks if there is any work to do, producing a structured output indicating `has_work: false`.
2. **Execution Gate**: A downstream stage that actually performs the logic, conditioned on the triage gate.

Tandem will naturally recognize `has_work: false` and cleanly skip downstream execution, saving massive amounts of compute and tokens during empty polling cycles.

## Project knowledge and reuse

Generated missions and workflows should now start from **project-scoped promoted knowledge** by default.

That means:

- raw run notes stay local to the run
- validated outputs can be promoted for reuse
- later runs can preflight prior knowledge before recomputing

Your prompts should support that model.

Good prompting for reuse says:

- what should be preserved from upstream work
- what must be promoted only after validation
- what should remain local working state
- when the stage should reuse existing project knowledge instead of starting over

Bad prompting for reuse says:

- “search memory for anything relevant”
- “remember this forever”
- “use whatever you already know”

Those patterns create retrieval sprawl.

## When to tell Tandem to reuse prior knowledge

Prompts should encourage reuse when:

- the task is a continuation or refinement of earlier work
- the stage depends on prior decisions, constraints, or evidence
- the workflow is recurring and should avoid redoing stable work
- the stage is expected to build on promoted project knowledge

Prompts should avoid default reuse when:

- the task is purely local and deterministic
- the stage already has all needed upstream inputs
- the work is intentionally fresh exploration
- raw intermediate output has not been validated yet

## What to avoid

Avoid these common prompt failures:

- **Vague stage scope**: “figure out what to do next”
- **Missing handoff contract**: no artifact or schema for the next stage
- **Overloaded stages**: one step tries to inspect, decide, execute, and review
- **Implicit reuse**: assuming the agent will infer which prior knowledge to use
- **Flat memory language**: encouraging broad global recall instead of scoped reuse
- **No completion rule**: the agent can stop after a weak summary instead of producing the required artifact

## Good patterns by workflow type

### Research and synthesis

- separate discovery from extraction from synthesis
- require evidence-preserving outputs
- promote only validated claims

### Coding and debugging

- separate diagnosis from implementation from verification
- make the verification artifact explicit
- reuse prior fixes, constraints, and decisions only when the task family matches

### Operations and support

- separate intake from triage from action from notification
- keep state updates explicit
- do not let transient incident notes become default reusable truth

### Planning and execution

- separate intake, planning, execution, and review
- preserve the decision rationale for later runs
- reuse approved defaults, not raw brainstorming

## Practical authoring rules

When you or another LLM writes Tandem workflow or mission prompts:

- start from the contract, not the stage name
- write for repeatability, not just one successful demo run
- prefer artifact-backed handoffs over long rolling prose context

If you are specifically generating a showcase payload that needs to teach other agents how Tandem works, use [Tandem Wow Demo Playbook](./tandem-wow-demo-playbook/) as the first reference. That page shows how to combine the core docs into a small, readable, reusable demo.

- keep each stage narrow enough that failure and retry are understandable
- tell the stage how the next stage will consume the output
- assume the engine owns scheduling, validation, and policy gates

## Where this shows up in Tandem

Use this guidance when working in:

- the Advanced Swarm Builder / mission builder
- workflow plan generation
- V2 automation authoring
- scheduled mission design
- agent-authored workflow or mission specs

The same discipline applies whether the blueprint is created by a human, an LLM, or a hybrid flow.

## See also

- [Agent Workflow And Mission Quickstart](https://docs.tandem.ac/agent-workflow-mission-quickstart/)
- [Creating And Running Workflows And Missions](https://docs.tandem.ac/creating-and-running-workflows-and-missions/)
- [Engine Authentication For Agents](https://docs.tandem.ac/engine-authentication-for-agents/)
- [Control Panel (Web Admin)](https://docs.tandem.ac/control-panel/)
- [Agents & Sessions](https://docs.tandem.ac/agents-and-sessions/)
- [MCP Automated Agents](https://docs.tandem.ac/mcp-automated-agents/)
- [Scheduling Workflows And Automations](https://docs.tandem.ac/sdk/scheduling-automations/)
- [Architecture](https://docs.tandem.ac/architecture/)
