# Signal Triage Domain Proof

This note records the TAN-69 proof scope for applying signal-triage behavior
outside Bug Monitor.

Bug Monitor remains the first production vertical slice. The additional
domains here are deterministic server-side proofs, not a new generic framework
or user-facing API.

## Domains

### Research/Evidence

The Research/Evidence fixture proves that a signal can move from intake to a
reviewed brief or recommendation only when it has:

- a known source
- a clear title and summary
- evidence references
- sufficient confidence
- non-duplicate status
- human review before promotion
- research claims, sources, and recommendation text

Low-confidence or speculative input and missing evidence are blocked with
inspectable gate reasons.

### Use-Case Discovery

The Use-Case Discovery fixture proves that a product/workflow opportunity can
be turned into a reviewed proposal without enabling a workflow. It requires:

- observed problem evidence
- candidate use case
- expected value
- reviewed risks
- human review before promotion
- disabled-by-default rollout state
- governed memory policy if work-pattern learning is enabled

Attempts to auto-enable the candidate workflow, or to enable memory learning
without scope, review, retention, and source refs, are blocked.

## Shared Extraction Decision

Shared signal-triage primitives are still deferred. The new
`signal_triage` module intentionally stays as a deterministic proof module
with typed domain fixtures and gates. Extracting a broader framework should wait
until these domains are connected to a real intake surface and prove the same
contract under operator workflows, not only unit tests.

## Verification

Run the focused server tests:

```powershell
cargo test -p tandem-server signal_triage -- --nocapture
```

If shared Bug Monitor code is touched, also rerun:

```powershell
cargo test -p tandem-server bug_monitor -- --nocapture
```
