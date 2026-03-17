# Tandem North Star

Tandem is becoming a durable runtime for autonomous work.

The goal is not to build another chat assistant, IDE wrapper, or transcript-driven agent shell. The goal is to build the system of record for long-running work across coding, research, writing, publishing, and recurring automation.

Chat will remain a useful interface. It will not be the source of truth.

The source of truth should be the engine: runs, tasks, blackboards, checkpoints, artifacts, validations, approvals, receipts, and replayable event history.

---

## Why Tandem Exists

Most agent products still treat the conversation as the runtime.

That is good enough for short demos. It breaks down under real work.

When the transcript is doing too much, systems become fragile:

- long-running tasks lose context
- retries behave inconsistently
- failures are hard to audit
- resume behavior is weak
- parallel agents collide or duplicate work
- tool calls are not durable enough
- non-coding workflows get bolted on as separate systems

Tandem exists to solve that problem.

We believe autonomous execution is fundamentally a systems problem, not a chat problem.

---

## What Tandem Is Becoming

Tandem is becoming a unified runtime for autonomous work with:

- engine-owned state instead of transcript-owned state
- blackboard-based coordination instead of ad hoc memory sharing
- one canonical run journal for lineage, replay, and recovery
- isolated execution scopes for safe parallel work
- runtime-owned artifacts, validations, and approvals
- approval-aware external actions with durable receipts
- one shared execution model across multiple domains

The intended outcome is a system that can:

- inspect context
- form a plan
- break work into tasks
- delegate execution
- coordinate through blackboards
- validate outputs
- recover from failure
- resume safely
- operate continuously over time

---

## Product Direction

Tandem should become three things at once.

### A durable autonomous coding runtime

It should be able to understand a repository, plan work, isolate changes, run tests, create commits and pull requests, track issues, and support review and merge workflows through durable runtime state.

### A general autonomous work engine

The same runtime should support research, writing, publishing, reporting, and recurring automation through the same core execution model, not through siloed subsystems.

### A control layer for long-running AI work

Tandem should provide structure, observability, recovery, and coordination for work that lasts longer than a single session and spans many tools, steps, and actors.

Over time, Tandem should also become a system that can safely help plan, implement, validate, and maintain Tandem itself.

---

## North Star Outcome

The north star is a system where Tandem can take ownership of a workspace, repository, or execution domain and operate on it durably over time.

In that future state, Tandem can:

- build a repo-aware or domain-aware plan from context
- divide work into sub-tasks and workstreams
- delegate execution across scoped orchestrators and workers
- coordinate through a canonical blackboard
- validate results through explicit runtime state
- produce artifacts, commits, issues, pull requests, and receipts
- resume and repair work after interruptions
- operate continuously with human approvals where needed

That is the destination.

Not an agent that sometimes writes code.

A runtime that can own and coordinate real work.

---

## Foundational Principles

### 1. Engine-owned state is canonical

The engine owns the truth of execution.

Runs, tasks, artifacts, validations, approvals, and receipts must live in durable engine state. Session transcripts may reflect the work, but they must not define it.

### 2. Chat is an interface, not the runtime

Conversations are useful for prompting, control, and visibility. They are not the coordination substrate for long-running autonomous work.

### 3. Blackboard coordination beats transcript coordination

Agents should coordinate through explicit shared state:

- facts
- decisions
- blockers
- artifact references
- validation results
- task ownership
- run lineage

They should not depend on conversation memory as the primary synchronization mechanism.

### 4. One runtime spine, many authoring surfaces

Tandem may expose multiple ways to describe work:

- sessions
- missions
- planners
- workflows
- packs
- routines
- control-panel flows

These can differ at the surface, but they should compile into one shared runtime model rather than becoming separate engines.

### 5. Durable lineage matters

Important autonomous actions should be replayable, inspectable, and attributable.

If work cannot be traced, resumed, and audited, it is not ready to be trusted.

### 6. Isolation is mandatory for parallel work

Meaningful parallel execution requires scoped ownership:

- repo leases
- worktree leases
- write scopes
- task ownership
- cleanup rules

Parallelism without isolation becomes chaos.

### 7. Validation must be runtime-owned

Success and failure should not live only in model prose.

Validators, gates, and acceptance outcomes should be represented explicitly in runtime state.

### 8. External actions must be safe and resumable

Outbound actions such as posting, sending, publishing, or merging must support:

- dry runs
- approval
- idempotency
- receipts
- replay-safe execution

### 9. Domain expansion should reuse the same core model

Coding is the hardest proving ground, but Tandem should not solve research, writing, or publishing by inventing unrelated runtime silos.

The same primitives should be reused wherever possible.

### 10. Tandem should eventually accelerate Tandem

The long-term test of the runtime is whether it can safely help develop, maintain, and improve Tandem itself.

---

## Canonical Runtime Shape

The architecture should converge around a small set of canonical runtime primitives.

### Canonical run journal

A durable run record that supports:

- lineage
- replay
- checkpoints
- resume
- observability
- debugging

### Canonical execution graph

A shared task graph or execution spec that supports:

- dependencies
- gates
- retries
- outputs
- validation
- task ownership

### Canonical scheduler and trigger layer

A shared way to start runs from:

- manual user input
- recurring schedules
- external events
- issue or PR watchers
- automation hooks

### Canonical blackboard

A shared coordination layer for:

- facts
- decisions
- blockers
- open questions
- artifacts
- summaries

### Canonical artifact and validator model

A runtime-owned model for:

- expected outputs
- output kinds
- validation rules
- validation outcomes
- publish or merge receipts

---

## What Success Looks Like

Tandem is on the right path when the following become normal:

- every run has one clear durable identity
- work can be replayed from runtime events
- failures preserve enough evidence to recover safely and assign responsibility
- coding tasks run in isolated scopes with explicit ownership
- validators determine completion state explicitly
- external actions leave durable receipts
- research, writing, and publishing reuse the same runtime spine as coding
- contributors can tell where new behavior belongs without guessing
- agents can extend Tandem without introducing a second runtime model
- Tandem can increasingly help build and maintain Tandem

---

## What We Must Avoid

These are anti-goals.

### Do not let chat become authoritative

No feature should quietly rely on the session transcript as the primary durable state model for long-running work.

### Do not add a second runtime

New capabilities should not create a parallel execution engine unless there is an exceptional reason.

### Do not generalize through siloed domain products

Research, writing, publishing, and coding should not become disconnected subsystems with separate execution semantics when one shared runtime model can support them.

### Do not hide validation in prompts alone

Validation should be explicit in runtime state and execution flow.

### Do not allow unconstrained parallel repo mutation

Parallel coding without scoped ownership and isolation is not acceptable.

### Do not let projections replace lineage

Derived views are useful, but they must not replace durable run truth.

---

## Guidance For Contributors And Agents

When making design or implementation decisions, prefer the option that:

- strengthens engine-owned state
- reduces split-state execution paths
- increases replayability and inspectability
- reuses canonical runtime primitives
- makes validation and artifacts first-class
- improves isolation and task ownership
- pushes behavior into the engine rather than scattering it across thin clients

Avoid the option that:

- depends on transcript memory for long-running behavior
- introduces new parallel runtime abstractions without strong justification
- hardcodes domain-specific logic where a reusable runtime primitive would be better
- treats pack-level conventions as a substitute for runtime guarantees

---

## Final Statement

Tandem is not trying to become a better chat window for agents.

Tandem is trying to become the runtime that long-running autonomous work can actually trust.

The end goal is a system that can own work, coordinate work, validate work, recover work, and eventually improve itself.

That is the north star.
