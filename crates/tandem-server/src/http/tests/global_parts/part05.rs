// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Continuation split from part03.rs for the file-size gate (same module via include!).


/// GOV-B1: arrange a run parked on a `publish` approval gate.
async fn arrange_awaiting_publish_gate(
    state: &AppState,
    automation_id: &str,
) -> crate::automation_v2::types::AutomationV2RunRecord {
    let automation = create_branched_test_automation_v2(state, automation_id).await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::AwaitingApproval;
            row.checkpoint.completed_nodes = vec![
                "research".to_string(),
                "analysis".to_string(),
                "draft".to_string(),
            ];
            row.checkpoint.pending_nodes = vec!["publish".to_string()];
            row.checkpoint.awaiting_gate = Some(crate::AutomationPendingGate {
                node_id: "publish".to_string(),
                title: "Publish approval".to_string(),
                instructions: Some("approve final publish step".to_string()),
                decisions: vec![
                    "approve".to_string(),
                    "rework".to_string(),
                    "cancel".to_string(),
                ],
                rework_targets: vec!["draft".to_string()],
                requested_at_ms: crate::now_ms(),
                upstream_node_ids: vec!["analysis".to_string(), "draft".to_string()],
                metadata: None,
                expiry_policy: None,
            });
            row.checkpoint.blocked_nodes = vec!["publish".to_string()];
        })
        .await
        .expect("updated run")
}

async fn arrange_governed_awaiting_publish_gate(
    state: &AppState,
    automation_id: &str,
    tenant_context: tandem_types::TenantContext,
    requester_id: &str,
    metadata: serde_json::Value,
) -> crate::automation_v2::types::AutomationV2RunRecord {
    let mut automation = create_branched_test_automation_v2_for_tenant(
        state,
        automation_id,
        &tenant_context,
    )
    .await;
    automation.creator_id = requester_id.to_string();
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("stored explicit automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::AwaitingApproval;
            row.checkpoint.completed_nodes = vec![
                "research".to_string(),
                "analysis".to_string(),
                "draft".to_string(),
            ];
            row.checkpoint.pending_nodes = vec!["publish".to_string()];
            row.checkpoint.awaiting_gate = Some(crate::AutomationPendingGate {
                node_id: "publish".to_string(),
                title: "Publish approval".to_string(),
                instructions: Some("approve final publish step".to_string()),
                decisions: vec![
                    "approve".to_string(),
                    "rework".to_string(),
                    "cancel".to_string(),
                ],
                rework_targets: vec!["draft".to_string()],
                requested_at_ms: crate::now_ms(),
                upstream_node_ids: vec!["analysis".to_string(), "draft".to_string()],
                metadata: Some(metadata.clone()),
                expiry_policy: None,
            });
            row.checkpoint.blocked_nodes = vec!["publish".to_string()];
        })
        .await
        .expect("updated run")
}

fn reviewer_decider(actor_id: &str) -> crate::automation_v2::governance::GovernanceActorRef {
    reviewer_decider_from_source(actor_id, "test")
}

fn reviewer_decider_from_source(
    actor_id: &str,
    source: &str,
) -> crate::automation_v2::governance::GovernanceActorRef {
    crate::automation_v2::governance::GovernanceActorRef::human(
        Some(actor_id.to_string()),
        source.to_string(),
    )
}

fn explicit_tenant(actor_id: &str) -> tandem_types::TenantContext {
    tandem_types::TenantContext::explicit_user_workspace("acme", "finance", None, actor_id)
}

fn elevated_gate_metadata(resource: &tandem_types::ResourceRef) -> serde_json::Value {
    json!({
        "gate": {
            "reviewer_eligibility": "elevated_reviewer",
            "risk_tier": "financial_record_access",
            "data_classes": ["financial_record"],
            "resource": resource,
        }
    })
}

