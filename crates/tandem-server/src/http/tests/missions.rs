use super::*;

#[tokio::test]
async fn mission_create_and_get_roundtrip() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/mission")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "title": "Ship control center",
                "goal": "Build mission scaffolding",
                "work_items": [
                    {"work_item_id":"w-1","title":"Implement API"}
                ]
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
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("json");
    let mission_id = create_payload
        .get("mission")
        .and_then(|v| v.get("mission_id"))
        .and_then(|v| v.as_str())
        .expect("mission id")
        .to_string();

    let get_req = Request::builder()
        .method("GET")
        .uri(format!("/mission/{mission_id}"))
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("json");
    assert_eq!(
        get_payload
            .get("mission")
            .and_then(|v| v.get("work_items"))
            .and_then(|v| v.as_array())
            .map(|v| v.len()),
        Some(1)
    );
}

#[tokio::test]
async fn mission_created_event_contract_snapshot() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/mission")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "title": "Event contract",
                "goal": "Capture mission.created shape",
                "work_items": [{"work_item_id":"w-1","title":"Task"}]
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
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("json");
    let mission_id = create_payload
        .get("mission")
        .and_then(|v| v.get("mission_id"))
        .and_then(|v| v.as_str())
        .expect("mission_id");

    let event = next_event_of_type(&mut rx, "mission.created").await;
    let snapshot = json!({
        "type": event.event_type,
        "properties": event.properties,
    });
    let expected = json!({
        "type": "mission.created",
        "properties": {
            "missionID": mission_id,
            "workItemCount": 1
        }
    });
    assert_eq!(snapshot, expected);
}

#[tokio::test]
async fn mission_updated_event_contract_snapshot() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/mission")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "title": "Mission update contract",
                "goal": "Capture mission.updated shape",
                "work_items": [{"work_item_id":"w-1","title":"Task"}]
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
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("json");
    let mission_id = create_payload
        .get("mission")
        .and_then(|v| v.get("mission_id"))
        .and_then(|v| v.as_str())
        .expect("mission_id")
        .to_string();

    let apply_req = Request::builder()
        .method("POST")
        .uri(format!("/mission/{mission_id}/event"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "event": {
                    "type": "mission_started",
                    "mission_id": mission_id
                }
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app
        .clone()
        .oneshot(apply_req)
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);

    let event = next_event_of_type(&mut rx, "mission.updated").await;
    let snapshot = json!({
        "type": event.event_type,
        "properties": event.properties,
    });
    let expected = json!({
        "type": "mission.updated",
        "properties": {
            "missionID": mission_id,
            "revision": 2,
            "status": "running",
            "commandCount": 0
        }
    });
    assert_eq!(snapshot, expected);
}

#[tokio::test]
async fn mission_apply_event_moves_item_to_rework_on_reviewer_denial() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/mission")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "title": "Gate flow",
                "goal": "Validate reducer flow",
                "work_items": [{"work_item_id":"w-1","title":"Patch logic"}]
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("create json");
    let mission_id = create_payload
        .get("mission")
        .and_then(|v| v.get("mission_id"))
        .and_then(|v| v.as_str())
        .expect("mission id")
        .to_string();

    let run_finished_req = Request::builder()
        .method("POST")
        .uri(format!("/mission/{mission_id}/event"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "event": {
                    "type": "run_finished",
                    "mission_id": mission_id,
                    "work_item_id": "w-1",
                    "run_id": "run-1",
                    "status": "success"
                }
            })
            .to_string(),
        ))
        .expect("run finished request");
    let run_finished_resp = app
        .clone()
        .oneshot(run_finished_req)
        .await
        .expect("run finished response");
    assert_eq!(run_finished_resp.status(), StatusCode::OK);

    let deny_req = Request::builder()
        .method("POST")
        .uri(format!("/mission/{mission_id}/event"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "event": {
                    "type": "approval_denied",
                    "mission_id": mission_id,
                    "work_item_id": "w-1",
                    "approval_id": "review-1",
                    "reason": "needs revision"
                }
            })
            .to_string(),
        ))
        .expect("deny request");
    let deny_resp = app.clone().oneshot(deny_req).await.expect("deny response");
    assert_eq!(deny_resp.status(), StatusCode::OK);
    let deny_body = to_bytes(deny_resp.into_body(), usize::MAX)
        .await
        .expect("deny body");
    let deny_payload: Value = serde_json::from_slice(&deny_body).expect("deny json");
    assert_eq!(
        deny_payload
            .get("mission")
            .and_then(|v| v.get("work_items"))
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("status"))
            .and_then(|v| v.as_str()),
        Some("rework")
    );
}

