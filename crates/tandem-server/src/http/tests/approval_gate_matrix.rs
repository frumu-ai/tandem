use super::*;

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
    let decision_id = decision.policy_decision_id.expect("policy decision id");

    let recorded = state
        .get_policy_decision(&decision_id)
        .await
        .expect("recorded policy decision");
    assert_eq!(recorded.decision, PolicyDecisionEffect::ApprovalRequired);
    let approval_id = recorded.approval_id.clone().expect("approval id");
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
