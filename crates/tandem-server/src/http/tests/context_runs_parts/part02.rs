
#[tokio::test]
async fn context_task_fail_transition_publishes_engine_event() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-task-engine-event",
                "objective": "publish a task failure event"
            })
            .to_string(),
        ))
        .expect("create run request");
    let create_run_resp = app
        .clone()
        .oneshot(create_run_req)
        .await
        .expect("create run response");
    assert_eq!(create_run_resp.status(), StatusCode::OK);

    let create_tasks_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-engine-event/tasks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tasks": [
                    {
                        "id": "task-1",
                        "task_type": "unit_work",
                        "status": "in_progress",
                        "payload": {"title": "Break the build"}
                    }
                ]
            })
            .to_string(),
        ))
        .expect("create tasks request");
    let create_tasks_resp = app
        .clone()
        .oneshot(create_tasks_req)
        .await
        .expect("create tasks response");
    assert_eq!(create_tasks_resp.status(), StatusCode::OK);

    let transition_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-engine-event/tasks/task-1/transition")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "fail",
                "command_id": "cmd-fail-event",
                "error": "PROMPT_RETRY_FAILED"
            })
            .to_string(),
        ))
        .expect("transition request");
    let transition_resp = app
        .clone()
        .oneshot(transition_req)
        .await
        .expect("transition response");
    assert_eq!(transition_resp.status(), StatusCode::OK);

    let event = next_event_of_type(&mut rx, "context.task.failed").await;
    assert_eq!(
        event.properties.get("runID").and_then(Value::as_str),
        Some("ctx-run-task-engine-event")
    );
    assert_eq!(
        event.properties.get("taskID").and_then(Value::as_str),
        Some("task-1")
    );
    assert_eq!(
        event.properties.get("error").and_then(Value::as_str),
        Some("PROMPT_RETRY_FAILED")
    );
}

#[tokio::test]
async fn context_task_create_rejects_implementation_without_output_target() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-task-contract-invalid",
                "objective": "contract validation"
            })
            .to_string(),
        ))
        .expect("create run request");
    let create_run_resp = app
        .clone()
        .oneshot(create_run_req)
        .await
        .expect("create run response");
    assert_eq!(create_run_resp.status(), StatusCode::OK);

    let create_task_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-contract-invalid/tasks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tasks": [
                    {
                        "id": "task-1",
                        "task_type": "implementation",
                        "status": "runnable",
                        "payload": {
                            "title": "Create the scaffold",
                            "task_kind": "implementation"
                        }
                    }
                ]
            })
            .to_string(),
        ))
        .expect("create task request");
    let create_task_resp = app
        .clone()
        .oneshot(create_task_req)
        .await
        .expect("create task response");
    assert_eq!(create_task_resp.status(), StatusCode::OK);
    let body = to_bytes(create_task_resp.into_body(), usize::MAX)
        .await
        .expect("create task body");
    let payload: Value = serde_json::from_slice(&body).expect("create task json");
    assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("TASK_OUTPUT_TARGET_REQUIRED")
    );
}

#[tokio::test]
async fn context_task_create_normalizes_nonwriting_contract_fields() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-task-contract-normalized",
                "objective": "contract normalization"
            })
            .to_string(),
        ))
        .expect("create run request");
    let create_run_resp = app
        .clone()
        .oneshot(create_run_req)
        .await
        .expect("create run response");
    assert_eq!(create_run_resp.status(), StatusCode::OK);

    let create_task_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-contract-normalized/tasks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tasks": [
                    {
                        "id": "task-1",
                        "task_type": "inspection",
                        "status": "runnable",
                        "payload": {
                            "title": "Inspect workspace and choose artifact path",
                            "task_kind": "inspection"
                        }
                    }
                ]
            })
            .to_string(),
        ))
        .expect("create task request");
    let create_task_resp = app
        .clone()
        .oneshot(create_task_req)
        .await
        .expect("create task response");
    assert_eq!(create_task_resp.status(), StatusCode::OK);
    let body = to_bytes(create_task_resp.into_body(), usize::MAX)
        .await
        .expect("create task body");
    let payload: Value = serde_json::from_slice(&body).expect("create task json");
    assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(true));
    let task = payload
        .get("tasks")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .expect("task");
    assert_eq!(
        task.get("task_type").and_then(Value::as_str),
        Some("inspection")
    );
    assert_eq!(
        task.get("payload")
            .and_then(|row| row.get("execution_mode"))
            .and_then(Value::as_str),
        Some("strict_nonwriting")
    );
}