#[tokio::test]
async fn agent_standup_compose_builds_workflow_automation_from_templates() {
    let state = test_state().await;
    let workspace_root =
        tandem_core::normalize_workspace_path(&state.workspace_index.snapshot().await.root)
            .expect("normalized workspace root");
    state
        .agent_teams
        .upsert_template(
            &workspace_root,
            tandem_orchestrator::AgentTemplate {
                template_id: "frontend-ui".to_string(),
                display_name: Some("Alice (Frontend UI)".to_string()),
                avatar_url: None,
                role: tandem_orchestrator::AgentRole::Worker,
                system_prompt: Some("You own frontend delivery.".to_string()),
                default_model: Some(json!({
                    "provider_id": "openai",
                    "model_id": "gpt-5-mini"
                })),
                skills: Vec::new(),
                default_budget: tandem_orchestrator::BudgetLimit::default(),
                capabilities: tandem_orchestrator::CapabilitySpec::default(),
            },
        )
        .await
        .expect("template upsert");
    let app = app_router(state.clone());

    let req = Request::builder()
        .method("POST")
        .uri("/agent-standup/compose")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "name": "Daily Engineering Standup",
                "workspace_root": workspace_root,
                "schedule": {
                    "type": "cron",
                    "cron_expression": "0 9 * * *",
                    "timezone": "UTC",
                    "misfire_policy": {
                        "type": "run_once"
                    }
                },
                "participant_template_ids": ["frontend-ui"],
                "report_path_template": "docs/standups/{{date}}.md",
                "model_policy": {
                    "default_model": {
                        "provider_id": "anthropic",
                        "model_id": "claude-sonnet-4"
                    }
                }
            })
            .to_string(),
        ))
        .expect("compose request");
    let resp = app.clone().oneshot(req).await.expect("compose response");
    let status = resp.status();
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("compose body");
    assert_eq!(
        status,
        StatusCode::OK,
        "compose response body: {}",
        String::from_utf8_lossy(&body)
    );
    let payload: Value = serde_json::from_slice(&body).expect("compose json");

    let automation = payload.get("automation").expect("automation");
    assert_eq!(
        automation
            .get("metadata")
            .and_then(|value| value.get("feature"))
            .and_then(Value::as_str),
        Some("agent_standup")
    );
    assert_eq!(
        automation
            .get("metadata")
            .and_then(|value| value.get("standup"))
            .and_then(|value| value.get("participant_template_ids"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        automation
            .get("flow")
            .and_then(|value| value.get("nodes"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(2)
    );
    let nodes = automation
        .get("flow")
        .and_then(|value| value.get("nodes"))
        .and_then(Value::as_array)
        .expect("nodes");
    assert_eq!(
        nodes[0]
            .get("output_contract")
            .and_then(|value| value.get("validator"))
            .and_then(Value::as_str),
        Some("standup_update")
    );
    assert_eq!(
        nodes[0]
            .get("output_contract")
            .and_then(|value| value.get("enforcement"))
            .and_then(|value| value.get("validation_profile"))
            .and_then(Value::as_str),
        Some("standup_update")
    );
    assert!(nodes[0]
        .get("output_contract")
        .and_then(|value| value.get("enforcement"))
        .and_then(|value| value.get("required_tools"))
        .and_then(Value::as_array)
        .is_some_and(|rows| rows.iter().any(|value| value.as_str() == Some("glob"))));
    assert!(nodes[0]
        .get("output_contract")
        .and_then(|value| value.get("enforcement"))
        .and_then(|value| value.get("required_tools"))
        .and_then(Value::as_array)
        .is_some_and(|rows| rows.iter().any(|value| value.as_str() == Some("grep"))));
    assert!(nodes[0]
        .get("output_contract")
        .and_then(|value| value.get("enforcement"))
        .and_then(|value| value.get("required_tools"))
        .and_then(Value::as_array)
        .is_some_and(|rows| rows
            .iter()
            .any(|value| value.as_str() == Some("memory_search"))));
    assert!(nodes[0]
        .get("output_contract")
        .and_then(|value| value.get("enforcement"))
        .and_then(|value| value.get("required_evidence"))
        .and_then(Value::as_array)
        .is_some_and(|rows| rows
            .iter()
            .any(|value| value.as_str() == Some("local_source_reads"))));
    assert_eq!(
        nodes[1]
            .get("output_contract")
            .and_then(|value| value.get("validator"))
            .and_then(Value::as_str),
        Some("generic_artifact")
    );
    assert_eq!(
        automation
            .get("agents")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|value| value.get("template_id"))
            .and_then(Value::as_str),
        Some("frontend-ui")
    );
    assert_eq!(
        automation
            .get("agents")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|value| value.get("model_policy"))
            .and_then(|value| value.get("default_model"))
            .and_then(|value| value.get("provider_id"))
            .and_then(Value::as_str),
        Some("anthropic")
    );
    assert_eq!(
        automation
            .get("agents")
            .and_then(Value::as_array)
            .and_then(|rows| rows.get(1))
            .and_then(|value| value.get("model_policy"))
            .and_then(|value| value.get("default_model"))
            .and_then(|value| value.get("model_id"))
            .and_then(Value::as_str),
        Some("claude-sonnet-4")
    );
    assert!(automation
        .get("flow")
        .and_then(|value| value.get("nodes"))
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(|value| value.get("objective"))
        .and_then(Value::as_str)
        .is_some_and(|value| value.contains("Report your actual progress")));
    assert!(automation
        .get("agents")
        .and_then(Value::as_array)
        .and_then(|rows| rows.get(1))
        .and_then(|value| value.get("tool_policy"))
        .and_then(|value| value.get("allowlist"))
        .and_then(Value::as_array)
        .is_some_and(|rows| rows
            .iter()
            .any(|value| value.as_str() == Some("memory_store"))));
}

#[tokio::test]
async fn agent_standup_compose_uses_global_saved_agents_across_workspaces() {
    let state = test_state().await;
    let default_workspace_root =
        tandem_core::normalize_workspace_path(&state.workspace_index.snapshot().await.root)
            .expect("normalized workspace root");
    state
        .agent_teams
        .upsert_template(
            &default_workspace_root,
            tandem_orchestrator::AgentTemplate {
                template_id: "shared-copywriter".to_string(),
                display_name: Some("Shared Copywriter".to_string()),
                avatar_url: None,
                role: tandem_orchestrator::AgentRole::Worker,
                system_prompt: Some("You own messaging and release notes.".to_string()),
                default_model: None,
                skills: Vec::new(),
                default_budget: tandem_orchestrator::BudgetLimit::default(),
                capabilities: tandem_orchestrator::CapabilitySpec::default(),
            },
        )
        .await
        .expect("template upsert");

    let alternate_workspace =
        std::env::temp_dir().join(format!("tandem-standup-global-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&alternate_workspace).expect("create alternate workspace");
    let alternate_workspace_root = tandem_core::normalize_workspace_path(
        alternate_workspace
            .to_str()
            .expect("alternate workspace root"),
    )
    .expect("normalized alternate workspace root");

    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/agent-standup/compose")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "name": "Shared Agent Standup",
                "workspace_root": alternate_workspace_root,
                "schedule": {
                    "type": "manual",
                    "timezone": "UTC",
                    "misfire_policy": {
                        "type": "run_once"
                    }
                },
                "participant_template_ids": ["shared-copywriter"],
                "report_path_template": "docs/standups/{{date}}.md"
            })
            .to_string(),
        ))
        .expect("compose request");
    let resp = app.clone().oneshot(req).await.expect("compose response");
    let status = resp.status();
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("compose body");
    assert_eq!(
        status,
        StatusCode::OK,
        "compose response body: {}",
        String::from_utf8_lossy(&body)
    );
}

