use super::*;

#[cfg(feature = "premium-governance")]
use crate::automation_v2::governance::{
    GovernanceActorRef, GovernanceApprovalRequest, GovernanceApprovalRequestType,
    GovernanceApprovalStatus,
};
use tandem_enterprise_contract::{
    EnterprisePolicyEffect, EnterprisePolicyRule, EnterprisePolicyScopeLevel,
};
use tandem_types::{
    approval_authorizes_execution, DataClass, GateRequest, PolicyDecisionEffect,
    ReviewerEligibility, ToolRiskTier,
};

fn tenant() -> TenantContext {
    TenantContext::explicit("acme", "acme", None)
}

#[tokio::test]
async fn external_customer_send_pauses_for_approval_and_records_evidence() {
    let state = test_state().await;
    let tenant = tenant();

    let (outcome, decision_id) = state
        .enforce_action_gate(
            &tenant,
            &GateRequest::external_customer_send(),
            Some("mcp.email.send".to_string()),
            Some("agent-sales".to_string()),
            1_000,
        )
        .await;

    assert!(
        outcome.requires_approval(),
        "external send must pause for approval"
    );
    let decision_id = decision_id.expect("gate decision must be recorded");

    // Policy decision evidence.
    let decisions = state.list_policy_decisions(&tenant, 50).await;
    let recorded = decisions
        .iter()
        .find(|d| d.decision_id == decision_id)
        .expect("recorded policy decision");
    assert_eq!(recorded.decision, PolicyDecisionEffect::ApprovalRequired);
    assert_eq!(recorded.policy_id.as_deref(), Some("approval_gate_matrix"));
    assert_eq!(recorded.risk_tier.as_deref(), Some("external_send"));

    // Protected audit evidence.
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"approval.gate.approval_required\""));
    assert!(audit.contains("\"org_id\":\"acme\""));
    assert!(audit.contains("agent-sales"));
}

#[tokio::test]
async fn sensitive_data_class_requires_elevated_reviewer_in_metadata() {
    let state = test_state().await;
    let tenant = tenant();

    let (outcome, decision_id) = state
        .enforce_action_gate(
            &tenant,
            &GateRequest::new(
                Some(ToolRiskTier::InternalWrite),
                Some(DataClass::FinancialRecord),
            ),
            Some("mcp.ledger.read".to_string()),
            Some("agent-fin".to_string()),
            1_000,
        )
        .await;

    assert!(outcome.requires_approval());
    assert_eq!(
        outcome.reviewer_eligibility,
        ReviewerEligibility::ElevatedReviewer
    );

    let decision_id = decision_id.expect("decision recorded");
    let decisions = state.list_policy_decisions(&tenant, 50).await;
    let recorded = decisions
        .iter()
        .find(|d| d.decision_id == decision_id)
        .expect("recorded policy decision");
    assert_eq!(
        recorded.metadata["gate"]["reviewer_eligibility"],
        "elevated_reviewer"
    );
    assert!(recorded.data_classes.contains(&DataClass::FinancialRecord));
}

#[tokio::test]
async fn low_risk_action_auto_allows_without_audit() {
    let state = test_state().await;
    let tenant = tenant();

    let (outcome, decision_id) = state
        .enforce_action_gate(
            &tenant,
            &GateRequest::new(Some(ToolRiskTier::ReadDiscover), Some(DataClass::Internal)),
            Some("mcp.search".to_string()),
            Some("agent-x".to_string()),
            1_000,
        )
        .await;

    assert!(outcome.is_allowed());
    assert!(decision_id.is_some(), "allow decisions are still recorded");

    // No protected audit event is written for an auto-allow.
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .unwrap_or_default();
    assert!(!audit.contains("approval.gate"));
}

