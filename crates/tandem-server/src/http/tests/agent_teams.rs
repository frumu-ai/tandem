use super::*;

#[tokio::test]
async fn agent_team_spawn_denied_when_policy_missing() {
    let state = test_state().await;
    let app = app_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/agent-team/spawn")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "role": "worker",
                "source": "ui_action",
                "justification": "need parallel implementation"
            })
            .to_string(),
        ))
        .expect("spawn request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(|v| v.as_str()),
        Some("spawn_policy_missing")
    );
}

#[tokio::test]
async fn agent_team_spawn_approved_with_policy_and_template() {
    let state = test_state().await;
    let workspace_root = state.workspace_index.snapshot().await.root;
    let canonical_repo_root = crate::runtime::worktrees::resolve_git_repo_root(&workspace_root);
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
    let app = app_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/agent-team/spawn")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "missionID": "m1",
                "role": "worker",
                "templateID": "worker-default",
                "source": "ui_action",
                "justification": "implement split test coverage"
            })
            .to_string(),
        ))
        .expect("spawn request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert!(payload.get("instanceID").and_then(|v| v.as_str()).is_some());
    let skill_hash = payload
        .get("skillHash")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(skill_hash.starts_with("sha256:"));
    let managed_worktree = payload
        .get("managedWorktree")
        .and_then(Value::as_object)
        .expect("managed worktree payload");
    assert_eq!(
        payload.get("workspaceRepoRoot").and_then(Value::as_str),
        canonical_repo_root.as_deref()
    );
    assert!(managed_worktree
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .contains("/.tandem/worktrees/"));
}