#[tokio::test]
async fn context_blackboard_patches_endpoint_includes_task_patch() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-bbp-task",
                "objective": "blackboard patches contract"
            })
            .to_string(),
        ))
        .expect("create run request");
    let create_run_resp = app
        .clone()
        .oneshot(create_run_req)
        .await
        .expect("create run response");
    assert_eq!(create_run_resp.status(), StatusCode::OK);

    let create_task_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-bbp-task/tasks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tasks": [
                    {
                        "id": "task-1",
                        "task_type": "analysis",
                        "status": "runnable",
                        "command_id": "task-create-1"
                    }
                ]
            })
            .to_string(),
        ))
        .expect("create task request");
    let create_task_resp = app
        .clone()
        .oneshot(create_task_req)
        .await
        .expect("create task response");
    assert_eq!(create_task_resp.status(), StatusCode::OK);

    let patches_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-bbp-task/blackboard/patches")
        .body(Body::empty())
        .expect("patches request");
    let patches_resp = app
        .clone()
        .oneshot(patches_req)
        .await
        .expect("patches response");
    assert_eq!(patches_resp.status(), StatusCode::OK);
    let patches_body = to_bytes(patches_resp.into_body(), usize::MAX)
        .await
        .expect("patches body");
    let patches_payload: Value = serde_json::from_slice(&patches_body).expect("patches json");
    let rows = patches_payload
        .get("patches")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(rows.iter().any(|row| {
        row.get("op")
            .and_then(Value::as_str)
            .map(|op| op == "add_task")
            .unwrap_or(false)
    }));
}

#[tokio::test]
async fn context_blackboard_legacy_payload_without_tasks_is_backward_compatible() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-legacy-blackboard",
                "objective": "legacy compatibility"
            })
            .to_string(),
        ))
        .expect("create run request");
    let create_run_resp = app
        .clone()
        .oneshot(create_run_req)
        .await
        .expect("create run response");
    assert_eq!(create_run_resp.status(), StatusCode::OK);

    let legacy_blackboard_path = super::super::context_runs::context_run_blackboard_path(
        &state,
        "ctx-run-legacy-blackboard",
    );
    std::fs::write(
        &legacy_blackboard_path,
        json!({
            "facts": [{"id":"f-1","ts_ms":1,"text":"legacy fact"}],
            "decisions": [],
            "open_questions": [],
            "artifacts": [],
            "summaries": {"rolling":"legacy rolling","latest_context_pack":""},
            "revision": 7
        })
        .to_string(),
    )
    .expect("write legacy blackboard");

    let get_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-legacy-blackboard/blackboard")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .get("blackboard")
            .and_then(|v| v.get("revision"))
            .and_then(Value::as_u64),
        Some(7)
    );
    assert_eq!(
        payload
            .get("blackboard")
            .and_then(|v| v.get("tasks"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        payload
            .get("blackboard")
            .and_then(|v| v.get("facts"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
}

#[tokio::test]
async fn context_blackboard_patch_rejects_task_mutation_ops() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-patch-reject",
                "objective": "reject blackboard task ops"
            })
            .to_string(),
        ))
        .expect("create run request");
    let create_run_resp = app
        .clone()
        .oneshot(create_run_req)
        .await
        .expect("create run response");
    assert_eq!(create_run_resp.status(), StatusCode::OK);

    let patch_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-patch-reject/blackboard/patches")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "op": "add_task",
                "payload": {
                    "id": "task-1",
                    "task_type": "analysis",
                    "status": "runnable"
                }
            })
            .to_string(),
        ))
        .expect("patch request");
    let patch_resp = app
        .clone()
        .oneshot(patch_req)
        .await
        .expect("patch response");
    assert_eq!(patch_resp.status(), StatusCode::OK);
    let patch_body = to_bytes(patch_resp.into_body(), usize::MAX)
        .await
        .expect("patch body");
    let patch_payload: Value = serde_json::from_slice(&patch_body).expect("patch json");
    assert_eq!(
        patch_payload.get("ok").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        patch_payload.get("code").and_then(Value::as_str),
        Some("TASK_PATCH_OP_DISABLED")
    );
}

