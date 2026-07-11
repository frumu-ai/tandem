# ACME Slack Demo Harness

The five-profile ACME governance demo has two layers with different evidence
value (TAN-667 originally, made production-real by TAN-682):

## Reusable live command

Stop the Tandem server, run the feature-gated command against the same state
directory, then restart the server:

```bash
cargo run -p tandem-ai --features acme-demo -- acme-slack-demo \
  --state-dir /absolute/path/to/tandem-state
```

If the server normally receives `TANDEM_STATE_DIR`, pass that exact value as
`--state-dir`. The command exits with a JSON report containing the five
`receipt_run_ids`, the persisted `approval_decision_ids`, and the receipt IDs
whose governance evidence correlates the decision and protected audit. After
the normal server is restarted, the five receipts are
selectable through the production API:

```text
GET /context/runs?run_type=session&source=channel:slack&limit=50
```

The command resets prior ACME demo sessions, their policy decisions,
`acme-live-*` memory records, and context-run directories only when all ACME
tenant/workspace/team/channel markers match. It preserves other tenants, Slack
installations, memory, and receipts. It then seeds the five-profile authority
graph and governed memory, sends five HMAC-signed events through a locally
bound production router, runs
the engine with the deterministic provider, delivers to a mock Slack API, and
waits until exactly five ACME context-run receipts are returned by the
production list API.

Limitations:

- Do not run this command concurrently with `tandem-engine serve` or the
  desktop sidecar. The current file-backed stores do not provide a process-wide
  transaction or writer lock. The stop/run/restart sequence is required.
- Provider and Slack transport are deterministic local mocks. No real Slack
  workspace is contacted and no model API key is required.
- The command uses a temporary engine config, so it does not replace the
  server's normal provider or Slack configuration.
- Finance requests `mcp.invoices.read_invoices` through the real approval
  bridge. Success requires the tool to remain unexecuted and the
  `ApprovalRequired` policy decision, protected-audit event, and correlated
  receipt governance-evidence package all to persist.
- Protected audit is append-only, so historical ACME approval events remain in
  the hash-chained ledger across resets. Each command reports and correlates
  only the newly created decision; old ACME sessions, decisions, and receipts
  are removed.
- The feature is excluded from normal builds and release artifacts unless
  `--features acme-demo` is explicitly supplied. `acme-demo` transitively
  enables `premium-governance` so the approval bridge and evidence export are
  present.

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

Run everything (fixture + E2E) with one command. The dedicated
`ACME Slack Governance Proof` workflow runs this exact feature set and then
runs the persistent command twice against the same state directory:

```bash
cargo test --locked -p tandem-server --features acme-demo acme_slack_demo --lib
```

The process-isolated equivalent is:

```bash
cargo nextest run -p tandem-server --features acme-demo -E 'test(acme_slack_demo)'
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