fn verified_reviewer_context(
    actor_id: &str,
    tenant_context: tandem_types::TenantContext,
    resource: tandem_types::ResourceRef,
    grant_permissions: Vec<tandem_types::AccessPermission>,
) -> tandem_types::VerifiedTenantContext {
    let principal = tandem_types::PrincipalRef::human_user(actor_id);
    let grant = tandem_types::ScopedGrant::new(
        "grant-reviewer",
        principal.clone(),
        resource.clone(),
        tandem_types::GrantSource::Direct,
    )
    .with_permissions(grant_permissions)
    .with_data_classes(vec![tandem_types::DataClass::FinancialRecord]);
    let strict_projection = tandem_types::StrictTenantContext::new(
        tenant_context.clone(),
        principal.clone(),
        tandem_types::AuthorityChain::from_request(
            tandem_types::RequestPrincipal::authenticated_user(actor_id, "test"),
        ),
        tandem_types::ResourceScope::root(resource),
        tandem_types::AssertionMetadata::new(
            "tandem-web",
            "tandem-runtime",
            1_000,
            9_999_999_999_999,
            "assertion-reviewer",
        ),
    )
    .with_grants(vec![grant])
    .with_data_boundary(tandem_types::DataBoundary::allow(vec![
        tandem_types::DataClass::FinancialRecord,
    ]));
    tandem_types::VerifiedTenantContext {
        tenant_context,
        human_actor: tandem_types::HumanActor::tandem_user(actor_id),
        authority_chain: tandem_types::AuthorityChain::from_request(
            tandem_types::RequestPrincipal::authenticated_user(actor_id, "test"),
        ),
        roles: Vec::new(),
        org_units: Vec::new(),
        capabilities: Vec::new(),
        policy_version: None,
        strict_projection: Some(strict_projection),
        issuer: "tandem-web".to_string(),
        audience: "tandem-runtime".to_string(),
        issued_at_ms: 1_000,
        expires_at_ms: 9_999_999_999_999,
        assertion_id: "assertion-reviewer".to_string(),
        assertion_key_id: None,
    }
}

fn hosted_gate_policy(
    version: u64,
    reviewer_has_hosted_use: bool,
    now: u64,
) -> tandem_enterprise_contract::hosted_policy::HostedPolicyBundle {
    let capabilities = tandem_enterprise_contract::hosted_policy::role_capabilities("member")
        .into_iter()
        .filter(|capability| reviewer_has_hosted_use || *capability != "hosted.use")
        .collect::<Vec<_>>();
    tandem_enterprise_contract::hosted_policy::HostedPolicyBundle::from_json(
        json!({
            "schema_version": 1,
            "policy_version": version,
            "organization_id": "org-a",
            "deployment_id": "dep-a",
            "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
            "users": [{
                "id": "reviewer", "email": null, "username": null,
                "role": "member", "capabilities": capabilities,
                "is_active": true, "email_verified": true
            }],
            "org_units": [{
                "id": "reviewers", "slug": "reviewers", "display_name": "Reviewers",
                "kind": "team", "state": "active"
            }],
            "org_unit_memberships": [{"unit_id": "reviewers", "user_id": "reviewer"}],
            "deployment_grants": []
        })
        .to_string()
        .as_bytes(),
    )
    .expect("hosted gate policy")
}

async fn hosted_gate_reviewer_context(
    state: &AppState,
    now: u64,
) -> tandem_types::VerifiedTenantContext {
    let tenant = tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".into()),
        "reviewer",
    );
    let mut claims = tandem_types::TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        now,
        now + 60_000,
        "gate-reviewer-assertion",
        tenant,
        tandem_types::HumanActor::tandem_user("reviewer"),
        tandem_types::AuthorityChain::from_request(
            tandem_types::RequestPrincipal::authenticated_user("reviewer", "tandem-web"),
        ),
        vec!["hosted:role:member".into()],
    );
    claims.policy_version = Some(1);
    claims.org_units = vec!["reviewers".into()];
    claims.capabilities = tandem_enterprise_contract::hosted_policy::role_capabilities("member")
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();
    let mut verified: tandem_types::VerifiedTenantContext = claims.into();
    let memberships = state
        .enterprise
        .hosted_policy
        .project(&mut verified)
        .expect("project hosted reviewer")
        .expect("hosted memberships");
    crate::http::middleware::enrich_verified_context_with_org_unit_grants(
        state,
        &mut verified,
        Some(memberships),
    )
    .await;
    verified
}

async fn arrange_hosted_review_gate(
    state: &AppState,
    automation_id: &str,
) -> (
    crate::automation_v2::types::AutomationV2RunRecord,
    tandem_types::ResourceRef,
) {
    let requester = tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".into()),
        "requester",
    );
    let resource = tandem_types::ResourceRef::new(
        "org-a",
        "dep-a",
        tandem_types::ResourceKind::Approval,
        format!("{automation_id}:publish"),
    );
    let run = arrange_governed_awaiting_publish_gate(
        state,
        automation_id,
        requester,
        "requester",
        elevated_gate_metadata(&resource),
    )
    .await;
    let grant_tenant = tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".into()),
        "reviewer",
    );
    state.enterprise.org_unit_access_grants.write().await.insert(
        "reviewer-approval-grant".into(),
        tandem_types::OrganizationUnitAccessGrant::active(
            "reviewer-approval-grant",
            grant_tenant,
            tandem_enterprise_contract::hosted_policy::hosted_unit_principal("reviewers"),
            resource.clone(),
            crate::now_ms(),
        )
        .with_permissions(vec![tandem_types::AccessPermission::Admin])
        .with_data_classes(vec![tandem_types::DataClass::FinancialRecord]),
    );
    (run, resource)
}

