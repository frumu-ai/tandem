use super::*;

fn tenant_agent_create_request(
    automation_id: &str,
    org_id: &str,
    workspace_id: &str,
    actor_id: &str,
    agent_id: &str,
) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", org_id)
        .header("x-tandem-workspace-id", workspace_id)
        .header("x-tandem-actor-id", actor_id)
        .header("x-tandem-request-source", "agent")
        .header("x-tandem-agent-id", agent_id)
        .body(Body::from(
            json!({
                "automation_id": automation_id,
                "name": format!("{automation_id} automation"),
                "status": "draft",
                "schedule": {
                    "type": "manual",
                    "timezone": "UTC",
                    "misfire_policy": { "type": "skip" }
                },
                "agents": [{
                    "agent_id": agent_id,
                    "display_name": "Agent Shared",
                    "skills": [],
                    "tool_policy": { "allowlist": ["read"], "denylist": [] },
                    "mcp_policy": { "allowed_servers": [] }
                }],
                "flow": {
                    "nodes": [{
                        "node_id": "node-1",
                        "agent_id": agent_id,
                        "objective": "Exercise tenant-scoped spend isolation",
                        "depends_on": []
                    }]
                },
                "execution": { "max_parallel_agents": 1 }
            })
            .to_string(),
        ))
        .expect("tenant agent create request")
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&body).expect("response json")
}

#[tokio::test]
async fn tenant_spend_pause_does_not_cross_to_same_named_agent() {
    let state = test_state().await;
    {
        let mut guard = state.automation_governance.write().await;
        guard.limits.weekly_spend_cap_usd = Some(10.0);
        guard.limits.spend_warning_threshold_ratio = 0.8;
    }
    let app = app_router(state.clone());
    let agent_id = "agent-shared";

    let tenant_a_create = app
        .clone()
        .oneshot(tenant_agent_create_request(
            "tenant-a-spend-paused",
            "org-a",
            "workspace-a",
            "user-a",
            agent_id,
        ))
        .await
        .expect("tenant a create response");
    assert_eq!(tenant_a_create.status(), StatusCode::OK);
    let automation_a = state
        .get_automation_v2("tenant-a-spend-paused")
        .await
        .expect("tenant a automation");
    let tenant_a = automation_a.tenant_context();
    let run_a = state
        .create_automation_v2_run(&automation_a, "manual")
        .await
        .expect("tenant a run");

    state
        .record_automation_v2_spend(&run_a.run_id, 6_000, 6_000, 12_000, 12.0)
        .await
        .expect("tenant a cap spend");
    assert!(state
        .tenant_agent_spend_summary(&tenant_a, agent_id)
        .await
        .expect("tenant a spend summary")
        .paused_at_ms
        .is_some());

    let tenant_a_retry = app
        .clone()
        .oneshot(tenant_agent_create_request(
            "tenant-a-spend-paused-retry",
            "org-a",
            "workspace-a",
            "user-a",
            agent_id,
        ))
        .await
        .expect("tenant a retry response");
    assert_eq!(tenant_a_retry.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_payload = response_json(tenant_a_retry).await;
    assert_eq!(
        retry_payload.get("code").and_then(Value::as_str),
        Some("AUTOMATION_V2_AGENT_SPEND_CAP_EXCEEDED")
    );

    let tenant_b_create = app
        .clone()
        .oneshot(tenant_agent_create_request(
            "tenant-b-same-agent",
            "org-b",
            "workspace-b",
            "user-b",
            agent_id,
        ))
        .await
        .expect("tenant b create response");
    assert_eq!(tenant_b_create.status(), StatusCode::OK);
    let automation_b = state
        .get_automation_v2("tenant-b-same-agent")
        .await
        .expect("tenant b automation");
    assert!(state
        .tenant_agent_spend_summary(&automation_b.tenant_context(), agent_id)
        .await
        .is_none());

    let run_b = state
        .create_automation_v2_run(&automation_b, "manual")
        .await
        .expect("tenant b run");
    let claimed_b = state.claim_specific_automation_v2_run(&run_b.run_id).await;
    assert!(
        claimed_b.is_some(),
        "tenant b run must not inherit tenant a cap"
    );
}
