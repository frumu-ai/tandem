use super::*;

use crate::app::state::governance::UnavailableGovernanceEngine;
use tandem_enterprise_contract::{
    DataClass, EnterprisePolicyEffect, EnterprisePolicyRule, EnterprisePolicyScopeLevel,
};
use tandem_types::{PolicyDecisionEffect, PolicyDecisionRecord, TenantContext};

fn tenant(org_id: &str, workspace_id: &str, actor_id: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(org_id, workspace_id, None, actor_id)
}

fn policy_decision(
    decision_id: &str,
    tenant_context: TenantContext,
    run_id: &str,
    created_at_ms: u64,
) -> PolicyDecisionRecord {
    PolicyDecisionRecord {
        decision_id: decision_id.to_string(),
        tenant_context,
        requester_context: None,
        actor_id: Some("agent-policy-test".to_string()),
        session_id: Some(format!("session-{decision_id}")),
        message_id: Some(format!("message-{decision_id}")),
        run_id: Some(run_id.to_string()),
        automation_id: Some("automation-policy-test".to_string()),
        node_id: None,
        tool: Some("mcp.bank.release_funds".to_string()),
        resource: None,
        data_classes: Vec::new(),
        risk_tier: Some("money_movement".to_string()),
        decision: PolicyDecisionEffect::ApprovalRequired,
        reason_code: "approval_required_unverified".to_string(),
        reason: "approval required".to_string(),
        policy_id: Some("fintech_strict".to_string()),
        grant_id: None,
        approval_id: None,
        audit_event_id: None,
        created_at_ms,
        metadata: json!({}),
    }
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&body).expect("response json")
}

#[tokio::test]
async fn policy_decisions_route_filters_tenant_before_limit() {
    let state = test_state().await;
    let tenant_a = tenant("org-a", "workspace-a", "user-a");
    let tenant_b = tenant("org-b", "workspace-b", "user-b");
    state
        .record_policy_decision(policy_decision(
            "decision-a-older",
            tenant_a.clone(),
            "run-shared",
            100,
        ))
        .await
        .expect("record tenant a decision");
    state
        .record_policy_decision(policy_decision(
            "decision-b-newer",
            tenant_b,
            "run-shared",
            300,
        ))
        .await
        .expect("record tenant b decision");

    let response = app_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/governance/policy-decisions?limit=1")
                .header("x-tandem-org-id", tenant_a.org_id.as_str())
                .header("x-tandem-workspace-id", tenant_a.workspace_id.as_str())
                .header("x-tandem-actor-id", "user-a")
                .body(Body::empty())
                .expect("policy decisions list request"),
        )
        .await
        .expect("policy decisions list response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["count"], json!(1));
    assert_eq!(
        payload["policy_decisions"][0]["decision_id"],
        json!("decision-a-older")
    );
}

#[tokio::test]
async fn policy_decisions_route_filters_run_and_tenant_before_limit() {
    let state = test_state().await;
    let tenant_a = tenant("org-a", "workspace-a", "user-a");
    let tenant_b = tenant("org-b", "workspace-b", "user-b");
    state
        .record_policy_decision(policy_decision(
            "decision-a-target-run",
            tenant_a.clone(),
            "run-target",
            100,
        ))
        .await
        .expect("record tenant a decision");
    state
        .record_policy_decision(policy_decision(
            "decision-b-target-run-newer",
            tenant_b,
            "run-target",
            300,
        ))
        .await
        .expect("record tenant b decision");

    let response = app_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/governance/policy-decisions?run_id=run-target&limit=1")
                .header("x-tandem-org-id", tenant_a.org_id.as_str())
                .header("x-tandem-workspace-id", tenant_a.workspace_id.as_str())
                .header("x-tandem-actor-id", "user-a")
                .body(Body::empty())
                .expect("policy decisions run list request"),
        )
        .await
        .expect("policy decisions run list response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["count"], json!(1));
    assert_eq!(
        payload["policy_decisions"][0]["decision_id"],
        json!("decision-a-target-run")
    );
}

