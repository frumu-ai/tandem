use super::*;

#[tokio::test]
async fn global_health_route_returns_healthy_shape() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("GET")
        .uri("/global/health")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("healthy").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(payload.get("ready").and_then(|v| v.as_bool()), Some(true));
    assert!(payload.get("phase").is_some());
    assert!(payload.get("startup_attempt_id").is_some());
    assert!(payload.get("startup_elapsed_ms").is_some());
    assert!(payload.get("version").and_then(|v| v.as_str()).is_some());
    assert!(payload.get("mode").and_then(|v| v.as_str()).is_some());
    assert!(payload.get("environment").is_some());
}

#[tokio::test]
async fn browser_status_route_returns_browser_readiness_shape() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("GET")
        .uri("/browser/status")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(payload.get("enabled").and_then(Value::as_bool).is_some());
    assert!(payload.get("runnable").and_then(Value::as_bool).is_some());
    assert!(payload.get("sidecar").is_some());
    assert!(payload.get("browser").is_some());
    assert!(payload.get("blocking_issues").is_some());
    assert!(payload.get("recommendations").is_some());
    assert!(payload.get("install_hints").is_some());
}

#[tokio::test]
async fn non_health_routes_are_blocked_until_runtime_ready() {
    let state = AppState::new_starting(Uuid::new_v4().to_string(), false);
    let app = app_router(state);
    let req = Request::builder()
        .method("GET")
        .uri("/provider")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(|v| v.as_str()),
        Some("ENGINE_STARTING")
    );
}

#[tokio::test]
async fn skills_endpoints_return_expected_shapes() {
    let state = test_state().await;
    let app = app_router(state);

    let list_req = Request::builder()
        .method("GET")
        .uri("/skills")
        .body(Body::empty())
        .expect("request");
    let list_resp = app.clone().oneshot(list_req).await.expect("response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("json");
    assert!(list_payload.is_array());

    let legacy_req = Request::builder()
        .method("GET")
        .uri("/skill")
        .body(Body::empty())
        .expect("request");
    let legacy_resp = app.clone().oneshot(legacy_req).await.expect("response");
    assert_eq!(legacy_resp.status(), StatusCode::OK);
    let legacy_body = to_bytes(legacy_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let legacy_payload: Value = serde_json::from_slice(&legacy_body).expect("json");
    assert!(legacy_payload.get("skills").is_some());
    assert!(legacy_payload.get("deprecation_warning").is_some());

    let generate_req = Request::builder()
        .method("POST")
        .uri("/skills/generate")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"prompt":"check my email every morning"}).to_string(),
        ))
        .expect("request");
    let generate_resp = app.clone().oneshot(generate_req).await.expect("response");
    assert_eq!(generate_resp.status(), StatusCode::OK);
    let generate_body = to_bytes(generate_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let generate_payload: Value = serde_json::from_slice(&generate_body).expect("json");
    assert_eq!(
        generate_payload.get("status").and_then(|v| v.as_str()),
        Some("generated_scaffold")
    );
    assert!(generate_payload.get("artifacts").is_some());

    let router_req = Request::builder()
        .method("POST")
        .uri("/skills/router/match")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "goal":"check my email every morning",
                "context_run_id":"ctx-run-skill-router-1"
            })
            .to_string(),
        ))
        .expect("request");
    let router_resp = app.clone().oneshot(router_req).await.expect("response");
    assert_eq!(router_resp.status(), StatusCode::OK);

    let blackboard_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-skill-router-1/blackboard")
        .body(Body::empty())
        .expect("request");
    let blackboard_resp = app.clone().oneshot(blackboard_req).await.expect("response");
    assert_eq!(blackboard_resp.status(), StatusCode::OK);
    let blackboard_body = to_bytes(blackboard_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let blackboard_payload: Value = serde_json::from_slice(&blackboard_body).expect("json");
    let tasks = blackboard_payload
        .get("blackboard")
        .and_then(|v| v.get("tasks"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(tasks.iter().any(|task| {
        task.get("task_type")
            .and_then(Value::as_str)
            .map(|v| v == "skill_router.match")
            .unwrap_or(false)
    }));

    let compile_req = Request::builder()
        .method("POST")
        .uri("/skills/compile")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"goal":"non matching empty set"}).to_string(),
        ))
        .expect("request");
    let compile_resp = app.clone().oneshot(compile_req).await.expect("response");
    assert_eq!(compile_resp.status(), StatusCode::BAD_REQUEST);

    let eval_req = Request::builder()
            .method("POST")
            .uri("/skills/evals/benchmark")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"cases":[{"prompt":"check my email every morning","expected_skill":"email-digest"}]}).to_string(),
            ))
            .expect("request");
    let eval_resp = app.clone().oneshot(eval_req).await.expect("response");
    assert_eq!(eval_resp.status(), StatusCode::OK);
    let eval_body = to_bytes(eval_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let eval_payload: Value = serde_json::from_slice(&eval_body).expect("json");
    assert_eq!(
        eval_payload.get("status").and_then(|v| v.as_str()),
        Some("scaffold")
    );
    assert!(eval_payload
        .get("accuracy")
        .and_then(|v| v.as_f64())
        .is_some());
}

