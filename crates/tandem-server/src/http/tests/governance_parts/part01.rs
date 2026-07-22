// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Continuation of governance HTTP tests, split from governance.rs to satisfy the
// per-file line-count policy. Included into the same module via governance.rs,
// so it shares the parent module's `use super::*;` imports and helpers.

/// GOV-B4: create a capability approval request and return its id. The request
/// headers control how `requested_by` resolves (human operator vs agent).
async fn create_capability_approval(
    app: &axum::Router,
    agent_id: &str,
    capability_key: &str,
    request_headers: &[(&str, &str)],
) -> String {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/governance/approvals")
        .header("content-type", "application/json");
    for (name, value) in request_headers {
        builder = builder.header(*name, *value);
    }
    let create_req = builder
        .body(Body::from(
            approval_request_payload(agent_id, capability_key).to_string(),
        ))
        .expect("approval create request");
    let create_resp = (*app)
        .clone()
        .oneshot(create_req)
        .await
        .expect("approval create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    response_json(create_resp)
        .await
        .get("approval")
        .and_then(|value| value.get("approval_id"))
        .and_then(Value::as_str)
        .expect("approval id")
        .to_string()
}

#[cfg(feature = "premium-governance")]
fn verified_governance_app(
    state: AppState,
    org_id: &str,
    workspace_id: &str,
    actor_id: &str,
) -> axum::Router {
    let tenant_context = TenantContext::explicit(
        org_id,
        workspace_id,
        Some(actor_id.to_string()),
    );
    let principal = tandem_types::RequestPrincipal::authenticated_user(actor_id, "tandem-test");
    let verified = tandem_types::VerifiedTenantContext {
        tenant_context,
        human_actor: tandem_types::HumanActor::tandem_user(actor_id),
        authority_chain: tandem_types::AuthorityChain::from_request(principal),
        roles: vec!["admin".to_string()],
        org_units: Vec::new(),
        capabilities: vec!["governance.review".to_string(), "governance.admin".to_string()],
        policy_version: None,
        strict_projection: None,
        issuer: "tandem-test".to_string(),
        audience: "tandem-runtime".to_string(),
        issued_at_ms: 1,
        expires_at_ms: 9_999_999_999_999,
        assertion_id: format!("governance-test-{org_id}-{workspace_id}-{actor_id}"),
        assertion_key_id: None,
    };
    app_router(state).layer(axum::Extension(verified))
}

#[cfg(feature = "premium-governance")]
/// GOV-B4: an agent-context caller cannot review (approve) a governance approval.
#[tokio::test]
async fn governance_approval_approve_rejects_agent_reviewer() {
    let state = test_state().await;
    let app = app_router(state);
    let approval_id = create_capability_approval(
        &app,
        "agent-b4-human-only",
        "creates_agents",
        &[("x-tandem-actor-id", "governance-operator")],
    )
    .await;

    let approve_req = Request::builder()
        .method("POST")
        .uri(format!("/governance/approvals/{approval_id}/approve"))
        .header("content-type", "application/json")
        .header("x-tandem-request-source", "agent")
        .header("x-tandem-agent-id", "agent-b4-human-only")
        .body(Body::from(json!({ "notes": "self" }).to_string()))
        .expect("approve request");
    let resp = app
        .clone()
        .oneshot(approve_req)
        .await
        .expect("approve response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response_json(resp)
            .await
            .get("code")
            .and_then(Value::as_str),
        Some("GOVERNANCE_APPROVAL_REQUIRES_HUMAN")
    );
}

#[cfg(feature = "premium-governance")]
/// GOV-B4: an agent-filed request cannot be self-approved by the same agent
/// identity (even when presented as a human actor sharing that id).
#[tokio::test]
async fn governance_approval_rejects_agent_self_review() {
    let state = test_state().await;
    let app = app_router(state);
    let approval_id = create_capability_approval(
        &app,
        "agent-b4-self",
        "creates_agents",
        &[
            ("x-tandem-request-source", "agent"),
            ("x-tandem-agent-id", "agent-b4-self"),
        ],
    )
    .await;

    let approve_req = Request::builder()
        .method("POST")
        .uri(format!("/governance/approvals/{approval_id}/approve"))
        .header("content-type", "application/json")
        .header("x-tandem-actor-id", "agent-b4-self")
        .body(Body::from(json!({ "notes": "self" }).to_string()))
        .expect("approve request");
    let resp = app
        .clone()
        .oneshot(approve_req)
        .await
        .expect("approve response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response_json(resp)
            .await
            .get("code")
            .and_then(Value::as_str),
        Some("GOVERNANCE_APPROVAL_SELF_REVIEW")
    );
}

#[cfg(feature = "premium-governance")]
/// A human-filed governance request still requires a different human reviewer.
#[tokio::test]
async fn governance_approval_rejects_human_self_review() {
    let state = test_state().await;
    let app = app_router(state);
    let approval_id = create_capability_approval(
        &app,
        "agent-human-self-review",
        "creates_agents",
        &[("x-tandem-actor-id", "human-requester")],
    )
    .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/governance/approvals/{approval_id}/approve"))
                .header("content-type", "application/json")
                .header("x-tandem-actor-id", "human-requester")
                .body(Body::from(json!({ "notes": "self review" }).to_string()))
                .expect("human self-review request"),
        )
        .await
        .expect("human self-review response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response_json(response).await["code"].as_str(),
        Some("GOVERNANCE_APPROVAL_SELF_REVIEW")
    );
}

