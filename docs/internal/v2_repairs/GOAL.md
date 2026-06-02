# Automations V2 Reliability Remediation Goal

Status: complete
Last updated: 2026-06-02
Owner: Engine / Workflow Runtime
Source plan: `docs/internal/v2_repairs/AUTOMATIONS_V2_RELIABILITY_REMEDIATION_PLAN.md`

## Objective

Make Automations V2 trustworthy for governed workflows by closing the gaps where a run can silently complete without doing the assigned work, hang indefinitely in a parked state, or fail/block work that should be repairable.

The reliability target is not "all runs pass." The target is stricter: a run must only complete when its node graph, declared deliverables, and required side effects have verifiable terminal evidence. Missing work must become repairable, blocked, or failed with actionable evidence.

## Success Criteria

The workstream is complete when all of the following are true:

- Run completion is gated by a contract-aware deliverable assertion, not just empty `pending_nodes`.
- The checkpoint invariant prevents inconsistent terminal states from being marked `Completed`.
- Required external side effects have successful receipts before completion.
- Missing or weak deliverables requeue repairable owner nodes while attempt budget remains.
- Workflow graph validation rejects cycles and normalizes / validates `input_refs` against readiness dependencies.
- Verification failures retry through the same repair path as other repairable validation failures.
- Verification command matching and failure classification no longer fail runs based on unrelated artifact prose or loose substring matches.
- Timer-triggered automations dedupe in-flight runs the same way watch-triggered automations do.
- Long-lived parked states have an explicit lifecycle policy: expire, escalate, auto-resume, or intentionally remain manual with visible status.
- Warning outcomes have one policy across downstream gating, workflow learning, and lifecycle reporting.
- Regression tests cover false-completion, false-failure, graph validation, side-effect receipt, and parked-state behavior.

## Non-goals

- No broad rewrite of `automation_v2/executor.rs` beyond the reliability gates needed here.
- No cross-restart live session resume unless C8 is explicitly accepted as in scope.
- No new runtime capability-grant UX until D1 design decisions are made.
- No blanket rejection of all warnings; warning behavior must follow the explicit B8/E10 policy decision.
- No connector-specific redesign unless a receipt requirement exposes a missing connector contract.

## Completion Gates

### Gate 1 - Terminal Integrity

Evidence required:

- Tests prove that a run cannot complete when a node is missing from terminal accounting.
- Tests prove that `completed_nodes` entries without node outputs do not satisfy completion.
- Tests prove that `needs_repair`, `verify_failed`, `failed`, and `blocked` outputs are never counted as completed.

Primary cards: X3, B2.

### Gate 2 - Deliverable Integrity

Evidence required:

- Tests prove that missing required run artifacts prevent completion.
- Tests prove that non-substantive markdown/report artifacts prevent completion.
- Tests prove that JSON artifacts must parse and satisfy required shape.
- Tests prove that required workspace writes and publish targets use the same resolver as completion assertions.
- Tests prove that repairable missing deliverables requeue the owning node while attempts remain.

Primary cards: X1, B2, E8, E9, B3/B4/B5.

### Gate 3 - Side-Effect Integrity

Evidence required:

- Tests prove that required email / connector mutations require successful tool-effect or connector receipts.
- Tests prove that model prose cannot satisfy required governed side effects.
- Tests prove that permission timeout / auto-approve-denied cases become loud blocked or failed states for required side effects.

Primary cards: X4, C10, C9, C5.

### Gate 4 - Graph Integrity

Evidence required:

- Plan validation rejects cycles across `depends_on` and `input_refs`.
- Readiness uses the same normalized dependency view as validation.
- `StrictSequential` either preserves topological requirements or rejects contradictory order.
- Compaction preserves/remaps `input_refs` instead of clearing them.

Primary cards: E1, E5, E6, E4.

### Gate 5 - Repair Integrity

Evidence required:

- `verify_failed` retries until `max_attempts` before terminal failure.
- Verification marker scans are scoped to verification output, not arbitrary artifact prose.
- Verification command matching is exact enough to avoid substring false positives.
- Recoverable tool errors either surface to the model for adaptation or retry without duplicate side effects.

Primary cards: C1, C2/C3, C5.

### Gate 6 - Parked-State Lifecycle

Evidence required:

- Approval gates have a deadline and configured expiry action, or an explicit manual-only policy with visible stale status.
- `GuardrailStopped` runs resume automatically once the corresponding override is approved.
- Stale reaping is based on no-progress / idle behavior and does not reap active work.

Primary cards: X2, A1, D2, C7, D3.

## Minimum Viable Slice

The recommended first implementation slice is:

1. X3 checkpoint invariant.
2. X1/B2 contract-aware deliverable assertion.
3. X4 side-effect receipt assertion for the highest-risk governed actions.
4. E1/E5 graph validation using `depends_on + input_refs`.
5. E7 timer dedup.
6. C1/C2/C3 verification retry and classification fixes.

This slice targets the two trust-eroding classes first: silent false-completion and loud false-failure.

## Policy Decisions

Resolved in this workstream:

- A1: approval gates use a manual-only lifecycle with visible stale status after the configured deadline; no automatic approve/deny action is taken.
- C9: per-run tool guard budgets are enforced by default; `.env.example` matches the code default and documents the email cap.
- C10: write-required/headless permission timeout and auto-approve-denied cases fail loudly instead of letting the model continue normally.
- B8/E10: `accepted_with_warnings` remains passable only with no unmet requirements, but is not a clean workflow-learning validation pass and does not generate positive learning evidence.

Still design-gated / out of current non-goal scope:

- D1: runtime-grantable capabilities and approval surface.
- C8: cross-restart live run resume; current non-goal accepts fail-loud/manual re-trigger behavior unless explicitly scoped in.

## Verification Plan

Before marking this goal complete:

- Inspect the implemented code paths, not just test names.
- Confirm each completion gate above has direct regression coverage.
- Run the relevant plan compiler, automation V2 executor, automation node-output, engine-loop, and scheduler tests.
- Confirm docs and kanban statuses match the implemented state.
- Confirm ignored docs are intentionally tracked or copied into the desired project channel if git tracking is required.
