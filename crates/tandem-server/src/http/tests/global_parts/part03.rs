
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
                    max_tool_calls: None,
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
                    max_tool_calls: None,
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
                max_tool_calls: None,
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
                max_tool_calls: None,
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
                max_tool_calls: None,
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