#[cfg(feature = "premium-governance")]
/// CT-09: an approval receipt issued in tenant A must not be approved (replayed)
/// from tenant B. The cross-tenant caller sees the same 404 as a missing receipt so
/// existence is not leaked, while the owning tenant can still approve its own receipt.
#[tokio::test]
async fn governance_approval_rejects_cross_tenant_approve_replay() {
    let state = test_state().await;
    let requester_app = verified_governance_app(state.clone(), "org-a", "workspace-a", "operator-a");
    let tenant_b_app = verified_governance_app(state.clone(), "org-b", "workspace-b", "operator-b");
    let reviewer_app = verified_governance_app(state, "org-a", "workspace-a", "reviewer-a");
    let approval_id = create_capability_approval(
        &requester_app,
        "agent-ct09-approve",
        "creates_agents",
        &[
            ("x-tandem-org-id", "org-a"),
            ("x-tandem-workspace-id", "workspace-a"),
            ("x-tandem-actor-id", "operator-a"),
        ],
    )
    .await;

    // Tenant B replays tenant A's receipt id.
    let replay = Request::builder()
        .method("POST")
        .uri(format!("/governance/approvals/{approval_id}/approve"))
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "org-b")
        .header("x-tandem-workspace-id", "workspace-b")
        .header("x-tandem-actor-id", "operator-b")
        .body(Body::from(json!({ "notes": "cross-tenant" }).to_string()))
        .expect("replay request");
    let resp = tenant_b_app.clone().oneshot(replay).await.expect("replay response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response_json(resp)
            .await
            .get("code")
            .and_then(Value::as_str),
        Some("GOVERNANCE_APPROVAL_NOT_FOUND")
    );

    // The owning tenant can still approve its own receipt (no-op-for-own-tenant).
    let owner = Request::builder()
        .method("POST")
        .uri(format!("/governance/approvals/{approval_id}/approve"))
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "org-a")
        .header("x-tandem-workspace-id", "workspace-a")
        .header("x-tandem-actor-id", "reviewer-a")
        .body(Body::from(json!({ "notes": "owner approves" }).to_string()))
        .expect("owner request");
    let resp = reviewer_app.clone().oneshot(owner).await.expect("owner response");
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        response_json(resp).await["approval"]["status"].as_str(),
        Some("approved")
    );
}

#[cfg(feature = "premium-governance")]
/// CT-09: revocation (deny) must stay tenant-scoped — tenant B cannot deny/revoke an
/// approval receipt issued in tenant A.
#[tokio::test]
async fn governance_approval_rejects_cross_tenant_deny_revocation() {
    let state = test_state().await;
    let requester_app = verified_governance_app(state.clone(), "org-a", "workspace-a", "operator-a");
    let tenant_b_app = verified_governance_app(state.clone(), "org-b", "workspace-b", "operator-b");
    let reviewer_app = verified_governance_app(state, "org-a", "workspace-a", "reviewer-a");
    let approval_id = create_capability_approval(
        &requester_app,
        "agent-ct09-deny",
        "creates_agents",
        &[
            ("x-tandem-org-id", "org-a"),
            ("x-tandem-workspace-id", "workspace-a"),
            ("x-tandem-actor-id", "operator-a"),
        ],
    )
    .await;

    let replay = Request::builder()
        .method("POST")
        .uri(format!("/governance/approvals/{approval_id}/deny"))
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "org-b")
        .header("x-tandem-workspace-id", "workspace-b")
        .header("x-tandem-actor-id", "operator-b")
        .body(Body::from(
            json!({ "notes": "cross-tenant revoke" }).to_string(),
        ))
        .expect("deny request");
    let resp = tenant_b_app.clone().oneshot(replay).await.expect("deny response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response_json(resp)
            .await
            .get("code")
            .and_then(Value::as_str),
        Some("GOVERNANCE_APPROVAL_NOT_FOUND")
    );

    // The owning tenant can still deny its own receipt.
    let owner = Request::builder()
        .method("POST")
        .uri(format!("/governance/approvals/{approval_id}/deny"))
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "org-a")
        .header("x-tandem-workspace-id", "workspace-a")
        .header("x-tandem-actor-id", "reviewer-a")
        .body(Body::from(json!({ "notes": "owner denies" }).to_string()))
        .expect("owner deny request");
    let resp = reviewer_app
        .clone()
        .oneshot(owner)
        .await
        .expect("owner deny response");
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        response_json(resp).await["approval"]["status"].as_str(),
        Some("denied")
    );
}

