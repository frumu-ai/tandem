# Enterprise Proof Walkthrough

Document status: buyer-verifiable repository walkthrough.

Implementation review: 2026-07-14 against `origin/main` at `801559fd`.
The walkthrough proves source-level behavior in the reviewed repository. It does
not prove deployment version, uptime, non-bypassable customer architecture,
control operation over time, or external auditor acceptance.

This walkthrough shows how a platform engineer can evaluate Tandem as governed
AI runtime infrastructure using capabilities present in the repository. It does
not assume the unreleased private enterprise sidecar, OIDC, SCIM, turnkey SIEM
connectors, WORM retention, or SOC2. Tandem does provide generic audit and
governance-evidence exports; those should not be described as turnkey SIEM
integration.

## One Governed Run

1. **Intent enters through a client.** A user, SDK call, control-panel surface, or channel request describes work to run. The client is an entrypoint, not the runtime.

2. **Plan preview scopes the work.** The workflow planner produces a preview before activation. The preview can include the workflow graph, selected tools, MCP connector scope, schedule, outputs, validations, and approval points.

3. **Apply materializes runtime state.** Once accepted, the plan is applied into workflow or automation state. From this point, the engine owns the durable run identity and execution graph.

4. **Execution uses scoped tools.** Runtime policy controls which built-in tools and MCP connector tools are callable for a workflow or step, and connection grants bind MCP execution to an acting account. Some discovery paths narrow what is shown to the model, but execution-time enforcement is the hard boundary; this walkthrough does not claim that every unauthorized schema is universally hidden before discovery.

5. **Approval gates pause consequential actions.** A send, post, publish, write, or other sensitive action can pause as a runtime-owned approval request. The Approvals Inbox and channel cards resolve the same underlying gate state instead of relying on prompt text. The fintech strict path can match an approved gate record to tenant, tool, exact action hash, and expiry before execution. The cryptographic approval-receipt verifier is a separate contract, and the repository does not establish universal signed-receipt consumption across every protected tool path.

6. **Artifacts are validated.** The run records output artifacts and validation metadata. Success and failure are runtime state, not only model prose.

7. **Audit records capture control decisions.** Approval decisions, denials, provider secret changes, MCP activity, governance events, and tool ledger activity can be written to sequence-numbered, hash-chained protected audit records. Admins can inspect `/audit/stream`, verify `/audit/ledger/manifest`, export deterministic NDJSON through `/audit/ledger/export`, and assemble run-governance evidence packages. These exports are not a substitute for configured retention, external immutable storage, or a managed SIEM connector.

8. **Replay and debug use the run journal.** The run history, checkpoints, lifecycle events, artifacts, validation outcomes, approval state, and repair attempts provide an operational debugging path.

## What A Buyer Can Verify In The Repo

- **Enterprise contract:** `crates/tandem-enterprise-contract` defines tenant context, principals, authority chains, secret refs, enterprise status, and no-op bridge foundations.
- **Plan compiler:** `crates/tandem-plan-compiler` owns plan packages, validation, runtime projection, preview, and bundle concepts.
- **Governance engine:** `crates/tandem-governance-engine` is separated as a source-available governance surface.
- **Approval aggregation:** `crates/tandem-server/src/http/approvals.rs` exposes pending approval aggregation, while the control panel renders `ApprovalsInboxPage`.
- **Protected audit and evidence export:** `crates/tandem-server/src/audit.rs`, `http/audit_stream.rs`, and `http/context_run_ledger.rs` implement hash-chained durable records, a ledger manifest, deterministic NDJSON export, and structured run-governance evidence packages.
- **Runtime docs:** `docs/WORKFLOW_RUNTIME.md` documents artifacts, validation, retries, repair, and runtime-owned workflow execution.
- **MCP identity and policy:** Runtime MCP definitions are separated from tenant/principal-scoped connections. OAuth sessions and run-as enforcement bind calls to an acting account, while Automation V2 supports step-level tool, server, connection-grant, and service/shared run-as policy.
- **Stateful orchestration:** The stateful runtime includes durable waits, tenant-scoped leases, pinned definition hashes, governed handoffs, deterministic event and effect-record identities, outbox and dead-letter records, compensation handling, and SQLite/PostgreSQL storage backends. This does not make every upstream provider effect idempotent.
- **Enterprise ingestion reference:** The Google Drive enterprise path demonstrates source-bound read-only credentials, fail-closed admission, high-risk quarantine/review, and tenant-scoped source-object lifecycle records. Other planned enterprise ingestion providers are not implemented.
- **Memory isolation and storage:** Memory retrieval applies tenant/resource/data-class/grant boundaries, supports encrypted storage modes, and has SQLite and PostgreSQL/pgvector backends.
- **Route-aware strict ingress boundary:** Hosted/enterprise-required HTTP modes require transport authentication and signed tenant context on non-public engine/API routes. Public OAuth callbacks, automation webhooks, and Slack event ingress bypass that global gate and rely on route-specific OAuth-state or webhook-signature controls. Neither boundary is the same as a deployed private enterprise sidecar.

