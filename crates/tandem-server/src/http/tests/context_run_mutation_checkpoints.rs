use super::*;

fn context_run_mutation_checkpoint_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[tokio::test]
async fn context_run_mutation_checkpoints_endpoint_returns_records_and_summary() {
    let _guard = context_run_mutation_checkpoint_test_lock().lock().await;
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-mutation-1",
                "objective": "inspect mutation checkpoints",
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

    let event_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-mutation-1/events")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "type": "mutation_checkpoint_recorded",
                "status": "running",
                "payload": {
                    "record": {
                        "session_id": "session-1",
                        "message_id": "message-1",
                        "tool": "write",
                        "outcome": "succeeded",
                        "file_count": 1,
                        "changed_file_count": 1,
                        "files": [{
                            "path": "src/lib.rs",
                            "resolved_path": "/workspace/src/lib.rs",
                            "existed_before": false,
                            "existed_after": true,
                            "changed": true,
                            "rollback_snapshot": {
                                "status": "not_needed"
                            }
                        }]
                    }
                }
            })
            .to_string(),
        ))
        .expect("event request");
    let event_resp = app
        .clone()
        .oneshot(event_req)
        .await
        .expect("event response");
    assert_eq!(event_resp.status(), StatusCode::OK);

    let inspect_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-mutation-1/checkpoints/mutations")
        .body(Body::empty())
        .expect("inspect request");
    let inspect_resp = app
        .clone()
        .oneshot(inspect_req)
        .await
        .expect("inspect response");
    assert_eq!(inspect_resp.status(), StatusCode::OK);
    let inspect_body = to_bytes(inspect_resp.into_body(), usize::MAX)
        .await
        .expect("inspect body");
    let inspect_payload: Value = serde_json::from_slice(&inspect_body).expect("inspect json");

    assert_eq!(
        inspect_payload
            .get("records")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        inspect_payload
            .get("summary")
            .and_then(|value| value.get("changed_file_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        inspect_payload
            .get("summary")
            .and_then(|value| value.get("by_tool"))
            .and_then(|value| value.get("write"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        inspect_payload
            .get("rollback_readiness")
            .and_then(|value| value.get("directly_revertible_file_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        inspect_payload
            .get("records")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("rollback_readiness"))
            .and_then(|value| value.get("by_action"))
            .and_then(|value| value.get("delete_created_file"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        inspect_payload
            .get("rollback_plan")
            .and_then(|value| value.get("executable_record_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        inspect_payload
            .get("records")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("rollback_plan"))
            .and_then(|value| value.get("operations"))
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|value| value.get("operation"))
            .and_then(|value| value.get("kind"))
            .and_then(Value::as_str),
        Some("delete_file")
    );

    let preview_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-mutation-1/checkpoints/mutations/rollback-preview")
        .body(Body::empty())
        .expect("preview request");
    let preview_resp = app
        .clone()
        .oneshot(preview_req)
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");

    assert_eq!(
        preview_payload.get("executable").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        preview_payload
            .get("steps")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        preview_payload
            .get("steps")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|value| value.get("operations"))
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|value| value.get("operation"))
            .and_then(|value| value.get("kind"))
            .and_then(Value::as_str),
        Some("delete_file")
    );
}

#[tokio::test]
async fn rollback_execute_applies_executable_preview_steps() {
    let _guard = context_run_mutation_checkpoint_test_lock().lock().await;
    let state = test_state().await;
    let app = app_router(state.clone());
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-context-rollback-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-mutation-exec-1",
                "objective": "execute rollback",
                "workspace": {
                    "workspace_id": "ctx-run-mutation-exec-1",
                    "canonical_path": workspace_root.to_string_lossy(),
                    "lease_epoch": 0
                }
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

    let get_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-mutation-exec-1")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    let workspace_root = get_payload["run"]["workspace"]["canonical_path"]
        .as_str()
        .expect("workspace path");
    let target_path = std::path::Path::new(workspace_root).join("src/lib.rs");
    std::fs::create_dir_all(target_path.parent().expect("parent")).expect("create parent");
    std::fs::write(&target_path, "temporary").expect("write file");

    let event_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-mutation-exec-1/events")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "type": "mutation_checkpoint_recorded",
                "status": "running",
                "payload": {
                    "record": {
                        "session_id": "session-1",
                        "message_id": "message-1",
                        "tool": "write",
                        "outcome": "succeeded",
                        "file_count": 1,
                        "changed_file_count": 1,
                        "files": [{
                            "path": "src/lib.rs",
                            "resolved_path": target_path.to_string_lossy().to_string(),
                            "existed_before": false,
                            "existed_after": true,
                            "changed": true,
                            "rollback_snapshot": {
                                "status": "not_needed"
                            }
                        }]
                    }
                }
            })
            .to_string(),
        ))
        .expect("event request");
    let event_resp = app
        .clone()
        .oneshot(event_req)
        .await
        .expect("event response");
    assert_eq!(event_resp.status(), StatusCode::OK);

    let get_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-mutation-exec-1")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    let mut paused_run = get_payload["run"].clone();
    paused_run["status"] = Value::String("paused".to_string());
    let put_req = Request::builder()
        .method("PUT")
        .uri("/context/runs/ctx-run-mutation-exec-1")
        .header("content-type", "application/json")
        .body(Body::from(paused_run.to_string()))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let preview_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-mutation-exec-1/checkpoints/mutations/rollback-preview")
        .body(Body::empty())
        .expect("preview request");
    let preview_resp = app
        .clone()
        .oneshot(preview_req)
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let selected_event_id = preview_payload
        .get("steps")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("event_id"))
        .and_then(Value::as_str)
        .expect("preview event id")
        .to_string();

    let execute_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-mutation-exec-1/checkpoints/mutations/rollback-execute")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "confirm": "rollback",
                "policy_ack": "allow_rollback_execution",
                "event_ids": [selected_event_id]
            })
            .to_string(),
        ))
        .expect("execute request");
    let execute_resp = app
        .clone()
        .oneshot(execute_req)
        .await
        .expect("execute response");
    assert_eq!(execute_resp.status(), StatusCode::OK);
    let execute_body = to_bytes(execute_resp.into_body(), usize::MAX)
        .await
        .expect("execute body");
    let execute_payload: Value = serde_json::from_slice(&execute_body).expect("execute json");

    assert_eq!(execute_payload["applied"].as_bool(), Some(true));
    assert_eq!(execute_payload["applied_operation_count"].as_u64(), Some(1));
    assert_eq!(
        execute_payload["applied_by_action"]["delete_created_file"].as_u64(),
        Some(1)
    );
    assert_eq!(
        execute_payload["steps"][0]["operations"][0]["kind"].as_str(),
        Some("delete_file")
    );
    assert!(!target_path.exists());

    let history_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-mutation-exec-1/checkpoints/mutations/rollback-history")
        .body(Body::empty())
        .expect("history request");
    let history_resp = app
        .clone()
        .oneshot(history_req)
        .await
        .expect("history response");
    assert_eq!(history_resp.status(), StatusCode::OK);
    let history_body = to_bytes(history_resp.into_body(), usize::MAX)
        .await
        .expect("history body");
    let history_payload: Value = serde_json::from_slice(&history_body).expect("history json");
    assert_eq!(
        history_payload["summary"]["by_outcome"]["applied"].as_u64(),
        Some(1)
    );
}