/// GOV-B1: an agent-context caller cannot decide (self-approve) an approval gate.
#[tokio::test]
async fn gate_decision_rejects_agent_context_caller() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let run = arrange_awaiting_publish_gate(&state, "auto-v2-gate-agent-reject").await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/gate", run.run_id))
                .header("content-type", "application/json")
                // Forge an agent identity. `request-source: agent` prevents the
                // control-panel short-circuit, so the actor resolves to an agent.
                .header("x-tandem-agent-id", "agent-a")
                .header("x-tandem-request-source", "agent")
                .body(Body::from(json!({ "decision": "approve" }).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let after = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after rejected decision");
    // The gate must remain undecided and the run still awaiting approval.
    assert_eq!(after.status, crate::AutomationRunStatus::AwaitingApproval);
    assert!(after.checkpoint.gate_history.is_empty());
}

#[tokio::test]
async fn governed_gate_rejects_requester_self_approval_and_audits() {
    let state = test_state().await;
    let tenant = explicit_tenant("requester");
    let resource = tandem_types::ResourceRef::new(
        "acme",
        "finance",
        tandem_types::ResourceKind::Approval,
        "auto-v2-self-approval:publish",
    );
    let run = arrange_governed_awaiting_publish_gate(
        &state,
        "auto-v2-self-approval",
        tenant.clone(),
        "requester",
        elevated_gate_metadata(&resource),
    )
    .await;

    let result = crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state.clone(),
        tenant,
        None,
        run.run_id.clone(),
        crate::http::routines_automations::AutomationV2GateDecisionInput {
            decision: "approve".to_string(),
            reason: None,
            approval_request_id: None,
            transition_id: None,
        },
        reviewer_decider("requester"),
    )
    .await;

    let (status, body) = result.expect_err("self approval rejected");
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body.0.get("code").and_then(serde_json::Value::as_str),
        Some("AUTOMATION_V2_GATE_SELF_APPROVAL_FORBIDDEN")
    );
    let after = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after rejected decision");
    assert_eq!(after.status, crate::AutomationRunStatus::AwaitingApproval);
    assert!(after.checkpoint.gate_history.is_empty());
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit");
    assert!(audit.contains("\"event_type\":\"automation.governance.gate_decision_denied\""));
    assert!(audit.contains("AUTOMATION_V2_GATE_SELF_APPROVAL_FORBIDDEN"));
    assert!(audit.contains("auto-v2-self-approval"));
}

#[tokio::test]
async fn gate_decision_rejects_cross_run_approval_request_and_records_evidence() {
    let state = test_state().await;
    let tenant = explicit_tenant("reviewer");
    let run = arrange_governed_awaiting_publish_gate(
        &state,
        "auto-v2-transition-guard-http",
        tenant.clone(),
        "requester",
        serde_json::json!({}),
    )
    .await;

    let result = crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state.clone(),
        tenant.clone(),
        None,
        run.run_id.clone(),
        crate::http::routines_automations::AutomationV2GateDecisionInput {
            decision: "approve".to_string(),
            reason: Some("stale card".to_string()),
            approval_request_id: Some("automation_v2:other-run:publish".to_string()),
            transition_id: None,
        },
        reviewer_decider("reviewer"),
    )
    .await;

    let (status, body) = result.expect_err("cross-run approval rejected");
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body.0.get("code").and_then(serde_json::Value::as_str),
        Some("AUTOMATION_V2_GATE_TRANSITION_GUARD_DENIED")
    );
    let after = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after rejected transition");
    assert_eq!(after.status, crate::AutomationRunStatus::AwaitingApproval);
    assert_eq!(
        after
            .checkpoint
            .gate_history
            .last()
            .map(|record| record.decision.as_str()),
        Some("guard_denied")
    );
    let decisions = state.list_policy_decisions(&tenant, 50).await;
    assert!(decisions.iter().any(|decision| {
        decision.policy_id.as_deref() == Some("automation_v2_transition_guard")
            && decision.run_id.as_deref() == Some(run.run_id.as_str())
            && decision.decision == tandem_types::PolicyDecisionEffect::Deny
    }));
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit");
    assert!(audit.contains("AUTOMATION_V2_GATE_TRANSITION_GUARD_DENIED"));

    let expected_request_id = format!("automation_v2:{}:publish", run.run_id);
    let expected_transition_id = format!("{expected_request_id}:decision");
    crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state.clone(),
        tenant,
        None,
        run.run_id.clone(),
        crate::http::routines_automations::AutomationV2GateDecisionInput {
            decision: "approve".to_string(),
            reason: Some("current card".to_string()),
            approval_request_id: Some(expected_request_id),
            transition_id: Some(expected_transition_id),
        },
        reviewer_decider("reviewer"),
    )
    .await
    .expect("matching approval request applies after denial");
    let approved = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after approved transition");
    assert_eq!(approved.status, crate::AutomationRunStatus::Queued);
    assert_eq!(
        approved
            .checkpoint
            .gate_history
            .last()
            .map(|record| record.decision.as_str()),
        Some("approve")
    );
}

