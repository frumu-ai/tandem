use super::*;

fn automation_v2_payload(
    automation_id: &str,
    agent_id: &str,
    capabilities: Option<Value>,
) -> Value {
    let mut payload = json!({
        "automation_id": automation_id,
        "name": format!("{} automation", automation_id),
        "status": "draft",
        "schedule": {
            "type": "manual",
            "timezone": "UTC",
            "misfire_policy": { "type": "skip" }
        },
        "agents": [
            {
                "agent_id": agent_id,
                "display_name": "Agent One",
                "skills": [],
                "tool_policy": { "allowlist": ["read"], "denylist": [] },
                "mcp_policy": { "allowed_servers": [] }
            }
        ],
        "flow": {
            "nodes": [
                {
                    "node_id": "node-1",
                    "agent_id": agent_id,
                    "objective": "Exercise the recursion gate",
                    "depends_on": []
                }
            ]
        },
        "execution": { "max_parallel_agents": 1 }
    });
    if let Some(capabilities) = capabilities {
        payload["capabilities"] = capabilities;
    }
    payload
}

fn automation_v2_payload_with_mcp_servers(
    automation_id: &str,
    agent_id: &str,
    servers: &[&str],
) -> Value {
    let mut payload = automation_v2_payload(automation_id, agent_id, None);
    payload["agents"][0]["mcp_policy"]["allowed_servers"] = json!(servers);
    payload
}

fn approval_request_payload(agent_id: &str, capability_key: &str) -> Value {
    json!({
        "request_type": "capability_request",
        "target_resource": { "type": "agent", "id": agent_id },
        "rationale": format!("approve {} for recursion-gate test", capability_key),
        "context": { "capability_key": capability_key }
    })
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&body).expect("response json")
}

async fn approve_capability_request(app: &axum::Router, agent_id: &str, capability_key: &str) {
    let create_req = Request::builder()
        .method("POST")
        .uri("/governance/approvals")
        .header("content-type", "application/json")
        .header("x-tandem-actor-id", "governance-operator")
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
    let create_payload = response_json(create_resp).await;
    let approval_id = create_payload
        .get("approval")
        .and_then(|value| value.get("approval_id"))
        .and_then(Value::as_str)
        .expect("approval id")
        .to_string();

    let approve_req = Request::builder()
        .method("POST")
        .uri(format!("/governance/approvals/{approval_id}/approve"))
        .header("content-type", "application/json")
        .header("x-tandem-actor-id", "governance-operator")
        .body(Body::from(
            json!({ "notes": "approved for test" }).to_string(),
        ))
        .expect("approval decision request");
    let approve_resp = (*app)
        .clone()
        .oneshot(approve_req)
        .await
        .expect("approval decision response");
    assert_eq!(approve_resp.status(), StatusCode::OK);
    let approve_payload = response_json(approve_resp).await;
    assert_eq!(
        approve_payload
            .get("approval")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str),
        Some("approved")
    );
}

async fn approve_quota_override_request(app: &axum::Router, approval_id: &str) {
    let approve_req = Request::builder()
        .method("POST")
        .uri(format!("/governance/approvals/{approval_id}/approve"))
        .header("content-type", "application/json")
        .header("x-tandem-actor-id", "governance-operator")
        .body(Body::from(
            json!({ "notes": "approved for spend-cap test" }).to_string(),
        ))
        .expect("quota override approval request");
    let approve_resp = (*app)
        .clone()
        .oneshot(approve_req)
        .await
        .expect("quota override approval response");
    assert_eq!(approve_resp.status(), StatusCode::OK);
    let approve_payload = response_json(approve_resp).await;
    assert_eq!(
        approve_payload
            .get("approval")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str),
        Some("approved")
    );
}

async fn approve_approval_request(app: &axum::Router, approval_id: &str, notes: &str) {
    let approve_req = Request::builder()
        .method("POST")
        .uri(format!("/governance/approvals/{approval_id}/approve"))
        .header("content-type", "application/json")
        .header("x-tandem-actor-id", "governance-operator")
        .body(Body::from(json!({ "notes": notes }).to_string()))
        .expect("approval decision request");
    let approve_resp = (*app)
        .clone()
        .oneshot(approve_req)
        .await
        .expect("approval decision response");
    assert_eq!(approve_resp.status(), StatusCode::OK);
    let approve_payload = response_json(approve_resp).await;
    assert_eq!(
        approve_payload
            .get("approval")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str),
        Some("approved")
    );
}