#[tokio::test]
async fn rollback_execute_blocks_advisory_only_steps() {
    let _guard = context_run_mutation_checkpoint_test_lock().lock().await;
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-mutation-blocked-1",
                "objective": "blocked rollback",
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

    let event_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-mutation-blocked-1/events")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "type": "mutation_checkpoint_recorded",
                "status": "running",
                "payload": {
                    "record": {
                        "session_id": "session-1",
                        "message_id": "message-1",
                        "tool": "edit",
                        "outcome": "succeeded",
                        "file_count": 1,
                        "changed_file_count": 1,
                        "files": [{
                            "path": "src/lib.rs",
                            "resolved_path": "/workspace/src/lib.rs",
                            "existed_before": true,
                            "existed_after": true,
                            "changed": true,
                            "rollback_snapshot": {
                                "status": "too_large",
                                "byte_count": 64000
                            }
                        }]
                    }
                }
            })
            .to_string(),
        ))
        .expect("event request");
    let event_resp = app
        .clone()
        .oneshot(event_req)
        .await
        .expect("event response");
    assert_eq!(event_resp.status(), StatusCode::OK);

    let get_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-mutation-blocked-1")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    let mut paused_run = get_payload["run"].clone();
    paused_run["status"] = Value::String("paused".to_string());
    let put_req = Request::builder()
        .method("PUT")
        .uri("/context/runs/ctx-run-mutation-blocked-1")
        .header("content-type", "application/json")
        .body(Body::from(paused_run.to_string()))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let preview_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-mutation-blocked-1/checkpoints/mutations/rollback-preview")
        .body(Body::empty())
        .expect("preview request");
    let preview_resp = app
        .clone()
        .oneshot(preview_req)
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let selected_event_id = preview_payload
        .get("steps")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("event_id"))
        .and_then(Value::as_str)
        .expect("preview event id")
        .to_string();

    let execute_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-mutation-blocked-1/checkpoints/mutations/rollback-execute")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "confirm": "rollback",
                "policy_ack": "allow_rollback_execution",
                "event_ids": [selected_event_id]
            })
            .to_string(),
        ))
        .expect("execute request");
    let execute_resp = app
        .clone()
        .oneshot(execute_req)
        .await
        .expect("execute response");
    assert_eq!(execute_resp.status(), StatusCode::OK);
    let execute_body = to_bytes(execute_resp.into_body(), usize::MAX)
        .await
        .expect("execute body");
    let execute_payload: Value = serde_json::from_slice(&execute_body).expect("execute json");

    assert_eq!(execute_payload["applied"].as_bool(), Some(false));
    assert_eq!(
        execute_payload["reason"].as_str(),
        Some("selected rollback step is advisory_only")
    );
}

