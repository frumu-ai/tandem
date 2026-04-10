use super::*;
use std::process::Command;

fn init_git_repo() -> std::path::PathBuf {
    let repo_root = std::env::temp_dir().join(format!("tandem-worktree-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&repo_root).expect("create repo dir");
    let status = Command::new("git")
        .args(["init"])
        .current_dir(&repo_root)
        .status()
        .expect("git init");
    assert!(status.success());
    let status = Command::new("git")
        .args(["config", "user.email", "tests@tandem.local"])
        .current_dir(&repo_root)
        .status()
        .expect("git config email");
    assert!(status.success());
    let status = Command::new("git")
        .args(["config", "user.name", "Tandem Tests"])
        .current_dir(&repo_root)
        .status()
        .expect("git config name");
    assert!(status.success());
    std::fs::write(repo_root.join("README.md"), "# test\n").expect("seed readme");
    let status = Command::new("git")
        .args(["add", "README.md"])
        .current_dir(&repo_root)
        .status()
        .expect("git add");
    assert!(status.success());
    let status = Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&repo_root)
        .status()
        .expect("git commit");
    assert!(status.success());
    repo_root
}

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

async fn insert_test_lease(state: &AppState, lease_id: &str) {
    let now = crate::now_ms();
    state.engine_leases.write().await.insert(
        lease_id.to_string(),
        crate::EngineLease {
            lease_id: lease_id.to_string(),
            client_id: "tests".to_string(),
            client_type: "http-test".to_string(),
            acquired_at_ms: now,
            last_renewed_at_ms: now,
            ttl_ms: 60_000,
        },
    );
}

#[tokio::test]
async fn managed_worktree_endpoints_are_idempotent_and_cleanup_branch() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let repo_root = init_git_repo();
    let repo_root_str = repo_root.to_string_lossy().to_string();
    insert_test_lease(&state, "lease-1").await;

    let create_req = Request::builder()
        .method("POST")
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root_str,
                "task_id": "task-a",
                "owner_run_id": "run-1",
                "lease_id": "lease-1",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create worktree response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create worktree body"),
    )
    .expect("create worktree json");
    assert_eq!(
        create_payload.get("ok").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        create_payload.get("managed").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        create_payload.get("reused").and_then(Value::as_bool),
        Some(false)
    );
    let worktree_path = create_payload
        .get("path")
        .and_then(Value::as_str)
        .expect("worktree path")
        .to_string();
    let branch = create_payload
        .get("branch")
        .and_then(Value::as_str)
        .expect("branch")
        .to_string();
    assert!(worktree_path.contains("/.tandem/worktrees/"));
    assert!(std::path::Path::new(&worktree_path).exists());

    let create_again_req = Request::builder()
        .method("POST")
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "task_id": "task-a",
                "owner_run_id": "run-1",
                "lease_id": "lease-1",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree again request");
    let create_again_resp = app
        .clone()
        .oneshot(create_again_req)
        .await
        .expect("create worktree again response");
    assert_eq!(create_again_resp.status(), StatusCode::OK);
    let create_again_payload: Value = serde_json::from_slice(
        &to_bytes(create_again_resp.into_body(), usize::MAX)
            .await
            .expect("create worktree again body"),
    )
    .expect("create worktree again json");
    assert_eq!(
        create_again_payload.get("reused").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        create_again_payload.get("path").and_then(Value::as_str),
        Some(worktree_path.as_str())
    );
    assert_eq!(
        create_again_payload.get("branch").and_then(Value::as_str),
        Some(branch.as_str())
    );

    let list_req = Request::builder()
        .method("GET")
        .uri(format!(
            "/worktree?repo_root={}&managed_only=true",
            repo_root.to_string_lossy()
        ))
        .body(Body::empty())
        .expect("list worktrees request");
    let list_resp = app
        .clone()
        .oneshot(list_req)
        .await
        .expect("list worktrees response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_payload: Value = serde_json::from_slice(
        &to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("list worktrees body"),
    )
    .expect("list worktrees json");
    assert!(list_payload
        .as_array()
        .is_some_and(|rows| rows.iter().any(|row| {
            row.get("path").and_then(Value::as_str) == Some(worktree_path.as_str())
                && row.get("task_id").and_then(Value::as_str) == Some("task-a")
                && row.get("owner_run_id").and_then(Value::as_str) == Some("run-1")
                && row.get("lease_id").and_then(Value::as_str) == Some("lease-1")
                && row.get("managed").and_then(Value::as_bool) == Some(true)
        })));

    let delete_req = Request::builder()
        .method("DELETE")
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "path": worktree_path,
                "lease_id": "lease-1"
            })
            .to_string(),
        ))
        .expect("delete worktree request");
    let delete_resp = app
        .clone()
        .oneshot(delete_req)
        .await
        .expect("delete worktree response");
    assert_eq!(delete_resp.status(), StatusCode::OK);
    let delete_payload: Value = serde_json::from_slice(
        &to_bytes(delete_resp.into_body(), usize::MAX)
            .await
            .expect("delete worktree body"),
    )
    .expect("delete worktree json");
    assert_eq!(
        delete_payload.get("ok").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        delete_payload
            .get("branch_deleted")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(!std::path::Path::new(
        delete_payload
            .get("path")
            .and_then(Value::as_str)
            .expect("deleted path")
    )
    .exists());
    let branch_output = Command::new("git")
        .args(["branch", "--list", &branch])
        .current_dir(&repo_root)
        .output()
        .expect("git branch list");
    assert!(String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .is_empty());

    let _ = std::fs::remove_dir_all(repo_root);
}

#[tokio::test]
async fn managed_worktree_create_rejects_unknown_lease() {
    let state = test_state().await;
    let app = app_router(state);
    let repo_root = init_git_repo();

    let create_req = Request::builder()
        .method("POST")
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "task_id": "task-b",
                "owner_run_id": "run-2",
                "lease_id": "missing-lease",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree request");
    let create_resp = app
        .oneshot(create_req)
        .await
        .expect("create worktree response");
    assert_eq!(create_resp.status(), StatusCode::CONFLICT);

    let _ = std::fs::remove_dir_all(repo_root);
}

#[tokio::test]
async fn managed_worktree_create_rejects_external_path_override() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let repo_root = init_git_repo();
    insert_test_lease(&state, "lease-path-boundary").await;
    let external_path = std::env::temp_dir().join(format!("tandem-external-{}", Uuid::new_v4()));

    let create_req = Request::builder()
        .method("POST")
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "path": external_path.to_string_lossy(),
                "task_id": "task-path",
                "owner_run_id": "run-path",
                "lease_id": "lease-path-boundary",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree request");
    let create_resp = app
        .oneshot(create_req)
        .await
        .expect("create worktree response");
    assert_eq!(create_resp.status(), StatusCode::CONFLICT);
    assert!(!external_path.exists());

    let _ = std::fs::remove_dir_all(repo_root);
}

#[tokio::test]
async fn managed_worktree_mutations_require_matching_active_lease() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let repo_root = init_git_repo();
    insert_test_lease(&state, "lease-1").await;

    let create_req = Request::builder()
        .method("POST")
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "task_id": "task-c",
                "owner_run_id": "run-3",
                "lease_id": "lease-1",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create worktree response");
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create worktree body"),
    )
    .expect("create worktree json");
    let worktree_path = create_payload
        .get("path")
        .and_then(Value::as_str)
        .expect("worktree path")
        .to_string();

    let reset_without_lease = Request::builder()
        .method("POST")
        .uri("/worktree/reset")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "path": worktree_path
            })
            .to_string(),
        ))
        .expect("reset worktree request");
    let reset_resp = app
        .clone()
        .oneshot(reset_without_lease)
        .await
        .expect("reset worktree response");
    assert_eq!(reset_resp.status(), StatusCode::CONFLICT);

    let delete_wrong_lease = Request::builder()
        .method("DELETE")
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "path": worktree_path,
                "lease_id": "lease-other"
            })
            .to_string(),
        ))
        .expect("delete wrong lease request");
    let delete_wrong_resp = app
        .clone()
        .oneshot(delete_wrong_lease)
        .await
        .expect("delete wrong lease response");
    assert_eq!(delete_wrong_resp.status(), StatusCode::CONFLICT);

    let delete_req = Request::builder()
        .method("DELETE")
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "path": worktree_path,
                "lease_id": "lease-1"
            })
            .to_string(),
        ))
        .expect("delete worktree request");
    let delete_resp = app
        .clone()
        .oneshot(delete_req)
        .await
        .expect("delete worktree response");
    assert_eq!(delete_resp.status(), StatusCode::OK);

    let _ = std::fs::remove_dir_all(repo_root);
}