#[cfg(feature = "premium-governance")]
/// CT-09: listing approvals is tenant-scoped — one tenant's receipts must never be
/// enumerated by another.
#[tokio::test]
async fn governance_approvals_list_is_tenant_scoped() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let approval_id = create_capability_approval(
        &app,
        "agent-ct09-list",
        "creates_agents",
        &[
            ("x-tandem-org-id", "org-a"),
            ("x-tandem-workspace-id", "workspace-a"),
            ("x-tandem-actor-id", "operator-a"),
        ],
    )
    .await;

    let list_for = |org: &str, workspace: &str, actor: &str| {
        Request::builder()
            .method("GET")
            .uri("/governance/approvals")
            .header("x-tandem-org-id", org)
            .header("x-tandem-workspace-id", workspace)
            .header("x-tandem-actor-id", actor)
            .body(Body::empty())
            .expect("list request")
    };

    // Tenant B must not see tenant A's approval.
    let resp = app
        .clone()
        .oneshot(list_for("org-b", "workspace-b", "operator-b"))
        .await
        .expect("tenant-b list response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = response_json(resp).await;
    let ids_b: Vec<&str> = body["approvals"]
        .as_array()
        .expect("approvals array")
        .iter()
        .filter_map(|row| row.get("approval_id").and_then(Value::as_str))
        .collect();
    assert!(
        !ids_b.contains(&approval_id.as_str()),
        "tenant B must not see tenant A's approval: {ids_b:?}"
    );

    // A sibling actor in tenant A also cannot enumerate the request.
    let resp = app
        .clone()
        .oneshot(list_for("org-a", "workspace-a", "operator-sibling"))
        .await
        .expect("tenant-a sibling list response");
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(response_json(resp).await["approvals"]
        .as_array()
        .expect("approvals array")
        .is_empty());

    // The requester still sees its own approval.
    let resp = app
        .clone()
        .oneshot(list_for("org-a", "workspace-a", "operator-a"))
        .await
        .expect("tenant-a list response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = response_json(resp).await;
    let ids_a: Vec<&str> = body["approvals"]
        .as_array()
        .expect("approvals array")
        .iter()
        .filter_map(|row| row.get("approval_id").and_then(Value::as_str))
        .collect();
    assert!(
        ids_a.contains(&approval_id.as_str()),
        "tenant A must see its own approval: {ids_a:?}"
    );

    // An eligible tenant reviewer can enumerate the review queue.
    let reviewer_app = verified_governance_app(
        state,
        "org-a",
        "workspace-a",
        "reviewer-a",
    );
    let resp = reviewer_app
        .clone()
        .oneshot(list_for("org-a", "workspace-a", "reviewer-a"))
        .await
        .expect("reviewer list response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = response_json(resp).await;
    assert!(body["approvals"]
        .as_array()
        .expect("approvals array")
        .iter()
        .any(|row| row["approval_id"] == approval_id));

    let resp = reviewer_app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/governance/approvals")
                .header("x-tandem-org-id", "org-a")
                .header("x-tandem-workspace-id", "workspace-a")
                .header("x-tandem-actor-id", "reviewer-a")
                .header("x-tandem-agent-id", "agent-reviewer")
                .body(Body::empty())
                .expect("agent reviewer list request"),
        )
        .await
        .expect("agent reviewer list response");
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        response_json(resp).await["approvals"]
            .as_array()
            .expect("approvals array")
            .is_empty(),
        "authoritative agent must not inherit reviewer-wide approval access"
    );
}

#[cfg(feature = "premium-governance")]
/// Known automation IDs must not expose governance or grant records across tenants.
#[tokio::test]
async fn automation_governance_reads_reject_cross_tenant_known_ids() {
    let state = test_state().await;
    let app = app_router(state);
    let automation_id = "auto-governance-cross-tenant";

    let create = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "org-a")
        .header("x-tandem-workspace-id", "workspace-a")
        .header("x-tandem-actor-id", "owner-a")
        .body(Body::from(
            automation_v2_payload(automation_id, "agent-a", None).to_string(),
        ))
        .expect("tenant-a automation create");
    assert_eq!(
        app.clone()
            .oneshot(create)
            .await
            .expect("tenant-a create response")
            .status(),
        StatusCode::OK
    );

    for uri in [
        format!("/automations/v2/{automation_id}/governance"),
        format!("/automations/v2/{automation_id}/grants"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .header("x-tandem-org-id", "org-b")
                    .header("x-tandem-workspace-id", "workspace-b")
                    .header("x-tandem-actor-id", "reader-b")
                    .body(Body::empty())
                    .expect("tenant-b known-id request"),
            )
            .await
            .expect("tenant-b known-id response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response_json(response).await["code"].as_str(),
            Some("AUTOMATION_GOVERNANCE_NOT_FOUND")
        );
    }
}

