# ACME Slack Demo Harness

TAN-667 adds a deterministic replay harness for the department-scoped Slack agent demo.

**Scope honesty:** this harness is a fixture-backed receipt-shape validator, not
an end-to-end run. `run_acme_slack_demo_harness()` maps the seeded
`acme_demo_dataset()` profiles into governance-receipt JSON and validates the
receipt contract. It does **not** exercise signed Slack ingress, construct real
sessions, run the engine loop, query memory, invoke the policy/approval
systems, persist audit evidence, or deliver a Slack response. The
production-path five-profile E2E (real ingress → engine → policy → memory →
persisted evidence → receipt APIs) is tracked as TAN-682, and connecting the
control-panel receipt screen to persisted runs as TAN-686. Do not cite this
harness's output as end-to-end governance evidence.

Run the five-profile fixture harness with:

```bash
cargo test -p tandem-server acme_slack_demo_harness --lib
```

The harness replays one Slack-style prompt across the seeded requester profiles:

```text
@tandem what changed with customer ACME this week?
```

Profiles covered:

- Sales user
- Engineering user
- Finance user
- Leadership user
- External contractor

For each profile the harness emits and validates a control-panel-compatible
governance receipt shape containing Slack identity, resolved Tandem principal,
tenant/runtime context, department role/grants, memory returned vs hidden,
tools offered/used/hidden or blocked by approval, policy decisions,
approval-required events, redactions, and the final Slack-visible response —
all derived from the fixture dataset, not from a live governed run.

The harness is resettable because it consumes `acme_demo_dataset()` only; no
live Slack delivery, manual copy/paste, or durable external state is required.