async fn pending_lifecycle_approval_id(
    state: &AppState,
    resource_type: &str,
    resource_id: &str,
) -> String {
    state
        .list_approval_requests(
            Some(crate::automation_v2::governance::GovernanceApprovalRequestType::LifecycleReview),
            Some(crate::automation_v2::governance::GovernanceApprovalStatus::Pending),
        )
        .await
        .into_iter()
        .find(|request| {
            request.target_resource.resource_type == resource_type
                && request.target_resource.id == resource_id
        })
        .expect("pending lifecycle approval")
        .approval_id
}

#[tokio::test]
async fn automations_v2_create_rejects_lineage_depth_over_limit() {
    let state = test_state().await;
    let app = app_router(state);

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", "agent-depth-test")
        .header(
            "x-tandem-agent-ancestor-ids",
            "agent-parent-1,agent-parent-2,agent-parent-3",
        )
        .body(Body::from(
            automation_v2_payload("auto-v2-depth-test", "agent-depth-test", None).to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::FORBIDDEN);
    let create_payload = response_json(create_resp).await;
    assert_eq!(
        create_payload.get("code").and_then(Value::as_str),
        Some("AUTOMATION_V2_LINEAGE_DEPTH_EXCEEDED")
    );
}

#[tokio::test]
async fn automations_v2_create_requires_approved_capability_request() {
    let state = test_state().await;
    let app = app_router(state);
    let agent_id = "agent-recursion-create";

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            automation_v2_payload(
                "auto-v2-recursion-create",
                agent_id,
                Some(json!({ "creates_agents": true, "modifies_grants": false })),
            )
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::FORBIDDEN);
    let create_payload = response_json(create_resp).await;
    assert_eq!(
        create_payload.get("code").and_then(Value::as_str),
        Some("AUTOMATION_V2_CAPABILITY_ESCALATION_FORBIDDEN")
    );

    approve_capability_request(&app, agent_id, "creates_agents").await;

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            automation_v2_payload(
                "auto-v2-recursion-create",
                agent_id,
                Some(json!({ "creates_agents": true, "modifies_grants": false })),
            )
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_payload = response_json(create_resp).await;
    assert_eq!(
        create_payload
            .get("automation")
            .and_then(|value| value.get("creator_id"))
            .and_then(Value::as_str),
        Some(agent_id)
    );
    assert_eq!(
        create_payload
            .get("automation")
            .and_then(|value| value.get("metadata"))
            .and_then(|value| value.get("capabilities"))
            .and_then(|value| value.get("creates_agents"))
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn automations_v2_patch_requires_approved_capability_request() {
    let state = test_state().await;
    let app = app_router(state);
    let agent_id = "agent-recursion-patch";

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            automation_v2_payload("auto-v2-recursion-patch", agent_id, None).to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let patch_req = Request::builder()
        .method("PATCH")
        .uri("/automations/v2/auto-v2-recursion-patch")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            json!({
                "capabilities": { "creates_agents": true, "modifies_grants": false }
            })
            .to_string(),
        ))
        .expect("patch request");
    let patch_resp = app
        .clone()
        .oneshot(patch_req)
        .await
        .expect("patch response");
    assert_eq!(patch_resp.status(), StatusCode::FORBIDDEN);
    let patch_payload = response_json(patch_resp).await;
    assert_eq!(
        patch_payload.get("code").and_then(Value::as_str),
        Some("AUTOMATION_V2_CAPABILITY_ESCALATION_FORBIDDEN")
    );

    approve_capability_request(&app, agent_id, "creates_agents").await;

    let patch_req = Request::builder()
        .method("PATCH")
        .uri("/automations/v2/auto-v2-recursion-patch")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            json!({
                "capabilities": { "creates_agents": true, "modifies_grants": false }
            })
            .to_string(),
        ))
        .expect("patch request");
    let patch_resp = app
        .clone()
        .oneshot(patch_req)
        .await
        .expect("patch response");
    assert_eq!(patch_resp.status(), StatusCode::OK);
    let patch_payload = response_json(patch_resp).await;
    assert_eq!(
        patch_payload
            .get("automation")
            .and_then(|value| value.get("metadata"))
            .and_then(|value| value.get("capabilities"))
            .and_then(|value| value.get("creates_agents"))
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn automations_v2_spend_caps_pause_and_resume_after_quota_override() {
    let state = test_state().await;
    {
        let mut guard = state.automation_governance.write().await;
        guard.limits.weekly_spend_cap_usd = Some(10.0);
        guard.limits.spend_warning_threshold_ratio = 0.8;
    }
    let app = app_router(state.clone());
    let agent_id = "agent-spend-test";
    let automation_id = "auto-v2-spend-test";

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            automation_v2_payload(automation_id, agent_id, None).to_string(),
        ))
        .expect("spend create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("spend create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let automation = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");

    state
        .record_automation_v2_spend(&run.run_id, 4_000, 1_000, 5_000, 8.25)
        .await
        .expect("record warning spend");

    let spend_req = Request::builder()
        .method("GET")
        .uri(format!("/governance/agents/{agent_id}/spend"))
        .body(Body::empty())
        .expect("spend lookup request");
    let spend_resp = app
        .clone()
        .oneshot(spend_req)
        .await
        .expect("spend lookup response");
    assert_eq!(spend_resp.status(), StatusCode::OK);
    let spend_payload = response_json(spend_resp).await;
    assert_eq!(
        spend_payload
            .get("spend")
            .and_then(|value| value.get("agent_id"))
            .and_then(Value::as_str),
        Some(agent_id)
    );
    assert_eq!(
        spend_payload
            .get("spend")
            .and_then(|value| value.get("weekly"))
            .and_then(|value| value.get("cost_usd"))
            .and_then(Value::as_f64),
        Some(8.25)
    );
    assert!(spend_payload
        .get("spend")
        .and_then(|value| value.get("weekly"))
        .and_then(|value| value.get("soft_warning_at_ms"))
        .and_then(Value::as_u64)
        .is_some());

    let spend_list_req = Request::builder()
        .method("GET")
        .uri("/governance/spend")
        .body(Body::empty())
        .expect("spend list request");
    let spend_list_resp = app
        .clone()
        .oneshot(spend_list_req)
        .await
        .expect("spend list response");
    assert_eq!(spend_list_resp.status(), StatusCode::OK);
    let spend_list_payload = response_json(spend_list_resp).await;
    assert!(
        spend_list_payload
            .get("count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 1
    );

    state
        .record_automation_v2_spend(&run.run_id, 1_000, 0, 1_000, 2.0)
        .await
        .expect("record cap spend");

    let updated_run = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("paused run");
    assert_eq!(updated_run.status, crate::AutomationRunStatus::Paused);
    assert!(updated_run
        .pause_reason
        .as_deref()
        .unwrap_or_default()
        .contains("weekly spend cap exceeded"));

    let agent_spend = state
        .agent_spend_summary(agent_id)
        .await
        .expect("agent spend summary");
    assert!(agent_spend.paused_at_ms.is_some());
    assert!(agent_spend.weekly.hard_stop_at_ms.is_some());

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            automation_v2_payload("auto-v2-spend-test-2", agent_id, None).to_string(),
        ))
        .expect("spend blocked create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("spend blocked create response");
    assert_eq!(create_resp.status(), StatusCode::TOO_MANY_REQUESTS);
    let create_payload = response_json(create_resp).await;
    assert_eq!(
        create_payload.get("code").and_then(Value::as_str),
        Some("AUTOMATION_V2_AGENT_SPEND_CAP_EXCEEDED")
    );

    let approvals = state
        .list_approval_requests(
            Some(crate::automation_v2::governance::GovernanceApprovalRequestType::QuotaOverride),
            Some(crate::automation_v2::governance::GovernanceApprovalStatus::Pending),
        )
        .await;
    let approval = approvals
        .into_iter()
        .find(|request| request.target_resource.id == agent_id)
        .expect("quota override approval");
    assert_eq!(
        approval.request_type,
        crate::automation_v2::governance::GovernanceApprovalRequestType::QuotaOverride
    );

    approve_quota_override_request(&app, &approval.approval_id).await;

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            automation_v2_payload("auto-v2-spend-test-3", agent_id, None).to_string(),
        ))
        .expect("spend override create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("spend override create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn automations_v2_creation_review_threshold_blocks_and_can_be_acknowledged() {
    let state = test_state().await;
    {
        let mut guard = state.automation_governance.write().await;
        guard.limits.per_agent_creation_review_threshold = 2;
        guard.limits.per_agent_daily_creation_limit = 100;
        guard.limits.active_agent_automation_cap = 100;
    }
    let app = app_router(state.clone());
    let agent_id = "agent-creation-review";

    for index in 1..=2 {
        let automation_id = format!("auto-v2-creation-review-{index}");
        let create_req = Request::builder()
            .method("POST")
            .uri("/automations/v2")
            .header("content-type", "application/json")
            .header("x-tandem-agent-id", agent_id)
            .body(Body::from(
                automation_v2_payload(&automation_id, agent_id, None).to_string(),
            ))
            .expect("creation review create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("creation review create response");
        assert_eq!(create_resp.status(), StatusCode::OK);
    }

    let reviews_req = Request::builder()
        .method("GET")
        .uri("/governance/reviews")
        .body(Body::empty())
        .expect("reviews request");
    let reviews_resp = app
        .clone()
        .oneshot(reviews_req)
        .await
        .expect("reviews response");
    assert_eq!(reviews_resp.status(), StatusCode::OK);
    let reviews_payload = response_json(reviews_resp).await;
    assert_eq!(
        reviews_payload
            .get("agent_creation_reviews")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        reviews_payload
            .get("pending_approvals")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );

    let blocked_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            automation_v2_payload("auto-v2-creation-review-3", agent_id, None).to_string(),
        ))
        .expect("blocked creation review request");
    let blocked_resp = app
        .clone()
        .oneshot(blocked_req)
        .await
        .expect("blocked creation review response");
    assert_eq!(blocked_resp.status(), StatusCode::TOO_MANY_REQUESTS);
    let blocked_payload = response_json(blocked_resp).await;
    assert_eq!(
        blocked_payload.get("code").and_then(Value::as_str),
        Some("AUTOMATION_V2_AGENT_REVIEW_REQUIRED")
    );

    let approval_id = pending_lifecycle_approval_id(&state, "agent", agent_id).await;
    approve_approval_request(&app, &approval_id, "approved for creation-review test").await;

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            automation_v2_payload("auto-v2-creation-review-3", agent_id, None).to_string(),
        ))
        .expect("unblocked creation review request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("unblocked creation review response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let reviews_req = Request::builder()
        .method("GET")
        .uri("/governance/reviews")
        .body(Body::empty())
        .expect("reviews request after approval");
    let reviews_resp = app
        .clone()
        .oneshot(reviews_req)
        .await
        .expect("reviews response after approval");
    assert_eq!(reviews_resp.status(), StatusCode::OK);
    let reviews_payload = response_json(reviews_resp).await;
    assert_eq!(
        reviews_payload
            .get("agent_creation_reviews")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
}

#[tokio::test]
async fn automations_v2_run_drift_creates_lifecycle_review_and_clears_on_acknowledgement() {
    let state = test_state().await;
    {
        let mut guard = state.automation_governance.write().await;
        guard.limits.run_review_threshold = 1;
    }
    let app = app_router(state.clone());
    let agent_id = "agent-run-drift";
    let automation_id = "auto-v2-run-drift";

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            automation_v2_payload(automation_id, agent_id, None).to_string(),
        ))
        .expect("run drift create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("run drift create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let automation = state
        .get_automation_v2(automation_id)
        .await
        .expect("created automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Completed;
            row.detail = Some("completed for run-drift review".to_string());
        })
        .await
        .expect("complete run");

    let governance_req = Request::builder()
        .method("GET")
        .uri(format!("/automations/v2/{automation_id}/governance"))
        .body(Body::empty())
        .expect("governance request");
    let governance_resp = app
        .clone()
        .oneshot(governance_req)
        .await
        .expect("governance response");
    assert_eq!(governance_resp.status(), StatusCode::OK);
    let governance_payload = response_json(governance_resp).await;
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("review_required"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("review_kind"))
            .and_then(Value::as_str),
        Some("run_drift")
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("runs_since_review"))
            .and_then(Value::as_u64),
        Some(1)
    );

    let reviews_req = Request::builder()
        .method("GET")
        .uri("/governance/reviews")
        .body(Body::empty())
        .expect("reviews request");
    let reviews_resp = app
        .clone()
        .oneshot(reviews_req)
        .await
        .expect("reviews response");
    assert_eq!(reviews_resp.status(), StatusCode::OK);
    let reviews_payload = response_json(reviews_resp).await;
    assert_eq!(
        reviews_payload
            .get("automation_lifecycle_reviews")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        reviews_payload
            .get("pending_approvals")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );

    let approval_id = pending_lifecycle_approval_id(&state, "automation", automation_id).await;
    approve_approval_request(&app, &approval_id, "approved for run-drift test").await;

    let governance_req = Request::builder()
        .method("GET")
        .uri(format!("/automations/v2/{automation_id}/governance"))
        .body(Body::empty())
        .expect("governance request after approval");
    let governance_resp = app
        .clone()
        .oneshot(governance_req)
        .await
        .expect("governance response after approval");
    assert_eq!(governance_resp.status(), StatusCode::OK);
    let governance_payload = response_json(governance_resp).await;
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("review_required"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("runs_since_review"))
            .and_then(Value::as_u64),
        Some(0)
    );
}

