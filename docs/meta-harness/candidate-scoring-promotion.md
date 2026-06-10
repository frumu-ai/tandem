# Candidate Scoring and Promotion

## Goal

This document defines how scored traces become ranked candidates and how candidates move through promotion states. It complements `optimizer-loop.md` and the existing score model in `crates/tandem-meta-harness-eval/src/scoring.rs`.

## Candidate scoring path

1. **Scored trace/result**: one evaluated observation with score, fixture/task identity, version identity, and any score dimensions already produced by the evaluation harness.
2. **Scored version summary**: aggregation of observations for a candidate version compared with a baseline version. This is the design-level counterpart to the existing scored version model test surface in `crates/tandem-meta-harness-eval/tests/scored_version_model.rs`.
3. **Candidate score summary**: review-ready summary containing aggregate score, baseline score, absolute delta, relative delta, regression count, confidence indicator, coverage, and trace evidence links.
4. **Ranked candidate**: candidate score summary plus policy evaluation and tie-break metadata.

No speculative implementation change is required for `scoring.rs`; future code should adapt the existing structures or add a thin proposer-facing adapter only when implementation work begins.

## Scoring interpretation

- **Score delta**: primary signal. A positive delta over baseline is required for automated promotion and normally expected for manual approval.
- **Regressions**: any material per-fixture or per-dimension regression must block automated promotion and surface a risk flag for human review.
- **Confidence**: reflects evidence volume, repeatability, and variance where available. Low confidence does not necessarily reject a candidate, but it lowers rank and requires manual approval.
- **Coverage**: candidates lacking required trace coverage are not promotable until re-evaluated or explicitly waived.
- **Tie-breaking**: if aggregate deltas are effectively equal, prefer the candidate with fewer regressions, higher confidence, broader coverage, smaller/risk-lower diff, then earlier completed evaluation time for determinism.

## Promotion lifecycle

The MVP lifecycle uses the following states:

- `pending_approval`: proposal passed minimum evidence checks and is waiting for manual or automated decision.
- `rejected`: proposal was declined by policy or reviewer. Rejected candidates remain auditable and are not eligible for promotion without a new evaluation/proposal.
- `approved`: proposal was approved for a specific promotion target but has not yet been promoted.
- `promoted`: proposal was successfully applied to the promotion target.
- `superseded`: proposal is no longer the selected candidate because another approved/promoted candidate replaced it or because the target moved forward.

Allowed transitions:

| From | To | Required condition |
| --- | --- | --- |
| `pending_approval` | `approved` | Human approval or automated policy pass with sufficient evidence |
| `pending_approval` | `rejected` | Human rejection, hard regression, missing required evidence, or policy failure |
| `approved` | `promoted` | Promotion action succeeds for the recorded target |
| `approved` | `superseded` | Another candidate is promoted or the target changes before promotion |
| `pending_approval` | `superseded` | Candidate is no longer comparable to the current target |
| `promoted` | `superseded` | A later candidate is promoted to the same target |

## Promotion gates

### Automated promotion

Automated promotion is allowed only when all MVP gates pass:

- Aggregate score delta is above the configured threshold.
- No material regression flags are present.
- Confidence is at or above the configured minimum.
- Required trace coverage is complete.
- Candidate targets an automation-enabled promotion surface.
- Audit record can be written before and after promotion.

### Human-approved promotion

Human approval may override low confidence, minor regressions, or incomplete non-critical coverage only when the reviewer records a rationale. Human approval may not bypass missing identity/provenance, absent score summary, or an unavailable promotion target.

## Consistency with current tests

The existing `scored_version_model.rs` test surface is treated as the compatibility anchor for score aggregation and version comparison semantics. If implementation later discovers that this lifecycle needs fields not represented by that model, the follow-up should add adapter tests rather than changing score meaning silently.

## MVP versus later enhancements

- **MVP:** deterministic ranking, explicit lifecycle states, manual approval, and conservative automated promotion gates.
- **Later:** learned ranking, richer confidence modeling, target-specific policy simulation, and multi-reviewer approval workflows.

## Open questions

- Exact numeric thresholds for "material" regression and "sufficient" confidence are policy decisions and should be configured per promotion target.
- Whether confidence should be stored as a normalized scalar, enum, or explanation object should be decided during implementation.