#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn authorized_admin_and_independent_reviewer_can_create_tenant_grant() {
    let state = test_state().await;
    let requester_app =
        verified_governance_app(state.clone(), "org-a", "workspace-a", "operator-a");
    let reviewer_app =
        verified_governance_app(state.clone(), "org-a", "workspace-a", "reviewer-a");
    let tenant_b_app =
        verified_governance_app(state, "org-b", "workspace-b", "operator-b");
    let automation_id = "auto-governance-positive-grant";

    let create = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "org-a")
        .header("x-tandem-workspace-id", "workspace-a")
        .header("x-tandem-actor-id", "operator-a")
        .body(Body::from(
            automation_v2_payload(automation_id, "agent-a", None).to_string(),
        ))
        .expect("automation create");
    assert_eq!(
        requester_app
            .clone()
            .oneshot(create)
            .await
            .expect("automation create response")
            .status(),
        StatusCode::OK
    );

    let approval_create = Request::builder()
        .method("POST")
        .uri("/governance/approvals")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "org-a")
        .header("x-tandem-workspace-id", "workspace-a")
        .header("x-tandem-actor-id", "operator-a")
        .body(Body::from(
            json!({
                "request_type": "capability_request",
                "target_resource": { "type": "automation", "id": automation_id },
                "rationale": "allow a tenant-scoped automation modifier",
                "context": {
                    "action": "grant_modify_access",
                    "parameters": {
                        "grantedToAgentID": "agent-modifier",
                        "reason": "reviewed delegation"
                    }
                }
            })
            .to_string(),
        ))
        .expect("approval create");
    let response = requester_app
        .clone()
        .oneshot(approval_create)
        .await
        .expect("approval create response");
    assert_eq!(response.status(), StatusCode::OK);
    let approval_id = response_json(response).await["approval"]["approval_id"]
        .as_str()
        .expect("approval id")
        .to_string();

    let approve = Request::builder()
        .method("POST")
        .uri(format!("/governance/approvals/{approval_id}/approve"))
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "org-a")
        .header("x-tandem-workspace-id", "workspace-a")
        .header("x-tandem-actor-id", "reviewer-a")
        .body(Body::from(
            json!({ "notes": "independent review complete" }).to_string(),
        ))
        .expect("approval decision");
    assert_eq!(
        reviewer_app
            .oneshot(approve)
            .await
            .expect("approval decision response")
            .status(),
        StatusCode::OK
    );

    let create_grant = |org: &str, workspace: &str, actor: &str, agent_id: &str| {
        Request::builder()
            .method("POST")
            .uri(format!("/automations/v2/{automation_id}/grants"))
            .header("content-type", "application/json")
            .header("x-tandem-org-id", org)
            .header("x-tandem-workspace-id", workspace)
            .header("x-tandem-actor-id", actor)
            .body(Body::from(
                json!({
                    "approval_id": approval_id,
                    "granted_to_agent_id": agent_id,
                    "reason": "reviewed delegation"
                })
                .to_string(),
            ))
            .expect("grant create")
    };

    let cross_tenant = tenant_b_app
        .oneshot(create_grant(
            "org-b",
            "workspace-b",
            "operator-b",
            "agent-modifier",
        ))
        .await
        .expect("cross-tenant grant response");
    assert_eq!(cross_tenant.status(), StatusCode::NOT_FOUND);

    let mismatched_payload = requester_app
        .clone()
        .oneshot(create_grant(
            "org-a",
            "workspace-a",
            "operator-a",
            "agent-not-reviewed",
        ))
        .await
        .expect("mismatched grant response");
    assert_eq!(mismatched_payload.status(), StatusCode::FORBIDDEN);

    let response = requester_app
        .clone()
        .oneshot(create_grant(
            "org-a",
            "workspace-a",
            "operator-a",
            "agent-modifier",
        ))
        .await
        .expect("authorized grant response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_json(response).await["grant"]["granted_to"]["actor_id"].as_str(),
        Some("agent-modifier")
    );

    let replay = requester_app
        .oneshot(create_grant(
            "org-a",
            "workspace-a",
            "operator-a",
            "agent-modifier",
        ))
        .await
        .expect("replayed grant response");
    assert_eq!(replay.status(), StatusCode::FORBIDDEN);
}

#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn approved_lifecycle_retry_does_not_acknowledge_newer_review() {
    let state = test_state().await;
    let tenant = TenantContext::explicit(
        "org-review-retry",
        "workspace-review-retry",
        Some("owner".to_string()),
    );
    let automation = super::global::create_test_automation_v2_for_tenant(
        &state,
        "auto-governance-review-retry",
        &tenant,
    )
    .await;
    {
        let mut governance = state.automation_governance.write().await;
        let record = governance
            .records
            .get_mut(&automation.automation_id)
            .expect("governance record");
        record.review_required = true;
        record.review_kind = Some(
            crate::automation_v2::governance::AutomationLifecycleReviewKind::RunDrift,
        );
        record.paused_for_lifecycle = true;
        record.review_request_id = None;
    }
    let approval = state
        .request_approval(
            crate::automation_v2::governance::GovernanceApprovalRequestType::LifecycleReview,
            crate::automation_v2::governance::GovernanceActorRef::system("run_review"),
            crate::automation_v2::governance::GovernanceResourceRef {
                resource_type: "automation".to_string(),
                id: automation.automation_id.clone(),
            },
            "review run drift".to_string(),
            json!({"trigger": "run_drift"}),
            None,
            &tenant,
        )
        .await
        .expect("request lifecycle review");
    let reviewer = crate::automation_v2::governance::GovernanceActorRef::human(
        Some("reviewer".to_string()),
        "test",
    );
    state
        .decide_approval_request(
            &approval.approval_id,
            reviewer.clone(),
            true,
            Some("reviewed".to_string()),
            &tenant,
        )
        .await
        .expect("approve old review")
        .expect("approved receipt");
    {
        let mut governance = state.automation_governance.write().await;
        let record = governance
            .records
            .get_mut(&automation.automation_id)
            .expect("governance record");
        record.review_required = true;
        record.review_kind = Some(
            crate::automation_v2::governance::AutomationLifecycleReviewKind::HealthDrift,
        );
        record.paused_for_lifecycle = true;
        record.review_request_id = Some("apr-newer-review".to_string());
    }
    state
        .decide_approval_request(
            &approval.approval_id,
            reviewer,
            true,
            Some("retry old receipt".to_string()),
            &tenant,
        )
        .await
        .expect("retry is idempotent")
        .expect("existing approved receipt");
    let record = state
        .get_automation_governance(&automation.automation_id)
        .await
        .expect("new review remains");
    assert!(record.review_required);
    assert!(record.paused_for_lifecycle);
    assert_eq!(
        record.review_kind,
        Some(crate::automation_v2::governance::AutomationLifecycleReviewKind::HealthDrift)
    );
    assert_eq!(record.review_request_id.as_deref(), Some("apr-newer-review"));
}

