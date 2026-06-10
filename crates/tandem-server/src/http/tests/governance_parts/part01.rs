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
/// CT-09: an approval receipt issued in tenant A must not be approved (replayed)
/// from tenant B. The cross-tenant caller sees the same 404 as a missing receipt so
/// existence is not leaked, while the owning tenant can still approve its own receipt.
#[tokio::test]
async fn governance_approval_rejects_cross_tenant_approve_replay() {
    let state = test_state().await;
    let app = app_router(state);
    let approval_id = create_capability_approval(
        &app,
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
    let resp = app.clone().oneshot(replay).await.expect("replay response");
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
        .header("x-tandem-actor-id", "operator-a")
        .body(Body::from(json!({ "notes": "owner approves" }).to_string()))
        .expect("owner request");
    let resp = app.clone().oneshot(owner).await.expect("owner response");
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
    let app = app_router(state);
    let approval_id = create_capability_approval(
        &app,
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
    let resp = app.clone().oneshot(replay).await.expect("deny response");
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
        .header("x-tandem-actor-id", "operator-a")
        .body(Body::from(json!({ "notes": "owner denies" }).to_string()))
        .expect("owner deny request");
    let resp = app
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
    let app = app_router(state);
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

    let list_for = |org: &str, workspace: &str| {
        Request::builder()
            .method("GET")
            .uri("/governance/approvals")
            .header("x-tandem-org-id", org)
            .header("x-tandem-workspace-id", workspace)
            .header("x-tandem-actor-id", "operator")
            .body(Body::empty())
            .expect("list request")
    };

    // Tenant B must not see tenant A's approval.
    let resp = app
        .clone()
        .oneshot(list_for("org-b", "workspace-b"))
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

    // Tenant A still sees its own approval.
    let resp = app
        .clone()
        .oneshot(list_for("org-a", "workspace-a"))
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
