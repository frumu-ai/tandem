# ACME Slack Demo Harness

TAN-667 adds a deterministic replay harness for the department-scoped Slack agent demo.

Run the full five-profile harness with:

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

For each profile the harness emits and validates a control-panel-compatible governance receipt containing Slack identity, resolved Tandem principal, tenant/runtime context, department role/grants, memory returned vs hidden, tools offered/used/hidden or blocked by approval, policy decisions, approval-required events, redactions, and the final Slack-visible response.

The harness is resettable because it consumes `acme_demo_dataset()` only; no live Slack delivery, manual copy/paste, or durable external state is required.
