// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[tokio::test]
async fn create_automation_v2_dry_run_records_completed_audit_run() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "automation_id": "auto-v2-dry-run",
                "name": "Dry run workflow",
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
                            "objective": "Dry run scope check",
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

    let dry_run_req = Request::builder()
        .method("POST")
        .uri("/automations/v2/auto-v2-dry-run/run_now")
        .header("content-type", "application/json")
        .body(Body::from(json!({"dry_run": true}).to_string()))
        .expect("dry run request");
    let dry_run_resp = app
        .clone()
        .oneshot(dry_run_req)
        .await
        .expect("dry run response");
    assert_eq!(dry_run_resp.status(), StatusCode::OK);
    let dry_run_body = to_bytes(dry_run_resp.into_body(), usize::MAX)
        .await
        .expect("dry run body");
    let dry_run_payload: Value = serde_json::from_slice(&dry_run_body).expect("dry run json");
    assert_eq!(
        dry_run_payload.get("dry_run").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        dry_run_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        dry_run_payload
            .get("run")
            .and_then(|row| row.get("trigger_type"))
            .and_then(Value::as_str),
        Some("manual_dry_run")
    );
    let run_id = dry_run_payload
        .get("run")
        .and_then(|row| row.get("run_id"))
        .and_then(Value::as_str)
        .expect("dry run id")
        .to_string();
    let stored = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("stored dry run");
    assert_eq!(stored.status, crate::AutomationRunStatus::Completed);
    assert_eq!(stored.trigger_type, "manual_dry_run");
}

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

fn test_automation_v2_spec(automation_id: &str) -> crate::AutomationV2Spec {
    crate::AutomationV2Spec {
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
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
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
                allowed_connections: Vec::new(),
            },
            approval_policy: None,
        }],
        flow: crate::AutomationFlowSpec {
            nodes: vec![
                crate::AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "draft".to_string(),
                    agent_id: "agent-a".to_string(),
                    objective: "Create draft".to_string(),
                    depends_on: Vec::new(),
                    input_refs: Vec::new(),
                    output_contract: None,
                    tool_policy: None,
                    mcp_policy: None,
                    retry_policy: None,
                    timeout_ms: None,
                    max_tool_calls: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    wait: None,
                    metadata: Some(json!({
                        "builder": {
                            "title": "Draft",
                            "prompt": "Write draft v1",
                            "role": "writer"
                        }
                    })),
                },
                crate::AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "review".to_string(),
                    agent_id: "agent-a".to_string(),
                    objective: "Review draft".to_string(),
                    depends_on: vec!["draft".to_string()],
                    input_refs: vec![crate::AutomationFlowInputRef {
                        from_step_id: "draft".to_string(),
                        alias: "draft".to_string(),
                    }],
                    output_contract: None,
                    tool_policy: None,
                    mcp_policy: None,
                    retry_policy: None,
                    timeout_ms: None,
                    max_tool_calls: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Review),
                    gate: None,
                    wait: None,
                    metadata: Some(json!({
                        "builder": {
                            "title": "Review",
                            "prompt": "Review the draft",
                            "role": "reviewer"
                        }
                    })),
                },
                crate::AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "approval".to_string(),
                    agent_id: "agent-a".to_string(),
                    objective: "Approve output".to_string(),
                    depends_on: vec!["review".to_string()],
                    input_refs: vec![crate::AutomationFlowInputRef {
                        from_step_id: "review".to_string(),
                        alias: "review".to_string(),
                    }],
                    output_contract: None,
                    tool_policy: None,
                    mcp_policy: None,
                    retry_policy: None,
                    timeout_ms: None,
                    max_tool_calls: None,
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
                        expiry_policy: None,
                    }),
                    wait: None,
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
            profile: None,
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
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    }
}

pub(in crate::http::tests) async fn create_test_automation_v2(
    state: &AppState,
    automation_id: &str,
) -> crate::AutomationV2Spec {
    state
        .put_automation_v2(test_automation_v2_spec(automation_id))
        .await
        .expect("store automation")
}

pub(in crate::http::tests) async fn create_test_automation_v2_for_tenant(
    state: &AppState,
    automation_id: &str,
    tenant_context: &tandem_types::TenantContext,
) -> crate::AutomationV2Spec {
    let mut automation = test_automation_v2_spec(automation_id);
    automation.set_tenant_context(tenant_context);
    state
        .put_automation_v2(automation)
        .await
        .expect("store tenant automation")
}