#[tokio::test]
async fn admin_and_channel_routes_require_auth_when_api_token_enabled() {
    let state = test_state().await;
    state.set_api_token(Some("tk_test".to_string())).await;
    let app = app_router(state);

    for (method, uri) in [
        ("GET", "/channels/config"),
        ("GET", "/channels/status"),
        ("POST", "/channels/discord/verify"),
        ("POST", "/admin/reload-config"),
        ("GET", "/memory"),
    ] {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request");
        let resp = app.clone().oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

#[test]
fn sanitize_relative_subpath_accepts_safe_relative_paths() {
    let parsed =
        sanitize_relative_subpath(Some("channel_uploads/telegram")).expect("safe relative path");
    assert_eq!(
        parsed.to_string_lossy().replace('\\', "/"),
        "channel_uploads/telegram"
    );
}

#[test]
fn sanitize_relative_subpath_rejects_parent_segments() {
    let err = sanitize_relative_subpath(Some("../secrets")).expect_err("must reject parent");
    assert_eq!(err, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn automation_v2_run_get_projects_nodes_into_context_blackboard_tasks() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "automation_id": "auto-v2-blackboard-1",
                "name": "Automation Blackboard Projection",
                "status": "active",
                "schedule": {
                    "type": "manual",
                    "timezone": "UTC",
                    "misfire_policy": { "type": "skip" }
                },
                "agents": [
                    {
                        "agent_id": "agent-a",
                        "display_name": "Agent A",
                        "skills": [],
                        "tool_policy": { "allowlist": ["read"], "denylist": [] },
                        "mcp_policy": { "allowed_servers": [] }
                    }
                ],
                "flow": {
                    "nodes": [
                        {
                            "node_id": "node-1",
                            "agent_id": "agent-a",
                            "objective": "Analyze incoming signal",
                            "depends_on": []
                        }
                    ]
                },
                "execution": { "max_parallel_agents": 1 }
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let run_now_req = Request::builder()
        .method("POST")
        .uri("/automations/v2/auto-v2-blackboard-1/run_now")
        .body(Body::empty())
        .expect("run now request");
    let run_now_resp = app
        .clone()
        .oneshot(run_now_req)
        .await
        .expect("run now response");
    assert_eq!(run_now_resp.status(), StatusCode::OK);
    let run_now_body = to_bytes(run_now_resp.into_body(), usize::MAX)
        .await
        .expect("run now body");
    let run_now_payload: Value = serde_json::from_slice(&run_now_body).expect("run now json");
    let run_id = run_now_payload
        .get("run")
        .and_then(|v| v.get("run_id"))
        .and_then(Value::as_str)
        .expect("run id")
        .to_string();

    let run_get_req = Request::builder()
        .method("GET")
        .uri(format!("/automations/v2/runs/{run_id}"))
        .body(Body::empty())
        .expect("run get request");
    let run_get_resp = app
        .clone()
        .oneshot(run_get_req)
        .await
        .expect("run get response");
    assert_eq!(run_get_resp.status(), StatusCode::OK);
    let run_get_body = to_bytes(run_get_resp.into_body(), usize::MAX)
        .await
        .expect("run get body");
    let run_get_payload: Value = serde_json::from_slice(&run_get_body).expect("run get json");
    let context_run_id = run_get_payload
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("context run id")
        .to_string();

    let blackboard_req = Request::builder()
        .method("GET")
        .uri(format!("/context/runs/{context_run_id}/blackboard"))
        .body(Body::empty())
        .expect("blackboard request");
    let blackboard_resp = app
        .clone()
        .oneshot(blackboard_req)
        .await
        .expect("blackboard response");
    assert_eq!(blackboard_resp.status(), StatusCode::OK);
    let blackboard_body = to_bytes(blackboard_resp.into_body(), usize::MAX)
        .await
        .expect("blackboard body");
    let blackboard_payload: Value =
        serde_json::from_slice(&blackboard_body).expect("blackboard json");
    let tasks = blackboard_payload
        .get("blackboard")
        .and_then(|v| v.get("tasks"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(tasks.iter().any(|task| {
        task.get("task_type")
            .and_then(Value::as_str)
            .map(|row| row == "automation_v2.node")
            .unwrap_or(false)
            && task
                .get("workflow_node_id")
                .and_then(Value::as_str)
                .map(|row| row == "node-1")
                .unwrap_or(false)
    }));
}