#[tokio::test]
async fn mission_started_triggers_orchestrator_runtime_spawn_for_assigned_agent() {
    let state = test_state().await;
    let workspace_root = state.workspace_index.snapshot().await.root;
    state
        .agent_teams
        .set_for_test(
            Some(workspace_root),
            Some(tandem_orchestrator::SpawnPolicy {
                enabled: true,
                require_justification: true,
                max_agents: Some(20),
                max_concurrent: Some(10),
                child_budget_percent_of_parent_remaining: Some(50),
                spawn_edges: {
                    let mut map = std::collections::HashMap::new();
                    map.insert(
                        tandem_orchestrator::AgentRole::Orchestrator,
                        tandem_orchestrator::RoleSpawnRule {
                            behavior: Some(tandem_orchestrator::SpawnBehavior::Allow),
                            can_spawn: vec![tandem_orchestrator::AgentRole::Worker],
                        },
                    );
                    map
                },
                required_skills: std::collections::HashMap::new(),
                role_defaults: std::collections::HashMap::new(),
                mission_total_budget: None,
                cost_per_1k_tokens_usd: None,
                skill_sources: Default::default(),
            }),
            vec![tandem_orchestrator::AgentTemplate {
                template_id: "worker-default".to_string(),
                display_name: None,
                avatar_url: None,
                role: tandem_orchestrator::AgentRole::Worker,
                system_prompt: Some("You are a worker".to_string()),
                default_model: None,
                skills: vec![],
                default_budget: tandem_orchestrator::BudgetLimit::default(),
                capabilities: tandem_orchestrator::CapabilitySpec::default(),
            }],
        )
        .await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/mission")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "title": "Mission with assigned worker",
                "goal": "exercise orchestrator runtime spawn",
                "work_items": [{
                    "work_item_id":"w-assign-1",
                    "title":"Ship patch",
                    "assigned_agent":"worker"
                }]
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
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("json");
    let mission_id = create_payload
        .get("mission")
        .and_then(|v| v.get("mission_id"))
        .and_then(|v| v.as_str())
        .expect("mission id")
        .to_string();

    let start_req = Request::builder()
        .method("POST")
        .uri(format!("/mission/{mission_id}/event"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "event": {
                    "type": "mission_started",
                    "mission_id": mission_id
                }
            })
            .to_string(),
        ))
        .expect("start request");
    let start_resp = app
        .clone()
        .oneshot(start_req)
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("json");
    assert_eq!(
        start_payload
            .get("orchestratorSpawns")
            .and_then(|v| v.as_array())
            .map(|rows| !rows.is_empty()),
        Some(true)
    );
    assert_eq!(
        start_payload
            .get("orchestratorSpawns")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("ok"))
            .and_then(|v| v.as_bool()),
        Some(true)
    );

    let spawn_event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("event");
            if event.event_type == "agent_team.spawn.approved" {
                return event;
            }
        }
    })
    .await
    .expect("spawn event timeout");
    assert_eq!(
        spawn_event
            .properties
            .get("source")
            .and_then(|v| v.as_str()),
        Some("orchestrator_runtime")
    );
}