#[tokio::test]
async fn agent_team_spawn_uses_managed_worktree_and_cancel_cleans_it_up() {
    let state = test_state().await;
    let workspace_root = state.workspace_index.snapshot().await.root;
    state
        .agent_teams
        .set_for_test(
            Some(workspace_root.clone()),
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

    let req = Request::builder()
        .method("POST")
        .uri("/agent-team/spawn")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "missionID": "m-worktree",
                "role": "worker",
                "templateID": "worker-default",
                "source": "ui_action",
                "justification": "need isolated worker workspace"
            })
            .to_string(),
        ))
        .expect("spawn request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let instance_id = payload
        .get("instanceID")
        .and_then(Value::as_str)
        .expect("instance id")
        .to_string();
    let session_id = payload
        .get("sessionID")
        .and_then(Value::as_str)
        .expect("session id")
        .to_string();

    let session = state
        .storage
        .get_session(&session_id)
        .await
        .expect("child session");
    let worker_workspace_root = session.workspace_root.expect("worker workspace root");
    assert!(worker_workspace_root.contains("/.tandem/worktrees/"));
    assert!(std::path::Path::new(&worker_workspace_root).exists());

    let instance = state
        .agent_teams
        .instance_for_session(&session_id)
        .await
        .expect("instance for session");
    let managed_worktree = instance
        .metadata
        .as_ref()
        .and_then(|row| row.get("managedWorktree"))
        .cloned()
        .expect("managed worktree metadata");
    assert_eq!(
        managed_worktree.get("path").and_then(Value::as_str),
        Some(worker_workspace_root.as_str())
    );
    assert_eq!(
        managed_worktree.get("repoRoot").and_then(Value::as_str),
        crate::runtime::worktrees::resolve_git_repo_root(&workspace_root).as_deref()
    );

    let cancel_req = Request::builder()
        .method("POST")
        .uri(format!("/agent-team/instance/{instance_id}/cancel"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reason": "cleanup managed workspace"
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
    assert!(!std::path::Path::new(&worker_workspace_root).exists());
}

#[tokio::test]
async fn agent_team_spawn_agent_tool_uses_same_policy_gate() {
    let state = test_state().await;
    let workspace_root = state.workspace_index.snapshot().await.root;
    state
        .agent_teams
        .set_for_test(
            Some(workspace_root.clone()),
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
    let session = Session::new(Some("spawn tool".to_string()), Some(workspace_root.clone()));
    let session_id = session.id.clone();
    state
        .storage
        .save_session(session)
        .await
        .expect("save session");
    let hook = crate::agent_teams::ServerSpawnAgentHook::new(state.clone());
    let result = tandem_core::SpawnAgentHook::spawn_agent(
        &hook,
        tandem_core::SpawnAgentToolContext {
            session_id: session_id.clone(),
            message_id: "msg-tool-spawn".to_string(),
            tool_call_id: Some("tool-call-1".to_string()),
            args: json!({
                "missionID": "m2",
                "role": "worker",
                "templateID": "worker-default",
                "source": "tool_call",
                "justification": "parallelize task"
            }),
        },
    )
    .await
    .expect("spawn agent hook result");

    assert_eq!(
        result.metadata.get("ok").and_then(Value::as_bool),
        Some(true)
    );
    let child_session_id = result
        .metadata
        .get("sessionID")
        .and_then(Value::as_str)
        .expect("child session id");
    assert_ne!(child_session_id, session_id.as_str());
    assert!(
        state.storage.get_session(child_session_id).await.is_some(),
        "spawn hook should create a child session"
    );
    let managed_worktree = result
        .metadata
        .get("managedWorktree")
        .and_then(Value::as_object)
        .expect("managed worktree metadata");
    assert_eq!(
        managed_worktree.get("repoRoot").and_then(Value::as_str),
        crate::runtime::worktrees::resolve_git_repo_root(&workspace_root).as_deref()
    );
    assert_eq!(
        result
            .metadata
            .get("workspaceRepoRoot")
            .and_then(Value::as_str),
        crate::runtime::worktrees::resolve_git_repo_root(&workspace_root).as_deref()
    );
    assert!(managed_worktree
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .contains("/.tandem/worktrees/"));
}

#[tokio::test]
async fn agent_team_cancel_instance_endpoint_updates_status() {
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

    let spawn_req = Request::builder()
        .method("POST")
        .uri("/agent-team/spawn")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "missionID": "m3",
                "role": "worker",
                "templateID": "worker-default",
                "source": "ui_action",
                "justification": "work chunk"
            })
            .to_string(),
        ))
        .expect("spawn request");
    let spawn_resp = app
        .clone()
        .oneshot(spawn_req)
        .await
        .expect("spawn response");
    assert_eq!(spawn_resp.status(), StatusCode::OK);
    let spawn_body = to_bytes(spawn_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let spawn_payload: Value = serde_json::from_slice(&spawn_body).expect("json");
    let instance_id = spawn_payload
        .get("instanceID")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    assert!(!instance_id.is_empty());

    let cancel_req = Request::builder()
        .method("POST")
        .uri(format!("/agent-team/instance/{instance_id}/cancel"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reason": "manual stop"
            })
            .to_string(),
        ))
        .expect("cancel request");
    let cancel_resp = app.oneshot(cancel_req).await.expect("cancel response");
    assert_eq!(cancel_resp.status(), StatusCode::OK);
    let cancel_body = to_bytes(cancel_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let cancel_payload: Value = serde_json::from_slice(&cancel_body).expect("json");
    assert_eq!(
        cancel_payload.get("status").and_then(|v| v.as_str()),
        Some("cancelled")
    );
}

