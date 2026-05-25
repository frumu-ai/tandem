---
name: mission-blueprint-compiler
description: Design Tandem mission blueprints with scoped workstreams, explicit handoffs, review gates, and prompts that compile into reliable staged missions.
version: 1.0.0
tags:
  - tandem
  - mission
  - workflow
  - orchestration
  - prompting
compatibility: tandem
---

# Mission Blueprint Compiler

## Purpose

Use this skill when the goal is to create a Tandem mission blueprint for the Advanced Swarm Builder or any mission-compiler flow.

This skill is for authoring a _good mission blueprint_, not for doing the underlying task itself.

The output should help Tandem compile a mission with:

- one shared goal
- several scoped workstreams
- explicit stage dependencies
- concrete handoffs
- optional review, test, or approval gates
- outputs that later stages can trust and reuse

## What Tandem Expects

Tandem mission blueprints are not one giant freeform prompt. They are structured plans with prompts attached to each workstream.

At minimum, a useful mission blueprint needs:

- `mission_id`
- `title`
- `goal`
- `workspace_root`
- at least one `workstream`

Each workstream should include:

- `workstream_id`
- `title`
- `objective`
- `role`
- `prompt`
- `depends_on`
- `input_refs`
- `output_contract`

Review stages should be added only when a real quality, verification, or human approval check is needed.

## Core Authoring Rule

Do not write one prompt that tries to control the entire mission.

Instead:

1. Put the shared outcome in the mission goal.
2. Put stable facts and constraints in shared context.
3. Give each workstream one clear responsibility.
4. Make every downstream dependency explicit.
5. Define exactly what artifact or structured output each stage must produce.
6. Use review gates only where promotion, approval, or quality control matters.

## Mission Design Principles

### 1. Make the mission outcome-based

The mission goal should describe the end result, not a long checklist of internal steps.

Good:

- "Produce an approved operational rollout package for the new intake process."

Weak:

- "Read files, analyze process, write notes, revise plan, review plan, summarize results."

### 2. Give each workstream one job

A workstream should do one coherent piece of work.

Good workstream responsibilities:

- discover current state
- extract constraints
- synthesize a plan
- execute a scoped change
- verify output
- review for approval

Weak workstream responsibilities:

- "handle everything upstream"
- "do all implementation and testing and handoff"

### 3. Use dependencies only for real handoffs

Use `depends_on` when a later stage genuinely requires an upstream output.

Good dependency:

- `prepare_plan` depends on `discover_inputs`

Bad dependency:

- every stage depends on every earlier stage "just in case"

### 4. Treat outputs as contracts

Each workstream should produce something the next stage can actually use.

Prefer:

- markdown memo
- structured JSON summary
- implementation notes
- verification report
- approval decision

Avoid:

- "thoughts"
- "analysis"
- "some findings"

### 5. Separate production from review

Do not overload one workstream with both creation and approval when the output matters.

Use:

- producer workstream
- reviewer or tester stage
- optional approval gate

### 6. Keep mission-wide context stable

Use mission `shared_context` for:

- audience
- quality bar
- allowed sources
- deadlines
- risk constraints
- compliance rules
- reuse expectations

Do not bury those inside only one workstream prompt if they apply to the whole mission.

## How To Write Strong Workstream Prompts

Every workstream prompt should answer five questions:

1. Who are you?
2. What is your local assignment?
3. What inputs are you allowed to rely on?
4. What exact output must you produce?
5. What must you not do?

Use this structure:

```text
Role:
You are the [role].

Mission:
[One-sentence local assignment.]

Inputs:
Use [specific upstream handoffs, workspace context, approved sources].
Treat [named upstream artifact(s)] as the source of truth for this stage.

Output contract:
Produce [artifact or structured result] containing [required sections or fields].
Make it usable by the next stage without redoing this work.

Guardrails:
- Preserve relevant upstream evidence and constraints.
- Do not invent missing facts.
- Do not repeat earlier stages unless the prompt explicitly asks for verification.
- Keep scope limited to this workstream.
- Be explicit about uncertainty, blockers, or missing inputs.
```

## Generic Workstream Prompt Patterns

### Discovery / intake

Use when the stage should understand current state before later work begins.

```text
Act as a structured discovery operator. Inspect the available inputs, identify the most relevant facts, constraints, and unknowns, and produce a compact handoff that later stages can use without repeating broad discovery.
```

### Extraction / normalization

Use when the stage turns messy inputs into structured outputs.

```text
Act as a normalization and extraction specialist. Convert the upstream material into a structured representation with consistent terminology, concrete evidence, and clearly separated facts, assumptions, and unresolved questions.
```

