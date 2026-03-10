use super::*;

async fn wait_for_automation_v2_run_failure(
    state: &AppState,
    run_id: &str,
    timeout_ms: u64,
) -> Option<crate::AutomationV2RunRecord> {
    let start = std::time::Instant::now();
    loop {
        if start.elapsed().as_millis() as u64 > timeout_ms {
            return None;
        }
        if let Some(run) = state.get_automation_v2_run(run_id).await {
            if run.status == crate::AutomationRunStatus::Failed {
                return Some(run);
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

async fn create_test_automation_v2(
    state: &AppState,
    automation_id: &str,
) -> crate::AutomationV2Spec {
    let automation = crate::AutomationV2Spec {
        automation_id: automation_id.to_string(),
        name: "Test Automation".to_string(),
        description: Some("automation for runtime regression coverage".to_string()),
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        agents: vec![crate::AutomationAgentProfile {
            agent_id: "agent-a".to_string(),
            template_id: Some("template-a".to_string()),
            display_name: "Agent A".to_string(),
            avatar_url: None,
            model_policy: Some(json!({
                "default_model": { "provider_id": "openai", "model_id": "gpt-4.1-mini" }
            })),
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: vec!["read".to_string()],
                denylist: Vec::new(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
            },
            approval_policy: None,
        }],
        flow: crate::AutomationFlowSpec {
            nodes: vec![
                crate::AutomationFlowNode {
                    node_id: "draft".to_string(),
                    agent_id: "agent-a".to_string(),
                    objective: "Create draft".to_string(),
                    depends_on: Vec::new(),
                    input_refs: Vec::new(),
                    output_contract: None,
                    retry_policy: None,
                    timeout_ms: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    metadata: Some(json!({
                        "builder": {
                            "title": "Draft",
                            "prompt": "Write draft v1",
                            "role": "writer"
                        }
                    })),
                },
                crate::AutomationFlowNode {
                    node_id: "review".to_string(),
                    agent_id: "agent-a".to_string(),
                    objective: "Review draft".to_string(),
                    depends_on: vec!["draft".to_string()],
                    input_refs: vec![crate::AutomationFlowInputRef {
                        from_step_id: "draft".to_string(),
                        alias: "draft".to_string(),
                    }],
                    output_contract: None,
                    retry_policy: None,
                    timeout_ms: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Review),
                    gate: None,
                    metadata: Some(json!({
                        "builder": {
                            "title": "Review",
                            "prompt": "Review the draft",
                            "role": "reviewer"
                        }
                    })),
                },
                crate::AutomationFlowNode {
                    node_id: "approval".to_string(),
                    agent_id: "agent-a".to_string(),
                    objective: "Approve output".to_string(),
                    depends_on: vec!["review".to_string()],
                    input_refs: vec![crate::AutomationFlowInputRef {
                        from_step_id: "review".to_string(),
                        alias: "review".to_string(),
                    }],
                    output_contract: None,
                    retry_policy: None,
                    timeout_ms: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Approval),
                    gate: Some(crate::AutomationApprovalGate {
                        required: true,
                        decisions: vec![
                            "approve".to_string(),
                            "rework".to_string(),
                            "cancel".to_string(),
                        ],
                        rework_targets: vec!["draft".to_string()],
                        instructions: Some("Check the review output".to_string()),
                    }),
                    metadata: Some(json!({
                        "builder": {
                            "title": "Approval",
                            "prompt": "",
                            "role": "approver"
                        }
                    })),
                },
            ],
        },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
        creator_id: "test".to_string(),
        workspace_root: Some(".".to_string()),
        metadata: Some(json!({
            "builder_kind": "mission_blueprint",
            "mission": {
                "mission_id": "mission-test",
                "title": "Mission Test",
                "goal": "Verify runtime reset logic",
                "success_criteria": ["done"],
                "phases": [{ "phase_id": "phase_1", "title": "Phase 1", "execution_mode": "soft" }],
                "milestones": [{ "milestone_id": "m1", "title": "Milestone 1", "required_stage_ids": ["draft", "review"] }]
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    };
    state
        .put_automation_v2(automation)
        .await
        .expect("store automation")
}

async fn create_branched_test_automation_v2(
    state: &AppState,
    automation_id: &str,
) -> crate::AutomationV2Spec {
    let automation = crate::AutomationV2Spec {
        automation_id: automation_id.to_string(),
        name: "Branched Test Automation".to_string(),
        description: Some("automation for branched runtime regression coverage".to_string()),
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        agents: vec![crate::AutomationAgentProfile {
            agent_id: "agent-a".to_string(),
            template_id: Some("template-a".to_string()),
            display_name: "Agent A".to_string(),
            avatar_url: None,
            model_policy: Some(json!({
                "default_model": { "provider_id": "openai", "model_id": "gpt-4.1-mini" }
            })),
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: vec!["read".to_string()],
                denylist: Vec::new(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
            },
            approval_policy: None,
        }],
        flow: crate::AutomationFlowSpec {
            nodes: vec![
                crate::AutomationFlowNode {
                    node_id: "research".to_string(),
                    agent_id: "agent-a".to_string(),
                    objective: "Research inputs".to_string(),
                    depends_on: Vec::new(),
                    input_refs: Vec::new(),
                    output_contract: None,
                    retry_policy: None,
                    timeout_ms: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    metadata: Some(json!({
                        "builder": {
                            "title": "Research",
                            "prompt": "Gather research",
                            "role": "researcher"
                        }
                    })),
                },
                crate::AutomationFlowNode {
                    node_id: "analysis".to_string(),
                    agent_id: "agent-a".to_string(),
                    objective: "Analyze research".to_string(),
                    depends_on: vec!["research".to_string()],
                    input_refs: vec![crate::AutomationFlowInputRef {
                        from_step_id: "research".to_string(),
                        alias: "research".to_string(),
                    }],
                    output_contract: None,
                    retry_policy: None,
                    timeout_ms: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    metadata: Some(json!({
                        "builder": {
                            "title": "Analysis",
                            "prompt": "Analyze research findings",
                            "role": "analyst"
                        }
                    })),
                },
                crate::AutomationFlowNode {
                    node_id: "draft".to_string(),
                    agent_id: "agent-a".to_string(),
                    objective: "Draft output".to_string(),
                    depends_on: vec!["research".to_string()],
                    input_refs: vec![crate::AutomationFlowInputRef {
                        from_step_id: "research".to_string(),
                        alias: "research".to_string(),
                    }],
                    output_contract: None,
                    retry_policy: None,
                    timeout_ms: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    metadata: Some(json!({
                        "builder": {
                            "title": "Draft",
                            "prompt": "Write draft",
                            "role": "writer"
                        }
                    })),
                },
                crate::AutomationFlowNode {
                    node_id: "publish".to_string(),
                    agent_id: "agent-a".to_string(),
                    objective: "Publish final output".to_string(),
                    depends_on: vec!["analysis".to_string(), "draft".to_string()],
                    input_refs: vec![
                        crate::AutomationFlowInputRef {
                            from_step_id: "analysis".to_string(),
                            alias: "analysis".to_string(),
                        },
                        crate::AutomationFlowInputRef {
                            from_step_id: "draft".to_string(),
                            alias: "draft".to_string(),
                        },
                    ],
                    output_contract: None,
                    retry_policy: None,
                    timeout_ms: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    metadata: Some(json!({
                        "builder": {
                            "title": "Publish",
                            "prompt": "Combine analysis and draft",
                            "role": "publisher"
                        }
                    })),
                },
            ],
        },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(2),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
        creator_id: "test".to_string(),
        workspace_root: Some(".".to_string()),
        metadata: Some(json!({
            "builder_kind": "mission_blueprint",
            "mission": {
                "mission_id": "branched-mission-test",
                "title": "Branched Mission Test",
                "goal": "Verify branch-local recovery logic",
                "success_criteria": ["done"]
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    };
    state
        .put_automation_v2(automation)
        .await
        .expect("store automation")
}

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
async fn browser_install_route_is_registered() {
    std::env::set_var(
        "TANDEM_BROWSER_RELEASES_URL",
        "http://127.0.0.1:9/releases/tags",
    );
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/browser/install")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    let status = resp.status();
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("browser_install_failed")
    );

    std::env::remove_var("TANDEM_BROWSER_RELEASES_URL");
}

#[tokio::test]
async fn browser_smoke_test_route_is_registered() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/browser/smoke-test")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"url":"https://example.com"}"#))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    let status = resp.status();
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("browser_smoke_test_failed")
    );
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

#[tokio::test]
async fn automations_v2_create_rejects_relative_workspace_root() {
    let state = test_state().await;
    let app = app_router(state);

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "automation_id": "auto-v2-invalid-root",
                "name": "Invalid Root Automation",
                "status": "draft",
                "workspace_root": "relative/path",
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
    assert_eq!(create_resp.status(), StatusCode::BAD_REQUEST);
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("create json");
    assert_eq!(
        create_payload.get("code").and_then(Value::as_str),
        Some("AUTOMATION_V2_CREATE_FAILED")
    );
}

#[tokio::test]
async fn automations_v2_patch_rejects_relative_workspace_root() {
    let state = test_state().await;
    let app = app_router(state);

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "automation_id": "auto-v2-patch-invalid-root",
                "name": "Patch Invalid Root Automation",
                "status": "draft",
                "workspace_root": "/tmp/valid-root",
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

    let patch_req = Request::builder()
        .method("PATCH")
        .uri("/automations/v2/auto-v2-patch-invalid-root")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workspace_root": "relative/path"
            })
            .to_string(),
        ))
        .expect("patch request");
    let patch_resp = app
        .clone()
        .oneshot(patch_req)
        .await
        .expect("patch response");
    assert_eq!(patch_resp.status(), StatusCode::BAD_REQUEST);
    let patch_body = to_bytes(patch_resp.into_body(), usize::MAX)
        .await
        .expect("patch body");
    let patch_payload: Value = serde_json::from_slice(&patch_body).expect("patch json");
    assert_eq!(
        patch_payload.get("code").and_then(Value::as_str),
        Some("AUTOMATION_V2_UPDATE_FAILED")
    );
}

#[tokio::test]
async fn automations_v2_executor_fails_run_when_workspace_root_missing() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let missing_root = std::env::temp_dir().join(format!(
        "tandem-automation-v2-missing-root-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&missing_root);

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "automation_id": "auto-v2-runtime-missing-root",
                "name": "Runtime Missing Root Automation",
                "status": "active",
                "workspace_root": missing_root.to_string_lossy(),
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
        .uri("/automations/v2/auto-v2-runtime-missing-root/run_now")
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
        .and_then(|row| row.get("run_id"))
        .and_then(Value::as_str)
        .expect("run id")
        .to_string();

    let executor = tokio::spawn(crate::run_automation_v2_executor(state.clone()));
    let failed = wait_for_automation_v2_run_failure(&state, &run_id, 5_000)
        .await
        .expect("run should fail for missing workspace root");
    executor.abort();

    assert!(failed
        .detail
        .as_deref()
        .map(|detail| detail.contains("does not exist"))
        .unwrap_or(false));
}

#[tokio::test]
async fn automations_v2_executor_fails_run_when_workspace_root_is_file() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let file_root = std::env::temp_dir().join(format!(
        "tandem-automation-v2-workspace-file-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::write(&file_root, "not-a-directory").expect("write workspace root file");

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "automation_id": "auto-v2-runtime-file-root",
                "name": "Runtime File Root Automation",
                "status": "active",
                "workspace_root": file_root.to_string_lossy(),
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
        .uri("/automations/v2/auto-v2-runtime-file-root/run_now")
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
        .and_then(|row| row.get("run_id"))
        .and_then(Value::as_str)
        .expect("run id")
        .to_string();

    let executor = tokio::spawn(crate::run_automation_v2_executor(state.clone()));
    let failed = wait_for_automation_v2_run_failure(&state, &run_id, 5_000)
        .await
        .expect("run should fail for file workspace root");
    executor.abort();
    let _ = std::fs::remove_file(&file_root);

    assert!(failed
        .detail
        .as_deref()
        .map(|detail| detail.contains("is not a directory"))
        .unwrap_or(false));
}

#[tokio::test]
async fn automations_v2_gate_rework_clears_downstream_outputs_and_requeues_subtree() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-rework-reset").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    let now = crate::now_ms();
    let updated = state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::AwaitingApproval;
            row.checkpoint.completed_nodes = vec!["draft".to_string(), "review".to_string()];
            row.checkpoint.pending_nodes = vec!["approval".to_string()];
            row.checkpoint.awaiting_gate = Some(crate::AutomationPendingGate {
                node_id: "approval".to_string(),
                title: "Approval".to_string(),
                instructions: Some("approve".to_string()),
                decisions: vec![
                    "approve".to_string(),
                    "rework".to_string(),
                    "cancel".to_string(),
                ],
                rework_targets: vec!["draft".to_string()],
                requested_at_ms: now,
                upstream_node_ids: vec!["review".to_string()],
            });
            row.checkpoint
                .node_outputs
                .insert("draft".to_string(), json!({"summary":"draft"}));
            row.checkpoint
                .node_outputs
                .insert("review".to_string(), json!({"summary":"review"}));
        })
        .await
        .expect("updated run");
    assert_eq!(updated.status, crate::AutomationRunStatus::AwaitingApproval);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/gate", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "decision": "rework" }).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let repaired = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after rework");
    assert_eq!(repaired.status, crate::AutomationRunStatus::Queued);
    assert_eq!(repaired.checkpoint.completed_nodes, Vec::<String>::new());
    assert!(repaired
        .checkpoint
        .pending_nodes
        .iter()
        .any(|id| id == "draft"));
    assert!(repaired
        .checkpoint
        .pending_nodes
        .iter()
        .any(|id| id == "review"));
    assert!(repaired
        .checkpoint
        .pending_nodes
        .iter()
        .any(|id| id == "approval"));
    assert!(!repaired.checkpoint.node_outputs.contains_key("draft"));
    assert!(!repaired.checkpoint.node_outputs.contains_key("review"));
    assert!(repaired
        .checkpoint
        .gate_history
        .iter()
        .any(|entry| entry.decision == "rework"));
}

#[tokio::test]
async fn automations_v2_run_recover_from_pause_preserves_completed_state_and_records_history() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-pause-recover").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Paused;
            row.pause_reason = Some("paused for operator review".to_string());
            row.checkpoint.completed_nodes = vec!["draft".to_string()];
            row.checkpoint.pending_nodes = vec!["review".to_string(), "approval".to_string()];
            row.checkpoint
                .node_outputs
                .insert("draft".to_string(), json!({"summary":"draft"}));
            row.checkpoint.awaiting_gate = Some(crate::AutomationPendingGate {
                node_id: "approval".to_string(),
                title: "Approval".to_string(),
                instructions: Some("approve".to_string()),
                decisions: vec![
                    "approve".to_string(),
                    "rework".to_string(),
                    "cancel".to_string(),
                ],
                rework_targets: vec!["draft".to_string()],
                requested_at_ms: crate::now_ms(),
                upstream_node_ids: vec!["review".to_string()],
            });
            row.checkpoint.blocked_nodes = vec!["approval".to_string()];
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/recover", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "reason": "continue after operator pause" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let recovered = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after recover");
    assert_eq!(recovered.status, crate::AutomationRunStatus::Queued);
    assert_eq!(
        recovered.checkpoint.completed_nodes,
        vec!["draft".to_string()]
    );
    assert_eq!(
        recovered.checkpoint.pending_nodes,
        vec!["review".to_string(), "approval".to_string()]
    );
    assert!(recovered.checkpoint.node_outputs.contains_key("draft"));
    assert!(recovered.checkpoint.awaiting_gate.is_none());
    assert!(recovered.checkpoint.blocked_nodes.is_empty());
    assert_eq!(
        recovered.resume_reason.as_deref(),
        Some("continue after operator pause")
    );
    let recover_event = recovered
        .checkpoint
        .lifecycle_history
        .iter()
        .find(|entry| entry.event == "run_recovered_from_pause")
        .expect("recover from pause event");
    assert_eq!(
        recover_event.reason.as_deref(),
        Some("continue after operator pause")
    );
}