fn branched_test_automation_v2_spec(automation_id: &str) -> crate::AutomationV2Spec {
    crate::AutomationV2Spec {
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
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
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
                allowed_connections: Vec::new(),
            },
            approval_policy: None,
        }],
        flow: crate::AutomationFlowSpec {
            nodes: vec![
                crate::AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "research".to_string(),
                    agent_id: "agent-a".to_string(),
                    objective: "Research inputs".to_string(),
                    depends_on: Vec::new(),
                    input_refs: Vec::new(),
                    output_contract: None,
                    tool_policy: None,
                    mcp_policy: None,
                    retry_policy: None,
                    timeout_ms: None,
                    max_tool_calls: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    wait: None,
                    metadata: Some(json!({
                        "builder": {
                            "title": "Research",
                            "prompt": "Gather research",
                            "role": "researcher"
                        }
                    })),
                },
                crate::AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "analysis".to_string(),
                    agent_id: "agent-a".to_string(),
                    objective: "Analyze research".to_string(),
                    depends_on: vec!["research".to_string()],
                    input_refs: vec![crate::AutomationFlowInputRef {
                        from_step_id: "research".to_string(),
                        alias: "research".to_string(),
                    }],
                    output_contract: None,
                    tool_policy: None,
                    mcp_policy: None,
                    retry_policy: None,
                    timeout_ms: None,
                    max_tool_calls: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    wait: None,
                    metadata: Some(json!({
                        "builder": {
                            "title": "Analysis",
                            "prompt": "Analyze research findings",
                            "role": "analyst"
                        }
                    })),
                },
                crate::AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "draft".to_string(),
                    agent_id: "agent-a".to_string(),
                    objective: "Draft output".to_string(),
                    depends_on: vec!["research".to_string()],
                    input_refs: vec![crate::AutomationFlowInputRef {
                        from_step_id: "research".to_string(),
                        alias: "research".to_string(),
                    }],
                    output_contract: None,
                    tool_policy: None,
                    mcp_policy: None,
                    retry_policy: None,
                    timeout_ms: None,
                    max_tool_calls: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    wait: None,
                    metadata: Some(json!({
                        "builder": {
                            "title": "Draft",
                            "prompt": "Write draft",
                            "role": "writer"
                        }
                    })),
                },
                crate::AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
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
                    tool_policy: None,
                    mcp_policy: None,
                    retry_policy: None,
                    timeout_ms: None,
                    max_tool_calls: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    wait: None,
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
            profile: None,
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
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    }
}

async fn create_branched_test_automation_v2(
    state: &AppState,
    automation_id: &str,
) -> crate::AutomationV2Spec {
    state
        .put_automation_v2(branched_test_automation_v2_spec(automation_id))
        .await
        .expect("store automation")
}

async fn create_branched_test_automation_v2_for_tenant(
    state: &AppState,
    automation_id: &str,
    tenant_context: &tandem_types::TenantContext,
) -> crate::AutomationV2Spec {
    let mut automation = branched_test_automation_v2_spec(automation_id);
    automation.set_tenant_context(tenant_context);
    state
        .put_automation_v2(automation)
        .await
        .expect("store tenant automation")
}

#[tokio::test]
async fn global_health_route_returns_only_minimal_public_shape() {
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
    assert_eq!(payload.as_object().map(|object| object.len()), Some(2));
}