#[tokio::test]
async fn agent_team_capability_policy_denies_network_tool_by_default() {
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

    let spawn_req = Request::builder()
        .method("POST")
        .uri("/agent-team/spawn")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "missionID": "m4",
                "role": "worker",
                "templateID": "worker-default",
                "source": "ui_action",
                "justification": "run safe task"
            })
            .to_string(),
        ))
        .expect("spawn request");
    let spawn_resp = app
        .clone()
        .oneshot(spawn_req)
        .await
        .expect("spawn response");
    assert_eq!(spawn_resp.status(), StatusCode::OK);
    let spawn_body = to_bytes(spawn_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let spawn_payload: Value = serde_json::from_slice(&spawn_body).expect("json");
    let child_session_id = spawn_payload
        .get("sessionID")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    assert!(!child_session_id.is_empty());

    let prompt_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{child_session_id}/prompt_async"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "parts": [
                    {
                        "type": "text",
                        "text": "/tool websearch {\"query\":\"rust async\"}"
                    }
                ]
            })
            .to_string(),
        ))
        .expect("prompt request");
    let prompt_resp = app
        .clone()
        .oneshot(prompt_req)
        .await
        .expect("prompt response");
    assert_eq!(prompt_resp.status(), StatusCode::NO_CONTENT);

    let denied_event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("event");
            if event.event_type == "agent_team.capability.denied" {
                return event;
            }
        }
    })
    .await
    .expect("capability denied timeout");
    assert_eq!(
        denied_event
            .properties
            .get("sessionID")
            .and_then(|v| v.as_str()),
        Some(child_session_id.as_str())
    );
    assert_eq!(
        denied_event.properties.get("tool").and_then(|v| v.as_str()),
        Some("websearch")
    );
}

#[tokio::test]
async fn agent_team_provider_usage_event_updates_token_usage() {
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
                default_budget: tandem_orchestrator::BudgetLimit {
                    max_tokens: Some(10_000),
                    max_steps: None,
                    max_tool_calls: None,
                    max_duration_ms: None,
                    max_cost_usd: None,
                },
                capabilities: tandem_orchestrator::CapabilitySpec::default(),
            }],
        )
        .await;
    let app = app_router(state.clone());

    let spawn_req = Request::builder()
        .method("POST")
        .uri("/agent-team/spawn")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "missionID": "m5",
                "role": "worker",
                "templateID": "worker-default",
                "source": "ui_action",
                "justification": "usage update test"
            })
            .to_string(),
        ))
        .expect("spawn request");
    let spawn_resp = app
        .clone()
        .oneshot(spawn_req)
        .await
        .expect("spawn response");
    assert_eq!(spawn_resp.status(), StatusCode::OK);
    let spawn_body = to_bytes(spawn_resp.into_body(), usize::MAX)
        .await
        .expect("spawn body");
    let spawn_payload: Value = serde_json::from_slice(&spawn_body).expect("json");
    let session_id = spawn_payload
        .get("sessionID")
        .and_then(|v| v.as_str())
        .expect("session id")
        .to_string();

    let usage_event = EngineEvent::new(
        "provider.usage",
        json!({
            "sessionID": session_id,
            "messageID": "msg-1",
            "promptTokens": 12,
            "completionTokens": 34,
            "totalTokens": 46
        }),
    );
    state
        .agent_teams
        .handle_engine_event(&state, &usage_event)
        .await;

    let list_req = Request::builder()
        .method("GET")
        .uri("/agent-team/instances?missionID=m5")
        .body(Body::empty())
        .expect("list request");
    let list_resp = app.oneshot(list_req).await.expect("list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("json");
    assert_eq!(
        list_payload
            .get("instances")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("metadata"))
            .and_then(|v| v.get("budgetUsage"))
            .and_then(|v| v.get("tokensUsed"))
            .and_then(|v| v.as_u64()),
        Some(46)
    );
}