#[tokio::test]
async fn governed_gate_normalizes_channel_identity_for_self_approval() {
    let state = test_state().await;
    let tenant = explicit_tenant("channel:slack:U123");
    let resource = tandem_types::ResourceRef::new(
        "acme",
        "finance",
        tandem_types::ResourceKind::Approval,
        "auto-v2-channel-self-approval:publish",
    );
    let run = arrange_governed_awaiting_publish_gate(
        &state,
        "auto-v2-channel-self-approval",
        tenant.clone(),
        "requester",
        elevated_gate_metadata(&resource),
    )
    .await;

    let result = crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state.clone(),
        tenant,
        None,
        run.run_id.clone(),
        crate::http::routines_automations::AutomationV2GateDecisionInput {
            decision: "approve".to_string(),
            reason: None,
            approval_request_id: None,
            transition_id: None,
        },
        reviewer_decider_from_source("U123", "slack"),
    )
    .await;

    let (status, body) = result.expect_err("channel self approval rejected");
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body.0.get("code").and_then(serde_json::Value::as_str),
        Some("AUTOMATION_V2_GATE_SELF_APPROVAL_FORBIDDEN")
    );
}

#[tokio::test]
async fn governed_gate_rejects_elevated_reviewer_without_matching_authority() {
    let state = test_state().await;
    let requester_tenant = explicit_tenant("requester");
    let reviewer_tenant = explicit_tenant("reviewer");
    let resource = tandem_types::ResourceRef::new(
        "acme",
        "finance",
        tandem_types::ResourceKind::Approval,
        "auto-v2-reviewer-denied:publish",
    );
    let run = arrange_governed_awaiting_publish_gate(
        &state,
        "auto-v2-reviewer-denied",
        requester_tenant,
        "requester",
        elevated_gate_metadata(&resource),
    )
    .await;

    let result = crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state.clone(),
        reviewer_tenant,
        None,
        run.run_id.clone(),
        crate::http::routines_automations::AutomationV2GateDecisionInput {
            decision: "approve".to_string(),
            reason: None,
            approval_request_id: None,
            transition_id: None,
        },
        reviewer_decider("reviewer"),
    )
    .await;

    let (status, body) = result.expect_err("authority rejected");
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body.0.get("code").and_then(serde_json::Value::as_str),
        Some("AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_REQUIRED")
    );
}

#[tokio::test]
async fn governed_gate_allows_channel_verified_elevated_reviewer() {
    let state = test_state().await;
    let requester_tenant = explicit_tenant("requester");
    let channel_tenant = explicit_tenant("channel:slack:U999");
    let resource = tandem_types::ResourceRef::new(
        "acme",
        "finance",
        tandem_types::ResourceKind::Approval,
        "auto-v2-channel-reviewer-allowed:publish",
    );
    let run = arrange_governed_awaiting_publish_gate(
        &state,
        "auto-v2-channel-reviewer-allowed",
        requester_tenant,
        "requester",
        elevated_gate_metadata(&resource),
    )
    .await;

    let result = crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state.clone(),
        channel_tenant,
        None,
        run.run_id.clone(),
        crate::http::routines_automations::AutomationV2GateDecisionInput {
            decision: "approve".to_string(),
            reason: Some("channel approve-tier reviewer".to_string()),
            approval_request_id: None,
            transition_id: None,
        },
        reviewer_decider_from_source("U999", "slack"),
    )
    .await;

    assert!(
        result.is_ok(),
        "channel verified reviewer can approve elevated gate"
    );
    let after = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after approved channel decision");
    assert_eq!(after.status, crate::AutomationRunStatus::Queued);
    assert_eq!(after.checkpoint.gate_history.len(), 1);
}

#[tokio::test]
async fn governed_gate_allows_elevated_reviewer_with_matching_authority() {
    let state = test_state().await;
    let requester_tenant = explicit_tenant("requester");
    let reviewer_tenant = explicit_tenant("reviewer");
    let resource = tandem_types::ResourceRef::new(
        "acme",
        "finance",
        tandem_types::ResourceKind::Approval,
        "auto-v2-reviewer-allowed:publish",
    );
    let run = arrange_governed_awaiting_publish_gate(
        &state,
        "auto-v2-reviewer-allowed",
        requester_tenant,
        "requester",
        elevated_gate_metadata(&resource),
    )
    .await;
    let verified = verified_reviewer_context(
        "reviewer",
        reviewer_tenant.clone(),
        resource,
        vec![tandem_types::AccessPermission::Admin],
    );

    let result = crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state.clone(),
        reviewer_tenant,
        Some(verified),
        run.run_id.clone(),
        crate::http::routines_automations::AutomationV2GateDecisionInput {
            decision: "approve".to_string(),
            reason: Some("eligible reviewer".to_string()),
            approval_request_id: None,
            transition_id: None,
        },
        reviewer_decider("reviewer"),
    )
    .await;

    assert!(result.is_ok(), "authorized reviewer can approve");
    let after = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after approved decision");
    assert_eq!(after.status, crate::AutomationRunStatus::Queued);
    assert_eq!(after.checkpoint.gate_history.len(), 1);
}