#[tokio::test]
async fn policy_decision_records_use_enterprise_policy_resolver() {
    let state = test_state().await;
    let tenant_a = tenant("org-a", "workspace-a", "user-a");
    state.enterprise.policy_rules.write().await.insert(
        "enterprise-finance-deny".to_string(),
        EnterprisePolicyRule::new(
            "enterprise-finance-deny",
            "enterprise-finance-floor",
            EnterprisePolicyScopeLevel::Enterprise,
            EnterprisePolicyEffect::Deny,
        )
        .with_tenant_context(tenant_a.clone())
        .with_tool_patterns(vec!["mcp.bank.*".to_string()])
        .with_reason(
            "enterprise_finance_floor",
            "enterprise policy denies finance tool access",
        ),
    );

    let mut decision = policy_decision("decision-local-allow", tenant_a.clone(), "run-target", 100);
    decision.decision = PolicyDecisionEffect::Allow;
    decision.reason_code = "local_policy_allow".to_string();
    decision.reason = "local tool authority allowed the request".to_string();

    let recorded = state
        .record_policy_decision(decision)
        .await
        .expect("record policy decision");

    assert_eq!(recorded.decision, PolicyDecisionEffect::Deny);
    assert_eq!(
        recorded.policy_id.as_deref(),
        Some("enterprise-finance-floor")
    );
    assert_eq!(recorded.reason_code, "enterprise_finance_floor");
    let snapshot = recorded
        .effective_policy_snapshot()
        .expect("effective policy snapshot");
    assert_eq!(snapshot.effect, EnterprisePolicyEffect::Deny);
    assert_eq!(snapshot.inherited_sources.len(), 2);
    assert_eq!(
        snapshot
            .decision_source
            .as_ref()
            .map(|source| source.rule_id.as_str()),
        Some("enterprise-finance-deny")
    );
    assert!(
        snapshot
            .inherited_sources
            .iter()
            .any(|source| source.rule_id == "fintech_strict:decision-local-allow"),
        "runtime fallback source should be retained for replay"
    );
}

#[tokio::test]
async fn policy_decision_resolver_matches_every_recorded_data_class() {
    let state = test_state().await;
    let tenant_a = tenant("org-a", "workspace-a", "user-a");
    state.enterprise.policy_rules.write().await.insert(
        "enterprise-financial-record-deny".to_string(),
        EnterprisePolicyRule::new(
            "enterprise-financial-record-deny",
            "enterprise-data-floor",
            EnterprisePolicyScopeLevel::Enterprise,
            EnterprisePolicyEffect::Deny,
        )
        .with_tenant_context(tenant_a.clone())
        .with_data_classes(vec![DataClass::FinancialRecord])
        .with_reason(
            "enterprise_financial_record_floor",
            "enterprise policy denies financial record access",
        ),
    );

    let mut decision = policy_decision("decision-multi-class", tenant_a, "run-target", 100);
    decision.decision = PolicyDecisionEffect::Allow;
    decision.data_classes = vec![DataClass::CustomerData, DataClass::FinancialRecord];
    decision.reason_code = "local_policy_allow".to_string();
    decision.reason = "local tool authority allowed the request".to_string();

    let recorded = state
        .record_policy_decision(decision)
        .await
        .expect("record policy decision");

    assert_eq!(recorded.decision, PolicyDecisionEffect::Deny);
    assert_eq!(recorded.reason_code, "enterprise_financial_record_floor");
    let snapshot = recorded
        .effective_policy_snapshot()
        .expect("effective policy snapshot");
    assert_eq!(
        snapshot
            .decision_source
            .as_ref()
            .map(|source| source.rule_id.as_str()),
        Some("enterprise-financial-record-deny")
    );
}

#[tokio::test]
async fn policy_decisions_route_requires_premium_governance() {
    let mut state = test_state().await;
    state.governance_engine = Arc::new(UnavailableGovernanceEngine);

    let response = app_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/governance/policy-decisions")
                .body(Body::empty())
                .expect("policy decisions list request"),
        )
        .await
        .expect("policy decisions list response");

    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let payload = response_json(response).await;
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("PREMIUM_FEATURE_REQUIRED")
    );
}