#[tokio::test]
async fn automations_v2_health_check_pauses_expired_automation_and_requests_retirement_review() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let agent_id = "agent-expired-health";
    let automation_id = "auto-v2-expired-health";

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            automation_v2_payload(automation_id, agent_id, None).to_string(),
        ))
        .expect("expired health create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("expired health create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    {
        let mut guard = state.automation_governance.write().await;
        let record = guard
            .records
            .get_mut(automation_id)
            .expect("governance record");
        record.expires_at_ms = Some(crate::now_ms().saturating_sub(1));
        record.review_required = false;
        record.review_kind = None;
        record.expired_at_ms = None;
        record.paused_for_lifecycle = false;
    }

    let finding_count = state
        .run_automation_governance_health_check()
        .await
        .expect("health check");
    assert!(finding_count >= 1);

    let paused = state
        .get_automation_v2(automation_id)
        .await
        .expect("paused automation");
    assert_eq!(paused.status, crate::AutomationV2Status::Paused);

    let governance = state
        .get_automation_governance(automation_id)
        .await
        .expect("expired governance");
    assert!(governance.expired_at_ms.is_some());
    assert!(governance.paused_for_lifecycle);
    assert_eq!(
        governance.review_kind,
        Some(crate::automation_v2::governance::AutomationLifecycleReviewKind::Expired)
    );

    let reviews_req = Request::builder()
        .method("GET")
        .uri("/governance/reviews")
        .body(Body::empty())
        .expect("reviews request");
    let reviews_resp = app
        .clone()
        .oneshot(reviews_req)
        .await
        .expect("reviews response");
    assert_eq!(reviews_resp.status(), StatusCode::OK);
    let reviews_payload = response_json(reviews_resp).await;
    assert!(reviews_payload
        .get("automation_lifecycle_reviews")
        .and_then(Value::as_array)
        .is_some_and(|rows| {
            rows.iter()
                .any(|row| row.get("automation_id").and_then(Value::as_str) == Some(automation_id))
        }));
    assert!(reviews_payload
        .get("pending_approvals")
        .and_then(Value::as_array)
        .is_some_and(|rows| {
            rows.iter().any(|row| {
                row.get("request_type").and_then(Value::as_str) == Some("retirement_action")
                    && row
                        .get("target_resource")
                        .and_then(|value| value.get("id"))
                        .and_then(Value::as_str)
                        == Some(automation_id)
            })
        }));

    let future_expires_at_ms = crate::now_ms().saturating_add(24 * 60 * 60 * 1000);
    let extend_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/v2/{automation_id}/extend"))
        .header("content-type", "application/json")
        .header("x-tandem-actor-id", "governance-operator")
        .body(Body::from(
            json!({
                "expires_at_ms": future_expires_at_ms,
                "reason": "restore after expiration test"
            })
            .to_string(),
        ))
        .expect("extend request");
    let extend_resp = app
        .clone()
        .oneshot(extend_req)
        .await
        .expect("extend response");
    assert_eq!(extend_resp.status(), StatusCode::OK);
    let extend_payload = response_json(extend_resp).await;
    assert_eq!(
        extend_payload
            .get("automation")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str),
        Some("active")
    );

    let governance_req = Request::builder()
        .method("GET")
        .uri(format!("/automations/v2/{automation_id}/governance"))
        .body(Body::empty())
        .expect("governance request after extend");
    let governance_resp = app
        .clone()
        .oneshot(governance_req)
        .await
        .expect("governance response after extend");
    assert_eq!(governance_resp.status(), StatusCode::OK);
    let governance_payload = response_json(governance_resp).await;
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("paused_for_lifecycle"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("expired_at_ms"))
            .and_then(Value::as_u64),
        None
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("expires_at_ms"))
            .and_then(Value::as_u64),
        Some(future_expires_at_ms)
    );
}