#[tokio::test]
async fn releasing_lease_cleans_up_managed_worktrees() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let repo_root = init_git_repo();
    insert_test_lease(&state, "lease-cleanup").await;

    let create_req = Request::builder()
        .method("POST")
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "task_id": "task-d",
                "owner_run_id": "run-4",
                "lease_id": "lease-cleanup",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create worktree response");
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create worktree body"),
    )
    .expect("create worktree json");
    let worktree_path = create_payload
        .get("path")
        .and_then(Value::as_str)
        .expect("worktree path")
        .to_string();
    let branch = create_payload
        .get("branch")
        .and_then(Value::as_str)
        .expect("branch")
        .to_string();

    let release_req = Request::builder()
        .method("POST")
        .uri("/global/lease/release")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "lease_id": "lease-cleanup" }).to_string(),
        ))
        .expect("release request");
    let release_resp = app
        .clone()
        .oneshot(release_req)
        .await
        .expect("release response");
    assert_eq!(release_resp.status(), StatusCode::OK);
    let release_payload: Value = serde_json::from_slice(
        &to_bytes(release_resp.into_body(), usize::MAX)
            .await
            .expect("release body"),
    )
    .expect("release json");
    assert_eq!(
        release_payload.get("ok").and_then(Value::as_bool),
        Some(true)
    );
    assert!(release_payload
        .get("released_worktrees")
        .and_then(Value::as_array)
        .is_some_and(|rows| rows
            .iter()
            .any(|row| row.as_str() == Some(worktree_path.as_str()))));
    assert!(!std::path::Path::new(&worktree_path).exists());

    let branch_output = Command::new("git")
        .args(["branch", "--list", &branch])
        .current_dir(&repo_root)
        .output()
        .expect("git branch list");
    assert!(String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .is_empty());

    let _ = std::fs::remove_dir_all(repo_root);
}