#[tokio::test]
async fn context_blackboard_persistence_omits_task_rows_after_task_creation() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-blackboard-persist",
                "objective": "persist blackboard without task rows"
            })
            .to_string(),
        ))
        .expect("create run request");
    let create_run_resp = app
        .clone()
        .oneshot(create_run_req)
        .await
        .expect("create run response");
    assert_eq!(create_run_resp.status(), StatusCode::OK);

    let create_task_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-blackboard-persist/tasks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tasks": [
                    {
                        "id": "task-1",
                        "task_type": "analysis",
                        "status": "runnable"
                    }
                ]
            })
            .to_string(),
        ))
        .expect("create task request");
    let create_task_resp = app
        .clone()
        .oneshot(create_task_req)
        .await
        .expect("create task response");
    assert_eq!(create_task_resp.status(), StatusCode::OK);

    let persisted_blackboard_path = super::super::context_runs::context_run_blackboard_path(
        &state,
        "ctx-run-blackboard-persist",
    );
    let persisted_blackboard_raw =
        std::fs::read_to_string(&persisted_blackboard_path).expect("read persisted blackboard");
    let persisted_blackboard: Value =
        serde_json::from_str(&persisted_blackboard_raw).expect("persisted blackboard json");
    assert_eq!(
        persisted_blackboard
            .get("tasks")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );

    let get_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-blackboard-persist/blackboard")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    assert_eq!(
        get_payload
            .get("blackboard")
            .and_then(|v| v.get("tasks"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
}

#[tokio::test]
async fn context_tasks_claim_and_transition_contract_roundtrip() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-task-contract",
                "objective": "task contract"
            })
            .to_string(),
        ))
        .expect("create run request");
    let create_run_resp = app
        .clone()
        .oneshot(create_run_req)
        .await
        .expect("create run response");
    assert_eq!(create_run_resp.status(), StatusCode::OK);

    let create_task_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-contract/tasks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tasks": [
                    {
                        "id": "task-1",
                        "task_type": "build",
                        "status": "runnable",
                        "payload": {"title":"Build"}
                    }
                ]
            })
            .to_string(),
        ))
        .expect("create task request");
    let create_task_resp = app
        .clone()
        .oneshot(create_task_req)
        .await
        .expect("create task response");
    assert_eq!(create_task_resp.status(), StatusCode::OK);

    let claim_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-contract/tasks/claim")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "agent_id": "agent-contract",
                "command_id": "claim-contract-1"
            })
            .to_string(),
        ))
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
    let task_rev = claim_payload
        .get("task")
        .and_then(|v| v.get("task_rev"))
        .and_then(Value::as_u64)
        .expect("task_rev");
    let lease_token = claim_payload
        .get("task")
        .and_then(|v| v.get("lease_token"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .expect("lease token");

    let complete_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-contract/tasks/task-1/transition")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "complete",
                "expected_task_rev": task_rev,
                "lease_token": lease_token,
                "agent_id": "agent-contract",
                "command_id": "complete-contract-1"
            })
            .to_string(),
        ))
        .expect("complete request");
    let complete_resp = app
        .clone()
        .oneshot(complete_req)
        .await
        .expect("complete response");
    assert_eq!(complete_resp.status(), StatusCode::OK);
    let complete_body = to_bytes(complete_resp.into_body(), usize::MAX)
        .await
        .expect("complete body");
    let complete_payload: Value = serde_json::from_slice(&complete_body).expect("complete json");
    assert_eq!(
        complete_payload
            .get("task")
            .and_then(|v| v.get("status"))
            .and_then(Value::as_str),
        Some("done")
    );
    assert!(complete_payload
        .get("patch")
        .and_then(|v| v.get("seq"))
        .and_then(Value::as_u64)
        .is_some());
}

