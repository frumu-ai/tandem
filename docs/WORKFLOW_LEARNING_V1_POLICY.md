# Workflow Learning v1 — Production Validation & Auto-Apply Policy

**Status:** Decided (TAN-44 / GCL-04)
**Date:** 2026-06-06
**Scope:** The existing Workflow *Learning* loop (repairing/improving an existing
workflow from its own run history). This is distinct from Goal Capability
Learning, which composes a *new* workflow toward a goal.

## Background

Workflow Learning generates `WorkflowLearningCandidate`s from terminal automation
runs:

| Kind | Trigger | Stored confidence |
|------|---------|-------------------|
| `MemoryFact` | a run passes validation | 0.65 |
| `RepairHint` | ≥2 failures with the same fingerprint | 0.70 |
| `PromptPatch` | ≥2 failures with the same fingerprint | 0.75 |
| `GraphPatch` | ≥3 failures, or a still-failing applied `PromptPatch` | 0.80 |

Each candidate moves through a status lifecycle:
`Proposed → Approved / Applied / Rejected / Superseded`, plus an automatic
`Applied → Regressed`.

Before this work, two things were implicit:

1. **Promotion** (`Proposed → Applied`) was entirely human, via the review
   endpoint. Confidence was stored but never used in any decision.
2. **The before/after regression check** was hardcoded inline in
   `finalize_terminal_automation_v2_run_learning` (a `+ f64::EPSILON` comparison
   with a magic `3`-run minimum), with no documented rationale and no test.

## Decisions

### 1. Production-readiness validation confidence

A candidate is **production-ready** only when its *observed* effect is validated
against a captured baseline — not when it is merely proposed. Concretely:

- The stored per-kind `confidence` (0.65–0.80) is a **generation prior**, not a
  production-readiness signal. It reflects how likely the heuristic is to be
  *relevant*, not how likely the change is to *help in production*.
- Production-readiness is established **after apply** by the before/after gate:
  a candidate is trusted only once it has accumulated enough post-apply runs and
  has not regressed its guarded metrics (see §3).
- Therefore confidence gates *eligibility for auto-apply* (a necessary, not
  sufficient, condition), while the before/after gate establishes *validated
  production-readiness*.

### 2. Auto-promote / auto-apply policy

**Auto-apply is OFF by default and fails closed to human review.** Promotion
stays human-driven unless an operator explicitly opts in
(`TANDEM_WORKFLOW_LEARNING_AUTO_APPLY=on`). When enabled, a freshly proposed
candidate is auto-applied only if **all** of these hold (evaluated in order):

1. **Not structural.** `GraphPatch` and any candidate with `needs_plan_bundle`
   are **categorically blocked** from auto-apply — graph rewrites always require
   a human, regardless of confidence or thresholds.
2. **Confidence** ≥ `min_confidence` (default 0.80).
3. **Evidence** — the workflow's recent-run `sample_size` ≥
   `min_baseline_sample_size` (default 5).
4. **No active human steering** — the recent human-intervention rate
   (`human_intervention_count / sample_size`) ≤ `max_human_intervention_rate`
   (default 0.0, i.e. *any* recent human steering vetoes auto-apply). An empty
   sample is treated as maximal uncertainty (rate 1.0), not zero risk.

When a candidate is auto-applied, the workflow's current metrics snapshot is
captured as its `baseline_before` — exactly as the human review endpoint does —
so the before/after gate can subsequently judge it. An auto-apply emits a
`workflow_learning.candidate.auto_applied` event for audit.

Anything that is not auto-applied is left `Proposed` for the human review
endpoint — i.e. today's behavior is preserved exactly when the switch is off.

### 3. Before/after thresholds and human-intervention gates

The before/after (regression) gate runs on every applied candidate as new
terminal runs arrive:

- **Minimum post-apply sample.** No verdict is rendered until at least
  `post_apply_min_sample_size` (default 3) runs have completed *after* the
  baseline. Until then the verdict is `Insufficient` (hold).
- **Guarded metrics.** A candidate is marked `Regressed` if either
  `completion_rate` **or** `validation_pass_rate` falls below its baseline by
  more than `regression_margin` (default `f64::EPSILON`). Equal-or-better metrics
  are `Healthy`. These defaults reproduce the previous inline behavior exactly.
- **Human-intervention gate.** `human_intervention_count` (runs with a gate
  decision or an operator stop) feeds the auto-apply veto in §2. Sustained human
  intervention is treated as a signal that the workflow is *not* a safe
  auto-apply target.

## Configuration

All knobs fail closed to the conservative defaults above:

| Env var | Default | Meaning |
|---------|---------|---------|
| `TANDEM_WORKFLOW_LEARNING_AUTO_APPLY` | `off` | Master switch for auto-apply |
| `TANDEM_WORKFLOW_LEARNING_MIN_CONFIDENCE` | `0.80` | Min candidate confidence for auto-apply |
| `TANDEM_WORKFLOW_LEARNING_MIN_BASELINE_SAMPLE` | `5` | Min recent-run evidence for auto-apply |
| `TANDEM_WORKFLOW_LEARNING_MAX_HUMAN_INTERVENTION_RATE` | `0.0` | Max recent human-intervention rate before veto |
| `TANDEM_WORKFLOW_LEARNING_POST_APPLY_MIN_SAMPLE` | `3` | Post-apply runs before a regression verdict |
| `TANDEM_WORKFLOW_LEARNING_REGRESSION_MARGIN` | `f64::EPSILON` | Allowed metric slack before `Regressed` |

## Implementation

The policy is a single declarative type,
`crate::workflow_learning_policy::WorkflowLearningPromotionPolicy`, with two pure,
unit-tested functions:

- `evaluate_promotion(candidate, metrics) -> PromotionDecision`
  (`AutoApply` / `RequireHumanReview` / `Block`).
- `evaluate_regression(baseline, latest) -> RegressionVerdict`
  (`Insufficient` / `Healthy` / `Regressed`).

Both are wired into `finalize_terminal_automation_v2_run_learning`:
- newly generated candidates are routed through `evaluate_promotion` (no-op while
  auto-apply is disabled);
- the `Applied → Regressed` transition is routed through `evaluate_regression`,
  replacing the previous inline check with identical default behavior.

Because `Default` keeps auto-apply off and uses the prior regression thresholds,
routing the status machine through the policy is behavior-preserving until an
operator opts in.