#[tokio::test]
async fn automations_v2_run_cancel_records_operator_stop_kind_and_clears_active_ids() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-stop-kind").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Running;
            row.active_session_ids = vec!["session-a".to_string(), "session-b".to_string()];
            row.active_instance_ids = vec!["instance-a".to_string()];
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/cancel", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "reason": "kill switch triggered by operator" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let cancelled = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("cancelled run");
    assert_eq!(cancelled.status, crate::AutomationRunStatus::Cancelled);
    assert_eq!(
        cancelled.stop_kind,
        Some(crate::AutomationStopKind::OperatorStopped)
    );
    assert_eq!(
        cancelled.stop_reason.as_deref(),
        Some("kill switch triggered by operator")
    );
    assert!(cancelled.active_session_ids.is_empty());
    assert!(cancelled.active_instance_ids.is_empty());
    let stop_event = cancelled
        .checkpoint
        .lifecycle_history
        .iter()
        .find(|entry| entry.event == "run_stopped")
        .expect("run stopped event");
    assert_eq!(
        stop_event.stop_kind,
        Some(crate::AutomationStopKind::OperatorStopped)
    );
    assert_eq!(
        stop_event.reason.as_deref(),
        Some("kill switch triggered by operator")
    );
}

#[tokio::test]
async fn automations_v2_run_recover_on_failed_branch_preserves_completed_sibling_branch() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_branched_test_automation_v2(&state, "auto-v2-branch-recover").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Failed;
            row.checkpoint.completed_nodes = vec![
                "research".to_string(),
                "analysis".to_string(),
                "draft".to_string(),
            ];
            row.checkpoint.pending_nodes = vec!["publish".to_string()];
            row.checkpoint
                .node_outputs
                .insert("research".to_string(), json!({"summary":"research"}));
            row.checkpoint
                .node_outputs
                .insert("analysis".to_string(), json!({"summary":"analysis"}));
            row.checkpoint
                .node_outputs
                .insert("draft".to_string(), json!({"summary":"draft"}));
            row.checkpoint.last_failure = Some(crate::AutomationFailureRecord {
                node_id: "draft".to_string(),
                reason: "bad draft".to_string(),
                failed_at_ms: crate::now_ms(),
            });
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/recover", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "reason": "retry only the failed draft branch" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let recovered = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after recover");
    assert_eq!(recovered.status, crate::AutomationRunStatus::Queued);
    assert!(recovered
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "research"));
    assert!(recovered
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "analysis"));
    assert!(!recovered
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "draft"));
    assert!(!recovered
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "publish"));
    assert!(recovered.checkpoint.node_outputs.contains_key("research"));
    assert!(recovered.checkpoint.node_outputs.contains_key("analysis"));
    assert!(!recovered.checkpoint.node_outputs.contains_key("draft"));
    assert!(!recovered.checkpoint.node_outputs.contains_key("publish"));
    assert!(recovered
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "draft"));
    assert!(recovered
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "publish"));
    assert!(!recovered
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "analysis"));
    assert!(recovered.checkpoint.last_failure.is_none());
    let recover_event = recovered
        .checkpoint
        .lifecycle_history
        .iter()
        .find(|entry| entry.event == "run_recovered")
        .expect("recover event");
    assert_eq!(
        recover_event.reason.as_deref(),
        Some("retry only the failed draft branch")
    );
}