#[tokio::test]
async fn enterprise_policy_override_blocks_locally_allowed_action_gate() {
    let state = test_state().await;
    let tenant = tenant();
    state.enterprise.policy_rules.write().await.insert(
        "enterprise-bank-tool-deny".to_string(),
        EnterprisePolicyRule::new(
            "enterprise-bank-tool-deny",
            "enterprise-bank-floor",
            EnterprisePolicyScopeLevel::Enterprise,
            EnterprisePolicyEffect::Deny,
        )
        .with_tenant_context(tenant.clone())
        .with_tool_patterns(vec!["mcp.bank.*".to_string()])
        .with_reason(
            "enterprise_bank_tool_floor",
            "enterprise policy denies bank tool access",
        ),
    );

    let (outcome, decision_id) = state
        .enforce_action_gate(
            &tenant,
            &GateRequest::new(Some(ToolRiskTier::ReadDiscover), Some(DataClass::Internal)),
            Some("mcp.bank.release_funds".to_string()),
            Some("agent-fin".to_string()),
            1_000,
        )
        .await;

    assert!(outcome.is_denied());
    assert_eq!(outcome.reason_code, "enterprise_bank_tool_floor");
    let decision_id = decision_id.expect("decision recorded");
    let decisions = state.list_policy_decisions(&tenant, 50).await;
    let recorded = decisions
        .iter()
        .find(|d| d.decision_id == decision_id)
        .expect("recorded policy decision");
    assert_eq!(recorded.decision, PolicyDecisionEffect::Deny);
    assert_eq!(recorded.reason_code, "enterprise_bank_tool_floor");

    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"approval.gate.denied\""));
    assert!(audit.contains("enterprise_bank_tool_floor"));
}

#[tokio::test]
async fn unclassified_action_fails_closed_to_approval() {
    let state = test_state().await;
    let tenant = tenant();

    let (outcome, _) = state
        .enforce_action_gate(&tenant, &GateRequest::new(None, None), None, None, 1_000)
        .await;

    assert!(
        outcome.requires_approval(),
        "unknown action must not auto-allow"
    );
    assert_eq!(
        outcome.reviewer_eligibility,
        ReviewerEligibility::ElevatedReviewer
    );
    assert_eq!(outcome.reason_code, "unresolved_policy_fail_closed");
}

#[tokio::test]
async fn expired_approval_cannot_authorize_execution() {
    // An approval whose TTL has elapsed cannot authorize execution even though
    // it was approved.
    let expires_at = 5_000;
    assert!(approval_authorizes_execution(true, expires_at, 4_999));
    assert!(!approval_authorizes_execution(true, expires_at, 5_000));
    assert!(!approval_authorizes_execution(false, expires_at, 4_000));
}

#[tokio::test]
async fn tool_policy_gate_pauses_high_risk_tool_and_skips_reads() {
    use tandem_core::ToolPolicyContext;

    let state = test_state().await;
    let ctx = |tool: &str| ToolPolicyContext {
        session_id: "session-gate".to_string(),
        message_id: "message-gate".to_string(),
        tenant_context: Some(tenant()),
        verified_tenant_context: None,
        tool: tool.to_string(),
        args: json!({}),
    };

    // A customer-facing send classifies as ExternalSend and is paused.
    let send = crate::agent_teams::evaluate_action_gate_tool_policy(
        &state,
        &ctx("mcp.email.send"),
        "mcp.email.send",
    )
    .await
    .expect("high-risk tool must be gated");
    assert!(!send.allowed, "external send must be paused for approval");
    assert!(
        send.policy_decision_id.is_some(),
        "gate records a policy decision"
    );

    // The gate left protected audit evidence.
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"approval.gate.approval_required\""));

    // A read/discovery tool is not gated here.
    let read = crate::agent_teams::evaluate_action_gate_tool_policy(
        &state,
        &ctx("mcp.search"),
        "mcp.search",
    )
    .await;
    assert!(read.is_none(), "low-risk read tools fall through the gate");
}

#[cfg(feature = "premium-governance")]
fn protected_action_context() -> tandem_core::ToolPolicyContext {
    tandem_core::ToolPolicyContext {
        session_id: "session-action-approval".to_string(),
        message_id: "message-action-approval".to_string(),
        tenant_context: Some(tenant()),
        verified_tenant_context: None,
        tool: "mcp.email.send".to_string(),
        args: json!({
            "to": "ops@example.com",
            "subject": "status",
            "body": "Deployment finished."
        }),
    }
}