#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn lifecycle_pause_blocks_run_creation_outside_tenant_quarantine() {
    let state = test_state().await;
    let tenant = TenantContext::explicit(
        "org-lifecycle-pause",
        "workspace-lifecycle-pause",
        Some("owner".to_string()),
    );
    let automation = super::global::create_test_automation_v2_for_tenant(
        &state,
        "auto-governance-lifecycle-pause",
        &tenant,
    )
    .await;
    {
        let mut governance = state.automation_governance.write().await;
        let record = governance
            .records
            .get_mut(&automation.automation_id)
            .expect("governance record");
        record.review_required = true;
        record.review_kind = Some(
            crate::automation_v2::governance::AutomationLifecycleReviewKind::DependencyRevoked,
        );
        record.paused_for_lifecycle = true;
    }
    assert!(state
        .create_automation_v2_run(&automation, "scheduler")
        .await
        .is_err());
}

#[tokio::test]
async fn governance_bootstrap_migrates_unscoped_record_and_grant_tenant() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation_id = "auto-governance-tenant-migration";
    let tenant_context =
        TenantContext::explicit("org-migrate", "workspace-migrate", Some("owner".to_string()));

    let create = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "org-migrate")
        .header("x-tandem-workspace-id", "workspace-migrate")
        .header("x-tandem-actor-id", "owner")
        .body(Body::from(
            automation_v2_payload(automation_id, "agent-a", None).to_string(),
        ))
        .expect("automation create");
    assert_eq!(
        app.oneshot(create)
            .await
            .expect("automation create response")
            .status(),
        StatusCode::OK
    );
    state
        .grant_automation_modify_access(
            automation_id,
            crate::automation_v2::governance::GovernanceActorRef::agent(
                Some("grantee".to_string()),
                "test",
            ),
            crate::automation_v2::governance::GovernanceActorRef::human(
                Some("owner".to_string()),
                "test",
            ),
            None,
            &tenant_context,
        )
        .await
        .expect("seed scoped grant");
    {
        let mut governance = state.automation_governance.write().await;
        let record = governance
            .records
            .get_mut(automation_id)
            .expect("governance record");
        record.tenant_context = None;
        record.modify_grants[0].tenant_context = None;
    }

    assert_eq!(
        state
            .bootstrap_automation_governance()
            .await
            .expect("bootstrap migration"),
        1
    );
    let record = state
        .get_automation_governance(automation_id)
        .await
        .expect("migrated governance record");
    let owner = record.tenant_context.as_ref().expect("record tenant");
    assert_eq!(owner.org_id, "org-migrate");
    assert_eq!(owner.workspace_id, "workspace-migrate");
    let grant_owner = record.modify_grants[0]
        .tenant_context
        .as_ref()
        .expect("grant tenant");
    assert_eq!(grant_owner.org_id, "org-migrate");
    assert_eq!(grant_owner.workspace_id, "workspace-migrate");
}

