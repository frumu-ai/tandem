# Approval Surfaces

## Purpose

This design slice defines the human approval surfaces required for the meta-harness optimizer without importing a large raw checklist set. It complements the optimizer loop and candidate scoring/promotion design by naming the grouped capabilities reviewers need to compare, approve, reject, and promote candidates.

## MVP review surfaces

### Candidate queue

The MVP queue groups candidates by promotion target and current lifecycle state:

- `pending_approval`: candidate has passed automated proposal gates and is awaiting a decision.
- `approved`: candidate is accepted by a reviewer but has not yet been promoted to the target slot.
- `rejected`: candidate was reviewed and should not be reconsidered unless a new proposal revision is created.
- `promoted`: candidate was installed into the selected target.
- `superseded`: candidate was replaced by a better candidate, a newer run, or a changed baseline before promotion.

The queue should show enough summary information to triage review order: candidate id, source run, target, current state, aggregate score, score delta from baseline, risk flags, confidence indicator, and age.

### Candidate comparison view

A reviewer must be able to compare a candidate against the active baseline and, when useful, against other pending candidates. The minimum comparison information is:

- score summary: aggregate score, per-dimension scores, normalized deltas, confidence, and tie-break status;
- trace evidence: links or identifiers for scored traces/results that contributed to the candidate score;
- diff or behavior summary: what changed in prompt, model configuration, policy, tool wiring, or harness behavior;
- risk flags: regressions, low confidence, missing trace coverage, unstable scores, policy/safety concerns, and incompatible target assumptions;
- promotion action: the exact target that would be updated if the candidate is approved and promoted.

### Decision form

The approval UI or API must capture the required audit fields for every decision:

- approver identity;
- decision timestamp;
- decision: approve, reject, request changes/defer, promote, or supersede;
- rationale, including why regressions or low-confidence results are acceptable if applicable;
- promotion target for approve/promote decisions;
- candidate id, baseline id, scoring run id, and version/model identifiers needed to reproduce the decision context.

### Promotion control

Promotion must be a deliberate action. MVP promotion can be either:

1. reviewer approves and promotes in one step when policy allows it; or
2. reviewer approves first, then an operator or automated gate promotes the approved candidate.

Both paths must record the same audit fields and must not silently promote a candidate that is rejected, superseded, or still missing required review information.

## Later enhancements

Later approval surfaces can add richer capabilities without changing the MVP audit contract:

- side-by-side trace replay and transcript diffing;
- reviewer assignment, SLA, and notification workflows;
- batch approval for candidates with identical risk profiles;
- calibrated confidence visualizations across historical runs;
- policy-specific approval templates for safety, cost, latency, or domain quality;
- automatic superseding when a newer candidate dominates an older pending candidate.

## Assumptions and open questions

- Assumption: approval identity is supplied by the surrounding product/auth layer rather than by the evaluator crate.
- Assumption: raw trace payloads may be large, so the approval surface should link to trace evidence and show excerpts rather than embedding every event.
- Open question: whether `approved` and `promoted` are always distinct states for all promotion targets, or whether some targets permit atomic approve-and-promote transitions only.
- Open question: which promotion targets require more than one approver.