#[tokio::test]
async fn expired_leases_are_pruned_and_cleanup_managed_worktrees() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let repo_root = init_git_repo();
    let now = crate::now_ms();
    state.engine_leases.write().await.insert(
        "lease-expired".to_string(),
        crate::EngineLease {
            lease_id: "lease-expired".to_string(),
            client_id: "tests".to_string(),
            client_type: "http-test".to_string(),
            acquired_at_ms: now.saturating_sub(120_000),
            last_renewed_at_ms: now.saturating_sub(120_000),
            ttl_ms: 5_000,
        },
    );

    let create_req = Request::builder()
        .method("POST")
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "task_id": "task-e",
                "owner_run_id": "run-5",
                "lease_id": "lease-expired",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create worktree response");
    assert_eq!(create_resp.status(), StatusCode::CONFLICT);

    insert_test_lease(&state, "lease-fresh").await;
    let create_req = Request::builder()
        .method("POST")
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "task_id": "task-f",
                "owner_run_id": "run-6",
                "lease_id": "lease-fresh",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create worktree response");
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create worktree body"),
    )
    .expect("create worktree json");
    let worktree_path = create_payload
        .get("path")
        .and_then(Value::as_str)
        .expect("worktree path")
        .to_string();
    let branch = create_payload
        .get("branch")
        .and_then(Value::as_str)
        .expect("branch")
        .to_string();

    {
        let mut leases = state.engine_leases.write().await;
        let lease = leases.get_mut("lease-fresh").expect("fresh lease present");
        lease.last_renewed_at_ms = now.saturating_sub(120_000);
        lease.ttl_ms = 5_000;
    }

    let health_req = Request::builder()
        .method("GET")
        .uri("/global/health")
        .body(Body::empty())
        .expect("health request");
    let health_resp = app
        .clone()
        .oneshot(health_req)
        .await
        .expect("health response");
    assert_eq!(health_resp.status(), StatusCode::OK);
    assert!(!std::path::Path::new(&worktree_path).exists());
    let branch_output = Command::new("git")
        .args(["branch", "--list", &branch])
        .current_dir(&repo_root)
        .output()
        .expect("git branch list");
    assert!(String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .is_empty());

    let _ = std::fs::remove_dir_all(repo_root);
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
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
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
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
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
    assert!(payload.get("build_id").and_then(|v| v.as_str()).is_some());
    assert!(payload
        .get("binary_path")
        .and_then(|v| v.as_str())
        .is_some());
    assert!(payload
        .get("binary_modified_at_ms")
        .and_then(|v| v.as_u64())
        .is_some());
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
async fn skills_compile_pack_builder_recipe_emits_automation_preview() {
    let state = test_state().await;
    let app = app_router(state);

    let install_req = Request::builder()
        .method("POST")
        .uri("/skills/generate/install")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "location":"global",
                "artifacts":{
                    "SKILL.md":"---\nname: recipe-compiler-test\ndescription: Recipe compiler test skill.\nversion: 0.1.0\n---\n\n# Skill: recipe compiler test\n\n## Purpose\nCompile into automation preview.\n\n## Inputs\n- user prompt\n\n## Agents\n- worker\n\n## Tools\n- webfetch\n\n## Workflow\n1. Interpret user intent\n2. Execute workflow steps\n3. Return result\n\n## Outputs\n- completed task result\n\n## Schedule compatibility\n- manual\n",
                    "workflow.yaml":"kind: pack_builder_recipe\nskill_id: recipe-compiler-test\nexecution_mode: team\ngoal_template: \"Research '{{query}}' and produce a cited report.\"\n"
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
                "skill_name":"recipe-compiler-test",
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
        Some("recipe-compiler-test")
    );
    assert_eq!(
        automation_preview
            .pointer("/metadata/skill_workflow_kind")
            .and_then(Value::as_str),
        Some("pack_builder_recipe")
    );
    assert_eq!(
        automation_preview
            .pointer("/metadata/operator_preferences/execution_mode")
            .and_then(Value::as_str),
        Some("team")
    );
    assert_eq!(
        automation_preview
            .pointer("/agents/0/skills/0")
            .and_then(Value::as_str),
        Some("recipe-compiler-test")
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
                retry_policy: None,
                timeout_ms: None,
                stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                gate: None,
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
}

#[tokio::test]
async fn automation_v2_backlog_task_claim_and_requeue_routes_work() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let automation = crate::AutomationV2Spec {
        automation_id: "auto-v2-backlog-actions-1".to_string(),
        name: "Repo Backlog Actions".to_string(),
        description: Some("Claim and requeue projected backlog tasks".to_string()),
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
                retry_policy: None,
                timeout_ms: None,
                stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                gate: None,
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
                        "text": "# Coding Backlog Plan\n\n```json\n{\"backlog_tasks\":[{\"task_id\":\"BACKLOG-201\",\"title\":\"Claim me\",\"description\":\"Projected task.\",\"task_kind\":\"code_change\",\"repo_root\":\".\",\"write_scope\":\"src/lib.rs\",\"acceptance_criteria\":\"Claim and requeue works.\",\"task_dependencies\":[],\"verification_state\":\"pending\",\"task_owner\":\"repo-implementer\",\"verification_command\":\"cargo test\",\"status\":\"runnable\",\"priority\":3}]}\n```",
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

    let claim_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/automations/v2/runs/{}/backlog/tasks/backlog-plan-backlog-task-BACKLOG-201/claim",
            run.run_id
        ))
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("claim request");
    let claim_resp = app
        .clone()
        .oneshot(claim_req)
        .await
        .expect("claim response");
    assert_eq!(claim_resp.status(), StatusCode::OK);
    let claim_body = to_bytes(claim_resp.into_body(), usize::MAX)
        .await
        .expect("claim body");
    let claim_payload: Value = serde_json::from_slice(&claim_body).expect("claim json");
    let context_run_id = claim_payload
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("claim context run id");
    assert_eq!(
        claim_payload
            .get("linked_context_run_id")
            .and_then(Value::as_str),
        Some(context_run_id)
    );
    assert_eq!(
        claim_payload.get("agent_id").and_then(Value::as_str),
        Some("repo-implementer")
    );
    assert_eq!(
        claim_payload
            .get("task")
            .and_then(|task| task.get("status"))
            .and_then(Value::as_str),
        Some("in_progress")
    );

    let requeue_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/automations/v2/runs/{}/backlog/tasks/backlog-plan-backlog-task-BACKLOG-201/requeue",
            run.run_id
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "reason": "manual backlog requeue from test" }).to_string(),
        ))
        .expect("requeue request");
    let requeue_resp = app
        .clone()
        .oneshot(requeue_req)
        .await
        .expect("requeue response");
    assert_eq!(requeue_resp.status(), StatusCode::OK);
    let requeue_body = to_bytes(requeue_resp.into_body(), usize::MAX)
        .await
        .expect("requeue body");
    let requeue_payload: Value = serde_json::from_slice(&requeue_body).expect("requeue json");
    assert_eq!(
        requeue_payload.get("contextRunID").and_then(Value::as_str),
        Some(context_run_id)
    );
    assert_eq!(
        requeue_payload
            .get("linked_context_run_id")
            .and_then(Value::as_str),
        Some(context_run_id)
    );
    assert_eq!(
        requeue_payload
            .get("task")
            .and_then(|task| task.get("status"))
            .and_then(Value::as_str),
        Some("runnable")
    );
    assert_eq!(
        requeue_payload
            .get("task")
            .and_then(|task| task.get("last_error"))
            .and_then(Value::as_str),
        Some("manual backlog requeue from test")
    );
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
async fn automations_v2_create_rejects_revoked_shared_context_pack() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let workspace_root = std::env::temp_dir()
        .join(format!(
            "tandem-automation-v2-shared-context-{}",
            Uuid::new_v4()
        ))
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(&workspace_root).expect("workspace root");

    let publish_req = Request::builder()
        .method("POST")
        .uri("/context/packs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "title": "Shared automation context",
                "workspace_root": workspace_root,
                "project_key": "project-a",
                "source_plan_id": "plan-shared",
                "plan_package": {
                    "plan_id": "plan-shared",
                    "title": "Shared Plan",
                    "context_objects": []
                },
                "approved_plan_materialization": {
                    "plan_id": "plan-shared",
                    "plan_revision": 1
                },
                "runtime_context": { "routines": [] }
            })
            .to_string(),
        ))
        .expect("publish request");
    let publish_resp = app
        .clone()
        .oneshot(publish_req)
        .await
        .expect("publish response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_body = to_bytes(publish_resp.into_body(), usize::MAX)
        .await
        .expect("publish body");
    let publish_payload: Value = serde_json::from_slice(&publish_body).expect("publish json");
    let pack_id = publish_payload
        .get("context_pack")
        .and_then(|value| value.get("pack_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .expect("pack id");

    let revoke_req = Request::builder()
        .method("POST")
        .uri(format!("/context/packs/{pack_id}/revoke"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "actor_metadata": { "source": "test" } }).to_string(),
        ))
        .expect("revoke request");
    let revoke_resp = app
        .clone()
        .oneshot(revoke_req)
        .await
        .expect("revoke response");
    assert_eq!(revoke_resp.status(), StatusCode::OK);

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "automation_id": "auto-v2-shared-context",
                "name": "Shared context automation",
                "status": "draft",
                "workspace_root": workspace_root,
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
                            "objective": "Use shared context",
                            "depends_on": []
                        }
                    ]
                },
                "metadata": {
                    "shared_context_bindings": [
                        { "pack_id": pack_id, "required": true }
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
    assert_eq!(create_resp.status(), StatusCode::CONFLICT);
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("create json");
    assert_eq!(
        create_payload.get("code").and_then(Value::as_str),
        Some("AUTOMATION_V2_SHARED_CONTEXT_PACK_INVALID")
    );
}

#[tokio::test]
async fn automations_v2_create_rejects_shared_context_pack_workspace_mismatch() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let workspace_root = std::env::temp_dir()
        .join(format!(
            "tandem-automation-v2-shared-context-workspace-{}",
            Uuid::new_v4()
        ))
        .to_string_lossy()
        .to_string();
    let other_workspace_root = std::env::temp_dir()
        .join(format!(
            "tandem-automation-v2-shared-context-other-workspace-{}",
            Uuid::new_v4()
        ))
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(&workspace_root).expect("workspace root");
    std::fs::create_dir_all(&other_workspace_root).expect("other workspace root");

    let publish_req = Request::builder()
        .method("POST")
        .uri("/context/packs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "title": "Workspace scoped shared context",
                "workspace_root": workspace_root,
                "project_key": "project-a",
                "source_plan_id": "plan-workspace",
                "plan_package": {
                    "plan_id": "plan-workspace",
                    "title": "Workspace Plan",
                    "context_objects": []
                },
                "approved_plan_materialization": {
                    "plan_id": "plan-workspace",
                    "plan_revision": 1
                },
                "runtime_context": { "routines": [] }
            })
            .to_string(),
        ))
        .expect("publish request");
    let publish_resp = app
        .clone()
        .oneshot(publish_req)
        .await
        .expect("publish response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_body = to_bytes(publish_resp.into_body(), usize::MAX)
        .await
        .expect("publish body");
    let publish_payload: Value = serde_json::from_slice(&publish_body).expect("publish json");
    let pack_id = publish_payload
        .get("context_pack")
        .and_then(|value| value.get("pack_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .expect("pack id");

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "automation_id": "auto-v2-shared-context-workspace-mismatch",
                "name": "Workspace mismatch automation",
                "status": "draft",
                "workspace_root": other_workspace_root,
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
                            "objective": "Use shared context",
                            "depends_on": []
                        }
                    ]
                },
                "metadata": {
                    "shared_context_bindings": [
                        { "pack_id": pack_id, "required": true }
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
    assert_eq!(create_resp.status(), StatusCode::FORBIDDEN);
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("create json");
    assert_eq!(
        create_payload.get("code").and_then(Value::as_str),
        Some("AUTOMATION_V2_SHARED_CONTEXT_PACK_SCOPE_MISMATCH")
    );
}

#[tokio::test]
async fn automations_v2_create_rejects_shared_context_pack_project_mismatch() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let workspace_root = std::env::temp_dir()
        .join(format!(
            "tandem-automation-v2-shared-context-project-{}",
            Uuid::new_v4()
        ))
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(&workspace_root).expect("workspace root");

    let publish_req = Request::builder()
        .method("POST")
        .uri("/context/packs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "title": "Project scoped shared context",
                "workspace_root": workspace_root,
                "project_key": "project-a",
                "source_plan_id": "plan-project",
                "plan_package": {
                    "plan_id": "plan-project",
                    "title": "Project Plan",
                    "context_objects": []
                },
                "approved_plan_materialization": {
                    "plan_id": "plan-project",
                    "plan_revision": 1
                },
                "runtime_context": { "routines": [] }
            })
            .to_string(),
        ))
        .expect("publish request");
    let publish_resp = app
        .clone()
        .oneshot(publish_req)
        .await
        .expect("publish response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_body = to_bytes(publish_resp.into_body(), usize::MAX)
        .await
        .expect("publish body");
    let publish_payload: Value = serde_json::from_slice(&publish_body).expect("publish json");
    let pack_id = publish_payload
        .get("context_pack")
        .and_then(|value| value.get("pack_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .expect("pack id");

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "automation_id": "auto-v2-shared-context-project-mismatch",
                "name": "Project mismatch automation",
                "status": "draft",
                "workspace_root": workspace_root,
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
                            "objective": "Use shared context",
                            "depends_on": []
                        }
                    ]
                },
                "metadata": {
                    "shared_context_project_key": "project-b",
                    "shared_context_bindings": [
                        { "pack_id": pack_id, "required": true }
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
    assert_eq!(create_resp.status(), StatusCode::FORBIDDEN);
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("create json");
    assert_eq!(
        create_payload.get("code").and_then(Value::as_str),
        Some("AUTOMATION_V2_SHARED_CONTEXT_PACK_SCOPE_MISMATCH")
    );
}

#[tokio::test]
async fn automations_v2_create_allows_shared_context_pack_project_allowlist() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let workspace_root = std::env::temp_dir()
        .join(format!(
            "tandem-automation-v2-shared-context-allowlist-{}",
            Uuid::new_v4()
        ))
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(&workspace_root).expect("workspace root");

    let publish_req = Request::builder()
        .method("POST")
        .uri("/context/packs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "title": "Allowlisted shared context",
                "workspace_root": workspace_root,
                "project_key": "project-a",
                "allowed_project_keys": ["project-b"],
                "source_plan_id": "plan-allowlisted",
                "plan_package": {
                    "plan_id": "plan-allowlisted",
                    "title": "Allowlisted Plan",
                    "context_objects": []
                },
                "approved_plan_materialization": {
                    "plan_id": "plan-allowlisted",
                    "plan_revision": 1
                },
                "runtime_context": { "routines": [] }
            })
            .to_string(),
        ))
        .expect("publish request");
    let publish_resp = app
        .clone()
        .oneshot(publish_req)
        .await
        .expect("publish response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_body = to_bytes(publish_resp.into_body(), usize::MAX)
        .await
        .expect("publish body");
    let publish_payload: Value = serde_json::from_slice(&publish_body).expect("publish json");
    let pack_id = publish_payload
        .get("context_pack")
        .and_then(|value| value.get("pack_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .expect("pack id");

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations/v2")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "automation_id": "auto-v2-shared-context-allowlisted",
                "name": "Allowlisted shared context automation",
                "status": "draft",
                "workspace_root": workspace_root,
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
                            "objective": "Use shared context",
                            "depends_on": []
                        }
                    ]
                },
                "metadata": {
                    "shared_context_project_key": "project-b",
                    "shared_context_bindings": [
                        { "pack_id": pack_id, "required": true }
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
            row.checkpoint.node_attempts.insert("draft".to_string(), 2);
            row.active_session_ids = vec!["session-a".to_string()];
            row.latest_session_id = Some("session-a".to_string());
            row.active_instance_ids = vec!["instance-a".to_string()];
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
            row.checkpoint.node_attempts.insert("draft".to_string(), 2);
            row.active_session_ids = vec!["session-a".to_string()];
            row.latest_session_id = Some("session-a".to_string());
            row.active_instance_ids = vec!["instance-a".to_string()];
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let context_run_id = payload
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("context run id");
    assert_eq!(
        payload.get("linked_context_run_id").and_then(Value::as_str),
        Some(context_run_id)
    );
    assert_eq!(
        payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(context_run_id)
    );

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
    assert_eq!(
        recovered.checkpoint.blocked_nodes,
        vec!["approval".to_string()]
    );
    assert_eq!(recovered.checkpoint.node_attempts.get("draft"), Some(&2));
    assert!(recovered.checkpoint.node_attempts.get("review").is_none());
    assert!(recovered.active_session_ids.is_empty());
    assert!(recovered.active_instance_ids.is_empty());
    assert!(recovered.latest_session_id.is_none());
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
async fn automations_v2_run_recover_from_stale_pause_clears_pending_outputs_and_attempts() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-stale-pause-recover").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Paused;
            row.pause_reason = Some("stale_no_provider_activity".to_string());
            row.checkpoint.completed_nodes = vec!["draft".to_string()];
            row.checkpoint.pending_nodes = vec!["review".to_string(), "approval".to_string()];
            row.checkpoint
                .node_outputs
                .insert("draft".to_string(), json!({"summary":"draft"}));
            row.checkpoint.node_outputs.insert(
                "review".to_string(),
                json!({
                    "status": "needs_repair",
                    "blocked_reason": "node execution stalled after no provider activity for at least 300s"
                }),
            );
            row.checkpoint.node_attempts.insert("draft".to_string(), 2);
            row.checkpoint.node_attempts.insert("review".to_string(), 2);
            row.checkpoint.node_attempts.insert("approval".to_string(), 1);
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
                    json!({ "reason": "recover stale pause" }).to_string(),
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
    assert!(recovered.checkpoint.node_outputs.contains_key("draft"));
    assert!(!recovered.checkpoint.node_outputs.contains_key("review"));
    assert!(recovered.checkpoint.node_attempts.get("draft") == Some(&2));
    assert!(recovered.checkpoint.node_attempts.get("review").is_none());
    assert!(recovered.checkpoint.node_attempts.get("approval").is_none());
}

#[tokio::test]
async fn automations_v2_run_recover_allows_completed_runs_with_blocked_node_evidence() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-completed-blocked-recover").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Completed;
            row.finished_at_ms = Some(crate::now_ms());
            row.checkpoint.completed_nodes = vec!["draft".to_string(), "review".to_string()];
            row.checkpoint.pending_nodes = vec!["approval".to_string()];
            row.checkpoint.node_outputs.insert(
                "draft".to_string(),
                json!({"status":"completed","summary":"draft"}),
            );
            row.checkpoint.node_outputs.insert(
                "review".to_string(),
                json!({"status":"blocked","summary":"review blocked"}),
            );
            row.checkpoint.node_attempts.insert("review".to_string(), 2);
            row.active_session_ids = vec!["session-a".to_string()];
            row.latest_session_id = Some("session-a".to_string());
            row.active_instance_ids = vec!["instance-a".to_string()];
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
                    json!({ "reason": "retry completed run with blocked node evidence" })
                        .to_string(),
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
        .any(|node_id| node_id == "draft"));
    assert!(!recovered
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "review"));
    assert!(recovered.checkpoint.node_outputs.contains_key("draft"));
    assert!(!recovered.checkpoint.node_outputs.contains_key("review"));
    assert!(recovered.checkpoint.node_attempts.get("review").is_none());
    assert!(recovered
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "review"));
    assert!(recovered.active_session_ids.is_empty());
    assert!(recovered.active_instance_ids.is_empty());
    assert!(recovered.latest_session_id.is_none());
    assert_eq!(
        recovered.resume_reason.as_deref(),
        Some("retry completed run with blocked node evidence")
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
    let _ = state
        .add_automation_v2_session(&run.run_id, "session-a")
        .await;
    let _ = state
        .add_automation_v2_session(&run.run_id, "session-b")
        .await;
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let context_run_id = payload
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("context run id");
    assert_eq!(
        payload.get("linked_context_run_id").and_then(Value::as_str),
        Some(context_run_id)
    );
    assert_eq!(
        payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(context_run_id)
    );

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
    state
        .apply_provider_usage_to_runs("session-a", 10, 20, 30)
        .await;
    let after_usage = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after late usage");
    assert_eq!(after_usage.total_tokens, 0);
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
async fn automations_v2_run_pause_clears_active_sessions_and_instances() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-pause-active-cleanup").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    let _ = state
        .add_automation_v2_session(&run.run_id, "session-a")
        .await;
    let _ = state
        .add_automation_v2_session(&run.run_id, "session-b")
        .await;
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Running;
            row.active_session_ids = vec!["session-a".to_string(), "session-b".to_string()];
            row.active_instance_ids = vec!["instance-a".to_string(), "instance-b".to_string()];
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/pause", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "reason": "pause for operator checkpoint" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let context_run_id = payload
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("context run id");
    assert_eq!(
        payload.get("linked_context_run_id").and_then(Value::as_str),
        Some(context_run_id)
    );
    assert_eq!(
        payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(context_run_id)
    );

    let paused = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("paused run");
    assert_eq!(paused.status, crate::AutomationRunStatus::Paused);
    assert!(paused.active_session_ids.is_empty());
    assert!(paused.active_instance_ids.is_empty());
    let pause_event = paused
        .checkpoint
        .lifecycle_history
        .iter()
        .find(|entry| entry.event == "run_paused")
        .expect("run paused event");
    assert_eq!(
        pause_event.reason.as_deref(),
        Some("pause for operator checkpoint")
    );
    state
        .apply_provider_usage_to_runs("session-a", 10, 20, 30)
        .await;
    let after_usage = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after late usage");
    assert_eq!(after_usage.total_tokens, 0);
}