#[tokio::test]
async fn mission_total_budget_exhaustion_blocks_followup_spawn() {
    let state = test_state().await;
    let workspace_root = state.workspace_index.snapshot().await.root;
    state
        .agent_teams
        .set_for_test(
            Some(workspace_root),
            Some(tandem_orchestrator::SpawnPolicy {
                enabled: true,
                require_justification: true,
                max_agents: Some(20),
                max_concurrent: Some(10),
                child_budget_percent_of_parent_remaining: Some(50),
                mission_total_budget: Some(tandem_orchestrator::BudgetLimit {
                    max_tokens: Some(40),
                    max_steps: None,
                    max_tool_calls: None,
                    max_duration_ms: None,
                    max_cost_usd: None,
                }),
                cost_per_1k_tokens_usd: None,
                spawn_edges: {
                    let mut map = std::collections::HashMap::new();
                    map.insert(
                        tandem_orchestrator::AgentRole::Orchestrator,
                        tandem_orchestrator::RoleSpawnRule {
                            behavior: Some(tandem_orchestrator::SpawnBehavior::Allow),
                            can_spawn: vec![tandem_orchestrator::AgentRole::Worker],
                        },
                    );
                    map
                },
                required_skills: std::collections::HashMap::new(),
                role_defaults: std::collections::HashMap::new(),
                skill_sources: Default::default(),
            }),
            vec![tandem_orchestrator::AgentTemplate {
                template_id: "worker-default".to_string(),
                display_name: None,
                avatar_url: None,
                role: tandem_orchestrator::AgentRole::Worker,
                system_prompt: None,
                default_model: None,
                skills: vec![],
                default_budget: tandem_orchestrator::BudgetLimit::default(),
                capabilities: tandem_orchestrator::CapabilitySpec::default(),
            }],
        )
        .await;
    let app = app_router(state.clone());

    let spawn_1_req = Request::builder()
        .method("POST")
        .uri("/agent-team/spawn")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "missionID": "m-budget",
                "role": "worker",
                "templateID": "worker-default",
                "source": "ui_action",
                "justification": "initial worker"
            })
            .to_string(),
        ))
        .expect("spawn 1");
    let spawn_1_resp = app
        .clone()
        .oneshot(spawn_1_req)
        .await
        .expect("spawn 1 response");
    assert_eq!(spawn_1_resp.status(), StatusCode::OK);
    let spawn_1_body = to_bytes(spawn_1_resp.into_body(), usize::MAX)
        .await
        .expect("spawn 1 body");
    let spawn_1_payload: Value = serde_json::from_slice(&spawn_1_body).expect("spawn 1 json");
    let session_id = spawn_1_payload
        .get("sessionID")
        .and_then(|v| v.as_str())
        .expect("session id")
        .to_string();

    state
        .agent_teams
        .handle_engine_event(
            &state,
            &EngineEvent::new(
                "provider.usage",
                json!({
                    "sessionID": session_id,
                    "totalTokens": 50
                }),
            ),
        )
        .await;

    let spawn_2_req = Request::builder()
        .method("POST")
        .uri("/agent-team/spawn")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "missionID": "m-budget",
                "role": "worker",
                "templateID": "worker-default",
                "source": "ui_action",
                "justification": "follow-up worker"
            })
            .to_string(),
        ))
        .expect("spawn 2");
    let spawn_2_resp = app
        .clone()
        .oneshot(spawn_2_req)
        .await
        .expect("spawn 2 response");
    assert_eq!(spawn_2_resp.status(), StatusCode::FORBIDDEN);
    let spawn_2_body = to_bytes(spawn_2_resp.into_body(), usize::MAX)
        .await
        .expect("spawn 2 body");
    let spawn_2_payload: Value = serde_json::from_slice(&spawn_2_body).expect("spawn 2 json");
    assert_eq!(
        spawn_2_payload.get("code").and_then(|v| v.as_str()),
        Some("spawn_mission_budget_exceeded")
    );
}