#[tokio::test]
async fn context_task_events_include_patch_seq_after_commit_helper_refactor() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-task-event-patch-seq",
                "objective": "task events keep patch sequence"
            })
            .to_string(),
        ))
        .expect("create run request");
    let create_run_resp = app
        .clone()
        .oneshot(create_run_req)
        .await
        .expect("create run response");
    assert_eq!(create_run_resp.status(), StatusCode::OK);

    let create_task_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-event-patch-seq/tasks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tasks": [
                    {
                        "id": "task-1",
                        "task_type": "build",
                        "status": "runnable"
                    }
                ]
            })
            .to_string(),
        ))
        .expect("create task request");
    let create_task_resp = app
        .clone()
        .oneshot(create_task_req)
        .await
        .expect("create task response");
    assert_eq!(create_task_resp.status(), StatusCode::OK);

    let claim_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-event-patch-seq/tasks/claim")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "agent_id": "agent-contract"
            })
            .to_string(),
        ))
        .expect("claim request");
    let claim_resp = app
        .clone()
        .oneshot(claim_req)
        .await
        .expect("claim response");
    assert_eq!(claim_resp.status(), StatusCode::OK);

    let events_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-task-event-patch-seq/events")
        .body(Body::empty())
        .expect("events request");
    let events_resp = app
        .clone()
        .oneshot(events_req)
        .await
        .expect("events response");
    assert_eq!(events_resp.status(), StatusCode::OK);
    let events_body = to_bytes(events_resp.into_body(), usize::MAX)
        .await
        .expect("events body");
    let events_payload: Value = serde_json::from_slice(&events_body).expect("events json");
    let rows = events_payload
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let task_events = rows
        .into_iter()
        .filter(|row| {
            row.get("type")
                .and_then(Value::as_str)
                .map(|ty| ty.starts_with("context.task."))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    assert!(!task_events.is_empty());
    assert!(task_events.iter().all(|row| {
        row.get("payload")
            .and_then(|payload| payload.get("patch_seq"))
            .and_then(Value::as_u64)
            .is_some()
    }));
}

#[tokio::test]
async fn context_task_commands_are_idempotent_and_patch_seq_is_monotonic() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-task-idempotency-matrix",
                "objective": "idempotency matrix"
            })
            .to_string(),
        ))
        .expect("create run request");
    let create_run_resp = app
        .clone()
        .oneshot(create_run_req)
        .await
        .expect("create run response");
    assert_eq!(create_run_resp.status(), StatusCode::OK);

    let create_task_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-idempotency-matrix/tasks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tasks": [
                    {
                        "id": "task-1",
                        "task_type": "analysis",
                        "status": "runnable",
                        "command_id": "create-task-cmd-1"
                    }
                ]
            })
            .to_string(),
        ))
        .expect("create task request");
    let create_task_resp = app
        .clone()
        .oneshot(create_task_req)
        .await
        .expect("create task response");
    assert_eq!(create_task_resp.status(), StatusCode::OK);

    let create_task_dedup_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-idempotency-matrix/tasks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tasks": [
                    {
                        "id": "task-1",
                        "task_type": "analysis",
                        "status": "runnable",
                        "command_id": "create-task-cmd-1"
                    }
                ]
            })
            .to_string(),
        ))
        .expect("create task dedup request");
    let create_task_dedup_resp = app
        .clone()
        .oneshot(create_task_dedup_req)
        .await
        .expect("create task dedup response");
    assert_eq!(create_task_dedup_resp.status(), StatusCode::OK);
    let create_task_dedup_body = to_bytes(create_task_dedup_resp.into_body(), usize::MAX)
        .await
        .expect("create dedup body");
    let create_task_dedup_payload: Value =
        serde_json::from_slice(&create_task_dedup_body).expect("create dedup json");
    assert_eq!(
        create_task_dedup_payload
            .get("tasks")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );

    let claim_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-idempotency-matrix/tasks/claim")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "agent_id": "agent-idempotent",
                "command_id": "claim-task-cmd-1"
            })
            .to_string(),
        ))
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
    let claim_task_rev = claim_payload
        .get("task")
        .and_then(|v| v.get("task_rev"))
        .and_then(Value::as_u64)
        .expect("claim task rev");
    let lease_token = claim_payload
        .get("task")
        .and_then(|v| v.get("lease_token"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .expect("claim lease token");

    let claim_dedup_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-idempotency-matrix/tasks/claim")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "agent_id": "agent-idempotent",
                "command_id": "claim-task-cmd-1"
            })
            .to_string(),
        ))
        .expect("claim dedup request");
    let claim_dedup_resp = app
        .clone()
        .oneshot(claim_dedup_req)
        .await
        .expect("claim dedup response");
    assert_eq!(claim_dedup_resp.status(), StatusCode::OK);
    let claim_dedup_body = to_bytes(claim_dedup_resp.into_body(), usize::MAX)
        .await
        .expect("claim dedup body");
    let claim_dedup_payload: Value =
        serde_json::from_slice(&claim_dedup_body).expect("claim dedup json");
    assert_eq!(
        claim_dedup_payload.get("deduped").and_then(Value::as_bool),
        Some(true)
    );
    assert!(claim_dedup_payload
        .get("task")
        .map(Value::is_null)
        .unwrap_or(false));

    let complete_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-idempotency-matrix/tasks/task-1/transition")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "complete",
                "agent_id": "agent-idempotent",
                "command_id": "complete-task-cmd-1",
                "expected_task_rev": claim_task_rev,
                "lease_token": lease_token
            })
            .to_string(),
        ))
        .expect("complete request");
    let complete_resp = app
        .clone()
        .oneshot(complete_req)
        .await
        .expect("complete response");
    assert_eq!(complete_resp.status(), StatusCode::OK);

    let complete_dedup_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-idempotency-matrix/tasks/task-1/transition")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "complete",
                "agent_id": "agent-idempotent",
                "command_id": "complete-task-cmd-1",
                "expected_task_rev": claim_task_rev + 1
            })
            .to_string(),
        ))
        .expect("complete dedup request");
    let complete_dedup_resp = app
        .clone()
        .oneshot(complete_dedup_req)
        .await
        .expect("complete dedup response");
    assert_eq!(complete_dedup_resp.status(), StatusCode::OK);
    let complete_dedup_body = to_bytes(complete_dedup_resp.into_body(), usize::MAX)
        .await
        .expect("complete dedup body");
    let complete_dedup_payload: Value =
        serde_json::from_slice(&complete_dedup_body).expect("complete dedup json");
    assert_eq!(
        complete_dedup_payload
            .get("deduped")
            .and_then(Value::as_bool),
        Some(true)
    );

    let patches_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-task-idempotency-matrix/blackboard/patches")
        .body(Body::empty())
        .expect("patches request");
    let patches_resp = app
        .clone()
        .oneshot(patches_req)
        .await
        .expect("patches response");
    assert_eq!(patches_resp.status(), StatusCode::OK);
    let patches_body = to_bytes(patches_resp.into_body(), usize::MAX)
        .await
        .expect("patches body");
    let patches_payload: Value = serde_json::from_slice(&patches_body).expect("patches json");
    let rows = patches_payload
        .get("patches")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(rows.len(), 3);
    let mut seqs = rows
        .iter()
        .filter_map(|row| row.get("seq").and_then(Value::as_u64))
        .collect::<Vec<_>>();
    assert_eq!(seqs.len(), 3);
    let mut sorted = seqs.clone();
    sorted.sort_unstable();
    assert_eq!(seqs, sorted);
    assert_eq!(
        rows.iter()
            .filter_map(|row| row.get("op").and_then(Value::as_str))
            .collect::<Vec<_>>(),
        vec!["add_task", "update_task_state", "update_task_state"]
    );
    seqs.dedup();
    assert_eq!(seqs.len(), 3);
}