#[tokio::test]
async fn rollback_execute_requires_explicit_step_selection() {
    let _guard = context_run_mutation_checkpoint_test_lock().lock().await;
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-mutation-missing-selection-1",
                "objective": "missing selection",
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

    let get_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-mutation-missing-selection-1")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    let mut paused_run = get_payload["run"].clone();
    paused_run["status"] = Value::String("paused".to_string());
    let put_req = Request::builder()
        .method("PUT")
        .uri("/context/runs/ctx-run-mutation-missing-selection-1")
        .header("content-type", "application/json")
        .body(Body::from(paused_run.to_string()))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let execute_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-mutation-missing-selection-1/checkpoints/mutations/rollback-execute")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "confirm": "rollback",
                "event_ids": []
            })
            .to_string(),
        ))
        .expect("execute request");
    let execute_resp = app
        .clone()
        .oneshot(execute_req)
        .await
        .expect("execute response");
    assert_eq!(execute_resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn rollback_execute_blocks_unknown_selected_step_ids() {
    let _guard = context_run_mutation_checkpoint_test_lock().lock().await;
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-mutation-unknown-selection-1",
                "objective": "unknown selection",
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

    let get_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-mutation-unknown-selection-1")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    let mut paused_run = get_payload["run"].clone();
    paused_run["status"] = Value::String("paused".to_string());
    let put_req = Request::builder()
        .method("PUT")
        .uri("/context/runs/ctx-run-mutation-unknown-selection-1")
        .header("content-type", "application/json")
        .body(Body::from(paused_run.to_string()))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let execute_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-mutation-unknown-selection-1/checkpoints/mutations/rollback-execute")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "confirm": "rollback",
                "policy_ack": "allow_rollback_execution",
                "event_ids": ["missing-event-id"]
            })
            .to_string(),
        ))
        .expect("execute request");
    let execute_resp = app
        .clone()
        .oneshot(execute_req)
        .await
        .expect("execute response");
    assert_eq!(execute_resp.status(), StatusCode::OK);
    let execute_body = to_bytes(execute_resp.into_body(), usize::MAX)
        .await
        .expect("execute body");
    let execute_payload: Value = serde_json::from_slice(&execute_body).expect("execute json");

    assert_eq!(execute_payload["applied"].as_bool(), Some(false));
    assert_eq!(
        execute_payload["reason"].as_str(),
        Some("selected rollback step was not found in current preview")
    );
    assert_eq!(
        execute_payload["missing_event_ids"][0].as_str(),
        Some("missing-event-id")
    );
}

#[tokio::test]
async fn rollback_execute_blocks_when_policy_ack_is_missing() {
    let _guard = context_run_mutation_checkpoint_test_lock().lock().await;
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-mutation-policy-1",
                "objective": "missing policy ack",
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

    let get_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-mutation-policy-1")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    let mut paused_run = get_payload["run"].clone();
    paused_run["status"] = Value::String("paused".to_string());
    let put_req = Request::builder()
        .method("PUT")
        .uri("/context/runs/ctx-run-mutation-policy-1")
        .header("content-type", "application/json")
        .body(Body::from(paused_run.to_string()))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let execute_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-mutation-policy-1/checkpoints/mutations/rollback-execute")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "confirm": "rollback",
                "event_ids": ["missing-event-id"]
            })
            .to_string(),
        ))
        .expect("execute request");
    let execute_resp = app
        .clone()
        .oneshot(execute_req)
        .await
        .expect("execute response");
    assert_eq!(execute_resp.status(), StatusCode::OK);
    let execute_body = to_bytes(execute_resp.into_body(), usize::MAX)
        .await
        .expect("execute body");
    let execute_payload: Value = serde_json::from_slice(&execute_body).expect("execute json");

    assert_eq!(execute_payload["applied"].as_bool(), Some(false));
    assert_eq!(
        execute_payload["reason"].as_str(),
        Some("rollback execution requires explicit policy acknowledgement")
    );
}

#[tokio::test]
async fn rollback_execute_blocks_when_run_status_is_not_eligible() {
    let _guard = context_run_mutation_checkpoint_test_lock().lock().await;
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-mutation-status-1",
                "objective": "status gate",
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

    let execute_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-mutation-status-1/checkpoints/mutations/rollback-execute")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "confirm": "rollback",
                "policy_ack": "allow_rollback_execution",
                "event_ids": ["missing-event-id"]
            })
            .to_string(),
        ))
        .expect("execute request");
    let execute_resp = app
        .clone()
        .oneshot(execute_req)
        .await
        .expect("execute response");
    assert_eq!(execute_resp.status(), StatusCode::OK);
    let execute_body = to_bytes(execute_resp.into_body(), usize::MAX)
        .await
        .expect("execute body");
    let execute_payload: Value = serde_json::from_slice(&execute_body).expect("execute json");

    assert_eq!(execute_payload["applied"].as_bool(), Some(false));
    assert_eq!(
        execute_payload["reason"].as_str(),
        Some("rollback execution is not allowed for the current run status")
    );
}
