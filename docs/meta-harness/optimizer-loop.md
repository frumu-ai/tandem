# Optimizer Loop

## Purpose

This public design slice describes how the meta-harness optimizer consumes scored traces and turns them into candidate proposals. It is a design contract rather than a request to change evaluator implementation. Existing scoring concepts remain owned by `crates/tandem-meta-harness-eval/src/scoring.rs`; the optimizer should consume those scored results instead of redefining score math in proposer code.

## End-to-end flow

1. **Trace execution** records task attempts, tool use, model behavior, outcomes, and evaluator metadata needed to reproduce the run.
2. **Scoring** converts traces/results into the scored model represented by the evaluator crate. The optimizer treats these records as immutable evidence: score values, per-dimension results, error/regression markers, and identifiers for the trace/result source.
3. **Aggregation** groups scored records by candidate source, baseline, target surface, and comparable evaluation set. Aggregation computes aggregate score, score delta from baseline, regression count, coverage, and confidence.
4. **Proposal generation** asks proposers to transform eligible aggregates into candidate proposals. A proposal names what would change, why the scored evidence supports it, and which promotion target it can update.
5. **Ranking and gating** compares proposals against the current baseline and each other. The best proposals move to `pending_approval`; proposals that fail gates are retained as rejected or diagnostic evidence.
6. **Approval and promotion** sends pending candidates to the approval surfaces described in `docs/meta-harness/approval-surfaces.md`. Approved candidates can then be promoted according to the lifecycle in `docs/meta-harness/candidate-scoring-promotion.md`.

## Proposer wiring

### Inputs

Each proposer receives a normalized proposal request:

- scored trace/result identifiers and score summaries from the evaluator crate;
- baseline version/model/config identifiers;
- candidate source identifiers such as run id, experiment id, prompt id, or configuration revision;
- target surface, for example prompt template, model routing policy, tool policy, harness configuration, or evaluator configuration;
- comparison context: evaluation set, time window, minimum coverage, and current promotion policy;
- risk context: known regressions, missing coverage, unstable scores, policy flags, cost/latency flags, and compatibility constraints.

### Responsibilities

A proposer is responsible for:

- mapping scored evidence to a candidate proposal without changing the underlying score semantics;
- explaining the proposed behavioral or configuration difference from the baseline;
- preserving trace evidence links so reviewers can inspect why a candidate was created;
- declaring the promotion target and any constraints for applying the candidate;
- emitting risk flags when the scored evidence contains regressions, low confidence, or incomplete coverage;
- refusing to propose a candidate when required evidence is missing or incomparable.

### Outputs

The proposer emits a candidate proposal record with:

- candidate id and source run identifiers;
- baseline id and promotion target;
- score summary, score delta, confidence, coverage, and regression summary;
- ranked evidence references to scored traces/results;
- diff or behavior summary;
- lifecycle state, initially `pending_approval` only after automated gates pass;
- audit seed data needed by approval and promotion surfaces.

### Integration points

- **Evaluator crate:** `crates/tandem-meta-harness-eval/src/scoring.rs` remains the source of scoring concepts and scored result structures. The optimizer imports or serializes those outputs; it should not duplicate scoring rules in proposer code.
- **Version model tests:** `crates/tandem-meta-harness-eval/tests/scored_version_model.rs` is the compatibility surface for how scored versions are represented. This design assumes candidate ids and baseline ids can be related to that scored version model; if the test surface cannot express that relationship, the follow-up is to extend the model rather than hide the mismatch in proposer logic.
- **Candidate lifecycle:** state transitions and promotion gates are defined in `candidate-scoring-promotion.md`.
- **Approval surfaces:** reviewer-facing requirements and audit fields are defined in `approval-surfaces.md`.

## MVP gates

For MVP, a candidate can enter human review only when:

- the scored evidence is comparable to the baseline evaluation set;
- aggregate score delta is positive or policy explicitly allows a trade-off;
- no blocking regression is present;
- confidence and coverage meet configured minimums;
- the proposer can name a concrete promotion target and behavior/diff summary;
- required audit seed data is present.

Automated promotion, where allowed, must use the same gates plus target-specific safety rules. Human approval is still required for targets that affect production behavior, safety policy, or evaluator semantics unless product policy explicitly marks the target as auto-promotable.

## Assumptions and open questions

- Assumption: scored traces/results are immutable once consumed by an optimizer run.
- Assumption: the optimizer compares candidates only within the same evaluation set unless an explicit normalization policy is added.
- Assumption: proposer output can reference raw traces by id/link instead of embedding full trace payloads.
- Open question: the exact storage boundary between optimizer proposals and evaluator scored results is not defined here.
- Open question: confidence calibration may need historical run data that is not part of the current evaluator crate.
- Open question: auto-promotion eligibility may vary by promotion target and should be configured outside this design slice.