#[cfg(feature = "premium-governance")]
async fn evaluate_protected_action(state: &AppState) -> tandem_core::ToolPolicyDecision {
    let ctx = protected_action_context();
    crate::agent_teams::evaluate_action_gate_tool_policy(state, &ctx, &ctx.tool)
        .await
        .expect("external send must resolve through the action gate")
}

#[cfg(feature = "premium-governance")]
async fn action_gate_approval(state: &AppState) -> GovernanceApprovalRequest {
    state
        .list_approval_requests_for_tenant(
            Some(GovernanceApprovalRequestType::ElevatedCapability),
            None,
            &tenant(),
        )
        .await
        .into_iter()
        .next()
        .expect("durable action-gate approval request")
}

#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn action_gate_pending_retry_reuses_durable_request() {
    let state = test_state().await;

    let first = evaluate_protected_action(&state).await;
    let second = evaluate_protected_action(&state).await;
    assert!(!first.allowed);
    assert!(!second.allowed);
    for decision in [&first, &second] {
        let dispatch = decision
            .dispatch_decision
            .as_ref()
            .expect("pending action gate is a first-class dispatch outcome");
        assert_eq!(
            dispatch.outcome,
            tandem_tools::ToolDispatchPolicyOutcome::ApprovalRequired
        );
        assert_eq!(
            dispatch
                .approval_requirement
                .as_ref()
                .map(|requirement| requirement.policy_id.as_str()),
            Some("approval_gate_matrix")
        );
    }

    let approvals = state
        .list_approval_requests_for_tenant(
            Some(GovernanceApprovalRequestType::ElevatedCapability),
            None,
            &tenant(),
        )
        .await;
    assert_eq!(approvals.len(), 1, "pending retries must deduplicate");
    let approval = &approvals[0];
    assert_eq!(approval.status, GovernanceApprovalStatus::Pending);
    assert_eq!(
        approval
            .context
            .pointer("/action_gate/session_id")
            .and_then(Value::as_str),
        Some("session-action-approval")
    );
    assert_eq!(
        approval
            .context
            .pointer("/action_gate/message_id")
            .and_then(Value::as_str),
        Some("message-action-approval")
    );
    assert_eq!(
        approval
            .context
            .pointer("/action_gate/tool")
            .and_then(Value::as_str),
        Some("mcp.email.send")
    );
    assert_eq!(
        approval
            .context
            .pointer("/action_gate/risk_tier")
            .and_then(Value::as_str),
        Some("external_send")
    );
    assert_eq!(
        approval
            .context
            .pointer("/action_gate/policy_id")
            .and_then(Value::as_str),
        Some("approval_gate_matrix")
    );
    assert!(approval
        .context
        .pointer("/action_gate/action_hash")
        .and_then(Value::as_str)
        .is_some());

    for decision in [first, second] {
        let decision_id = decision.policy_decision_id.expect("policy decision id");
        let recorded = state
            .get_policy_decision(&decision_id)
            .await
            .expect("linked policy decision");
        assert_eq!(
            recorded.approval_id.as_deref(),
            Some(approval.approval_id.as_str())
        );
        assert_eq!(
            recorded.session_id.as_deref(),
            Some("session-action-approval")
        );
        assert_eq!(
            recorded.message_id.as_deref(),
            Some("message-action-approval")
        );
    }
}

#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn action_gate_new_message_gets_a_distinct_approval() {
    let state = test_state().await;
    assert!(!evaluate_protected_action(&state).await.allowed);

    let mut next = protected_action_context();
    next.message_id = "message-action-approval-next".to_string();
    let decision = crate::agent_teams::evaluate_action_gate_tool_policy(&state, &next, &next.tool)
        .await
        .expect("new message remains approval gated");
    assert!(!decision.allowed);

    let approvals = state
        .list_approval_requests_for_tenant(
            Some(GovernanceApprovalRequestType::ElevatedCapability),
            None,
            &tenant(),
        )
        .await;
    assert_eq!(
        approvals.len(),
        2,
        "new messages must not reuse old receipts"
    );
}

