# ACME Slack Demo Harness

The five-profile ACME governance demo has two layers with different evidence
value (TAN-667 originally, made production-real by TAN-682):

## Production-path E2E (the real proof)

`crates/tandem-server/src/http/tests/acme_slack_demo_e2e.rs` executes the full
production path for all five seeded requester profiles:

- the authority graph (org units, memberships, grants) and department-tagged
  governed memory records are seeded into the **actual stores**;
- the same demo prompt is **HMAC-signed and submitted through the production
  `POST /channels/slack/events` route** for Sales, Engineering, Finance,
  Leadership, and the external contractor;
- each event resolves a distinct real `VerifiedTenantContext`, creates a
  governed session, and runs the engine loop against a **deterministic
  provider whose answer is derived from the memory that was actually injected
  into its prompt context**;
- hidden tools are asserted absent from the **model tool schema** the provider
  received (not from a fabricated receipt), and each department's forbidden
  marker data is asserted absent from both prompt context and the
  Slack-visible response;
- the final response is delivered to a **mock Slack API**
  (`chat.postMessage`) with channel/thread identity assertions;
- Finance's `mcp.invoices.read_invoices` invocation enters the **real CT-20
  approval gate** under a strict runtime auth mode: the tool never executes,
  and the approval-required policy decision plus hash-chained protected audit
  evidence are read back from the production stores;
- duplicate Slack deliveries are absorbed by the durable claim (no second run
  or post), and a full reset+replay run proves the harness is reproducible
  from the seeded dataset alone;
- each governed run persists a Slack-attributed context run (TAN-686):
  selectable via `GET /context/runs?run_type=session&source=channel:slack`,
  with the run ledger and (under premium governance) the governance-evidence
  package correlating the Slack requester identity and the approval-gate
  policy decision end to end — this is what the control panel's Slack
  Governance Receipts page reads.

Run everything (fixture + E2E) with one command — the same tests run in
required CI via the workspace nextest job:

```bash
cargo test -p tandem-server acme_slack_demo --lib
```

or process-isolated, as CI runs it:

```bash
cargo nextest run -p tandem-server -E 'test(acme_slack_demo)'
```

The prompt every profile asks:

```text
@tandem what changed with customer ACME this week?
```

## Receipt fixture (shape contract only — not E2E evidence)

`crate::acme_demo::harness::acme_slack_demo_receipt_fixture()` synthesizes the
control-panel governance-receipt JSON (Slack identity, resolved principal,
tenant context, department role/grants, memory returned vs hidden, tools
offered/used/hidden/blocked, policy decisions, approvals, redactions, final
response) directly from the seeded `acme_demo_dataset()`. It executes nothing
— no ingress, sessions, engine, policy, approvals, or persistence — and exists
solely to pin the receipt shape the control panel consumes. Do not cite its
output as end-to-end governance evidence; connecting the receipt screen to
runs persisted by the production path is TAN-686.