#[tokio::test]
async fn governed_gate_rejects_elevated_reviewer_with_unrelated_resource_grant() {
    let state = test_state().await;
    let requester_tenant = explicit_tenant("requester");
    let reviewer_tenant = explicit_tenant("reviewer");
    let resource = tandem_types::ResourceRef::new(
        "acme",
        "finance",
        tandem_types::ResourceKind::Approval,
        "auto-v2-reviewer-unrelated:publish",
    );
    let unrelated_resource = tandem_types::ResourceRef::new(
        "acme",
        "finance",
        tandem_types::ResourceKind::Approval,
        "another-automation:publish",
    );
    let run = arrange_governed_awaiting_publish_gate(
        &state,
        "auto-v2-reviewer-unrelated",
        requester_tenant,
        "requester",
        elevated_gate_metadata(&resource),
    )
    .await;
    let verified = verified_reviewer_context(
        "reviewer",
        reviewer_tenant.clone(),
        unrelated_resource,
        vec![tandem_types::AccessPermission::Admin],
    );

    let result = crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state.clone(),
        reviewer_tenant,
        Some(verified),
        run.run_id.clone(),
        crate::http::routines_automations::AutomationV2GateDecisionInput {
            decision: "approve".to_string(),
            reason: None,
            approval_request_id: None,
            transition_id: None,
        },
        reviewer_decider("reviewer"),
    )
    .await;

    let (status, body) = result.expect_err("unrelated resource grant rejected");
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body.0.get("code").and_then(serde_json::Value::as_str),
        Some("AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_DENIED")
    );
    let after = state.get_automation_v2_run(&run.run_id).await.expect("run");
    assert_eq!(after.status, crate::AutomationRunStatus::AwaitingApproval);
    assert!(after.checkpoint.gate_history.is_empty());
}

#[tokio::test]
async fn governed_gate_rechecks_revoked_hosted_resource_grant_at_commit() {
    let state = test_state().await;
    let now = crate::now_ms();
    state
        .enterprise
        .hosted_policy
        .install_test_bundle(hosted_gate_policy(1, true, now))
        .expect("initial hosted policy");
    let (run, resource) = arrange_hosted_review_gate(&state, "hosted-gate-grant-revoked").await;
    let verified = hosted_gate_reviewer_context(&state, now).await;
    assert_eq!(
        verified
            .strict_projection
            .as_ref()
            .unwrap()
            .evaluate_access(
                &resource,
                tandem_types::AccessPermission::Admin,
                tandem_types::DataClass::FinancialRecord,
                crate::now_ms(),
            )
            .decision,
        tandem_types::AccessDecision::Allow,
        "ingress authorized the reviewer before revocation"
    );
    state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .remove("reviewer-approval-grant");
    let reviewer_tenant = verified.tenant_context.clone();
    let result = crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state.clone(),
        reviewer_tenant,
        Some(verified.clone()),
        run.run_id.clone(),
        crate::http::routines_automations::AutomationV2GateDecisionInput {
            decision: "approve".to_string(),
            reason: None,
            approval_request_id: None,
            transition_id: None,
        },
        reviewer_decider("reviewer"),
    )
    .await;
    let (status, body) = result.expect_err("revoked reviewer grant denied at commit");
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body.0["code"],
        "AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_DENIED"
    );
    let after = state.get_automation_v2_run(&run.run_id).await.unwrap();
    assert_eq!(after.status, crate::AutomationRunStatus::AwaitingApproval);
    assert!(after.checkpoint.gate_history.is_empty());
    assert_eq!(after.updated_at_ms, run.updated_at_ms);
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit");
    assert!(audit.contains("AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_DENIED"));

    // The same still-valid assertion becomes eligible when its scoped grant is
    // restored. This controls for accidentally denying all hosted reviewers.
    state.enterprise.org_unit_access_grants.write().await.insert(
        "reviewer-approval-grant".into(),
        tandem_types::OrganizationUnitAccessGrant::active(
            "reviewer-approval-grant",
            verified.tenant_context.clone(),
            tandem_enterprise_contract::hosted_policy::hosted_unit_principal("reviewers"),
            resource,
            crate::now_ms(),
        )
        .with_permissions(vec![tandem_types::AccessPermission::Admin])
        .with_data_classes(vec![tandem_types::DataClass::FinancialRecord]),
    );
    crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state.clone(),
        verified.tenant_context.clone(),
        Some(verified),
        run.run_id.clone(),
        crate::http::routines_automations::AutomationV2GateDecisionInput {
            decision: "approve".to_string(),
            reason: None,
            approval_request_id: None,
            transition_id: None,
        },
        reviewer_decider("reviewer"),
    )
    .await
    .expect("reviewer with current matching grant may approve");
    let approved = state.get_automation_v2_run(&run.run_id).await.unwrap();
    assert_eq!(approved.status, crate::AutomationRunStatus::Queued);
    assert_eq!(approved.checkpoint.gate_history.len(), 1);
}