#[tokio::test]
async fn context_events_endpoint_rejects_task_event_types() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-event-task-reject",
                "objective": "reject task event append"
            })
            .to_string(),
        ))
        .expect("create run request");
    let create_run_resp = app
        .clone()
        .oneshot(create_run_req)
        .await
        .expect("create run response");
    assert_eq!(create_run_resp.status(), StatusCode::OK);

    let event_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-event-task-reject/events")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "type": "context.task.completed",
                "status": "running",
                "step_id": "task-1",
                "payload": {"task_id":"task-1"}
            })
            .to_string(),
        ))
        .expect("task event request");
    let event_resp = app
        .clone()
        .oneshot(event_req)
        .await
        .expect("task event response");
    assert_eq!(event_resp.status(), StatusCode::OK);
    let body = to_bytes(event_resp.into_body(), usize::MAX)
        .await
        .expect("task event body");
    let payload: Value = serde_json::from_slice(&body).expect("task event json");
    assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("TASK_EVENT_APPEND_DISABLED")
    );
}

#[tokio::test]
async fn context_task_events_include_revision_and_task_id() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-task-event-fields",
                "objective": "task event fields"
            })
            .to_string(),
        ))
        .expect("create run request");
    let create_run_resp = app
        .clone()
        .oneshot(create_run_req)
        .await
        .expect("create run response");
    assert_eq!(create_run_resp.status(), StatusCode::OK);

    let create_task_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-event-fields/tasks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tasks": [
                    {
                        "id": "task-1",
                        "task_type": "analysis",
                        "status": "runnable"
                    }
                ]
            })
            .to_string(),
        ))
        .expect("create task request");
    let create_task_resp = app
        .clone()
        .oneshot(create_task_req)
        .await
        .expect("create task response");
    assert_eq!(create_task_resp.status(), StatusCode::OK);

    let events_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-task-event-fields/events")
        .body(Body::empty())
        .expect("events request");
    let events_resp = app
        .clone()
        .oneshot(events_req)
        .await
        .expect("events response");
    assert_eq!(events_resp.status(), StatusCode::OK);
    let body = to_bytes(events_resp.into_body(), usize::MAX)
        .await
        .expect("events body");
    let payload: Value = serde_json::from_slice(&body).expect("events json");
    let first = payload
        .get("events")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned()
        .expect("first event");
    assert_eq!(first.get("task_id").and_then(Value::as_str), Some("task-1"));
    assert!(first.get("revision").and_then(Value::as_u64).is_some());
}