#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn action_gate_denial_executes_zero_times() {
    let state = test_state().await;
    assert!(!evaluate_protected_action(&state).await.allowed);
    let approval = action_gate_approval(&state).await;
    state
        .decide_approval_request(
            &approval.approval_id,
            crate::automation_v2::governance::GovernanceActorRef::human(
                Some("reviewer-1".to_string()),
                "test",
            ),
            false,
            Some("denied in test".to_string()),
            &tenant(),
        )
        .await
        .expect("deny approval")
        .expect("approval exists");

    let executions = usize::from(evaluate_protected_action(&state).await.allowed);
    assert_eq!(executions, 0, "denied action must never dispatch");
}

#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn action_gate_approval_resumes_exactly_one_execution() {
    let state = test_state().await;
    assert!(!evaluate_protected_action(&state).await.allowed);
    let approval = action_gate_approval(&state).await;
    state
        .decide_approval_request(
            &approval.approval_id,
            crate::automation_v2::governance::GovernanceActorRef::human(
                Some("reviewer-1".to_string()),
                "test",
            ),
            true,
            Some("approved in test".to_string()),
            &tenant(),
        )
        .await
        .expect("approve request")
        .expect("approval exists");

    let first_retry = evaluate_protected_action(&state).await;
    let second_retry = evaluate_protected_action(&state).await;
    let executions = usize::from(first_retry.allowed) + usize::from(second_retry.allowed);
    assert_eq!(executions, 1, "approved action must dispatch exactly once");

    let consumed = state
        .get_governance_approval_request(&approval.approval_id)
        .await
        .expect("consumed approval remains durable");
    assert!(consumed
        .context
        .pointer("/action_gate/consumed_at_ms")
        .and_then(Value::as_u64)
        .is_some());
}

#[tokio::test]
async fn egress_preflight_blocks_secret_content_before_external_send() {
    use tandem_core::ToolPolicyContext;

    let state = test_state().await;
    let ctx = ToolPolicyContext {
        session_id: "session-egress-secret".to_string(),
        message_id: "message-egress-secret".to_string(),
        tenant_context: Some(tenant()),
        verified_tenant_context: None,
        tool: "mcp.email.send".to_string(),
        args: json!({
            "to": "customer@example.com",
            "subject": "credential leak",
            "body": "use API key sk-test-secret for the integration"
        }),
    };

    let decision =
        crate::agent_teams::evaluate_egress_preflight_tool_policy(&state, &ctx, "mcp.email.send")
            .await
            .expect("secret-bearing external send must be blocked");
    assert!(!decision.allowed);
    let decision_id = decision.policy_decision_id.expect("policy decision id");

    let recorded = state
        .get_policy_decision(&decision_id)
        .await
        .expect("recorded policy decision");
    assert_eq!(recorded.decision, PolicyDecisionEffect::Deny);
    assert_eq!(recorded.policy_id.as_deref(), Some("egress_dlp_preflight"));
    assert!(recorded.data_classes.contains(&DataClass::Credential));
    assert!(recorded.approval_id.is_none());
    let preview = recorded.metadata["egress_preflight"]["safe_preview_markdown"]
        .as_str()
        .expect("safe preview");
    assert!(preview.contains("[redacted credential]"));
    assert!(!preview.contains("sk-test-secret"));

    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"egress.preflight.denied\""));
    assert!(audit.contains(&decision_id));
}

#[tokio::test]
async fn egress_preflight_blocks_truncated_array_payloads_fail_closed() {
    use tandem_core::ToolPolicyContext;

    let state = test_state().await;
    let rows = (0..25)
        .map(|idx| {
            json!({
                "row": idx,
                "message": format!("benign outbound row {idx}")
            })
        })
        .collect::<Vec<_>>();
    let ctx = ToolPolicyContext {
        session_id: "session-egress-truncated".to_string(),
        message_id: "message-egress-truncated".to_string(),
        tenant_context: Some(tenant()),
        verified_tenant_context: None,
        tool: "mcp.email.send".to_string(),
        args: json!({
            "to": "ops@example.com",
            "rows": rows
        }),
    };

    let decision =
        crate::agent_teams::evaluate_egress_preflight_tool_policy(&state, &ctx, "mcp.email.send")
            .await
            .expect("truncated external payload inspection must fail closed");
    assert!(!decision.allowed);
    let decision_id = decision.policy_decision_id.expect("policy decision id");

    let recorded = state
        .get_policy_decision(&decision_id)
        .await
        .expect("recorded policy decision");
    assert_eq!(recorded.decision, PolicyDecisionEffect::Deny);
    assert_eq!(recorded.reason_code, "egress_payload_inspection_truncated");
    assert_eq!(
        recorded
            .metadata
            .pointer("/egress_preflight/inspection_truncated")
            .and_then(Value::as_bool),
        Some(true)
    );

    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"inspection_truncated\":true"));
    assert!(audit.contains(&decision_id));
}