#[tokio::test]
async fn automations_v2_retire_and_extend_round_trip_lifecycle_state() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let agent_id = "agent-retire-extend";
    let automation_id = "auto-v2-retire-extend";

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            automation_v2_payload(automation_id, agent_id, None).to_string(),
        ))
        .expect("retire create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("retire create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let retire_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/v2/{automation_id}/retire"))
        .header("content-type", "application/json")
        .header("x-tandem-actor-id", "governance-operator")
        .body(Body::from(
            json!({ "reason": "manual retirement test" }).to_string(),
        ))
        .expect("retire request");
    let retire_resp = app
        .clone()
        .oneshot(retire_req)
        .await
        .expect("retire response");
    assert_eq!(retire_resp.status(), StatusCode::OK);
    let retire_payload = response_json(retire_resp).await;
    assert_eq!(
        retire_payload
            .get("automation")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str),
        Some("paused")
    );

    let governance_req = Request::builder()
        .method("GET")
        .uri(format!("/automations/v2/{automation_id}/governance"))
        .body(Body::empty())
        .expect("governance request after retire");
    let governance_resp = app
        .clone()
        .oneshot(governance_req)
        .await
        .expect("governance response after retire");
    assert_eq!(governance_resp.status(), StatusCode::OK);
    let governance_payload = response_json(governance_resp).await;
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("retired_at_ms"))
            .and_then(Value::as_u64)
            .is_some(),
        true
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("paused_for_lifecycle"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let reviews_req = Request::builder()
        .method("GET")
        .uri("/governance/reviews")
        .body(Body::empty())
        .expect("reviews request after retire");
    let reviews_resp = app
        .clone()
        .oneshot(reviews_req)
        .await
        .expect("reviews response after retire");
    assert_eq!(reviews_resp.status(), StatusCode::OK);
    let reviews_payload = response_json(reviews_resp).await;
    assert!(reviews_payload
        .get("automation_lifecycle_reviews")
        .and_then(Value::as_array)
        .is_some_and(|rows| {
            rows.iter()
                .any(|row| row.get("automation_id").and_then(Value::as_str) == Some(automation_id))
        }));

    let future_expires_at_ms = crate::now_ms().saturating_add(48 * 60 * 60 * 1000);
    let extend_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/v2/{automation_id}/extend"))
        .header("content-type", "application/json")
        .header("x-tandem-actor-id", "governance-operator")
        .body(Body::from(
            json!({
                "expires_at_ms": future_expires_at_ms,
                "reason": "reactivate after retirement test"
            })
            .to_string(),
        ))
        .expect("extend request");
    let extend_resp = app
        .clone()
        .oneshot(extend_req)
        .await
        .expect("extend response");
    assert_eq!(extend_resp.status(), StatusCode::OK);
    let extend_payload = response_json(extend_resp).await;
    assert_eq!(
        extend_payload
            .get("automation")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str),
        Some("active")
    );

    let governance_req = Request::builder()
        .method("GET")
        .uri(format!("/automations/v2/{automation_id}/governance"))
        .body(Body::empty())
        .expect("governance request after extend");
    let governance_resp = app
        .clone()
        .oneshot(governance_req)
        .await
        .expect("governance response after extend");
    assert_eq!(governance_resp.status(), StatusCode::OK);
    let governance_payload = response_json(governance_resp).await;
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("retired_at_ms"))
            .and_then(Value::as_u64),
        None
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("paused_for_lifecycle"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("expires_at_ms"))
            .and_then(Value::as_u64),
        Some(future_expires_at_ms)
    );
}