#[tokio::test]
async fn governed_gate_rechecks_revoked_hosted_use_at_commit() {
    let state = test_state().await;
    let now = crate::now_ms();
    state
        .enterprise
        .hosted_policy
        .install_test_bundle(hosted_gate_policy(1, true, now))
        .expect("initial hosted policy");
    let (run, _) = arrange_hosted_review_gate(&state, "hosted-gate-use-revoked").await;
    let verified = hosted_gate_reviewer_context(&state, now).await;
    assert!(verified
        .strict_projection
        .as_ref()
        .unwrap()
        .has_permission(tandem_types::AccessPermission::HostedUse));
    state
        .enterprise
        .hosted_policy
        .install_test_bundle(hosted_gate_policy(2, false, crate::now_ms()))
        .expect("new policy removes hosted use");
    let result = crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state.clone(),
        verified.tenant_context.clone(),
        Some(verified),
        run.run_id.clone(),
        crate::http::routines_automations::AutomationV2GateDecisionInput {
            decision: "approve".to_string(),
            reason: None,
            approval_request_id: None,
            transition_id: None,
        },
        reviewer_decider("reviewer"),
    )
    .await;
    let (status, body) = result.expect_err("revoked hosted use denied at commit");
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body.0["code"],
        "AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_DENIED"
    );
    let after = state.get_automation_v2_run(&run.run_id).await.unwrap();
    assert_eq!(after.status, crate::AutomationRunStatus::AwaitingApproval);
    assert!(after.checkpoint.gate_history.is_empty());
    assert_eq!(after.updated_at_ms, run.updated_at_ms);
}

#[tokio::test]
async fn governed_gate_rechecks_same_node_reviewer_policy_under_run_lock() {
    let state = test_state().await;
    let tenant = explicit_tenant("requester");
    let run = arrange_governed_awaiting_publish_gate(
        &state,
        "gate-policy-replaced",
        tenant.clone(),
        "requester",
        json!({}),
    )
    .await;
    let automation = state
        .get_automation_v2(&run.automation_id)
        .await
        .expect("automation");
    let submitted_gate = run.checkpoint.awaiting_gate.clone().expect("ordinary gate");
    let resource = tandem_types::ResourceRef::new(
        "acme",
        "finance",
        tandem_types::ResourceKind::Approval,
        "gate-policy-replaced:publish",
    );
    let mut runs = state.automation_v2_runs.write().await;
    let live_run = runs.get_mut(&run.run_id).expect("live run");
    let live_gate = live_run
        .checkpoint
        .awaiting_gate
        .as_mut()
        .expect("pending gate");
    live_gate.metadata = Some(elevated_gate_metadata(&resource));
    let live_gate = live_gate.clone();
    let result = crate::http::routines_automations::apply_gate_decision_with_current_authority(
        &state,
        live_run,
        &automation,
        &automation,
        &submitted_gate,
        &live_gate,
        "approve",
        None,
        &reviewer_decider("requester"),
        &tenant,
        None,
        None,
        None,
    );
    assert!(result.is_err(), "the old ordinary card cannot decide the elevated gate");
    assert_eq!(live_run.status, crate::AutomationRunStatus::AwaitingApproval);
    assert!(live_run.checkpoint.gate_history.is_empty());
}