#[tokio::test]
async fn egress_preflight_creates_safe_customer_data_approval_request() {
    use tandem_core::ToolPolicyContext;

    let state = test_state().await;
    if !state.premium_governance_enabled() {
        return;
    }

    let ctx = ToolPolicyContext {
        session_id: "session-egress-customer".to_string(),
        message_id: "message-egress-customer".to_string(),
        tenant_context: Some(tenant()),
        verified_tenant_context: None,
        tool: "mcp.email.send".to_string(),
        args: json!({
            "to": "alice.customer@example.com",
            "subject": "Renewal follow-up",
            "body": "Hi Alice, your account ACME-42 renewal is ready."
        }),
    };

    let decision =
        crate::agent_teams::evaluate_egress_preflight_tool_policy(&state, &ctx, "mcp.email.send")
            .await
            .expect("customer-data external send must require approval");
    assert!(!decision.allowed);
    let dispatch_approval_id = decision
        .dispatch_decision
        .as_ref()
        .expect("pending egress request is a first-class dispatch outcome")
        .approval_requirement
        .as_ref()
        .expect("structured egress approval metadata")
        .approval_request_id
        .clone();
    assert_eq!(
        decision
            .dispatch_decision
            .as_ref()
            .map(|dispatch| &dispatch.outcome),
        Some(&tandem_tools::ToolDispatchPolicyOutcome::ApprovalRequired)
    );
    let decision_id = decision.policy_decision_id.expect("policy decision id");

    let recorded = state
        .get_policy_decision(&decision_id)
        .await
        .expect("recorded policy decision");
    assert_eq!(recorded.decision, PolicyDecisionEffect::ApprovalRequired);
    let approval_id = recorded.approval_id.clone().expect("approval id");
    assert_eq!(dispatch_approval_id.as_deref(), Some(approval_id.as_str()));
    assert!(recorded.data_classes.contains(&DataClass::CustomerData));
    let preview = recorded.metadata["egress_preflight"]["safe_preview_markdown"]
        .as_str()
        .expect("safe preview");
    assert!(preview.contains("a***@example.com"));
    assert!(!preview.contains("alice.customer@example.com"));

    let approvals = state
        .list_approval_requests_for_tenant(None, None, &tenant())
        .await;
    let approval = approvals
        .iter()
        .find(|approval| approval.approval_id == approval_id)
        .expect("pending approval request");
    assert_eq!(
        approval.request_type,
        crate::automation_v2::governance::GovernanceApprovalRequestType::ExternalPost
    );
    assert_eq!(
        approval.context["safe_preview_markdown"].as_str(),
        Some(preview)
    );

    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"egress.preflight.approval_required\""));
    assert!(audit.contains(&decision_id));
    assert!(audit.contains(&approval_id));
}