#[tokio::test]
async fn global_diagnostics_route_is_authenticated_and_redacted() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("GET")
        .uri("/global/diagnostics")
        .extension(axum::extract::ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 43123))))
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(payload.get("phase").is_some());
    assert!(payload.get("version").and_then(Value::as_str).is_some());
    assert!(payload.get("memory_storage").is_some());
    assert!(payload.get("environment").is_some());
    for forbidden in [
        "binary_path",
        "binary_modified_at_ms",
        "workspace_root",
        "last_error",
    ] {
        assert!(
            payload.get(forbidden).is_none(),
            "diagnostics leaked forbidden field {forbidden}"
        );
    }
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
#[serial_test::serial]
async fn browser_install_route_is_registered() {
    std::env::set_var(
        "TANDEM_BROWSER_RELEASES_URL",
        "http://127.0.0.1:9/releases/tags",
    );
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .extension(direct_loopback_peer())
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
        .extension(direct_loopback_peer())
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
async fn skills_compile_pack_builder_recipe_emits_automation_preview() {
    let state = test_state().await;
    let app = app_router(state);
    let skill_name = format!("recipe-compiler-test-{}", Uuid::new_v4());
    let skill_md = format!(
        "---\nname: {skill_name}\ndescription: Recipe compiler test skill.\nversion: 0.1.0\n---\n\n# Skill: recipe compiler test\n\n## Purpose\nCompile into automation preview.\n\n## Inputs\n- user prompt\n\n## Agents\n- worker\n\n## Tools\n- webfetch\n\n## Workflow\n1. Interpret user intent\n2. Execute workflow steps\n3. Return result\n\n## Outputs\n- completed task result\n\n## Schedule compatibility\n- manual\n"
    );
    let workflow_yaml = format!(
        "kind: pack_builder_recipe\nskill_id: {skill_name}\nexecution_mode: team\ngoal_template: \"Research '{{{{query}}}}' and produce a cited report.\"\n"
    );

    let install_req = Request::builder()
        .method("POST")
        .uri("/skills/generate/install")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "location":"global",
                "artifacts":{
                    "SKILL.md": skill_md,
                    "workflow.yaml": workflow_yaml
                }
            })
            .to_string(),
        ))
        .expect("install request");
    let install_resp = app
        .clone()
        .oneshot(install_req)
        .await
        .expect("install response");
    assert_eq!(install_resp.status(), StatusCode::OK);

    let compile_req = Request::builder()
        .method("POST")
        .uri("/skills/compile")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "skill_name": skill_name,
                "goal":"Research tandem autonomous runtime patterns and produce a cited report.",
                "schedule":{"type":"manual"}
            })
            .to_string(),
        ))
        .expect("request");
    let compile_resp = app.oneshot(compile_req).await.expect("response");
    assert_eq!(compile_resp.status(), StatusCode::OK);
    let compile_body = to_bytes(compile_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let compile_payload: Value = serde_json::from_slice(&compile_body).expect("json");
    assert_eq!(
        compile_payload.get("workflow_kind").and_then(Value::as_str),
        Some("pack_builder_recipe")
    );
    assert_eq!(
        compile_payload
            .pointer("/execution_plan/default_action")
            .and_then(Value::as_str),
        Some("create_automation_v2")
    );
    let automation_preview = compile_payload
        .get("automation_preview")
        .expect("automation preview");
    assert_eq!(
        automation_preview.get("creator_id").and_then(Value::as_str),
        Some("skills_compile")
    );
    assert_eq!(
        automation_preview
            .pointer("/metadata/skill_name")
            .and_then(Value::as_str),
        Some(skill_name.as_str())
    );
    assert_eq!(
        automation_preview
            .pointer("/metadata/skill_workflow_kind")
            .and_then(Value::as_str),
        Some("pack_builder_recipe")
    );
    assert_eq!(
        automation_preview
            .pointer("/metadata/skill_execution_mode")
            .and_then(Value::as_str),
        Some("team")
    );
    assert_eq!(
        automation_preview
            .pointer("/agents/0/skills/0")
            .and_then(Value::as_str),
        Some(skill_name.as_str())
    );
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
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
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
    let run_now_context_run_id = run_now_payload
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("run now context run id")
        .to_string();
    assert_eq!(
        run_now_payload
            .get("linked_context_run_id")
            .and_then(Value::as_str),
        Some(run_now_context_run_id.as_str())
    );
    let run_id = run_now_payload
        .get("run")
        .and_then(|v| v.get("run_id"))
        .and_then(Value::as_str)
        .expect("run id")
        .to_string();
    assert_eq!(
        run_now_payload
            .get("run")
            .and_then(|v| v.get("contextRunID"))
            .and_then(Value::as_str),
        Some(run_now_context_run_id.as_str())
    );

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
    assert_eq!(context_run_id, run_now_context_run_id);
    assert_eq!(
        run_get_payload
            .get("linked_context_run_id")
            .and_then(Value::as_str),
        Some(context_run_id.as_str())
    );
    assert_eq!(
        run_get_payload
            .get("run")
            .and_then(|v| v.get("contextRunID"))
            .and_then(Value::as_str),
        Some(context_run_id.as_str())
    );

    let start = std::time::Instant::now();
    let tasks = loop {
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
        if tasks.iter().any(|task| {
            task.get("task_type")
                .and_then(Value::as_str)
                .map(|row| row == "automation_node")
                .unwrap_or(false)
                && task
                    .get("workflow_node_id")
                    .and_then(Value::as_str)
                    .map(|row| row == "node-1")
                    .unwrap_or(false)
        }) {
            break tasks;
        }
        assert!(
            start.elapsed().as_millis() < 5_000,
            "automation_v2 node task was not projected into the blackboard in time"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    assert!(!tasks.is_empty());
}

#[tokio::test]
async fn automation_v2_runs_list_exposes_context_run_links() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let automation = create_test_automation_v2(&state, "auto-v2-list-links").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/automations/v2/auto-v2-list-links/runs")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let runs = payload.get("runs").and_then(Value::as_array).expect("runs");
    let row = runs
        .iter()
        .find(|candidate| {
            candidate.get("run_id").and_then(Value::as_str) == Some(run.run_id.as_str())
        })
        .expect("matching run");
    let context_run_id = row
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("context run id");
    assert_eq!(
        row.get("linked_context_run_id").and_then(Value::as_str),
        Some(context_run_id)
    );

    let context_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/runs/{context_run_id}"))
                .body(Body::empty())
                .expect("context request"),
        )
        .await
        .expect("context response");
    assert_eq!(context_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn automation_v2_runs_all_exposes_context_run_links() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let automation = create_test_automation_v2(&state, "auto-v2-all-runs-links").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/automations/v2/runs?limit=10")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let runs = payload.get("runs").and_then(Value::as_array).expect("runs");
    let row = runs
        .iter()
        .find(|candidate| {
            candidate.get("run_id").and_then(Value::as_str) == Some(run.run_id.as_str())
        })
        .expect("matching run");
    let context_run_id = row
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("context run id");
    assert_eq!(
        row.get("linked_context_run_id").and_then(Value::as_str),
        Some(context_run_id)
    );

    let context_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/runs/{context_run_id}"))
                .body(Body::empty())
                .expect("context request"),
        )
        .await
        .expect("context response");
    assert_eq!(context_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn create_automation_v2_run_immediately_creates_context_run() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let automation = create_test_automation_v2(&state, "auto-v2-immediate-context").await;
    let run = state
        .create_automation_v2_run(&automation, "scheduled")
        .await
        .expect("create run");

    let context_run_id = crate::http::context_runs::automation_v2_context_run_id(&run.run_id);
    let context_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/runs/{context_run_id}"))
                .body(Body::empty())
                .expect("context request"),
        )
        .await
        .expect("context response");
    assert_eq!(context_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn automation_v2_run_projects_backlog_tasks_into_context_blackboard() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let automation = crate::AutomationV2Spec {
        automation_id: "auto-v2-backlog-project-1".to_string(),
        name: "Repo Backlog Projection".to_string(),
        description: Some("Project backlog items from planner output".to_string()),
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: vec![crate::AutomationAgentProfile {
            agent_id: "repo-planner".to_string(),
            template_id: None,
            display_name: "Repo Planner".to_string(),
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: vec!["glob".to_string(), "read".to_string(), "write".to_string()],
                denylist: Vec::new(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
                allowed_connections: Vec::new(),
            },
            approval_policy: None,
        }],
        flow: crate::AutomationFlowSpec {
            nodes: vec![crate::AutomationFlowNode {
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                node_id: "plan-backlog-task".to_string(),
                agent_id: "repo-planner".to_string(),
                objective: "Inspect the repository and write the coding backlog plan.".to_string(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: None,
                tool_policy: None,
                mcp_policy: None,
                retry_policy: None,
                timeout_ms: None,
                max_tool_calls: None,
                stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                gate: None,
                wait: None,
                metadata: Some(json!({
                    "builder": {
                        "title": "Plan Backlog Task",
                        "role": "delegator",
                        "output_path": "coding-backlog-plan.md",
                        "task_kind": "repo_plan",
                        "project_backlog_tasks": true,
                        "repo_root": ".",
                        "write_scope": "src, tests",
                        "acceptance_criteria": "Produce the scoped task backlog and verification plan.",
                        "verification_state": "planned",
                        "task_owner": "repo-planner",
                        "verification_command": "cargo test"
                    }
                })),
            }],
        },
        execution: crate::AutomationExecutionPolicy {
            profile: None,
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: vec!["coding-backlog-plan.md".to_string()],
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        metadata: None,
        workspace_root: Some("/tmp".to_string()),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("store automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Completed;
            row.checkpoint.completed_nodes = vec!["plan-backlog-task".to_string()];
            row.checkpoint.node_outputs.insert(
                "plan-backlog-task".to_string(),
                json!({
                    "node_id": "plan-backlog-task",
                    "contract_kind": "plan",
                    "summary": "Projected backlog plan",
                    "status": "completed",
                    "content": {
                        "text": "# Coding Backlog Plan\n\n```json\n{\"backlog_tasks\":[{\"task_id\":\"BACKLOG-101\",\"title\":\"Fix automation artifact restore path\",\"description\":\"Harden restoration so substantive outputs always win.\",\"task_kind\":\"code_change\",\"repo_root\":\".\",\"write_scope\":\"crates/tandem-server/src/lib.rs\",\"acceptance_criteria\":\"Placeholder overwrites are rejected and prior substantive artifact text is restored.\",\"task_dependencies\":[],\"verification_state\":\"pending\",\"task_owner\":\"implementer\",\"verification_command\":\"cargo test -p tandem-server\",\"status\":\"runnable\",\"priority\":2},{\"task_id\":\"BACKLOG-102\",\"title\":\"Add regression coverage for backlog projection\",\"description\":\"Cover planner JSON projection into the context blackboard.\",\"task_kind\":\"verification\",\"repo_root\":\".\",\"write_scope\":\"crates/tandem-server/src/http/tests/global.rs\",\"acceptance_criteria\":\"Blackboard exposes projected backlog items after planner output sync.\",\"task_dependencies\":[\"BACKLOG-101\"],\"verification_state\":\"pending\",\"task_owner\":\"verifier\",\"verification_command\":\"cargo test -p tandem-server automation_v2_run_projects_backlog_tasks_into_context_blackboard -- --nocapture\",\"status\":\"queued\",\"priority\":1}]}\n```",
                        "path": "coding-backlog-plan.md",
                        "raw_assistant_text": "done",
                        "session_id": "sess-plan"
                    }
                }),
            );
        })
        .await
        .expect("update run");
    let updated = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("updated run");
    crate::http::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &updated)
        .await
        .expect("sync blackboard");

    let context_run_id = crate::http::context_runs::automation_v2_context_run_id(&run.run_id);
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
        .and_then(|value| value.get("tasks"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(tasks
        .iter()
        .any(|task| { task.get("id").and_then(Value::as_str) == Some("node-plan-backlog-task") }));
    let projected = tasks
        .iter()
        .filter(|task| {
            task.get("task_type").and_then(Value::as_str) == Some("automation_backlog_item")
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(projected.len(), 2);
    assert!(projected.iter().any(|task| {
        task.get("id").and_then(Value::as_str) == Some("backlog-plan-backlog-task-BACKLOG-101")
            && task
                .get("payload")
                .and_then(|payload| payload.get("write_scope"))
                .and_then(Value::as_str)
                == Some("crates/tandem-server/src/lib.rs")
    }));
    assert!(projected.iter().any(|task| {
        task.get("id").and_then(Value::as_str) == Some("backlog-plan-backlog-task-BACKLOG-102")
            && task
                .get("depends_on_task_ids")
                .and_then(Value::as_array)
                .is_some_and(|deps| {
                    deps.iter()
                        .any(|dep| dep.as_str() == Some("backlog-plan-backlog-task-BACKLOG-101"))
                })
    }));

    let run_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/automations/v2/runs/{}", run.run_id))
                .body(Body::empty())
                .expect("run request"),
        )
        .await
        .expect("run response");
    assert_eq!(run_resp.status(), StatusCode::OK);
    let run_body = to_bytes(run_resp.into_body(), usize::MAX)
        .await
        .expect("run body");
    let run_payload: Value = serde_json::from_slice(&run_body).expect("run json");
    let run_record = run_payload.get("run").expect("run record");
    assert_eq!(
        run_record.get("stored_status").and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        run_record.get("status").and_then(Value::as_str),
        Some("running")
    );
    assert_eq!(
        run_record.get("statusDerivedNote").and_then(Value::as_str),
        Some("derived from projected task board")
    );
}