### Planning / synthesis

Use when the stage should make decisions or produce a coherent plan.

```text
Act as a planning and synthesis lead. Use the upstream evidence and constraints to produce a realistic, actionable plan that preserves dependencies, tradeoffs, and operational risks.
```

### Execution

Use when the stage performs changes or produces final-form deliverables.

```text
Act as an execution specialist. Carry out the scoped task using the approved inputs and produce the required deliverable without widening scope or redoing upstream planning work.
```

### Verification

Use when the stage must validate a prior output.

```text
Act as a verification operator. Check whether the upstream output satisfies the stated contract, record what was actually verified, and return a clear pass, fail, or blocked result with concrete evidence.
```

### Review

Use when the stage should critique readiness before promotion or handoff.

```text
Act as a critical reviewer. Evaluate the upstream output against the contract, evidence quality, decision quality, and downstream readiness. Approve only if the result is genuinely ready for reuse or handoff.
```

## Review And Approval Prompt Rules

Review prompts should not be generic.

They should explicitly state:

- what is being reviewed
- what counts as sufficient evidence
- what should trigger rework
- whether promotion or downstream reuse is allowed

Strong review prompt:

```text
Review the upstream handoff for completeness, evidence quality, internal consistency, and downstream usability. Reject outputs that hide uncertainty, skip required sections, or force later stages to rediscover missing context.
```

Weak review prompt:

```text
Review this and make sure it looks good.
```

For approval stages, the prompt should describe the approval standard, while the gate controls the actual approve / rework / cancel decisions.

## Output Contract Guidance

When generating a blueprint, prefer output contracts that are concrete and easy to validate.

Good output contract shapes:

- `markdown_memo`
- `structured_json`
- `plan`
- `verification_report`
- `review_decision`
- `handoff`

Each contract should imply:

- what format downstream expects
- what sections or fields must exist
- whether the next stage can consume it directly

## Knowledge And Long-Running Missions

If the mission is part of a repeated or long-running system:

- keep raw working material local to the stage or run
- promote only validated outputs for later reuse
- make the producing stage explicit
- make the consuming stage explicit

Good pattern:

- stage 1 produces a scoped artifact
- review stage validates it
- later stages consume the validated handoff

Bad pattern:

- every later stage "just redoes discovery if needed"

## Common Failure Modes

### 1. Mission goal is too broad

If the goal sounds like an entire department's job, the compiler output will be mushy.

### 2. Workstreams are really just vague labels

If a workstream title is specific but the prompt is vague, the stage will underperform.

### 3. Outputs are not concrete enough

If downstream work cannot tell what it should read or trust, the mission will repeat work.

### 4. Too many unnecessary dependencies

This creates slow, tangled missions and weakens parallelism.

### 5. Review stage has no real approval standard

This turns review into style commentary instead of a quality gate.

### 6. Shared context is missing key constraints

Then every workstream improvises its own rules.

## What A Good Mission Authoring Request Looks Like

If you are prompting an LLM to _generate a Tandem mission blueprint_, ask for:

- one mission goal
- measurable success criteria
- optional shared context
- 3-7 workstreams with one responsibility each
- explicit dependencies
- explicit input refs
- concrete output contracts
- optional review/test/approval stages only when needed

Use this meta-prompt:

```text
Design a Tandem mission blueprint for the following objective.

Requirements:
- Return one mission blueprint only.
- Use one shared mission goal and several scoped workstreams.
- Give each workstream one clear responsibility.
- Use explicit `depends_on` and `input_refs` only for real handoffs.
- Every workstream must include a concrete `prompt` and `output_contract`.
- Add review, test, or approval stages only where they materially improve quality or control promotion.
- Keep prompts specific about evidence, format, and downstream usability.
- Do not produce vague stages or placeholder contracts.

Objective:
[insert objective]

Shared constraints:
[insert constraints]

Preferred workspace root:
[insert workspace root]

Return valid blueprint-shaped JSON or YAML only.
```

## Strong Default Blueprint Shape

For most missions, start with:

1. `discover` or `intake`
2. `analyze` or `plan`
3. `execute` or `draft`
4. `verify` or `review`
5. `handoff` or `approve`

Not every mission needs all five, but this is a reliable starting pattern.

## Final Output Rule

When using this skill to generate a blueprint:

- return a mission blueprint, not prose about the blueprint
- keep prompts stage-specific
- keep outputs concrete
- prefer clarity over cleverness
- do not invent fields outside the expected mission blueprint shape