#[tokio::test]
async fn governed_gate_rechecks_live_definition_owner_under_run_lock() {
    let state = test_state().await;
    let tenant = explicit_tenant("requester");
    let run = arrange_governed_awaiting_publish_gate(
        &state,
        "gate-owner-replaced",
        tenant.clone(),
        "requester",
        json!({}),
    )
    .await;
    let submitted_automation = state
        .get_automation_v2(&run.automation_id)
        .await
        .expect("automation");
    let mut live_automation = submitted_automation.clone();
    let metadata = live_automation
        .metadata
        .get_or_insert_with(|| json!({}));
    metadata["resource_access"] = json!({
        "visibility": "private",
        "owner_principal": {"kind": "human_user", "id": "new-owner"}
    });
    let gate = run.checkpoint.awaiting_gate.clone().expect("gate");
    let verified = verified_reviewer_context(
        "requester",
        tenant.clone(),
        tandem_types::ResourceRef::new(
            "acme",
            "finance",
            tandem_types::ResourceKind::Approval,
            "gate-owner-replaced:publish",
        ),
        vec![tandem_types::AccessPermission::Admin],
    );
    let mut runs = state.automation_v2_runs.write().await;
    let live_run = runs.get_mut(&run.run_id).expect("live run");
    let result = crate::http::routines_automations::apply_gate_decision_with_current_authority(
        &state,
        live_run,
        &submitted_automation,
        &live_automation,
        &gate,
        &gate,
        "approve",
        None,
        &reviewer_decider("requester"),
        &tenant,
        Some(&verified),
        None,
        None,
    );
    assert!(result.is_err(), "former owner cannot decide the live definition's gate");
    assert_eq!(live_run.status, crate::AutomationRunStatus::AwaitingApproval);
    assert!(live_run.checkpoint.gate_history.is_empty());
}

#[tokio::test]
async fn ordinary_gate_still_requires_run_owner_or_admin() {
    let state = test_state().await;
    let requester_tenant = explicit_tenant("requester");
    let reviewer_tenant = explicit_tenant("reviewer");
    let resource = tandem_types::ResourceRef::new(
        "acme",
        "finance",
        tandem_types::ResourceKind::Approval,
        "auto-v2-ordinary-owner-only:publish",
    );
    let run = arrange_governed_awaiting_publish_gate(
        &state,
        "auto-v2-ordinary-owner-only",
        requester_tenant.clone(),
        "requester",
        json!({}),
    )
    .await;
    let verified = verified_reviewer_context(
        "reviewer",
        reviewer_tenant.clone(),
        resource.clone(),
        vec![tandem_types::AccessPermission::Admin],
    );

    let result = crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state.clone(),
        reviewer_tenant,
        Some(verified),
        run.run_id.clone(),
        crate::http::routines_automations::AutomationV2GateDecisionInput {
            decision: "approve".to_string(),
            reason: None,
            approval_request_id: None,
            transition_id: None,
        },
        reviewer_decider("reviewer"),
    )
    .await;

    let (status, body) = result.expect_err("ordinary gate remains owner-only");
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body.0.get("code").and_then(serde_json::Value::as_str),
        Some("AUTOMATION_V2_ACCESS_DENIED")
    );
    let after = state.get_automation_v2_run(&run.run_id).await.expect("run");
    assert_eq!(after.status, crate::AutomationRunStatus::AwaitingApproval);
    assert!(after.checkpoint.gate_history.is_empty());

    let owner = verified_reviewer_context(
        "requester",
        requester_tenant.clone(),
        resource,
        vec![tandem_types::AccessPermission::Admin],
    );
    let owner_result = crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state.clone(),
        requester_tenant,
        Some(owner),
        run.run_id.clone(),
        crate::http::routines_automations::AutomationV2GateDecisionInput {
            decision: "approve".to_string(),
            reason: None,
            approval_request_id: None,
            transition_id: None,
        },
        reviewer_decider("requester"),
    )
    .await;
    assert!(owner_result.is_ok(), "run owner can still decide ordinary gate");
    let after = state.get_automation_v2_run(&run.run_id).await.expect("run");
    assert_eq!(after.status, crate::AutomationRunStatus::Queued);
    assert_eq!(after.checkpoint.gate_history.len(), 1);
}

/// GOV-B1: a human decision is applied and attributed to a verified decider.
#[tokio::test]
async fn gate_decision_records_human_decider() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let run = arrange_awaiting_publish_gate(&state, "auto-v2-gate-human-decider").await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/gate", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "decision": "approve" }).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let after = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after human decision");
    assert_eq!(after.status, crate::AutomationRunStatus::Queued);
    let decision = after
        .checkpoint
        .gate_history
        .last()
        .expect("gate decision recorded");
    let decided_by = decision
        .decided_by
        .as_ref()
        .expect("decision attributes a decider");
    assert_eq!(
        decided_by.kind,
        crate::automation_v2::governance::GovernanceActorKind::Human
    );
}

/// GOV-B7: sharing an automation is a governed mutation. A human owner may change
/// visibility; an agent-context share is rejected by governance.
#[tokio::test]
async fn automation_v2_share_is_governed() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-share-b7").await;

    // Human (control-panel) owner widens visibility to org.
    let human_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/v2/{}/share", automation.automation_id))
        .header("content-type", "application/json")
        .body(Body::from(json!({ "visibility": "org" }).to_string()))
        .expect("share request");
    let human_resp = app.clone().oneshot(human_req).await.expect("share response");
    assert_eq!(human_resp.status(), StatusCode::OK);

    // Agent-context share is refused by the governance layer.
    let agent_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/v2/{}/share", automation.automation_id))
        .header("content-type", "application/json")
        .header("x-tandem-request-source", "agent")
        .header("x-tandem-agent-id", "agent-share")
        .body(Body::from(json!({ "visibility": "private" }).to_string()))
        .expect("agent share request");
    let agent_resp = app.clone().oneshot(agent_req).await.expect("agent share response");
    assert!(!agent_resp.status().is_success());
}

