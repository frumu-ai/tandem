---
title: Policy and Enforcement Model
description: Mode-qualified runtime guarantees for tool policy, approvals, receipts, connectors, and credentials.
---

This page describes the runtime behavior after the residual enforcement work in [PR #1900](https://github.com/frumu-ai/tandem/pull/1900). It separates three modes because a control that exists in enterprise mode is not automatically a local-mode guarantee.

- **Local/default** means `local_single_tenant` without premium governance.
- **Governed server** means server-created dispatch contexts with the central dispatcher, a deny-capable policy, and a durable receipt ledger.
- **Premium/enterprise** adds verified tenant and human identity plus the `premium-governance` policy engine.

Status terms: **enforced** is on the runtime execution path; **config-gated** is implemented but only active under the stated mode or configuration; **partial** is present but not an end-to-end guarantee; **absent** has no enforcement path.

## Mode matrix

| Control | Local/default | Governed server | Premium/enterprise | Load-bearing runtime and proof |
| --- | --- | --- | --- | --- |
| Deny-by-default tool dispatch | **Enforced for agent and server dispatch contexts.** The generic context starts with a deny-all policy. The explicit `dispatch_local` convenience call grants its named tool; for `batch`, it also grants exactly the nested canonical tools enumerated in that batch payload. | **Enforced.** Server contexts reuse the policy instances validated at boot. | **Enforced**, with enterprise rules evaluated inside the same path. | `crates/tandem-tools/src/tool_dispatcher.rs`; `crates/tandem-server/src/app/state/app_state_impl_parts/part01.rs`; `app::state::tests::server_context_installs_deny_capable_policy_and_real_ledger` |
| Parameter-aware authored rules | **Partial.** Predicate types and evaluator code compile, but local mode does not provide the verified enterprise identity and authoring posture. Do not present this as a local security boundary. | **Config-gated.** Arguments are passed to the server policy hook, but decisions depend on installed authored rules and runtime mode. | **Enforced when a published enterprise rule matches.** Selectors can inspect argument fields and produce allow, deny, or approval-required decisions. | `crates/tandem-server/src/agent_teams_parts/enterprise_authored_policy.rs`; `crates/tandem-enterprise-contract/src/policy_predicates.rs`; `authored_parameter_policy_matches_preview_and_records_runtime_decisions`; `authored_approval_resumes_exactly_one_execution` |
| Risk-tier approval routing | **Absent as a verified-identity guarantee.** Local mode deliberately skips strict-mode action-gate routing. | **Config-gated** by strict runtime authentication and tool security metadata. | **Enforced** for classified consequential tools in strict mode. | `crates/tandem-server/src/agent_teams_parts/part01.rs`; `crates/tandem-server/src/agent_teams_parts/action_gate_approval.rs`; `action_gate_approval_resumes_exactly_one_execution` |
| Egress DLP preflight | **Absent as a local-mode guarantee.** | **Config-gated** by strict runtime authentication. | **Enforced** before applicable external sends; an approval is bound to the payload digest and consumed once. | `crates/tandem-server/src/agent_teams_parts/egress_preflight.rs`; `crates/tandem-server/src/app/state/governance_action_gate.rs`; `approved_egress_receipt_authorizes_exactly_one_retry`; `egress_approval_is_bound_to_exact_arguments`; `concurrent_egress_retries_consume_one_receipt_atomically` |
| Approval reviewer identity | **Partial.** Local anonymous owner behavior is intentionally different and must not be described as enterprise separation of duties. | **Config-gated** by the verified-principal runtime mode. | **Enforced server-side.** Agent reviewers, self-review, and cross-tenant replay are rejected. | `crates/tandem-server/src/http/governance.rs`; `crates/tandem-server/src/http/tests/governance_parts/part01.rs`; `governance_approval_approve_rejects_agent_reviewer`; `governance_approval_rejects_agent_self_review` |
| Approval timeout behavior | **Enforced.** Approval waits must be bounded and cannot use resume-on-timeout. | **Enforced.** Unsafe JSON is rejected by server-side wait validation. | **Enforced.** Late approval cannot reopen the expired gate. | `crates/tandem-automation/src/orchestration.rs`; `crates/tandem-server/src/app/state/automation_v2_wait_nodes.rs`; `automation_v2_approval_wait_rejects_resume_on_timeout`; `approval_gate_timeout_survives_reload_and_rejects_late_decision` |
| Dispatch receipts | **Enforced on governed dispatch contexts.** A raw library consumer can still construct a no-op local context, so this is not a claim about arbitrary embedding code. | **Enforced and fail-closed.** Policy, execution-started, and terminal execution outcomes use the installed durable ledger. | **Enforced**, with approval and protected-audit records in addition to dispatch receipts. | `crates/tandem-tools/src/tool_dispatcher.rs`; `crates/tandem-server/src/app/state/tool_dispatch_outbox.rs`; `crates/tandem-server/src/app/state/mod.rs` |
| Credential injection | **Connector-dependent.** Raw secrets are resolved by the runtime/tool implementation, not placed in model-authored arguments. | **Enforced for the governed MCP bridge and Incident Monitor webhook tool.** The dispatcher sees a secret reference, while the executor resolves secret material below policy evaluation. | **Enforced with tenant-scoped connector credentials and verified run-as context.** | `crates/tandem-server/src/http/mcp_run_as.rs`; `crates/tandem-runtime/src/mcp_parts/part01.rs`; `crates/tandem-server/src/incident_monitor_webhook.rs` |
| System-initiated service calls | **Enforced after PR #1900** for coder GitHub Projects, pack builder, benchmarking, and Incident Monitor GitHub, Linear, generic MCP, and webhook destinations. | **Enforced through the same dispatcher, policy, outbox, and receipt ledger as agent calls.** | **Enforced**, including argument-aware enterprise policy and approvals. | `crates/tandem-server/src/http/mcp.rs`; `crates/tandem-server/src/http/coder_parts/part05.rs`; `crates/tandem-server/src/benchmarking/mod.rs`; `crates/tandem-server/src/incident_monitor_*.rs` |
| Boot composition guard | **Enforced for the shipped server composition.** | **Enforced.** Startup validates the exact policy and ledger objects stored for subsequent contexts; allow-all and no-op injections abort boot in tests. | **Enforced for the same installed composition.** | `crates/tandem-server/src/app/state/app_state_impl_parts/part01.rs`; `server_boot_rejects_allow_all_in_actual_dispatch_composition`; `server_boot_rejects_noop_ledger_in_actual_dispatch_composition` |

## Claims and qualifiers

Defensible: **Agent and server tool calls that enter Tandem's governed dispatcher are deny-by-default, and server builds install a durable receipt ledger.** For premium/enterprise deployments, parameter predicates, risk routing, egress preflight, and reviewer identity add stricter controls on that same path.

Defensible: **An approved egress or authored-policy action is bound to its exact arguments and approval identity, and the approval receipt authorizes one execution.** The named premium tests in `.github/governance-audit-critical-tests.txt` must be discovered and executed in CI.

Overreach: “Every Tandem embedding has enterprise governance.” Library consumers can construct local contexts, and local single-tenant mode intentionally omits premium identity and policy guarantees.

Overreach: “A template is a validated policy recommendation.” The CRM, finance, and coding templates remain draft and experimental until five observed concierge sessions and an explicit human go decision are recorded.

## Drift guard

The CI workflow maintains a named manifest of audit-critical premium tests, verifies that each test is discovered and executed, runs a separate non-premium fail-closed route test, and rejects reintroduction of the audited direct MCP call sites. Review this page whenever the manifest, runtime authentication gates, policy feature flags, or central dispatch composition changes.