#[tokio::test]
async fn context_run_get_repairs_snapshot_from_event_log() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-repair-snapshot",
                "objective": "repair snapshot"
            })
            .to_string(),
        ))
        .expect("create run request");
    let create_run_resp = app
        .clone()
        .oneshot(create_run_req)
        .await
        .expect("create run response");
    assert_eq!(create_run_resp.status(), StatusCode::OK);

    let event_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-repair-snapshot/events")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "type": "planning_started",
                "status": "planning",
                "payload": {"why_next_step":"repair me"}
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

    let run_state_path =
        super::super::context_runs::context_run_state_path(&state, "ctx-run-repair-snapshot");
    std::fs::write(
        &run_state_path,
        json!({
            "run_id": "ctx-run-repair-snapshot",
            "run_type": "interactive",
            "mcp_servers": [],
            "status": "queued",
            "objective": "repair snapshot",
            "workspace": {
                "workspace_id": "",
                "canonical_path": "",
                "lease_epoch": 0
            },
            "steps": [],
            "tasks": [],
            "why_next_step": null,
            "revision": 1,
            "last_event_seq": 0,
            "created_at_ms": 1,
            "updated_at_ms": 1
        })
        .to_string(),
    )
    .expect("write stale run state");

    let get_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-repair-snapshot")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|run| run.get("status"))
            .and_then(Value::as_str),
        Some("awaiting_approval")
    );
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|run| run.get("last_event_seq"))
            .and_then(Value::as_u64),
        Some(1)
    );
}