#[tokio::test]
async fn governance_bootstrap_persists_quarantine_for_tenant_mismatch() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation_id = "auto-governance-tenant-quarantine";
    let tenant = TenantContext::explicit("org-owner", "workspace-owner", Some("owner".to_string()));
    let create = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "org-owner")
        .header("x-tandem-workspace-id", "workspace-owner")
        .header("x-tandem-actor-id", "owner")
        .body(Body::from(
            automation_v2_payload(automation_id, "agent-a", None).to_string(),
        ))
        .expect("automation create");
    assert_eq!(
        app.oneshot(create)
            .await
            .expect("automation create response")
            .status(),
        StatusCode::OK
    );
    state
        .grant_automation_modify_access(
            automation_id,
            crate::automation_v2::governance::GovernanceActorRef::agent(
                Some("grantee".to_string()),
                "test",
            ),
            crate::automation_v2::governance::GovernanceActorRef::human(
                Some("owner".to_string()),
                "test",
            ),
            None,
            &tenant,
        )
        .await
        .expect("seed grant");
    {
        let mut governance = state.automation_governance.write().await;
        let record = governance.records.get_mut(automation_id).expect("record");
        record.tenant_context = Some(TenantContext::explicit(
            "org-foreign",
            "workspace-foreign",
            None,
        ));
    }

    assert_eq!(
        state
            .bootstrap_automation_governance()
            .await
            .expect("bootstrap quarantine"),
        1
    );
    let record = state
        .get_automation_governance(automation_id)
        .await
        .expect("quarantined record");
    let owner = record.tenant_context.as_ref().expect("quarantine owner");
    assert_eq!(owner.org_id, "org-owner");
    assert_eq!(owner.workspace_id, "workspace-owner");
    assert!(record.creation_paused);
    assert!(record.paused_for_lifecycle);
    assert!(record.review_required);
    assert_eq!(
        record.review_kind,
        Some(
            crate::automation_v2::governance::AutomationLifecycleReviewKind::TenantOwnershipMismatch
        )
    );
    assert!(record.modify_grants.is_empty());
    assert!(record.capability_grants.is_empty());
    let quarantine_finding = record
        .health_findings
        .iter()
        .find(|finding| {
            finding.kind
                == crate::automation_v2::governance::AutomationLifecycleReviewKind::TenantOwnershipMismatch
        })
        .expect("tenant mismatch finding");
    assert_eq!(
        quarantine_finding
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.pointer("/foreignTenant/org_id"))
            .and_then(Value::as_str),
        Some("org-foreign")
    );
    assert!(state
        .get_automation_governance_for_tenant(automation_id, &tenant)
        .await
        .is_some());
    let automation = state
        .get_automation_v2(automation_id)
        .await
        .expect("quarantined automation");
    assert_eq!(automation.status, crate::AutomationV2Status::Paused);
    #[cfg(feature = "premium-governance")]
    assert!(state
        .create_automation_v2_run(&automation, "webhook")
        .await
        .is_err());
    #[cfg(feature = "premium-governance")]
    {
        let response = app_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/governance/reviews")
                    .header("x-tandem-org-id", "org-owner")
                    .header("x-tandem-workspace-id", "workspace-owner")
                    .header("x-tandem-actor-id", "owner")
                    .body(Body::empty())
                    .expect("quarantine review list request"),
            )
            .await
            .expect("quarantine review list response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(body["automation_lifecycle_reviews"]
            .as_array()
            .expect("lifecycle reviews")
            .iter()
            .any(|review| review["automation_id"] == automation_id));
    }
    assert!(state
        .can_mutate_automation(
            automation_id,
            &crate::automation_v2::governance::GovernanceActorRef::human(
                Some("owner".to_string()),
                "test",
            ),
            false,
            &tenant,
        )
        .await
        .is_err());

    {
        let mut governance = state.automation_governance.write().await;
        let record = governance.records.get_mut(automation_id).expect("record");
        record.creation_paused = false;
        record.paused_for_lifecycle = false;
        record.review_required = false;
    }
    state
        .load_automation_governance()
        .await
        .expect("reload persisted governance");
    let reloaded = state
        .get_automation_governance(automation_id)
        .await
        .expect("reloaded quarantine");
    assert!(reloaded.creation_paused && reloaded.paused_for_lifecycle && reloaded.review_required);
    let owner = reloaded
        .tenant_context
        .as_ref()
        .expect("reloaded quarantine owner");
    assert_eq!(owner.org_id, "org-owner");
    assert_eq!(owner.workspace_id, "workspace-owner");

    #[cfg(feature = "premium-governance")]
    {
        let approval = state
            .request_approval(
                crate::automation_v2::governance::GovernanceApprovalRequestType::LifecycleReview,
                crate::automation_v2::governance::GovernanceActorRef::system(
                    "tenant_ownership_quarantine",
                ),
                crate::automation_v2::governance::GovernanceResourceRef {
                    resource_type: "automation".to_string(),
                    id: automation_id.to_string(),
                },
                "independent tenant ownership review".to_string(),
                json!({"trigger": "tenant_ownership_mismatch"}),
                None,
                &tenant,
            )
            .await
            .expect("quarantine lifecycle approval");

        tokio::fs::remove_file(&state.protected_audit_path)
            .await
            .expect("remove protected audit ledger");
        tokio::fs::create_dir_all(&state.protected_audit_path)
            .await
            .expect("make protected audit path unwritable as a file");
        crate::audit::reset_protected_audit_tail_for_test(&state.protected_audit_path).await;
        let failed = state
            .decide_approval_request(
                &approval.approval_id,
                crate::automation_v2::governance::GovernanceActorRef::human(
                    Some("owner".to_string()),
                    "test",
                ),
                true,
                Some("audit must succeed".to_string()),
                &tenant,
            )
            .await;
        assert!(failed.is_err());
        assert_eq!(
            state
                .get_governance_approval_request_for_tenant(&approval.approval_id, &tenant)
                .await
                .expect("approval remains visible")
                .status,
            crate::automation_v2::governance::GovernanceApprovalStatus::Pending
        );
        let still_quarantined = state
            .get_automation_governance(automation_id)
            .await
            .expect("quarantine survives failed review audit");
        assert!(still_quarantined.review_required);
        assert_eq!(
            still_quarantined.review_kind,
            Some(
                crate::automation_v2::governance::AutomationLifecycleReviewKind::TenantOwnershipMismatch
            )
        );
        tokio::fs::remove_dir_all(&state.protected_audit_path)
            .await
            .expect("remove audit failure directory");
        crate::audit::reset_protected_audit_tail_for_test(&state.protected_audit_path).await;

        let reviewer_app =
            verified_governance_app(state.clone(), "org-owner", "workspace-owner", "owner");
        let approve = reviewer_app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/governance/approvals/{}/approve",
                        approval.approval_id
                    ))
                    .header("content-type", "application/json")
                    .header("x-tandem-org-id", "org-owner")
                    .header("x-tandem-workspace-id", "workspace-owner")
                    .header("x-tandem-actor-id", "owner")
                    .body(Body::from(json!({"notes": "ownership verified"}).to_string()))
                    .expect("approve quarantine review"),
            )
            .await
            .expect("approve quarantine response");
        assert_eq!(approve.status(), StatusCode::OK);
        let acknowledged = state
            .get_automation_governance(automation_id)
            .await
            .expect("acknowledged quarantine");
        assert!(!acknowledged.review_required);
        assert!(acknowledged.creation_paused && acknowledged.paused_for_lifecycle);
        assert_eq!(
            acknowledged.review_kind,
            Some(
                crate::automation_v2::governance::AutomationLifecycleReviewKind::TenantOwnershipMismatch
            )
        );
        assert!(state
            .create_automation_v2_run(
                &state
                    .get_automation_v2(automation_id)
                    .await
                    .expect("still paused automation"),
                "manual",
            )
            .await
            .is_err());

        let resume = reviewer_app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/automations/v2/{automation_id}/resume"))
                    .header("x-tandem-org-id", "org-owner")
                    .header("x-tandem-workspace-id", "workspace-owner")
                    .header("x-tandem-actor-id", "owner")
                    .body(Body::empty())
                    .expect("resume quarantined automation"),
            )
            .await
            .expect("resume quarantine response");
        assert_eq!(resume.status(), StatusCode::OK);
        let restored_record = state
            .get_automation_governance(automation_id)
            .await
            .expect("restored governance");
        assert!(!restored_record.creation_paused);
        assert!(!restored_record.paused_for_lifecycle);
        assert_eq!(restored_record.review_kind, None);
        let restored = state
            .get_automation_v2(automation_id)
            .await
            .expect("restored automation");
        assert_eq!(restored.status, crate::AutomationV2Status::Active);
        state
            .create_automation_v2_run(&restored, "manual")
            .await
            .expect("reviewed and explicitly resumed run");
    }
}