#[tokio::test]
async fn approved_egress_receipt_authorizes_exactly_one_retry() {
    use tandem_core::ToolPolicyContext;

    let state = test_state().await;
    if !state.premium_governance_enabled() {
        return;
    }
    let ctx = ToolPolicyContext {
        session_id: "session-egress-once".to_string(),
        message_id: "message-egress-once".to_string(),
        tenant_context: Some(tenant()),
        verified_tenant_context: None,
        tool: "mcp.email.send".to_string(),
        args: json!({
            "to": "alice.customer@example.com",
            "subject": "Renewal follow-up",
            "body": "Customer account ACME-42 renewal is ready."
        }),
    };

    let pending =
        crate::agent_teams::evaluate_egress_preflight_tool_policy(&state, &ctx, "mcp.email.send")
            .await
            .expect("egress action enters approval");
    assert!(!pending.allowed);
    let approval_id = state
        .get_policy_decision(
            pending
                .policy_decision_id
                .as_deref()
                .expect("policy decision id"),
        )
        .await
        .and_then(|decision| decision.approval_id)
        .expect("approval id");
    state
        .decide_approval_request(
            &approval_id,
            crate::automation_v2::governance::GovernanceActorRef::human(
                Some("reviewer-1".to_string()),
                "test",
            ),
            true,
            Some("approved in test".to_string()),
            &tenant(),
        )
        .await
        .expect("approve egress request")
        .expect("approval exists");

    let first_retry =
        crate::agent_teams::evaluate_egress_preflight_tool_policy(&state, &ctx, "mcp.email.send")
            .await
            .expect("approved retry is evaluated");
    let second_retry =
        crate::agent_teams::evaluate_egress_preflight_tool_policy(&state, &ctx, "mcp.email.send")
            .await
            .expect("consumed retry is evaluated");

    assert_eq!(
        usize::from(first_retry.allowed) + usize::from(second_retry.allowed),
        1,
        "an approved egress receipt must authorize exactly one execution"
    );
    let consumed = state
        .get_governance_approval_request(&approval_id)
        .await
        .expect("consumed approval remains durable");
    assert!(consumed
        .context
        .pointer("/egress_dlp/consumed_at_ms")
        .and_then(Value::as_u64)
        .is_some());
}

#[tokio::test]
async fn consumed_egress_approval_fails_closed_when_policy_receipt_write_fails() {
    let mut state = test_state().await;
    if !state.premium_governance_enabled() {
        return;
    }
    let ctx = egress_test_context("Customer account ACME-42 renewal is ready.");
    approve_egress_test_request(&state, &ctx).await;
    let blocked_path = state
        .policy_decisions_path
        .parent()
        .expect("policy decision parent")
        .join("policy-decisions-blocked-dir");
    tokio::fs::create_dir_all(&blocked_path)
        .await
        .expect("create blocked policy decision path");
    state.policy_decisions_path = blocked_path;

    let retry =
        crate::agent_teams::evaluate_egress_preflight_tool_policy(&state, &ctx, "mcp.email.send")
            .await
            .expect("approved retry is evaluated");

    assert!(!retry.allowed);
    assert!(retry.policy_decision_id.is_none());
    assert!(retry
        .reason
        .as_deref()
        .is_some_and(|reason| reason.contains("durable policy decision receipt")));
}

async fn approve_egress_test_request(
    state: &AppState,
    ctx: &tandem_core::ToolPolicyContext,
) -> String {
    let pending =
        crate::agent_teams::evaluate_egress_preflight_tool_policy(state, ctx, "mcp.email.send")
            .await
            .expect("egress action enters approval");
    let approval_id = state
        .get_policy_decision(
            pending
                .policy_decision_id
                .as_deref()
                .expect("policy decision id"),
        )
        .await
        .and_then(|decision| decision.approval_id)
        .expect("approval id");
    state
        .decide_approval_request(
            &approval_id,
            crate::automation_v2::governance::GovernanceActorRef::human(
                Some("reviewer-1".to_string()),
                "test",
            ),
            true,
            Some("approved in test".to_string()),
            &tenant(),
        )
        .await
        .expect("approve egress request")
        .expect("approval exists");
    approval_id
}

fn egress_test_context(body: &str) -> tandem_core::ToolPolicyContext {
    tandem_core::ToolPolicyContext {
        session_id: "session-egress-binding".to_string(),
        message_id: "message-egress-binding".to_string(),
        tenant_context: Some(tenant()),
        verified_tenant_context: None,
        tool: "mcp.email.send".to_string(),
        args: json!({
            "to": "alice.customer@example.com",
            "subject": "Renewal follow-up",
            "body": body,
        }),
    }
}