#[tokio::test]
async fn automations_v2_pause_clears_active_state_for_running_runs() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-automation-pause").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Running;
            row.active_session_ids = vec!["session-a".to_string()];
            row.active_instance_ids = vec!["instance-a".to_string()];
            row.pause_reason = None;
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/automations/v2/auto-v2-automation-pause/pause")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "reason": "pause automation and all active runs" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let stored = state
        .get_automation_v2("auto-v2-automation-pause")
        .await
        .expect("stored automation");
    assert_eq!(stored.status, crate::AutomationV2Status::Paused);

    let paused_run = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("paused run");
    assert_eq!(paused_run.status, crate::AutomationRunStatus::Paused);
    assert_eq!(
        paused_run.pause_reason.as_deref(),
        Some("pause automation and all active runs")
    );
    assert!(paused_run.active_session_ids.is_empty());
    assert!(paused_run.active_instance_ids.is_empty());
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
            row.checkpoint.node_attempts.insert("draft".to_string(), 2);
            row.active_session_ids = vec!["session-a".to_string()];
            row.latest_session_id = Some("session-a".to_string());
            row.active_instance_ids = vec!["instance-a".to_string()];
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
    assert!(recovered.checkpoint.node_attempts.get("draft").is_none());
    assert!(recovered.active_session_ids.is_empty());
    assert!(recovered.active_instance_ids.is_empty());
    assert!(recovered.latest_session_id.is_none());
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
async fn automations_v2_run_recover_uses_failed_node_outputs_when_last_failure_missing() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_branched_test_automation_v2(&state, "auto-v2-derive-failure").await;
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
            row.checkpoint.node_outputs.insert(
                "draft".to_string(),
                json!({
                    "status": "verify_failed",
                    "failure_kind": "verification_failed",
                    "summary": "draft failed verification"
                }),
            );
            row.checkpoint.last_failure = None;
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
                    json!({ "reason": "recover from derived failed node" }).to_string(),
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
            row.active_session_ids = vec!["session-a".to_string()];
            row.latest_session_id = Some("session-a".to_string());
            row.active_instance_ids = vec!["instance-a".to_string()];
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
    assert_eq!(
        recovered.checkpoint.blocked_nodes,
        vec!["publish".to_string()]
    );
    assert_eq!(
        recovered.resume_reason.as_deref(),
        Some("continue branched mission after pause")
    );
    assert!(recovered.active_session_ids.is_empty());
    assert!(recovered.active_instance_ids.is_empty());
    assert!(recovered.latest_session_id.is_none());
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
async fn automations_v2_gate_rework_on_failed_branch_preserves_completed_sibling_branch() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_branched_test_automation_v2(&state, "auto-v2-branch-gate-rework").await;
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
            });
            row.checkpoint
                .node_outputs
                .insert("research".to_string(), json!({"summary":"research"}));
            row.checkpoint
                .node_outputs
                .insert("analysis".to_string(), json!({"summary":"analysis"}));
            row.checkpoint
                .node_outputs
                .insert("draft".to_string(), json!({"summary":"draft"}));
            row.checkpoint.blocked_nodes = vec!["publish".to_string()];
            row.checkpoint.node_attempts.insert("draft".to_string(), 2);
            row.active_session_ids = vec!["session-a".to_string()];
            row.latest_session_id = Some("session-a".to_string());
            row.active_instance_ids = vec!["instance-a".to_string()];
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/gate", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "decision": "rework", "reason": "redo only the draft branch" })
                        .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let updated = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after gate rework");
    assert_eq!(updated.status, crate::AutomationRunStatus::Queued);
    assert!(updated
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "research"));
    assert!(updated
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "analysis"));
    assert!(!updated
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "draft"));
    assert!(!updated
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "publish"));
    assert!(updated.checkpoint.node_outputs.contains_key("research"));
    assert!(updated.checkpoint.node_outputs.contains_key("analysis"));
    assert!(!updated.checkpoint.node_outputs.contains_key("draft"));
    assert!(!updated.checkpoint.node_outputs.contains_key("publish"));
    assert!(updated
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "draft"));
    assert!(updated
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "publish"));
    assert_eq!(
        updated.checkpoint.blocked_nodes,
        vec!["publish".to_string()]
    );
    assert!(!updated
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "analysis"));
    assert!(updated.checkpoint.awaiting_gate.is_none());
    assert!(updated.checkpoint.node_attempts.get("draft").is_none());
    assert!(updated.active_session_ids.is_empty());
    assert!(updated.active_instance_ids.is_empty());
    assert!(updated.latest_session_id.is_none());
    let gate_event = updated
        .checkpoint
        .gate_history
        .iter()
        .find(|entry| entry.decision == "rework")
        .expect("gate rework event");
    assert_eq!(
        gate_event.reason.as_deref(),
        Some("redo only the draft branch")
    );
}