#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn governance_approval_scope_includes_deployment_id() {
    let state = test_state().await;
    let mut issuer = TenantContext::explicit("org-a", "workspace-a", Some("requester".to_string()));
    issuer.deployment_id = Some("deployment-a".to_string());
    let mut sibling_deployment = issuer.clone();
    sibling_deployment.deployment_id = Some("deployment-b".to_string());
    let approval = state
        .request_approval(
            crate::automation_v2::governance::GovernanceApprovalRequestType::CapabilityRequest,
            crate::automation_v2::governance::GovernanceActorRef::human(
                Some("requester".to_string()),
                "test",
            ),
            crate::automation_v2::governance::GovernanceResourceRef {
                resource_type: "agent".to_string(),
                id: "agent-a".to_string(),
            },
            "deployment isolation".to_string(),
            json!({}),
            None,
            &issuer,
        )
        .await
        .expect("create approval");
    assert!(state
        .get_governance_approval_request_for_tenant(&approval.approval_id, &sibling_deployment)
        .await
        .is_none());
    assert!(state
        .list_approval_requests_for_tenant(None, None, &sibling_deployment)
        .await
        .is_empty());
}

#[tokio::test]
async fn governance_grant_audit_failure_leaves_state_unchanged() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation_id = "auto-governance-audit-failure";

    let create = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-actor-id", "owner")
        .body(Body::from(
            automation_v2_payload(automation_id, "agent-a", None).to_string(),
        ))
        .expect("automation create");
    assert_eq!(
        app.oneshot(create)
            .await
            .expect("automation create response")
            .status(),
        StatusCode::OK
    );

    tokio::fs::remove_file(&state.protected_audit_path)
        .await
        .expect("remove protected audit ledger");
    tokio::fs::create_dir_all(&state.protected_audit_path)
        .await
        .expect("make protected audit path unwritable as a file");
    crate::audit::reset_protected_audit_tail_for_test(&state.protected_audit_path).await;

    let tenant_context = TenantContext::local_implicit();
    let result = state
        .grant_automation_modify_access(
            automation_id,
            crate::automation_v2::governance::GovernanceActorRef::agent(Some("grantee".to_string()), "test"),
            crate::automation_v2::governance::GovernanceActorRef::human(Some("owner".to_string()), "test"),
            Some("audit must precede mutation".to_string()),
            &tenant_context,
        )
        .await;
    assert!(result.is_err(), "grant must fail when its audit cannot persist");
    let governance = state
        .get_automation_governance(automation_id)
        .await
        .expect("governance record");
    assert!(
        governance.modify_grants.is_empty(),
        "failed audit must leave no visible modify grant"
    );
}

/// GOV-B10: in the OSS/local engine a human may freely mutate their own (or an
/// unowned/local) automation, but a record with a DISTINCT identified human owner
/// may only be mutated by that owner. Local single-user operation (no identified
/// owner / anonymous actor) is never blocked.
#[cfg(not(feature = "premium-governance"))]
#[tokio::test]
async fn oss_mutation_denied_for_distinct_identified_non_owner() {
    let state = test_state().await;
    let app = app_router(state.clone());

    // Alice (an identified human) creates an automation via the control panel.
    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-actor-id", "alice")
        .body(Body::from(
            automation_v2_payload("auto-v2-b10-owner", "agent-a", None).to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    // Bob (a different identified human) cannot share/mutate Alice's automation.
    let bob_share = Request::builder()
        .method("POST")
        .uri("/automations/v2/auto-v2-b10-owner/share")
        .header("content-type", "application/json")
        .header("x-tandem-actor-id", "bob")
        .body(Body::from(json!({ "visibility": "org" }).to_string()))
        .expect("bob share request");
    let bob_resp = app
        .clone()
        .oneshot(bob_share)
        .await
        .expect("bob share response");
    assert_eq!(bob_resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response_json(bob_resp)
            .await
            .get("code")
            .and_then(Value::as_str),
        Some("AUTOMATION_V2_NOT_OWNER")
    );

    // Alice (the owner) can mutate her own automation.
    let alice_share = Request::builder()
        .method("POST")
        .uri("/automations/v2/auto-v2-b10-owner/share")
        .header("content-type", "application/json")
        .header("x-tandem-actor-id", "alice")
        .body(Body::from(json!({ "visibility": "org" }).to_string()))
        .expect("alice share request");
    let alice_resp = app
        .clone()
        .oneshot(alice_share)
        .await
        .expect("alice share response");
    assert_eq!(alice_resp.status(), StatusCode::OK);
}

/// GOV-B10: a purely local single-user flow (no identified actor on either side)
/// is never blocked by the ownership check.
#[cfg(not(feature = "premium-governance"))]
#[tokio::test]
async fn oss_local_anonymous_mutation_is_allowed() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .body(Body::from(
            automation_v2_payload("auto-v2-b10-local", "agent-a", None).to_string(),
        ))
        .expect("create request");
    assert_eq!(
        app.clone()
            .oneshot(create_req)
            .await
            .expect("resp")
            .status(),
        StatusCode::OK
    );

    let share_req = Request::builder()
        .method("POST")
        .uri("/automations/v2/auto-v2-b10-local/share")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "visibility": "org" }).to_string()))
        .expect("share request");
    assert_eq!(
        app.clone().oneshot(share_req).await.expect("resp").status(),
        StatusCode::OK
    );
}