#[tokio::test]
async fn egress_approval_is_bound_to_exact_arguments() {
    let state = test_state().await;
    if !state.premium_governance_enabled() {
        return;
    }
    let approved_ctx = egress_test_context("Customer account ACME-42 renewal is ready.");
    approve_egress_test_request(&state, &approved_ctx).await;
    let mutated_ctx = egress_test_context("Send customer account ACME-99 instead.");

    let mutated = crate::agent_teams::evaluate_egress_preflight_tool_policy(
        &state,
        &mutated_ctx,
        "mcp.email.send",
    )
    .await
    .expect("mutated action evaluated");
    let exact = crate::agent_teams::evaluate_egress_preflight_tool_policy(
        &state,
        &approved_ctx,
        "mcp.email.send",
    )
    .await
    .expect("exact action evaluated");

    assert!(!mutated.allowed, "changed arguments require a new approval");
    assert!(exact.allowed, "the exact approved action may execute once");
}

#[tokio::test]
async fn expired_egress_approval_fails_closed() {
    let state = test_state().await;
    if !state.premium_governance_enabled() {
        return;
    }
    let ctx = egress_test_context("Customer account ACME-42 renewal is ready.");
    let approval_id = approve_egress_test_request(&state, &ctx).await;
    state
        .automation_governance
        .write()
        .await
        .approvals
        .get_mut(&approval_id)
        .expect("approval")
        .expires_at_ms = 0;

    let retry =
        crate::agent_teams::evaluate_egress_preflight_tool_policy(&state, &ctx, "mcp.email.send")
            .await
            .expect("expired action evaluated");
    assert!(!retry.allowed);
}

#[tokio::test]
async fn concurrent_egress_retries_consume_one_receipt_atomically() {
    let state = test_state().await;
    if !state.premium_governance_enabled() {
        return;
    }
    let ctx = egress_test_context("Customer account ACME-42 renewal is ready.");
    approve_egress_test_request(&state, &ctx).await;

    let (first, second) = tokio::join!(
        crate::agent_teams::evaluate_egress_preflight_tool_policy(&state, &ctx, "mcp.email.send"),
        crate::agent_teams::evaluate_egress_preflight_tool_policy(&state, &ctx, "mcp.email.send")
    );
    let allowed = [first, second]
        .into_iter()
        .flatten()
        .filter(|decision| decision.allowed)
        .count();
    assert_eq!(
        allowed, 1,
        "only one concurrent retry may consume the receipt"
    );
}

#[tokio::test]
async fn egress_preflight_redacts_webhook_target_before_approval_audit() {
    use tandem_core::ToolPolicyContext;

    let state = test_state().await;
    if !state.premium_governance_enabled() {
        return;
    }
    let secret_webhook =
        "https://hooks.example.com/services/T000/B000/super-secret-token?sig=hidden";
    let ctx = ToolPolicyContext {
        session_id: "session-egress-webhook".to_string(),
        message_id: "message-egress-webhook".to_string(),
        tenant_context: Some(tenant()),
        verified_tenant_context: None,
        tool: "mcp.webhook.post".to_string(),
        args: json!({
            "webhook_url": secret_webhook,
            "body": "Customer alice.customer@example.com renewal amount is ready."
        }),
    };

    let decision =
        crate::agent_teams::evaluate_egress_preflight_tool_policy(&state, &ctx, "mcp.webhook.post")
            .await
            .expect("customer-data webhook post must require approval");
    assert!(!decision.allowed);
    let decision_id = decision.policy_decision_id.expect("policy decision id");
    let recorded = state
        .get_policy_decision(&decision_id)
        .await
        .expect("recorded policy decision");
    assert_eq!(recorded.decision, PolicyDecisionEffect::ApprovalRequired);
    let target = recorded.metadata["egress_preflight"]["target"]
        .as_str()
        .expect("redacted target");
    assert!(target.contains("https://hooks.example.com/[redacted-url-target]#"));
    assert!(!target.contains("super-secret-token"));
    assert!(!target.contains("sig=hidden"));

    let approval_id = recorded.approval_id.clone().expect("approval id");
    let approvals = state
        .list_approval_requests_for_tenant(None, None, &tenant())
        .await;
    let approval = approvals
        .iter()
        .find(|approval| approval.approval_id == approval_id)
        .expect("pending approval request");
    assert_eq!(approval.context["target"].as_str(), Some(target));
    assert!(!approval.context.to_string().contains("super-secret-token"));
    assert!(!approval.context.to_string().contains("sig=hidden"));

    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains(target));
    assert!(!audit.contains("super-secret-token"));
    assert!(!audit.contains("sig=hidden"));
}