#[tokio::test]
async fn agent_team_request_only_spawn_surfaces_in_approvals_endpoint() {
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
                    map.insert(
                        tandem_orchestrator::AgentRole::Worker,
                        tandem_orchestrator::RoleSpawnRule {
                            behavior: Some(tandem_orchestrator::SpawnBehavior::RequestOnly),
                            can_spawn: vec![tandem_orchestrator::AgentRole::Tester],
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
            vec![
                tandem_orchestrator::AgentTemplate {
                    template_id: "worker-default".to_string(),
                    display_name: None,
                    avatar_url: None,
                    role: tandem_orchestrator::AgentRole::Worker,
                    system_prompt: None,
                    default_model: None,
                    skills: vec![],
                    default_budget: tandem_orchestrator::BudgetLimit::default(),
                    capabilities: tandem_orchestrator::CapabilitySpec::default(),
                },
                tandem_orchestrator::AgentTemplate {
                    template_id: "tester-default".to_string(),
                    display_name: None,
                    avatar_url: None,
                    role: tandem_orchestrator::AgentRole::Tester,
                    system_prompt: None,
                    default_model: None,
                    skills: vec![],
                    default_budget: tandem_orchestrator::BudgetLimit::default(),
                    capabilities: tandem_orchestrator::CapabilitySpec::default(),
                },
            ],
        )
        .await;
    let app = app_router(state.clone());

    let spawn_worker_req = Request::builder()
        .method("POST")
        .uri("/agent-team/spawn")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "missionID": "m-approval",
                "role": "worker",
                "templateID": "worker-default",
                "source": "ui_action",
                "justification": "primary worker"
            })
            .to_string(),
        ))
        .expect("spawn worker");
    let spawn_worker_resp = app
        .clone()
        .oneshot(spawn_worker_req)
        .await
        .expect("spawn worker response");
    assert_eq!(spawn_worker_resp.status(), StatusCode::OK);
    let worker_body = to_bytes(spawn_worker_resp.into_body(), usize::MAX)
        .await
        .expect("worker body");
    let worker_payload: Value = serde_json::from_slice(&worker_body).expect("worker json");
    let worker_instance_id = worker_payload
        .get("instanceID")
        .and_then(|v| v.as_str())
        .expect("worker instance id")
        .to_string();

    let spawn_tester_req = Request::builder()
        .method("POST")
        .uri("/agent-team/spawn")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "missionID": "m-approval",
                "parentInstanceID": worker_instance_id,
                "role": "tester",
                "templateID": "tester-default",
                "source": "ui_action",
                "justification": "needs approval edge"
            })
            .to_string(),
        ))
        .expect("spawn tester");
    let spawn_tester_resp = app
        .clone()
        .oneshot(spawn_tester_req)
        .await
        .expect("spawn tester response");
    assert_eq!(spawn_tester_resp.status(), StatusCode::FORBIDDEN);
    let tester_body = to_bytes(spawn_tester_resp.into_body(), usize::MAX)
        .await
        .expect("tester body");
    let tester_payload: Value = serde_json::from_slice(&tester_body).expect("tester json");
    assert_eq!(
        tester_payload
            .get("requiresUserApproval")
            .and_then(|v| v.as_bool()),
        Some(true)
    );

    let approvals_req = Request::builder()
        .method("GET")
        .uri("/agent-team/approvals")
        .body(Body::empty())
        .expect("approvals request");
    let approvals_resp = app
        .oneshot(approvals_req)
        .await
        .expect("approvals response");
    assert_eq!(approvals_resp.status(), StatusCode::OK);
    let approvals_body = to_bytes(approvals_resp.into_body(), usize::MAX)
        .await
        .expect("approvals body");
    let approvals_payload: Value = serde_json::from_slice(&approvals_body).expect("approvals json");
    assert_eq!(
        approvals_payload
            .get("spawnApprovals")
            .and_then(|v| v.as_array())
            .map(|v| !v.is_empty()),
        Some(true)
    );
}

#[tokio::test]
async fn agent_team_missions_endpoint_returns_rollup_counts() {
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

    for mission_id in ["m6", "m6", "m7"] {
        let spawn_req = Request::builder()
            .method("POST")
            .uri("/agent-team/spawn")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "missionID": mission_id,
                    "role": "worker",
                    "templateID": "worker-default",
                    "source": "ui_action",
                    "justification": "rollup"
                })
                .to_string(),
            ))
            .expect("spawn request");
        let spawn_resp = app
            .clone()
            .oneshot(spawn_req)
            .await
            .expect("spawn response");
        assert_eq!(spawn_resp.status(), StatusCode::OK);
    }

    let req = Request::builder()
        .method("GET")
        .uri("/agent-team/missions")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(|v| v.as_u64()), Some(2));
    assert_eq!(
        payload
            .get("missions")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("missionID"))
            .and_then(|v| v.as_str()),
        Some("m6")
    );
    assert_eq!(
        payload
            .get("missions")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("instanceCount"))
            .and_then(|v| v.as_u64()),
        Some(2)
    );
}
