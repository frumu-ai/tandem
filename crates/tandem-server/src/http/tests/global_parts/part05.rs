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
    let mut automation = create_branched_test_automation_v2(state, automation_id).await;
    automation.creator_id = requester_id.to_string();
    automation.set_tenant_context(&tenant_context);
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
            });
            row.checkpoint.blocked_nodes = vec!["publish".to_string()];
        })
        .await
        .expect("updated run")
}

fn reviewer_decider(actor_id: &str) -> crate::automation_v2::governance::GovernanceActorRef {
    crate::automation_v2::governance::GovernanceActorRef::human(
        Some(actor_id.to_string()),
        "test".to_string(),
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
    }
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