/// GOV-B6a: a run queued before its agent hit the weekly spend cap must not
/// launch (and burn more budget); it is held as `Paused + GuardrailStopped` and
/// resumes once a quota override is approved.
#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn spend_capped_agent_run_is_held_at_launch_and_resumes_after_override() {
    let state = test_state().await;
    {
        let mut guard = state.automation_governance.write().await;
        guard.limits.weekly_spend_cap_usd = Some(10.0);
    }
    let app = app_router(state.clone());
    let agent_id = "agent-b6a-spend";
    let automation_id = "auto-v2-b6a-spend";

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-request-source", "agent")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            automation_v2_payload(automation_id, agent_id, None).to_string(),
        ))
        .expect("create request");
    assert_eq!(
        app.clone()
            .oneshot(create_req)
            .await
            .expect("create resp")
            .status(),
        StatusCode::OK
    );

    let automation = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation");
    // Two runs queued BEFORE the cap trips.
    let _run_a = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run a");
    let run_b = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run b");

    // Run A's spend trips the weekly cap -> agent spend-paused + override approval pending.
    state
        .record_automation_v2_spend(&_run_a.run_id, 6_000, 6_000, 12_000, 12.0)
        .await
        .expect("record cap spend");
    assert!(state
        .agent_spend_summary(agent_id)
        .await
        .expect("agent spend summary")
        .paused_at_ms
        .is_some());

    // Run B is still queued; claiming it must HOLD it, not launch it.
    assert_eq!(
        state
            .get_automation_v2_run(&run_b.run_id)
            .await
            .expect("run b")
            .status,
        crate::AutomationRunStatus::Queued
    );
    let claimed = state.claim_specific_automation_v2_run(&run_b.run_id).await;
    assert!(claimed.is_none(), "spend-capped run must not launch");
    let held = state
        .get_automation_v2_run(&run_b.run_id)
        .await
        .expect("held run");
    assert_eq!(held.status, crate::AutomationRunStatus::Paused);
    assert_eq!(
        held.stop_kind,
        Some(crate::automation_v2::types::AutomationStopKind::GuardrailStopped)
    );

    // Approve the auto-created quota override.
    let approval = state
        .list_approval_requests(
            Some(crate::automation_v2::governance::GovernanceApprovalRequestType::QuotaOverride),
            Some(crate::automation_v2::governance::GovernanceApprovalStatus::Pending),
        )
        .await
        .into_iter()
        .find(|request| request.target_resource.id == agent_id)
        .expect("quota override approval");
    approve_quota_override_request(&app, &approval.approval_id).await;

    // The guardrail-override resume sweep re-queues the held run...
    state.auto_resume_stale_reaped_runs().await;
    assert_eq!(
        state
            .get_automation_v2_run(&run_b.run_id)
            .await
            .expect("requeued run")
            .status,
        crate::AutomationRunStatus::Queued
    );

    // ...and now it launches.
    let relaunched = state.claim_specific_automation_v2_run(&run_b.run_id).await;
    assert!(
        relaunched.is_some(),
        "run should launch after override approval"
    );
    assert_eq!(
        state
            .get_automation_v2_run(&run_b.run_id)
            .await
            .expect("running run")
            .status,
        crate::AutomationRunStatus::Running
    );
}

/// GOV-B6a: with no governance state (OSS/local), the launch recheck is a no-op
/// and a queued run launches normally.
#[cfg(not(feature = "premium-governance"))]
#[tokio::test]
async fn run_launches_normally_without_governance_state() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let agent_id = "agent-b6a-oss";
    let automation_id = "auto-v2-b6a-oss";

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-request-source", "control_panel")
        .body(Body::from(
            automation_v2_payload(automation_id, agent_id, None).to_string(),
        ))
        .expect("create request");
    assert_eq!(
        app.clone()
            .oneshot(create_req)
            .await
            .expect("create resp")
            .status(),
        StatusCode::OK
    );

    let automation = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    let claimed = state.claim_specific_automation_v2_run(&run.run_id).await;
    assert!(
        claimed.is_some(),
        "run should launch with no governance state"
    );
    assert_eq!(
        state
            .get_automation_v2_run(&run.run_id)
            .await
            .expect("run")
            .status,
        crate::AutomationRunStatus::Running
    );
}
