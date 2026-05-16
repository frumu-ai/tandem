# Enterprise Proof Walkthrough

This walkthrough shows how a platform engineer can evaluate Tandem as governed AI runtime infrastructure using concepts already present in the repo. It does not assume unreleased enterprise sidecar, OIDC, SCIM, SIEM export, or SOC2 capabilities.

## One Governed Run

1. **Intent enters through a client.** A user, SDK call, control-panel surface, or channel request describes work to run. The client is an entrypoint, not the runtime.

2. **Plan preview scopes the work.** The workflow planner produces a preview before activation. The preview can include the workflow graph, selected tools, MCP connector scope, schedule, outputs, validations, and approval points.

3. **Apply materializes runtime state.** Once accepted, the plan is applied into workflow or automation state. From this point, the engine owns the durable run identity and execution graph.

4. **Execution uses scoped tools.** Runtime policy controls which built-in tools and MCP connector tools are visible and callable for a workflow or step. Per-task tool/MCP policy prevents broad connector access from leaking into steps that do not need it.

5. **Approval gates pause consequential actions.** A send, post, publish, write, or other sensitive action can pause as a runtime-owned approval request. The Approvals Inbox and channel cards resolve the same underlying gate state instead of relying on prompt text. For regulated actions, this should be paired with runtime-verified policy/approval evidence at tool execution time before it is treated as authorization.

6. **Artifacts are validated.** The run records output artifacts and validation metadata. Success and failure are runtime state, not only model prose.

7. **Audit records capture control decisions.** Approval decisions, denials, provider secret changes, MCP activity, governance events, and tool ledger activity can be written to protected audit records. Admins can inspect audit events through `/audit/stream`.

8. **Replay and debug use the run journal.** The run history, checkpoints, lifecycle events, artifacts, validation outcomes, approval state, and repair attempts provide an operational debugging path.

## What A Buyer Can Verify In The Repo

- **Enterprise contract:** `crates/tandem-enterprise-contract` defines tenant context, principals, authority chains, secret refs, enterprise status, and no-op bridge foundations.
- **Plan compiler:** `crates/tandem-plan-compiler` owns plan packages, validation, runtime projection, preview, and bundle concepts.
- **Governance engine:** `crates/tandem-governance-engine` is separated as a source-available governance surface.
- **Approval aggregation:** `crates/tandem-server/src/http/approvals.rs` exposes pending approval aggregation, while the control panel renders `ApprovalsInboxPage`.
- **Audit foundations:** Protected audit envelopes and `/audit/stream` expose durable control-plane evidence for approvals, tool ledger events, and channel capability changes.
- **Runtime docs:** `docs/WORKFLOW_RUNTIME.md` documents artifacts, validation, retries, repair, and runtime-owned workflow execution.
- **MCP/tool policy:** Automation V2 supports step-level tool and MCP policy so connector access can be narrowed for each part of a run.

## Demo Script For Platform Engineering

Use this order when presenting Tandem as infrastructure:

1. Show a plan preview before anything runs.
2. Point to the scoped tools and MCP connector permissions.
3. Start the run and show the durable run ID.
4. Trigger or inspect an approval gate.
5. Approve or rework through the inbox or a channel card.
6. Open the artifact and validation metadata.
7. Inspect the audit event stream or protected audit record.
8. Show how the run can be debugged from runtime state rather than a chat transcript.

The demo should leave the buyer with one clear conclusion: Tandem is the control plane and runtime record for agentic work, not another interface wrapped around a model.

## Fintech Proof Sprint

For fintech buyers, use compliance and risk operations as the first proof sprint. The safest demo is a compliance/risk update brief:

1. Preview a plan that scopes selected regulatory, payment-network, vendor, and internal policy sources.
2. Show per-step tool and MCP connector permissions.
3. Run the workflow and persist a durable run ID.
4. Produce a cited brief with affected controls, materiality, limitations, reviewer status, and artifact validation metadata.
5. Trigger an approval gate before any external communication, system-of-record update, customer-impacting step, or regulated action.
6. Inspect audit evidence through protected records or `/audit/stream`.
7. Show how replay/debug traces the source, artifact, approval, and policy path.

Keep the boundary explicit: this proof sprint demonstrates governed investigation and drafting. It does not demonstrate autonomous money movement, account freezes, customer approval, regulatory filings, credit decisions, or risk-rating changes. A buyer-facing fintech dry run should attach no protected external mutation tools unless the runtime verifies a matching approval/policy receipt at the protected tool call.

## Related Docs

- [AI runtime infrastructure](AI_RUNTIME_INFRASTRUCTURE.md)
- [Enterprise readiness](ENTERPRISE_READINESS.md)
- [Workflow runtime](WORKFLOW_RUNTIME.md)