## Demo Script For Platform Engineering

Use this order when presenting Tandem as infrastructure:

1. Show a plan preview before anything runs.
2. Point to the scoped tools and MCP connector permissions.
3. Start the run and show the durable run ID.
4. Trigger or inspect an approval gate.
5. Approve or rework through the inbox or a channel card.
6. Open the artifact and validation metadata.
7. Inspect the audit stream, verify the ledger manifest/export, and open the run-governance evidence package.
8. Show how the run can be debugged from runtime state rather than a chat transcript.

The defensible conclusion is narrower: for work that is actually routed through
Tandem's authenticated, policy-enforced runtime paths, Tandem can own the durable
run record, control decisions, and evidence chain rather than acting only as a
model interface. A buyer must separately verify that the proposed deployment
makes those paths non-bypassable.

## Fintech Proof Sprint

For fintech buyers, use compliance and risk operations as the first proof sprint. The safest demo is a compliance/risk update brief:

1. Preview a plan that scopes selected regulatory, payment-network, vendor, and internal policy sources.
2. Show per-step tool and MCP connector permissions.
3. Run the workflow and persist a durable run ID.
4. Produce a cited brief with affected controls, materiality, limitations, reviewer status, and artifact validation metadata.
5. Trigger an approval gate before any external communication, system-of-record update, customer-impacting step, or regulated action.
6. Inspect protected records, the ledger manifest/export, and the run-governance evidence package.
7. Show how replay/debug traces the source, artifact, approval, and policy path.

Keep the boundary explicit: this proof sprint demonstrates governed investigation and drafting. It does not demonstrate autonomous money movement, account freezes, customer approval, regulatory filings, credit decisions, or risk-rating changes. A buyer-facing fintech dry run should attach no protected external mutation tools unless the runtime verifies tenant and policy authority plus exact-action approval evidence at the protected tool call. If the claim specifically requires a cryptographically signed approval receipt, the demo must exercise a path that actually invokes the signed-receipt verifier rather than relying only on gate-history matching.

## What This Walkthrough Does Not Prove

- That customer traffic cannot route around Tandem.
- That every protected tool consumes cryptographically signed approval evidence.
- That the audit ledger is exported to immutable external storage or a SIEM.
- That hosted OAuth and signing secrets are universally KMS-backed.
- That OIDC, SCIM, hosted RBAC administration, private-sidecar enforcement, SOC2,
  uptime history, or customer control acceptance exists.
- That repository capability has produced commercial retention, regulatory
  outcomes, or accepted auditor evidence formats.

## Related Docs

- [AI runtime infrastructure](AI_RUNTIME_INFRASTRUCTURE.md)
- [Enterprise readiness](ENTERPRISE_READINESS.md)
- [Workflow runtime](WORKFLOW_RUNTIME.md)