#[tokio::test]
async fn automations_v2_run_repair_preserves_completed_sibling_branch() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_branched_test_automation_v2(&state, "auto-v2-branch-repair").await;
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
            row.checkpoint.node_attempts.insert("draft".to_string(), 2);
            row.active_session_ids = vec!["session-a".to_string()];
            row.latest_session_id = Some("session-a".to_string());
            row.active_instance_ids = vec!["instance-a".to_string()];
            row.checkpoint.last_failure = Some(crate::AutomationFailureRecord {
                node_id: "draft".to_string(),
                reason: "draft needs prompt fix".to_string(),
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
                        "prompt": "Write draft with clarified branch requirements",
                        "reason": "repair only the draft branch"
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let context_run_id = payload
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("context run id");
    assert_eq!(
        payload.get("linked_context_run_id").and_then(Value::as_str),
        Some(context_run_id)
    );
    assert_eq!(
        payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(context_run_id)
    );

    let repaired = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after repair");
    assert_eq!(repaired.status, crate::AutomationRunStatus::Queued);
    assert!(repaired
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "research"));
    assert!(repaired
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "analysis"));
    assert!(!repaired
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "draft"));
    assert!(!repaired
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "publish"));
    assert!(repaired.checkpoint.node_outputs.contains_key("research"));
    assert!(repaired.checkpoint.node_outputs.contains_key("analysis"));
    assert!(!repaired.checkpoint.node_outputs.contains_key("draft"));
    assert!(!repaired.checkpoint.node_outputs.contains_key("publish"));
    assert!(repaired.checkpoint.node_attempts.get("draft").is_none());
    assert!(repaired
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "draft"));
    assert!(repaired
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "publish"));
    assert_eq!(
        repaired.checkpoint.blocked_nodes,
        vec!["publish".to_string()]
    );
    assert!(!repaired
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "analysis"));
    assert!(repaired.active_session_ids.is_empty());
    assert!(repaired.active_instance_ids.is_empty());
    assert!(repaired.latest_session_id.is_none());
    assert!(repaired.checkpoint.last_failure.is_none());
    let repair_event = repaired
        .checkpoint
        .lifecycle_history
        .iter()
        .find(|entry| entry.event == "run_step_repaired")
        .expect("repair event");
    let metadata = repair_event.metadata.as_ref().expect("repair metadata");
    assert_eq!(
        metadata.get("node_id").and_then(Value::as_str),
        Some("draft")
    );
    assert_eq!(
        metadata.get("new_prompt").and_then(Value::as_str),
        Some("Write draft with clarified branch requirements")
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
            row.checkpoint.node_attempts.insert("draft".to_string(), 3);
            row.checkpoint.node_attempts.insert("review".to_string(), 2);
            row.checkpoint
                .node_attempts
                .insert("approval".to_string(), 1);
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
    assert!(repaired.checkpoint.node_attempts.get("draft").is_none());
    assert!(repaired.checkpoint.node_attempts.get("review").is_none());
    assert!(repaired.checkpoint.node_attempts.get("approval").is_none());
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

#[tokio::test]
async fn automations_v2_run_task_retry_resets_selected_subtree() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-task-retry").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Blocked;
            row.checkpoint.completed_nodes = vec!["draft".to_string(), "review".to_string()];
            row.checkpoint.pending_nodes = vec!["approval".to_string()];
            row.checkpoint
                .node_outputs
                .insert("draft".to_string(), json!({"summary":"draft"}));
            row.checkpoint
                .node_outputs
                .insert("review".to_string(), json!({"summary":"review"}));
            row.checkpoint.node_attempts.insert("review".to_string(), 2);
            row.checkpoint.blocked_nodes = vec!["approval".to_string()];
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/automations/v2/runs/{}/tasks/{}/retry",
                    run.run_id, "review"
                ))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "reason": "retry review from debugger"
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let context_run_id = payload
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("context run id");
    assert_eq!(
        payload.get("linked_context_run_id").and_then(Value::as_str),
        Some(context_run_id)
    );
    assert_eq!(
        payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(context_run_id)
    );

    let retried = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after retry");
    assert_eq!(retried.status, crate::AutomationRunStatus::Queued);
    assert!(retried
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "draft"));
    assert!(!retried
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "review"));
    assert!(!retried
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "approval"));
    assert!(retried.checkpoint.node_outputs.contains_key("draft"));
    assert!(!retried.checkpoint.node_outputs.contains_key("review"));
    assert!(retried.checkpoint.node_attempts.get("review").is_none());
    assert!(retried
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "review"));
    assert!(retried
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "approval"));
    let retry_event = retried
        .checkpoint
        .lifecycle_history
        .iter()
        .find(|entry| entry.event == "run_task_retried")
        .expect("retry event");
    let metadata = retry_event.metadata.as_ref().expect("retry metadata");
    assert_eq!(
        metadata.get("node_id").and_then(Value::as_str),
        Some("review")
    );
}

#[tokio::test]
async fn automations_v2_run_task_requeue_resets_selected_subtree() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-task-requeue").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Paused;
            row.checkpoint.completed_nodes = vec!["draft".to_string(), "review".to_string()];
            row.checkpoint.pending_nodes = vec!["approval".to_string()];
            row.checkpoint
                .node_outputs
                .insert("draft".to_string(), json!({"summary":"draft"}));
            row.checkpoint
                .node_outputs
                .insert("review".to_string(), json!({"summary":"review"}));
            row.checkpoint.node_attempts.insert("draft".to_string(), 2);
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/automations/v2/runs/{}/tasks/{}/requeue",
                    run.run_id, "draft"
                ))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "reason": "requeue draft from debugger"
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let context_run_id = payload
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("context run id");
    assert_eq!(
        payload.get("linked_context_run_id").and_then(Value::as_str),
        Some(context_run_id)
    );
    assert_eq!(
        payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(context_run_id)
    );

    let requeued = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after requeue");
    assert_eq!(requeued.status, crate::AutomationRunStatus::Queued);
    assert!(!requeued
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "draft"));
    assert!(!requeued
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "review"));
    assert!(!requeued.checkpoint.node_outputs.contains_key("draft"));
    assert!(!requeued.checkpoint.node_outputs.contains_key("review"));
    assert!(requeued.checkpoint.node_attempts.get("draft").is_none());
    assert!(requeued.active_session_ids.is_empty());
    assert!(requeued.active_instance_ids.is_empty());
    assert!(requeued.latest_session_id.is_none());
    assert!(requeued
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "draft"));
    assert!(requeued
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "review"));
    assert!(requeued
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "approval"));
    let requeue_event = requeued
        .checkpoint
        .lifecycle_history
        .iter()
        .find(|entry| entry.event == "run_task_requeued")
        .expect("requeue event");
    let metadata = requeue_event.metadata.as_ref().expect("requeue metadata");
    assert_eq!(
        metadata.get("node_id").and_then(Value::as_str),
        Some("draft")
    );
}

#[tokio::test]
async fn automations_v2_run_task_reset_preview_reports_exact_subtree() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-task-preview").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/automations/v2/runs/{}/tasks/{}/reset_preview",
                    run.run_id, "draft"
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let context_run_id = payload
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("context run id");
    assert_eq!(
        payload.get("linked_context_run_id").and_then(Value::as_str),
        Some(context_run_id)
    );
    let preview = payload.get("preview").expect("preview");
    assert_eq!(
        preview.get("node_id").and_then(Value::as_str),
        Some("draft")
    );
    assert_eq!(
        preview
            .get("reset_nodes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect::<Vec<_>>(),
        vec![
            "approval".to_string(),
            "draft".to_string(),
            "review".to_string()
        ]
    );
    assert_eq!(
        preview
            .get("cleared_outputs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect::<Vec<_>>(),
        Vec::<String>::new()
    );
    assert_eq!(
        preview
            .get("preserves_upstream_outputs")
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn automations_v2_run_task_continue_minimally_resets_blocked_node() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-task-continue").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Blocked;
            row.checkpoint.completed_nodes = vec!["draft".to_string()];
            row.checkpoint.pending_nodes = vec!["review".to_string(), "approval".to_string()];
            row.checkpoint.node_outputs.insert(
                "review".to_string(),
                json!({"status":"blocked","summary":"review blocked"}),
            );
            row.checkpoint.blocked_nodes = vec!["review".to_string(), "approval".to_string()];
            row.checkpoint.node_attempts.insert("review".to_string(), 2);
            row.active_session_ids = vec!["session-a".to_string()];
            row.latest_session_id = Some("session-a".to_string());
            row.active_instance_ids = vec!["instance-a".to_string()];
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/automations/v2/runs/{}/tasks/{}/continue",
                    run.run_id, "review"
                ))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "reason": "continue blocked review minimally"
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let context_run_id = payload
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("context run id");
    assert_eq!(
        payload.get("linked_context_run_id").and_then(Value::as_str),
        Some(context_run_id)
    );
    assert_eq!(
        payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(context_run_id)
    );

    let continued = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after continue");
    assert_eq!(continued.status, crate::AutomationRunStatus::Queued);
    assert!(continued
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "draft"));
    assert!(!continued.checkpoint.node_outputs.contains_key("review"));
    assert!(continued
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "review"));
    assert!(continued.checkpoint.node_attempts.get("review").is_none());
    assert!(continued.active_session_ids.is_empty());
    assert!(continued.active_instance_ids.is_empty());
    assert!(continued.latest_session_id.is_none());
    let continue_event = continued
        .checkpoint
        .lifecycle_history
        .iter()
        .find(|entry| entry.event == "run_task_continued")
        .expect("continue event");
    let metadata = continue_event.metadata.as_ref().expect("continue metadata");
    assert_eq!(
        metadata.get("node_id").and_then(Value::as_str),
        Some("review")
    );
}