#[tokio::test]
async fn automations_v2_run_recover_from_pause_preserves_branched_state() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation =
        create_branched_test_automation_v2(&state, "auto-v2-branch-pause-recover").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Paused;
            row.pause_reason = Some("operator paused branched mission".to_string());
            row.checkpoint.completed_nodes = vec!["research".to_string(), "analysis".to_string()];
            row.checkpoint.pending_nodes = vec!["draft".to_string(), "publish".to_string()];
            row.checkpoint
                .node_outputs
                .insert("research".to_string(), json!({"summary":"research"}));
            row.checkpoint
                .node_outputs
                .insert("analysis".to_string(), json!({"summary":"analysis"}));
            row.checkpoint.blocked_nodes = vec!["publish".to_string()];
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/recover", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "reason": "continue branched mission after pause" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let recovered = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after recover");
    assert_eq!(recovered.status, crate::AutomationRunStatus::Queued);
    assert_eq!(
        recovered.checkpoint.completed_nodes,
        vec!["research".to_string(), "analysis".to_string()]
    );
    assert_eq!(
        recovered.checkpoint.pending_nodes,
        vec!["draft".to_string(), "publish".to_string()]
    );
    assert!(recovered.checkpoint.node_outputs.contains_key("research"));
    assert!(recovered.checkpoint.node_outputs.contains_key("analysis"));
    assert!(!recovered.checkpoint.node_outputs.contains_key("draft"));
    assert!(recovered.checkpoint.blocked_nodes.is_empty());
    assert_eq!(
        recovered.resume_reason.as_deref(),
        Some("continue branched mission after pause")
    );
    let recover_event = recovered
        .checkpoint
        .lifecycle_history
        .iter()
        .find(|entry| entry.event == "run_recovered_from_pause")
        .expect("recover from pause event");
    assert_eq!(
        recover_event.reason.as_deref(),
        Some("continue branched mission after pause")
    );
}