/// GOV-B9: `run_now` now requires owner/admin altitude (not mere read visibility).
/// In local single-user mode (no verified context) the owner/admin check is a
/// no-op, so a local human can still trigger a run; an agent-context caller is
/// still refused by governance.
#[tokio::test]
async fn run_now_allowed_for_local_human_and_refused_for_agent() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-b9-runnow").await;

    let human_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/v2/{}/run_now", automation.automation_id))
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("run_now request");
    let human_resp = app.clone().oneshot(human_req).await.expect("run_now response");
    assert_eq!(human_resp.status(), StatusCode::OK);

    let agent_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/v2/{}/run_now", automation.automation_id))
        .header("content-type", "application/json")
        .header("x-tandem-request-source", "agent")
        .header("x-tandem-agent-id", "agent-b9")
        .body(Body::from(json!({}).to_string()))
        .expect("agent run_now request");
    let agent_resp = app.clone().oneshot(agent_req).await.expect("agent run_now response");
    assert!(!agent_resp.status().is_success());
}

/// GOV-X1: consequential-route regression guard. Every consequential automation
/// mutation route must refuse a forged agent-context request. Adding a new
/// mutation route without routing it through governance will fail this test.
/// Gated to the OSS build, where agent mutations are uniformly refused by the
/// `UnavailableGovernanceEngine`.
#[cfg(not(feature = "premium-governance"))]
#[tokio::test]
async fn consequential_routes_refuse_agent_context() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-x1-guard").await;
    let aid = automation.automation_id.clone();

    let create_payload = json!({
        "automation_id": "auto-x1-created-by-agent",
        "name": "x1 created by agent",
        "status": "draft",
        "schedule": { "type": "manual", "timezone": "UTC", "misfire_policy": { "type": "skip" } },
        "agents": [{
            "agent_id": "agent-x1",
            "display_name": "Agent X1",
            "skills": [],
            "tool_policy": { "allowlist": ["read"], "denylist": [] },
            "mcp_policy": { "allowed_servers": [] }
        }],
        "flow": { "nodes": [{ "node_id": "n1", "agent_id": "agent-x1", "objective": "x", "depends_on": [] }] },
        "execution": { "max_parallel_agents": 1 }
    });

    let cases: Vec<(&str, String, Option<Value>)> = vec![
        ("POST", "/automations/v2".to_string(), Some(create_payload)),
        ("POST", format!("/automations/v2/{aid}/run_now"), Some(json!({}))),
        ("POST", format!("/automations/v2/{aid}/share"), Some(json!({ "visibility": "org" }))),
        ("PATCH", format!("/automations/v2/{aid}"), Some(json!({ "name": "x1-patched" }))),
        ("DELETE", format!("/automations/v2/{aid}"), None),
    ];

    for (method, path, body) in cases {
        let req = Request::builder()
            .method(method)
            .uri(&path)
            .header("content-type", "application/json")
            .header("x-tandem-request-source", "agent")
            .header("x-tandem-agent-id", "agent-x1")
            .body(body.map(|b| Body::from(b.to_string())).unwrap_or_else(Body::empty))
            .expect("request");
        let resp = app.clone().oneshot(req).await.expect("response");
        assert!(
            !resp.status().is_success(),
            "agent-context {method} {path} must be refused, got {}",
            resp.status()
        );
    }
}

/// GOV-B8: a governance denial of a consequential mutation writes an attributed
/// `automation.governance.denied` protected audit event (not just an HTTP error).
#[cfg(not(feature = "premium-governance"))]
#[tokio::test]
async fn governance_denial_writes_protected_audit() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-b8-deny").await;

    let req = Request::builder()
        .method("POST")
        .uri(format!("/automations/v2/{}/share", automation.automation_id))
        .header("content-type", "application/json")
        .header("x-tandem-request-source", "agent")
        .header("x-tandem-agent-id", "agent-b8")
        .body(Body::from(json!({ "visibility": "org" }).to_string()))
        .expect("agent share request");
    let resp = app.clone().oneshot(req).await.expect("agent share response");
    assert!(!resp.status().is_success(), "agent mutation must be denied");

    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"automation.governance.denied\""));
    assert!(audit.contains("agent-b8"));
    assert!(audit.contains(&automation.automation_id));
}