#[tokio::test]
async fn automations_v2_run_task_continue_accepts_completed_runs_when_node_output_is_blocked() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-task-continue-completed").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Completed;
            row.finished_at_ms = Some(crate::now_ms());
            row.checkpoint.completed_nodes = vec!["draft".to_string(), "review".to_string()];
            row.checkpoint.pending_nodes = vec!["approval".to_string()];
            row.checkpoint.node_outputs.insert(
                "review".to_string(),
                json!({"status":"blocked","summary":"review blocked"}),
            );
            row.checkpoint.node_attempts.insert("review".to_string(), 2);
            row.active_session_ids = vec!["session-a".to_string()];
            row.latest_session_id = Some("session-a".to_string());
            row.active_instance_ids = vec!["instance-a".to_string()];
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/automations/v2/runs/{}/tasks/{}/continue",
                    run.run_id, "review"
                ))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "reason": "continue completed run with blocked output"
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let continued = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after continue");
    assert_eq!(continued.status, crate::AutomationRunStatus::Queued);
    assert!(continued
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "draft"));
    assert!(!continued
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "review"));
    assert!(!continued.checkpoint.node_outputs.contains_key("review"));
    assert!(continued
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "review"));
    assert!(continued.active_session_ids.is_empty());
    assert!(continued.active_instance_ids.is_empty());
    assert!(continued.latest_session_id.is_none());
}

#[tokio::test]
async fn automation_v2_research_workflow_smoke_exposes_blocked_artifact_state() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let automation = crate::AutomationV2Spec {
        automation_id: "auto-v2-smoke-research".to_string(),
        name: "Research Smoke".to_string(),
        description: Some("Canonical research workflow smoke test".to_string()),
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
            agent_id: "researcher".to_string(),
            template_id: None,
            display_name: "Researcher".to_string(),
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: vec![
                    "glob".to_string(),
                    "read".to_string(),
                    "websearch".to_string(),
                    "write".to_string(),
                ],
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
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "research-brief".to_string(),
                    agent_id: "researcher".to_string(),
                    objective: "Write the marketing brief".to_string(),
                    depends_on: Vec::new(),
                    input_refs: Vec::new(),
                    output_contract: Some(crate::AutomationFlowOutputContract {
                        kind: "brief".to_string(),
                        validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
                        enforcement: None,
                        schema: None,
                        summary_guidance: None,
                    }),
                    retry_policy: None,
                    timeout_ms: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    metadata: Some(json!({
                        "builder": {
                            "title": "Research Brief",
                            "role": "researcher",
                            "output_path": "marketing-brief.md",
                            "source_coverage_required": true
                        }
                    })),
                },
                crate::AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "draft-copy".to_string(),
                    agent_id: "researcher".to_string(),
                    objective: "Draft the post".to_string(),
                    depends_on: vec!["research-brief".to_string()],
                    input_refs: vec![crate::AutomationFlowInputRef {
                        from_step_id: "research-brief".to_string(),
                        alias: "marketing_brief".to_string(),
                    }],
                    output_contract: None,
                    retry_policy: None,
                    timeout_ms: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    metadata: Some(json!({
                        "builder": {
                            "title": "Draft Copy",
                            "role": "copywriter",
                            "output_path": "draft-post.md"
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
        output_targets: vec![
            "marketing-brief.md".to_string(),
            "draft-post.md".to_string(),
        ],
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: None,
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
        .add_automation_v2_session(&run.run_id, "sess-research-smoke")
        .await;
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Blocked;
            row.detail = Some("research coverage requirements were not met".to_string());
            row.checkpoint.pending_nodes = vec![
                "research-brief".to_string(),
                "draft-copy".to_string(),
            ];
            row.checkpoint.blocked_nodes = vec![
                "research-brief".to_string(),
                "draft-copy".to_string(),
            ];
            row.checkpoint.node_outputs.insert(
                "research-brief".to_string(),
                json!({
                    "node_id": "research-brief",
                    "status": "blocked",
                    "workflow_class": "research",
                    "phase": "blocked",
                    "failure_kind": "research_missing_reads",
                    "summary": "Blocked research brief preserved for inspection.",
                    "artifact_validation": {
                        "accepted_artifact_path": "marketing-brief.md",
                        "recovered_from_session_write": true,
                        "repair_attempted": true,
                        "repair_succeeded": false,
                        "blocking_classification": "tool_available_but_not_used",
                        "required_next_tool_actions": [
                            "Use `read` on concrete workspace files before finalizing the brief.",
                            "Move every discovered relevant file into either `Files reviewed` after `read`, or `Files not reviewed` with a reason."
                        ],
                        "repair_attempt": 1,
                        "repair_attempts_remaining": 4,
                        "unmet_requirements": ["concrete_read_required", "coverage_mode"]
                    },
                    "knowledge_preflight": {
                        "project_id": "proj-research-smoke",
                        "task_family": "research-brief",
                        "subject": "Produce a research brief with citations",
                        "coverage_key": "proj-research-smoke::research-brief::produce-a-research-brief-with-citations",
                        "decision": "no_prior_knowledge",
                        "reuse_reason": null,
                        "skip_reason": "no active promoted knowledge matched this coverage key",
                        "freshness_reason": null,
                        "items": []
                    },
                    "content": {
                        "path": "marketing-brief.md",
                        "text": "# Marketing Brief\n\n## Files reviewed\n\n## Files not reviewed\n- tandem-reference/readmes/repo-README.md: not read in this run.\n\n## Research status\nBlocked pending concrete file reads.",
                        "session_id": "sess-research-smoke"
                    }
                }),
            );
        })
        .await
        .expect("update run");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/automations/v2/runs/{}", run.run_id))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let run_payload = payload.get("run").expect("run payload");
    assert_eq!(
        run_payload.get("status").and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        run_payload.get("latest_session_id").and_then(Value::as_str),
        Some("sess-research-smoke")
    );
    let research_output = run_payload
        .get("checkpoint")
        .and_then(|value| value.get("node_outputs"))
        .and_then(|value| value.get("research-brief"))
        .expect("research output");
    assert_eq!(
        research_output
            .get("workflow_class")
            .and_then(Value::as_str),
        Some("research")
    );
    assert_eq!(
        research_output.get("failure_kind").and_then(Value::as_str),
        Some("research_missing_reads")
    );
    assert_eq!(
        research_output
            .get("validator_kind")
            .and_then(Value::as_str),
        Some("research_brief")
    );
    assert_eq!(
        research_output
            .get("validator_summary")
            .and_then(|value| value.get("outcome"))
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        research_output
            .get("validator_summary")
            .and_then(|value| value.get("unmet_requirements"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(2)
    );
    assert_eq!(
        research_output
            .get("artifact_validation")
            .and_then(|value| value.get("accepted_artifact_path"))
            .and_then(Value::as_str),
        Some("marketing-brief.md")
    );
    let repair_guidance = run_payload
        .get("nodeRepairGuidance")
        .and_then(|value| value.get("research-brief"))
        .expect("repair guidance");
    assert_eq!(
        repair_guidance.get("status").and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        repair_guidance
            .get("blockingClassification")
            .and_then(Value::as_str),
        Some("tool_available_but_not_used")
    );
    assert_eq!(
        repair_guidance.get("repairAttempt").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        repair_guidance
            .get("repairAttemptsRemaining")
            .and_then(Value::as_u64),
        Some(4)
    );
    assert_eq!(
        repair_guidance
            .get("requiredNextToolActions")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str),
        Some("Use `read` on concrete workspace files before finalizing the brief.")
    );
    assert_eq!(
        repair_guidance
            .get("validationBasis")
            .and_then(|value| value.get("authority"))
            .and_then(Value::as_str),
        Some("filesystem_and_receipts")
    );
    assert_eq!(
        repair_guidance
            .get("validationBasis")
            .and_then(|value| value.get("current_attempt_has_recorded_activity"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        repair_guidance
            .get("knowledgePreflight")
            .and_then(|value| value.get("decision"))
            .and_then(Value::as_str),
        Some("no_prior_knowledge")
    );
    assert_eq!(
        research_output
            .get("artifact_validation")
            .and_then(|value| value.get("validation_basis"))
            .and_then(|value| value.get("authority"))
            .and_then(Value::as_str),
        Some("filesystem_and_receipts")
    );
    assert_eq!(
        research_output
            .get("knowledge_preflight")
            .and_then(|value| value.get("skip_reason"))
            .and_then(Value::as_str),
        Some("no active promoted knowledge matched this coverage key")
    );
    assert_eq!(
        research_output.get("quality_mode").and_then(Value::as_str),
        Some("strict_research_v1")
    );
    assert_eq!(
        research_output
            .get("requested_quality_mode")
            .and_then(Value::as_str),
        None
    );
    assert_eq!(
        research_output
            .get("emergency_rollback_enabled")
            .and_then(Value::as_bool),
        Some(false)
    );
    let receipt_timeline = research_output
        .get("receipt_timeline")
        .and_then(Value::as_array)
        .expect("receipt timeline");
    assert!(receipt_timeline.len() >= 3);
    assert_eq!(
        receipt_timeline
            .last()
            .and_then(|value| value.get("event_type"))
            .and_then(Value::as_str),
        Some("validation_summary")
    );
    assert_eq!(
        run_payload
            .get("blockedNodeIDs")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["research-brief"])
    );
    assert_eq!(
        run_payload
            .get("needsRepairNodeIDs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert!(run_payload
        .get("last_activity_at_ms")
        .and_then(Value::as_u64)
        .is_some_and(|value| value > 0));
    assert!(run_payload
        .get("checkpoint")
        .and_then(|value| value.get("node_outputs"))
        .and_then(|value| value.get("draft-copy"))
        .is_none());
}

#[tokio::test]
async fn automation_v2_research_workflow_smoke_exposes_citation_validation_state() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let automation = crate::AutomationV2Spec {
        automation_id: "auto-v2-smoke-research-citations".to_string(),
        name: "Research Citation Smoke".to_string(),
        description: Some("Research citation validation smoke test".to_string()),
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
            agent_id: "researcher".to_string(),
            template_id: None,
            display_name: "Researcher".to_string(),
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: vec![
                    "glob".to_string(),
                    "read".to_string(),
                    "write".to_string(),
                    "websearch".to_string(),
                ],
                denylist: Vec::new(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
            },
            approval_policy: None,
        }],
        flow: crate::AutomationFlowSpec {
            nodes: vec![crate::AutomationFlowNode {
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                node_id: "research-brief".to_string(),
                agent_id: "researcher".to_string(),
                objective: "Produce a research brief with citations".to_string(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: Some(crate::AutomationFlowOutputContract {
                    kind: "brief".to_string(),
                    validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
                    enforcement: None,
                    schema: None,
                    summary_guidance: None,
                }),
                retry_policy: None,
                timeout_ms: None,
                stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                gate: None,
                metadata: Some(json!({
                    "builder": {
                        "output_path": "marketing-brief.md",
                        "web_research_expected": true,
                        "source_coverage_required": true
                    }
                })),
            }],
        },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: vec!["marketing-brief.md".to_string()],
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: None,
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
        .add_automation_v2_session(&run.run_id, "sess-research-citation-smoke")
        .await;
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Blocked;
            row.detail = Some("research citation requirements were not met".to_string());
            row.checkpoint.pending_nodes = vec!["research-brief".to_string()];
            row.checkpoint.blocked_nodes = vec!["research-brief".to_string()];
            row.checkpoint.node_outputs.insert(
                "research-brief".to_string(),
                json!({
                    "node_id": "research-brief",
                    "status": "blocked",
                    "workflow_class": "research",
                    "phase": "blocked",
                    "failure_kind": "research_citations_missing",
                    "summary": "Blocked research brief is missing citation-backed claims.",
                    "artifact_validation": {
                        "accepted_artifact_path": "marketing-brief.md",
                        "citation_count": 0,
                        "web_sources_reviewed_present": false,
                        "repair_attempted": true,
                        "repair_succeeded": false,
                        "unmet_requirements": ["citations_missing", "web_sources_reviewed_missing"]
                    },
                    "content": {
                        "path": "marketing-brief.md",
                        "text": "# Marketing Brief\n\n## Files reviewed\n- inputs/questions.md\n\n## Findings\nClaims are summarized here without explicit citations.\n",
                        "session_id": "sess-research-citation-smoke"
                    }
                }),
            );
        })
        .await
        .expect("update run");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/automations/v2/runs/{}", run.run_id))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let research_output = payload
        .get("run")
        .and_then(|value| value.get("checkpoint"))
        .and_then(|value| value.get("node_outputs"))
        .and_then(|value| value.get("research-brief"))
        .expect("research output");
    assert_eq!(
        research_output.get("failure_kind").and_then(Value::as_str),
        Some("research_citations_missing")
    );
    assert_eq!(
        research_output
            .get("validator_kind")
            .and_then(Value::as_str),
        Some("research_brief")
    );
    assert_eq!(
        research_output
            .get("validator_summary")
            .and_then(|value| value.get("unmet_requirements"))
            .and_then(Value::as_array)
            .map(|rows| rows.clone()),
        Some(vec![
            json!("citations_missing"),
            json!("web_sources_reviewed_missing")
        ])
    );
    assert_eq!(
        research_output
            .get("artifact_validation")
            .and_then(|value| value.get("citation_count"))
            .and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        research_output
            .get("artifact_validation")
            .and_then(|value| value.get("web_sources_reviewed_present"))
            .and_then(Value::as_bool),
        Some(false)
    );
}

#[tokio::test]
async fn automation_v2_artifact_workflow_smoke_exposes_completed_output_state() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-smoke-artifact").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    state
        .add_automation_v2_session(&run.run_id, "sess-artifact-smoke")
        .await;
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Completed;
            row.checkpoint.completed_nodes = vec![
                "draft".to_string(),
                "review".to_string(),
                "approval".to_string(),
            ];
            row.checkpoint.node_outputs.insert(
                "draft".to_string(),
                json!({
                    "node_id": "draft",
                    "status": "completed",
                    "workflow_class": "artifact",
                    "phase": "completed",
                    "summary": "Draft artifact accepted.",
                    "artifact_validation": {
                        "accepted_artifact_path": "artifact.md"
                    },
                    "content": {
                        "path": "artifact.md",
                        "text": "# Artifact\n\nReady for review.",
                        "session_id": "sess-artifact-smoke"
                    }
                }),
            );
        })
        .await
        .expect("update run");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/automations/v2/runs/{}", run.run_id))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let run_payload = payload.get("run").expect("run payload");
    assert_eq!(
        run_payload.get("status").and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        run_payload.get("latest_session_id").and_then(Value::as_str),
        Some("sess-artifact-smoke")
    );
    assert_eq!(
        run_payload
            .get("checkpoint")
            .and_then(|value| value.get("node_outputs"))
            .and_then(|value| value.get("draft"))
            .and_then(|value| value.get("artifact_validation"))
            .and_then(|value| value.get("accepted_artifact_path"))
            .and_then(Value::as_str),
        Some("artifact.md")
    );

    let context_run_id = payload
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("context run id");
    let blackboard_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/runs/{context_run_id}/blackboard"))
                .body(Body::empty())
                .expect("blackboard request"),
        )
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
    assert!(tasks.iter().any(|task| {
        task.get("id").and_then(Value::as_str) == Some("node-draft")
            && task.get("status").and_then(Value::as_str) == Some("done")
    }));
}