#[tokio::test]
async fn automations_v2_grant_revoke_pauses_automation_and_requests_dependency_review() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation_id = "auto-v2-dependency-grant";
    let agent_id = "agent-dependency-grant";

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-actor-id", "governance-operator")
        .body(Body::from(
            automation_v2_payload(automation_id, agent_id, None).to_string(),
        ))
        .expect("dependency grant create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("dependency grant create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let grant_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/v2/{automation_id}/grants"))
        .header("content-type", "application/json")
        .header("x-tandem-actor-id", "governance-operator")
        .body(Body::from(
            json!({
                "granted_to_agent_id": agent_id,
                "reason": "grant dependency management"
            })
            .to_string(),
        ))
        .expect("grant create request");
    let grant_resp = app
        .clone()
        .oneshot(grant_req)
        .await
        .expect("grant create response");
    assert_eq!(grant_resp.status(), StatusCode::OK);
    let grant_payload = response_json(grant_resp).await;
    let grant_id = grant_payload
        .get("grant")
        .and_then(|value| value.get("grant_id"))
        .and_then(Value::as_str)
        .expect("grant id")
        .to_string();

    let automation = state
        .get_automation_v2(automation_id)
        .await
        .expect("dependency grant automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("dependency grant run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Running;
            row.pause_reason = None;
        })
        .await
        .expect("dependency grant running");

    let revoke_req = Request::builder()
        .method("DELETE")
        .uri(format!("/automations/v2/{automation_id}/grants/{grant_id}"))
        .header("content-type", "application/json")
        .header("x-tandem-actor-id", "governance-operator")
        .body(Body::from(
            json!({ "reason": "dependency removed" }).to_string(),
        ))
        .expect("grant revoke request");
    let revoke_resp = app
        .clone()
        .oneshot(revoke_req)
        .await
        .expect("grant revoke response");
    assert_eq!(revoke_resp.status(), StatusCode::OK);

    let governance_req = Request::builder()
        .method("GET")
        .uri(format!("/automations/v2/{automation_id}/governance"))
        .body(Body::empty())
        .expect("dependency grant governance request");
    let governance_resp = app
        .clone()
        .oneshot(governance_req)
        .await
        .expect("dependency grant governance response");
    assert_eq!(governance_resp.status(), StatusCode::OK);
    let governance_payload = response_json(governance_resp).await;
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("paused_for_lifecycle"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("review_required"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("review_kind"))
            .and_then(Value::as_str),
        Some("dependency_revoked")
    );

    let paused_run = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("paused dependency grant run");
    assert_eq!(paused_run.status, crate::AutomationRunStatus::Paused);
    assert!(paused_run
        .pause_reason
        .as_deref()
        .unwrap_or_default()
        .contains("dependency removed"));

    let approvals = state
        .list_approval_requests(
            Some(crate::automation_v2::governance::GovernanceApprovalRequestType::LifecycleReview),
            Some(crate::automation_v2::governance::GovernanceApprovalStatus::Pending),
        )
        .await;
    let approval = approvals
        .into_iter()
        .find(|request| {
            request.target_resource.resource_type == "automation"
                && request.target_resource.id == automation_id
        })
        .expect("dependency revoke approval");
    assert_eq!(
        approval.context.get("trigger").and_then(Value::as_str),
        Some("dependency_revoked")
    );

    approve_approval_request(
        &app,
        &approval.approval_id,
        "approved for dependency-revocation test",
    )
    .await;

    let governance_req = Request::builder()
        .method("GET")
        .uri(format!("/automations/v2/{automation_id}/governance"))
        .body(Body::empty())
        .expect("dependency grant governance request after approval");
    let governance_resp = app
        .clone()
        .oneshot(governance_req)
        .await
        .expect("dependency grant governance response after approval");
    assert_eq!(governance_resp.status(), StatusCode::OK);
    let governance_payload = response_json(governance_resp).await;
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("review_required"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("review_kind"))
            .and_then(Value::as_str),
        None
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("paused_for_lifecycle"))
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn automations_v2_mcp_policy_narrowing_pauses_automation_and_requests_dependency_review() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation_id = "auto-v2-mcp-dependency";
    let agent_id = "agent-mcp-dependency";

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            automation_v2_payload_with_mcp_servers(automation_id, agent_id, &["notion", "slack"])
                .to_string(),
        ))
        .expect("mcp dependency create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("mcp dependency create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let automation = state
        .get_automation_v2(automation_id)
        .await
        .expect("mcp dependency automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("mcp dependency run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Running;
            row.pause_reason = None;
        })
        .await
        .expect("mcp dependency running");

    let patch_req = Request::builder()
        .method("PATCH")
        .uri(format!("/automations/v2/{automation_id}"))
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            json!({
                "agents": [
                    {
                        "agent_id": agent_id,
                        "display_name": "Agent One",
                        "skills": [],
                        "tool_policy": { "allowlist": ["read"], "denylist": [] },
                        "mcp_policy": { "allowed_servers": ["notion"] }
                    }
                ]
            })
            .to_string(),
        ))
        .expect("mcp dependency patch request");
    let patch_resp = app
        .clone()
        .oneshot(patch_req)
        .await
        .expect("mcp dependency patch response");
    assert_eq!(patch_resp.status(), StatusCode::OK);

    let governance_req = Request::builder()
        .method("GET")
        .uri(format!("/automations/v2/{automation_id}/governance"))
        .body(Body::empty())
        .expect("mcp dependency governance request");
    let governance_resp = app
        .clone()
        .oneshot(governance_req)
        .await
        .expect("mcp dependency governance response");
    assert_eq!(governance_resp.status(), StatusCode::OK);
    let governance_payload = response_json(governance_resp).await;
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("paused_for_lifecycle"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("review_required"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("review_kind"))
            .and_then(Value::as_str),
        Some("dependency_revoked")
    );

    let paused_run = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("paused mcp dependency run");
    assert_eq!(paused_run.status, crate::AutomationRunStatus::Paused);
    assert!(paused_run
        .pause_reason
        .as_deref()
        .unwrap_or_default()
        .contains("mcp"));

    let approvals = state
        .list_approval_requests(
            Some(crate::automation_v2::governance::GovernanceApprovalRequestType::LifecycleReview),
            Some(crate::automation_v2::governance::GovernanceApprovalStatus::Pending),
        )
        .await;
    let approval = approvals
        .into_iter()
        .find(|request| {
            request.target_resource.resource_type == "automation"
                && request.target_resource.id == automation_id
        })
        .expect("mcp dependency approval");
    assert_eq!(
        approval.context.get("trigger").and_then(Value::as_str),
        Some("dependency_revoked")
    );

    approve_approval_request(
        &app,
        &approval.approval_id,
        "approved for mcp-dependency test",
    )
    .await;

    let governance_req = Request::builder()
        .method("GET")
        .uri(format!("/automations/v2/{automation_id}/governance"))
        .body(Body::empty())
        .expect("mcp dependency governance request after approval");
    let governance_resp = app
        .clone()
        .oneshot(governance_req)
        .await
        .expect("mcp dependency governance response after approval");
    assert_eq!(governance_resp.status(), StatusCode::OK);
    let governance_payload = response_json(governance_resp).await;
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("review_required"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("review_kind"))
            .and_then(Value::as_str),
        None
    );
    assert_eq!(
        governance_payload
            .get("lifecycle")
            .and_then(|value| value.get("paused_for_lifecycle"))
            .and_then(Value::as_bool),
        Some(true)
    );
}