#[tokio::test]
async fn mission_canceled_triggers_orchestrator_runtime_instance_cancellation() {
    let state = test_state().await;
    let workspace_root = state.workspace_index.snapshot().await.root;
    state
        .agent_teams
        .set_for_test(
            Some(workspace_root),
            Some(tandem_orchestrator::SpawnPolicy {
                enabled: true,
                require_justification: true,
                max_agents: Some(20),
                max_concurrent: Some(10),
                child_budget_percent_of_parent_remaining: Some(50),
                spawn_edges: {
                    let mut map = std::collections::HashMap::new();
                    map.insert(
                        tandem_orchestrator::AgentRole::Orchestrator,
                        tandem_orchestrator::RoleSpawnRule {
                            behavior: Some(tandem_orchestrator::SpawnBehavior::Allow),
                            can_spawn: vec![tandem_orchestrator::AgentRole::Worker],
                        },
                    );
                    map
                },
                required_skills: std::collections::HashMap::new(),
                role_defaults: std::collections::HashMap::new(),
                mission_total_budget: None,
                cost_per_1k_tokens_usd: None,
                skill_sources: Default::default(),
            }),
            vec![tandem_orchestrator::AgentTemplate {
                template_id: "worker-default".to_string(),
                display_name: None,
                avatar_url: None,
                role: tandem_orchestrator::AgentRole::Worker,
                system_prompt: Some("You are a worker".to_string()),
                default_model: None,
                skills: vec![],
                default_budget: tandem_orchestrator::BudgetLimit::default(),
                capabilities: tandem_orchestrator::CapabilitySpec::default(),
            }],
        )
        .await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/mission")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "title": "Cancel mission bridge",
                "goal": "validate cancellation propagation",
                "work_items": [{
                    "work_item_id":"w-cancel-1",
                    "title":"Do work",
                    "assigned_agent":"worker"
                }]
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
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("json");
    let mission_id = create_payload
        .get("mission")
        .and_then(|v| v.get("mission_id"))
        .and_then(|v| v.as_str())
        .expect("mission id")
        .to_string();

    let start_req = Request::builder()
        .method("POST")
        .uri(format!("/mission/{mission_id}/event"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "event": {
                    "type": "mission_started",
                    "mission_id": mission_id
                }
            })
            .to_string(),
        ))
        .expect("start request");
    let start_resp = app
        .clone()
        .oneshot(start_req)
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);

    let cancel_req = Request::builder()
        .method("POST")
        .uri(format!("/mission/{mission_id}/event"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "event": {
                    "type": "mission_canceled",
                    "mission_id": mission_id,
                    "reason": "user stop"
                }
            })
            .to_string(),
        ))
        .expect("cancel request");
    let cancel_resp = app
        .clone()
        .oneshot(cancel_req)
        .await
        .expect("cancel response");
    assert_eq!(cancel_resp.status(), StatusCode::OK);
    let cancel_body = to_bytes(cancel_resp.into_body(), usize::MAX)
        .await
        .expect("cancel body");
    let cancel_payload: Value = serde_json::from_slice(&cancel_body).expect("json");
    assert_eq!(
        cancel_payload
            .get("orchestratorCancellations")
            .and_then(|v| v.get("triggered"))
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        cancel_payload
            .get("orchestratorCancellations")
            .and_then(|v| v.get("cancelledInstances"))
            .and_then(|v| v.as_u64()),
        Some(1)
    );

    let instances_req = Request::builder()
        .method("GET")
        .uri(format!("/agent-team/instances?missionID={mission_id}"))
        .body(Body::empty())
        .expect("instances request");
    let instances_resp = app
        .oneshot(instances_req)
        .await
        .expect("instances response");
    assert_eq!(instances_resp.status(), StatusCode::OK);
    let instances_body = to_bytes(instances_resp.into_body(), usize::MAX)
        .await
        .expect("instances body");
    let instances_payload: Value = serde_json::from_slice(&instances_body).expect("json");
    assert_eq!(
        instances_payload
            .get("instances")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("status"))
            .and_then(|v| v.as_str()),
        Some("cancelled")
    );
}