#[tokio::test]
async fn automation_v2_code_workflow_smoke_exposes_verify_failed_state() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let automation = crate::AutomationV2Spec {
        automation_id: "auto-v2-smoke-code".to_string(),
        name: "Code Smoke".to_string(),
        description: Some("Canonical coding workflow smoke test".to_string()),
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
            agent_id: "coder".to_string(),
            template_id: None,
            display_name: "Coder".to_string(),
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: vec![
                    "glob".to_string(),
                    "read".to_string(),
                    "edit".to_string(),
                    "apply_patch".to_string(),
                    "write".to_string(),
                    "bash".to_string(),
                ],
                denylist: Vec::new(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
            },
            approval_policy: None,
        }],
        flow: crate::AutomationFlowSpec {
            nodes: vec![crate::AutomationFlowNode {
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                node_id: "implement-fix".to_string(),
                agent_id: "coder".to_string(),
                objective: "Implement the repo fix and verify it".to_string(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: None,
                retry_policy: None,
                timeout_ms: None,
                stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                gate: None,
                metadata: Some(json!({
                    "builder": {
                        "title": "Implement Fix",
                        "role": "coder",
                        "task_kind": "code_change",
                        "verification_command": "cargo test -p tandem-server"
                    }
                })),
            }],
        },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: vec!["crates/tandem-server/src/lib.rs".to_string()],
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: None,
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
        .add_automation_v2_session(&run.run_id, "sess-code-smoke")
        .await;
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Blocked;
            row.detail = Some("verification failed".to_string());
            row.checkpoint.pending_nodes = vec!["implement-fix".to_string()];
            row.checkpoint.blocked_nodes = vec!["implement-fix".to_string()];
            row.checkpoint.node_outputs.insert(
                "implement-fix".to_string(),
                json!({
                    "node_id": "implement-fix",
                    "status": "verify_failed",
                    "workflow_class": "code",
                    "phase": "verification_failed",
                    "failure_kind": "verification_failed",
                    "summary": "Implementation landed but verification failed.",
                    "artifact_validation": {
                        "verification": {
                            "verification_expected": true,
                            "verification_ran": true,
                            "verification_failed": true,
                            "latest_verification_command": "cargo test -p tandem-server",
                            "latest_verification_failure": "1 test failed"
                        }
                    },
                    "content": {
                        "path": "crates/tandem-server/src/lib.rs",
                        "text": "patched content",
                        "session_id": "sess-code-smoke"
                    }
                }),
            );
        })
        .await
        .expect("update run");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/automations/v2/runs/{}", run.run_id))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let code_output = payload
        .get("run")
        .and_then(|value| value.get("checkpoint"))
        .and_then(|value| value.get("node_outputs"))
        .and_then(|value| value.get("implement-fix"))
        .expect("code output");
    assert_eq!(
        code_output.get("status").and_then(Value::as_str),
        Some("verify_failed")
    );
    assert_eq!(
        code_output.get("workflow_class").and_then(Value::as_str),
        Some("code")
    );
    assert_eq!(
        code_output.get("failure_kind").and_then(Value::as_str),
        Some("verification_failed")
    );
    assert_eq!(
        code_output.get("validator_kind").and_then(Value::as_str),
        Some("code_patch")
    );
    assert_eq!(
        code_output
            .get("validator_summary")
            .and_then(|value| value.get("outcome"))
            .and_then(Value::as_str),
        Some("verify_failed")
    );
    assert_eq!(
        code_output
            .get("validator_summary")
            .and_then(|value| value.get("verification_outcome"))
            .and_then(Value::as_str),
        Some("failed")
    );
    assert_eq!(
        code_output
            .get("artifact_validation")
            .and_then(|value| value.get("verification"))
            .and_then(|value| value.get("latest_verification_command"))
            .and_then(Value::as_str),
        Some("cargo test -p tandem-server")
    );
}

