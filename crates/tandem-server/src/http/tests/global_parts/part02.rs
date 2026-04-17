
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
                max_tool_calls: None,
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
    assert_eq!(
        payload
            .get("run")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str),
        Some("cancelled")
    );
    assert!(payload
        .get("run")
        .and_then(|value| value.get("activeSessionIDs"))
        .and_then(Value::as_array)
        .map(|values| values.is_empty())
        .unwrap_or(true));
    assert!(payload
        .get("run")
        .and_then(|value| value.get("activeInstanceIDs"))
        .and_then(Value::as_array)
        .map(|values| values.is_empty())
        .unwrap_or(true));

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
    assert_eq!(
        payload
            .get("run")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str),
        Some("paused")
    );
    assert!(payload
        .get("run")
        .and_then(|value| value.get("activeSessionIDs"))
        .and_then(Value::as_array)
        .map(|values| values.is_empty())
        .unwrap_or(true));
    assert!(payload
        .get("run")
        .and_then(|value| value.get("activeInstanceIDs"))
        .and_then(Value::as_array)
        .map(|values| values.is_empty())
        .unwrap_or(true));

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
