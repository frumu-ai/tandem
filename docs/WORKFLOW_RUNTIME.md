# Tandem Workflow Automation Runtime

## What It Is

Tandem's workflow automation runtime is the infrastructure layer that makes AI agents produce **verifiable, trustworthy artifacts** — not just responses that sound plausible.

Most AI systems are evaluated on "does it sound good?" Tandem workflows are evaluated on:

- Did it produce the exact artifact it said it would?
- Does the artifact follow the exact contract it promised?
- Is every claim in the artifact backed by verifiable evidence?
- Can the system self-heal when something goes wrong, without losing context?

This is fundamentally harder to build than a chat interface. It's the difference between "AI that talks about work" and "AI that does work and can prove it."

## Why Existing AI Systems Fall Short

AI agents are confident even when wrong. They will confidently tell you they reviewed a file that doesn't exist, cite sources that weren't read, or claim a task is complete when the artifact is hollow.

Traditional guardrails try to solve this with prompts: "be careful," "cite your sources," "don't hallucinate." These work in demos and fall apart in production because:

1. **Prompts don't enforce behavior** — they guide it. The AI can still take credit for work it didn't do.
2. **Session state is ephemeral** — there's no persistent record of what was actually read vs. what was claimed.
3. **Retries are destructive** — when a run fails, context is lost. The next attempt has no memory of what went wrong.
4. **Validation is shallow** — checking that a file exists isn't the same as checking that the file's contents match what was promised.

## What Tandem's Runtime Does Differently

### 1. Artifact Contracts

Every workflow stage declares what it will produce and what evidence backs it. These aren't suggestions — they're enforced contracts.

When a research stage says "I reviewed files A, B, and C," Tandem validates:

- Did the session actually read A, B, and C?
- Are the paths concrete (not `*.yaml` or directory placeholders)?
- Do the citations in the artifact match the reads in the session?

If any of these fail, the artifact is rejected. Not a warning — a hard block with a specific reason.

### 2. Preexisting Artifact Awareness

When a workflow retries after a failure, Tandem knows when an artifact is already valid from a prior attempt. It doesn't demand a fresh write just to satisfy current-attempt accounting.

This sounds simple but is profound in practice. Without it, the guardrail designed to prevent "lying about work" actually punishes the AI for being honest — recognizing that a file is already good and not rewriting it gets treated as a failure.

### 3. Stale State Elimination

Tandem tracks the validation outcome of every artifact at every attempt. When a research brief passes validation, downstream stages know it passed. The repair path reads current validation state, not cached failure reasons from superseded attempts.

No phantom failures. No "research coverage is still broken" messages after the research was already fixed.

### 4. Concrete Path Enforcement

Wildcard paths like `tandem/components/*.yaml` don't survive in machine-consumed fields. Every path in every structured handoff must be concrete — a real file at a real location. The runtime rejects globs, directory placeholders, and unresolvable paths before they can cause downstream failures.

### 5. Self-Healing Workflows

When a stage fails, Tandem generates repair context that tells the next attempt:

- What specifically failed
- What the current validation state of upstream artifacts is
- What the model should do differently

The workflow self-heals without manual intervention, without losing the work that was already correct, and without cascading failures into downstream stages.

## The User Experience Difference

| Before                                                                      | After                                                             |
| --------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| Artifact exists but UI says "failed"                                        | Artifact exists → UI says "passed" with clear validation trail    |
| Retry fails with "write required not satisfied" even though file is on disk | Retry surfaces: "artifact already valid from attempt 1, accepted" |
| Research brief passes but downstream still reports "coverage broken"        | Downstream sees current validation state: "research passed"       |
| AI cites `*.yaml` in source audit, downstream read fails                    | Concrete paths only — every path in handoffs is resolvable        |
| Workflow fails and requires full manual restart                             | Workflow self-heals with targeted repair context                  |

## Why This Matters

The gap between "AI in a notebook" and "AI in production" is trust. Can you trust the AI's output? Can you audit what it actually did vs. what it said it did? Can you let it run autonomously and have confidence the artifacts will be correct?

Tandem's workflow runtime closes that gap. It's infrastructure for AI operations — the layer that makes AI agents reliable enough to do real work in real systems.

The marketing content pipeline is an example. The same runtime runs any multi-stage workflow: code review, research synthesis, document generation, data processing pipelines. When the workflow says an artifact is complete and validated, it means it.

## Sharing a Generated Workflow

The portable unit for reuse is the workflow plan bundle, not the conversation transcript. A previewed or applied plan returns `plan_package_bundle`, and that bundle can be validated or imported in another workspace or install.

This is the same artifact that the Planner page hands downstream to Automations, Coding, and Orchestrator. The control panel adds convenience around naming, replay, and handoff, but the SDK and the backend are still centered on the bundle.

The reuse flow is:

1. Generate the plan with `POST /workflow-plans/preview` or `POST /workflow-plans/apply`.
2. Copy `plan_package_bundle` into the target environment.
3. Call `POST /workflow-plans/import/preview` to check `import_validation`, `plan_package_preview`, `derived_scope_snapshot`, and `summary`.
4. Call `POST /workflow-plans/import` if the import preview is compatible.
   - This creates a durable planner session with the imported draft and provenance.
   - The imported session is visible in the Workflow Center and can be reopened there.
   - It does not arm a runnable automation yet.
   - Schedule data stays staged until the user applies the plan.

Example response shape:

```json
{
  "import_validation": { "compatible": true },
  "plan_package_preview": { "lifecycle_state": "preview" },
  "derived_scope_snapshot": { "plan_id": "imported-example" },
  "summary": { "routine_count": 3, "credential_envelope_count": 2 }
}
```

If `import_validation.compatible` is false, the bundle needs to be fixed before reuse or the target workspace needs to be adjusted.

If you are driving the workflow from the SDK, the flow mirrors the HTTP calls:

```typescript
const started = await client.workflowPlans.chatStart({
  prompt: "Plan a release workflow with approval and handoff",
  planSource: "intent_planner_page",
  workspaceRoot: "/workspace/repos/tandem",
});

const revised = await client.workflowPlans.chatMessage({
  planId: started.plan.plan_id!,
  message: "Split the work into review, validate, and publish phases.",
});

const applied = await client.workflowPlans.apply({
  planId: revised.plan.plan_id!,
  creatorId: "planner-operator",
});

const previewImport = await client.workflowPlans.importPreview({
  bundle: applied.plan_package_bundle!,
});

if (previewImport.import_validation?.compatible) {
  await client.workflowPlans.importPlan({
    bundle: previewImport.bundle ?? applied.plan_package_bundle!,
  });
}
```

## How It Feels

You run a marketing campaign workflow. The AI discovers source material, researches positioning and competitors, drafts copy, reviews claims, and packages for publishing. You watch it work.

When it finishes:

- `marketing-brief.md` — validated, all citations verified, every path concrete
- `draft-post.md` — validated, claims backed by the brief's evidence
- `approved-post.md` — reviewed, no unsupported claims
- `publish-checklist.md` — complete, ready for human handoff

Every stage's output passed validation. Every artifact is trustworthy. The workflow self-healed two retry cycles without losing context.

**That's the difference.** Not "AI that ran" — AI that ran correctly, produced verifiable artifacts, and can prove it.