#[tokio::test]
async fn automation_v2_editorial_workflow_smoke_exposes_quality_validation_state() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let automation = crate::AutomationV2Spec {
        automation_id: "auto-v2-smoke-editorial".to_string(),
        name: "Editorial Smoke".to_string(),
        description: Some("Editorial validation smoke test".to_string()),
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
            agent_id: "writer".to_string(),
            template_id: None,
            display_name: "Writer".to_string(),
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: vec!["write".to_string()],
                denylist: Vec::new(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
            },
            approval_policy: None,
        }],
        flow: crate::AutomationFlowSpec {
            nodes: vec![crate::AutomationFlowNode {
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                node_id: "draft-report".to_string(),
                agent_id: "writer".to_string(),
                objective: "Draft the final markdown report".to_string(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: Some(crate::AutomationFlowOutputContract {
                    kind: "report_markdown".to_string(),
                    validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                    enforcement: None,
                    schema: None,
                    summary_guidance: None,
                }),
                retry_policy: None,
                timeout_ms: None,
                stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                gate: None,
                metadata: Some(json!({
                    "builder": {
                        "output_path": "final-report.md",
                        "role": "writer"
                    }
                })),
            }],
        },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: vec!["final-report.md".to_string()],
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: None,
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
        .add_automation_v2_session(&run.run_id, "sess-editorial-smoke")
        .await;
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Blocked;
            row.detail = Some("editorial quality requirements were not met".to_string());
            row.checkpoint.pending_nodes = vec!["draft-report".to_string()];
            row.checkpoint.blocked_nodes = vec!["draft-report".to_string()];
            row.checkpoint.node_outputs.insert(
                "draft-report".to_string(),
                json!({
                    "node_id": "draft-report",
                    "status": "blocked",
                    "workflow_class": "artifact",
                    "phase": "editorial_validation",
                    "failure_kind": "editorial_quality_failed",
                    "summary": "Blocked editorial draft is too weak to publish.",
                    "artifact_validation": {
                        "accepted_artifact_path": "final-report.md",
                        "heading_count": 1,
                        "paragraph_count": 1,
                        "repair_attempted": false,
                        "repair_succeeded": false,
                        "unmet_requirements": ["editorial_substance_missing", "markdown_structure_missing"]
                    },
                    "content": {
                        "path": "final-report.md",
                        "text": "# Draft\\n\\nTODO\\n",
                        "session_id": "sess-editorial-smoke"
                    }
                }),
            );
        })
        .await
        .expect("update run");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/automations/v2/runs/{}", run.run_id))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let draft_output = payload
        .get("run")
        .and_then(|value| value.get("checkpoint"))
        .and_then(|value| value.get("node_outputs"))
        .and_then(|value| value.get("draft-report"))
        .expect("draft output");
    assert_eq!(
        draft_output.get("failure_kind").and_then(Value::as_str),
        Some("editorial_quality_failed")
    );
    assert_eq!(
        draft_output.get("phase").and_then(Value::as_str),
        Some("editorial_validation")
    );
    assert_eq!(
        draft_output.get("validator_kind").and_then(Value::as_str),
        Some("generic_artifact")
    );
    assert_eq!(
        draft_output
            .get("validator_summary")
            .and_then(|value| value.get("unmet_requirements"))
            .and_then(Value::as_array)
            .map(|rows| rows.clone()),
        Some(vec![
            json!("editorial_substance_missing"),
            json!("markdown_structure_missing")
        ])
    );
    assert_eq!(
        draft_output
            .get("artifact_validation")
            .and_then(|value| value.get("heading_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        draft_output
            .get("artifact_validation")
            .and_then(|value| value.get("paragraph_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
}

#[tokio::test]
async fn automation_v2_publish_block_smoke_skips_external_action_receipts() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let automation = crate::AutomationV2Spec {
        automation_id: "auto-v2-smoke-editorial-publish".to_string(),
        name: "Editorial Publish Smoke".to_string(),
        description: Some("Publish is blocked until editorial issues are resolved".to_string()),
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
            agent_id: "publisher".to_string(),
            template_id: None,
            display_name: "Publisher".to_string(),
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: vec!["workflow_test.slack".to_string()],
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
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "draft-report".to_string(),
                    agent_id: "publisher".to_string(),
                    objective: "Draft the final markdown report".to_string(),
                    depends_on: Vec::new(),
                    input_refs: Vec::new(),
                    output_contract: Some(crate::AutomationFlowOutputContract {
                        kind: "report_markdown".to_string(),
                        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                        enforcement: None,
                        schema: None,
                        summary_guidance: None,
                    }),
                    retry_policy: None,
                    timeout_ms: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    metadata: Some(json!({
                        "builder": {
                            "output_path": "final-report.md",
                            "role": "writer"
                        }
                    })),
                },
                crate::AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "publish-report".to_string(),
                    agent_id: "publisher".to_string(),
                    objective: "Publish the final report to Slack".to_string(),
                    depends_on: vec!["draft-report".to_string()],
                    input_refs: vec![crate::AutomationFlowInputRef {
                        from_step_id: "draft-report".to_string(),
                        alias: "draft".to_string(),
                    }],
                    output_contract: None,
                    retry_policy: None,
                    timeout_ms: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    metadata: Some(json!({
                        "builder": {
                            "role": "publisher"
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
        output_targets: vec!["final-report.md".to_string()],
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: None,
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
            row.status = crate::AutomationRunStatus::Blocked;
            row.detail = Some("publish is blocked pending editorial fixes".to_string());
            row.checkpoint.pending_nodes = vec!["publish-report".to_string()];
            row.checkpoint.blocked_nodes =
                vec!["draft-report".to_string(), "publish-report".to_string()];
            row.checkpoint.node_outputs.insert(
                "draft-report".to_string(),
                json!({
                    "node_id": "draft-report",
                    "status": "blocked",
                    "workflow_class": "artifact",
                    "phase": "editorial_validation",
                    "failure_kind": "editorial_quality_failed",
                    "summary": "Blocked editorial draft is too weak to publish.",
                    "validator_kind": "generic_artifact",
                    "validator_summary": {
                        "kind": "generic_artifact",
                        "outcome": "blocked",
                        "reason": "editorial artifact is missing expected markdown structure",
                        "unmet_requirements": ["editorial_substance_missing", "markdown_structure_missing"]
                    },
                    "artifact_validation": {
                        "accepted_artifact_path": "final-report.md",
                        "heading_count": 1,
                        "paragraph_count": 1,
                        "repair_attempted": false,
                        "repair_succeeded": false,
                        "unmet_requirements": ["editorial_substance_missing", "markdown_structure_missing"]
                    }
                }),
            );
            row.checkpoint.node_outputs.insert(
                "publish-report".to_string(),
                json!({
                    "node_id": "publish-report",
                    "status": "blocked",
                    "workflow_class": "artifact",
                    "phase": "editorial_validation",
                    "failure_kind": "editorial_quality_failed",
                    "summary": "Publish blocked until editorial issues are resolved.",
                    "validator_summary": {
                        "outcome": "blocked",
                        "reason": "publish step blocked until upstream editorial issues are resolved: draft-report",
                        "unmet_requirements": ["editorial_clearance_required"]
                    },
                    "artifact_validation": {
                        "unmet_requirements": ["editorial_clearance_required"],
                        "semantic_block_reason": "publish step blocked until upstream editorial issues are resolved: draft-report"
                    }
                }),
            );
        })
        .await
        .expect("update run");

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
    let publish_output = run_payload
        .get("run")
        .and_then(|value| value.get("checkpoint"))
        .and_then(|value| value.get("node_outputs"))
        .and_then(|value| value.get("publish-report"))
        .expect("publish output");
    assert_eq!(
        publish_output.get("failure_kind").and_then(Value::as_str),
        Some("editorial_quality_failed")
    );
    assert_eq!(
        publish_output.get("phase").and_then(Value::as_str),
        Some("editorial_validation")
    );
    assert_eq!(
        publish_output
            .get("validator_summary")
            .and_then(|value| value.get("unmet_requirements"))
            .and_then(Value::as_array)
            .map(|rows| rows.clone()),
        Some(vec![json!("editorial_clearance_required")])
    );

    let external_actions_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/external-actions?limit=10")
                .body(Body::empty())
                .expect("external actions request"),
        )
        .await
        .expect("external actions response");
    assert_eq!(external_actions_resp.status(), StatusCode::OK);
    let external_actions_body = to_bytes(external_actions_resp.into_body(), usize::MAX)
        .await
        .expect("external actions body");
    let external_actions_payload: Value =
        serde_json::from_slice(&external_actions_body).expect("external actions json");
    assert_eq!(
        external_actions_payload
            .get("actions")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
}
