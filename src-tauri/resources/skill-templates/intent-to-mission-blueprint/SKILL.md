---
name: intent-to-mission-blueprint
description: Convert human operational intent into a Tandem mission blueprint with phased workstreams, explicit handoffs, reusable outputs, and recurrence-aware setup.
version: 1.0.0
tags:
  - tandem
  - mission
  - workflow
  - orchestration
  - autonomy
compatibility: tandem
---

# Intent To Mission Blueprint

## Purpose

Use this skill when a human describes an ongoing operational goal and you need to turn that intent into a Tandem `MissionBlueprint`.

This skill is specifically for translating _human intent_ into:

- a mission goal
- success criteria
- shared context
- workstreams
- dependencies
- output contracts
- review or approval gates
- recurrence-aware setup

It is not for executing the mission itself.

## What Good Output Looks Like

Return one mission blueprint only.

The blueprint should be:

- staged
- scoped
- operationally understandable
- safe for long-running reuse
- concrete enough to compile without hand-waving

The result should feel like something an operator could schedule and trust, not a brainstorming outline.

## Core Translation Rule

Translate the user’s intent into the _smallest coherent staged mission_.

Do not:

- explode one intent into ten weak stages
- collapse everything into one vague workstream
- create review gates everywhere
- treat raw intermediate output as reusable truth

## How To Interpret Human Intent

### 1. Find the real mission outcome

Convert vague requests into one concrete shared outcome.

Example:

- Human intent: "keep the team updated on important operational changes every morning"
- Mission goal: "Produce a reviewed daily operations update with validated changes, recommended actions, and an operator-ready handoff."

### 2. Find the repeated operating loop

Most long-running missions follow one of a few durable shapes:

- monitor -> analyze -> decide -> handoff
- intake -> plan -> execute -> verify -> review
- collect -> consolidate -> update state -> notify

Choose the smallest loop that matches the intent.

### 3. Identify the real handoffs

Each downstream stage should consume a clear upstream output.

Use `depends_on` and `input_refs` only where the handoff is real.

### 4. Design for recurrence

If the mission will run daily, weekly, or continuously:

- prefer stable artifacts
- make later stages reuse validated upstream work
- avoid forcing broad rediscovery every run
- include review or approval only where trust actually matters

### 5. Keep promoted outputs distinct from raw working state

Do not design the mission so every stage treats draft notes as durable truth.

Instead:

- discovery stages produce working artifacts
- synthesis or execution stages produce decision-ready artifacts
- review or approval stages determine what is ready for downstream reuse

## Mission Authoring Rules

### Goal

The mission goal should describe the end state, not the implementation steps.

### Success criteria

Use measurable or inspectable checks.

Good:

- "Final handoff identifies validated changes, required decisions, and clear next actions."

Weak:

- "Mission is helpful and thorough."

### Shared context

Include only stable mission-wide constraints:

- audience
- deadlines
- allowed sources
- risk constraints
- compliance or review rules
- quality bar
- cadence assumptions

### Workstreams

Each workstream must have:

- one responsibility
- one main artifact or result
- a clear prompt
- bounded scope

### Review stages

Add review, test, or approval stages only when they:

- protect downstream trust
- control promotion or delivery
- validate external action readiness
- catch failures that earlier stages are likely to miss

## Long-Running Mission Guidance

When the intent implies recurring operation over weeks or months:

- assume project-scoped knowledge reuse
- keep trust floor at promoted by default
- design stages so later runs can build on validated outputs
- avoid requiring every run to rediscover unchanged context
- keep stages inspectable and repairable

## Default Staged Patterns

### Pattern 1: Monitor -> Analyze -> Decide -> Handoff

Use for:

- operational monitoring
- workflow surveillance
- recurring change detection
- market or environment tracking

### Pattern 2: Intake -> Plan -> Execute -> Verify -> Review

Use for:

- execution pipelines
- implementation work
- structured project delivery
- repeated operational processing

### Pattern 3: Collect -> Consolidate -> Update State -> Notify

Use for:

- recurring state refresh
- knowledge or system updates
- periodic status synthesis
- publish/update loops

## Prompt-Writing Standard For Workstreams

Every workstream prompt should include:

- role
- local mission
- allowed inputs
- output contract
- guardrails

Good prompt skeleton:

```text
Act as the [role]. Use the named upstream inputs and approved workspace context only. Produce the required output in a form the next stage can consume directly. Preserve relevant evidence, avoid redoing upstream work, and record uncertainty or blockers explicitly.
```

## Output Rules

Return valid YAML only.

Include:

- `id`
- `label`
- `description`
- `schedule_defaults`
- `blueprint`

The `blueprint` must contain:

- `mission_id`
- `title`
- `goal`
- `success_criteria`
- `shared_context`
- `workspace_root`
- `phases`
- `milestones`
- `team`
- `workstreams`
- `review_stages`

## Strong Final Meta-Prompt

Use this when asked to generate a Tandem mission blueprint from human intent:

```text
Convert the following human operational intent into one Tandem mission blueprint.

Requirements:
- Return YAML only.
- Produce one shared mission goal.
- Use the smallest coherent staged mission shape.
- Give each workstream one clear responsibility.
- Use explicit dependencies and input refs only for real handoffs.
- Every workstream must have a concrete prompt and output contract.
- Design for recurring execution when the intent implies daily, weekly, or long-running operation.
- Default to project-scoped promoted knowledge reuse through validated outputs.
- Add review or approval gates only where they materially improve trust, verification, or promotion control.
- Avoid vague stages, vague prompts, and vague outputs.

Human intent:
[insert intent]

Constraints:
[insert constraints]

Preferred workspace root:
[insert workspace root]

Preferred cadence:
[insert cadence]
```