#[tokio::test]
async fn automations_v2_run_repair_resets_descendants_and_records_diff_metadata() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-step-repair").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Failed;
            row.checkpoint.completed_nodes = vec!["draft".to_string(), "review".to_string()];
            row.checkpoint.pending_nodes = vec!["approval".to_string()];
            row.checkpoint
                .node_outputs
                .insert("draft".to_string(), json!({"summary":"draft"}));
            row.checkpoint
                .node_outputs
                .insert("review".to_string(), json!({"summary":"review"}));
            row.checkpoint.last_failure = Some(crate::AutomationFailureRecord {
                node_id: "draft".to_string(),
                reason: "bad draft".to_string(),
                failed_at_ms: crate::now_ms(),
            });
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/repair", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "node_id": "draft",
                        "prompt": "Write draft v2 with corrections",
                        "template_id": "template-b",
                        "model_policy": {
                            "default_model": { "provider_id": "anthropic", "model_id": "claude-3-5-sonnet" }
                        },
                        "reason": "tighten draft prompt"
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let repaired = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after repair");
    assert_eq!(repaired.status, crate::AutomationRunStatus::Queued);
    assert_eq!(repaired.checkpoint.completed_nodes, Vec::<String>::new());
    assert!(repaired
        .checkpoint
        .pending_nodes
        .iter()
        .any(|id| id == "draft"));
    assert!(repaired
        .checkpoint
        .pending_nodes
        .iter()
        .any(|id| id == "review"));
    assert!(repaired
        .checkpoint
        .pending_nodes
        .iter()
        .any(|id| id == "approval"));
    assert!(!repaired.checkpoint.node_outputs.contains_key("draft"));
    assert!(!repaired.checkpoint.node_outputs.contains_key("review"));
    let repair_event = repaired
        .checkpoint
        .lifecycle_history
        .iter()
        .find(|entry| entry.event == "run_step_repaired")
        .expect("repair event");
    let metadata = repair_event.metadata.as_ref().expect("repair metadata");
    assert_eq!(
        metadata.get("previous_prompt").and_then(Value::as_str),
        Some("Write draft v1")
    );
    assert_eq!(
        metadata.get("new_prompt").and_then(Value::as_str),
        Some("Write draft v2 with corrections")
    );
    assert_eq!(
        metadata.get("previous_template_id").and_then(Value::as_str),
        Some("template-a")
    );
    assert_eq!(
        metadata.get("new_template_id").and_then(Value::as_str),
        Some("template-b")
    );

    let stored = state
        .get_automation_v2("auto-v2-step-repair")
        .await
        .expect("stored automation");
    let draft_node = stored
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "draft")
        .expect("draft node");
    assert_eq!(
        draft_node
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("builder"))
            .and_then(|builder| builder.get("prompt"))
            .and_then(Value::as_str),
        Some("Write draft v2 with corrections")
    );
    assert_eq!(stored.agents[0].template_id.as_deref(), Some("template-b"));
}