#[tokio::test]
async fn context_blackboard_get_repairs_projection_from_patch_log() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-repair-blackboard",
                "objective": "repair blackboard"
            })
            .to_string(),
        ))
        .expect("create run request");
    let create_run_resp = app
        .clone()
        .oneshot(create_run_req)
        .await
        .expect("create run response");
    assert_eq!(create_run_resp.status(), StatusCode::OK);

    let create_task_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-repair-blackboard/tasks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tasks": [
                    {
                        "id": "task-1",
                        "task_type": "analysis",
                        "status": "runnable"
                    }
                ]
            })
            .to_string(),
        ))
        .expect("create task request");
    let create_task_resp = app
        .clone()
        .oneshot(create_task_req)
        .await
        .expect("create task response");
    assert_eq!(create_task_resp.status(), StatusCode::OK);

    let blackboard_path = super::super::context_runs::context_run_blackboard_path(
        &state,
        "ctx-run-repair-blackboard",
    );
    std::fs::write(&blackboard_path, json!({"revision":0,"facts":[],"decisions":[],"open_questions":[],"artifacts":[],"tasks":[],"summaries":{"rolling":"","latest_context_pack":""}}).to_string())
        .expect("write stale blackboard");

    let get_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-repair-blackboard/blackboard")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    assert_eq!(
        get_payload
            .get("blackboard")
            .and_then(|bb| bb.get("revision"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        get_payload
            .get("blackboard")
            .and_then(|bb| bb.get("tasks"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
}

#[tokio::test]
async fn context_runs_mutate_independently_under_concurrency() {
    let state = test_state().await;
    let app = app_router(state.clone());

    for run_id in ["ctx-run-a", "ctx-run-b"] {
        let create_run_req = Request::builder()
            .method("POST")
            .uri("/context/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": run_id,
                    "objective": format!("run {}", run_id)
                })
                .to_string(),
            ))
            .expect("create run request");
        let create_run_resp = app
            .clone()
            .oneshot(create_run_req)
            .await
            .expect("create run response");
        assert_eq!(create_run_resp.status(), StatusCode::OK);

        let create_task_req = Request::builder()
            .method("POST")
            .uri(format!("/context/runs/{run_id}/tasks"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "tasks": [
                        {
                            "id": "task-1",
                            "task_type": "analysis",
                            "status": "runnable"
                        }
                    ]
                })
                .to_string(),
            ))
            .expect("create task request");
        let create_task_resp = app
            .clone()
            .oneshot(create_task_req)
            .await
            .expect("create task response");
        assert_eq!(create_task_resp.status(), StatusCode::OK);
    }

    let claim_a = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-a/tasks/claim")
        .header("content-type", "application/json")
        .body(Body::from(json!({"agent_id":"agent-a"}).to_string()))
        .expect("claim a request");
    let claim_b = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-b/tasks/claim")
        .header("content-type", "application/json")
        .body(Body::from(json!({"agent_id":"agent-b"}).to_string()))
        .expect("claim b request");

    let (resp_a, resp_b) = tokio::join!(app.clone().oneshot(claim_a), app.clone().oneshot(claim_b));
    let resp_a = resp_a.expect("claim a response");
    let resp_b = resp_b.expect("claim b response");
    assert_eq!(resp_a.status(), StatusCode::OK);
    assert_eq!(resp_b.status(), StatusCode::OK);

    let body_a = to_bytes(resp_a.into_body(), usize::MAX)
        .await
        .expect("claim a body");
    let body_b = to_bytes(resp_b.into_body(), usize::MAX)
        .await
        .expect("claim b body");
    let payload_a: Value = serde_json::from_slice(&body_a).expect("claim a json");
    let payload_b: Value = serde_json::from_slice(&body_b).expect("claim b json");
    assert_eq!(
        payload_a
            .get("task")
            .and_then(|task| task.get("lease_owner"))
            .and_then(Value::as_str),
        Some("agent-a")
    );
    assert_eq!(
        payload_b
            .get("task")
            .and_then(|task| task.get("lease_owner"))
            .and_then(Value::as_str),
        Some("agent-b")
    );
}
