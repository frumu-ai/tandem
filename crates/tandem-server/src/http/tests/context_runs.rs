use super::*;

#[test]
fn load_run_events_jsonl_filters_since_and_tail() {
    let test_root = std::env::temp_dir().join(format!("run-events-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&test_root).expect("mkdir");
    let path = test_root.join("events.jsonl");
    std::fs::write(
        &path,
        [
            serde_json::json!({"seq":1,"type":"run_created"}).to_string(),
            serde_json::json!({"seq":2,"type":"planning_started"}).to_string(),
            serde_json::json!({"seq":3,"type":"task_started"}).to_string(),
        ]
        .join("\n"),
    )
    .expect("write");

    let since = load_run_events_jsonl(&path, Some(1), None);
    assert_eq!(since.len(), 2);
    assert_eq!(since[0].get("seq").and_then(|v| v.as_u64()), Some(2));
    assert_eq!(since[1].get("seq").and_then(|v| v.as_u64()), Some(3));

    let tail = load_run_events_jsonl(&path, None, Some(1));
    assert_eq!(tail.len(), 1);
    assert_eq!(tail[0].get("seq").and_then(|v| v.as_u64()), Some(3));

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir_all(&test_root);
}

#[tokio::test]
async fn context_run_create_append_event_and_get() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-1",
                "objective": "test context run",
                "model_provider": "openrouter",
                "model_id": "google/gemini-3-flash-preview"
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
        .uri("/context/runs/ctx-run-1/events")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "type": "planning_started",
                "status": "planning",
                "payload": {"k":"v"}
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

    let list_events_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-1/events?since_seq=0")
        .body(Body::empty())
        .expect("list events request");
    let list_events_resp = app
        .clone()
        .oneshot(list_events_req)
        .await
        .expect("list events response");
    assert_eq!(list_events_resp.status(), StatusCode::OK);
    let list_events_body = to_bytes(list_events_resp.into_body(), usize::MAX)
        .await
        .expect("list events body");
    let list_events_payload: Value =
        serde_json::from_slice(&list_events_body).expect("list events json");
    assert_eq!(
        list_events_payload
            .get("events")
            .and_then(|v| v.as_array())
            .map(|rows| rows.len()),
        Some(1)
    );

    let get_run_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-1")
        .body(Body::empty())
        .expect("get run request");
    let get_run_resp = app
        .clone()
        .oneshot(get_run_req)
        .await
        .expect("get run response");
    assert_eq!(get_run_resp.status(), StatusCode::OK);
    let get_run_body = to_bytes(get_run_resp.into_body(), usize::MAX)
        .await
        .expect("get run body");
    let get_run_payload: Value = serde_json::from_slice(&get_run_body).expect("get run json");
    assert_eq!(
        get_run_payload
            .get("run")
            .and_then(|run| run.get("status"))
            .and_then(Value::as_str),
        Some("awaiting_approval")
    );
    assert_eq!(
        get_run_payload
            .get("run")
            .and_then(|run| run.get("model_provider"))
            .and_then(Value::as_str),
        Some("openrouter")
    );
    assert_eq!(
        get_run_payload
            .get("run")
            .and_then(|run| run.get("model_id"))
            .and_then(Value::as_str),
        Some("google/gemini-3-flash-preview")
    );
}

#[tokio::test]
async fn context_run_event_step_completed_sets_done_status() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-step-done",
                "objective": "step done transition"
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

    let sync_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-step-done/todos/sync")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "replace": true,
                "todos": [{"id":"step-1","content":"Do thing","status":"pending"}]
            })
            .to_string(),
        ))
        .expect("sync request");
    let sync_resp = app.clone().oneshot(sync_req).await.expect("sync response");
    assert_eq!(sync_resp.status(), StatusCode::OK);

    let started_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-step-done/events")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "type": "step_started",
                "status": "running",
                "step_id": "step-1",
                "payload": {"step_status":"in_progress"}
            })
            .to_string(),
        ))
        .expect("started request");
    let started_resp = app
        .clone()
        .oneshot(started_req)
        .await
        .expect("started response");
    assert_eq!(started_resp.status(), StatusCode::OK);

    let completed_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-step-done/events")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "type": "step_completed",
                "status": "running",
                "step_id": "step-1",
                "payload": {"step_status":"done"}
            })
            .to_string(),
        ))
        .expect("completed request");
    let completed_resp = app
        .clone()
        .oneshot(completed_req)
        .await
        .expect("completed response");
    assert_eq!(completed_resp.status(), StatusCode::OK);

    let get_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-step-done")
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
            .and_then(|run| run.get("steps"))
            .and_then(Value::as_array)
            .and_then(|steps| steps.first())
            .and_then(|step| step.get("status"))
            .and_then(Value::as_str),
        Some("done")
    );
}

#[tokio::test]
async fn context_run_list_supports_workspace_filter_and_limit() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_one = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-list-1",
                "objective": "first",
                "workspace": {
                    "workspace_id": "ws-1",
                    "canonical_path": "/tmp/ws-one",
                    "lease_epoch": 1
                }
            })
            .to_string(),
        ))
        .expect("create one request");
    let create_two = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-list-2",
                "objective": "second",
                "workspace": {
                    "workspace_id": "ws-2",
                    "canonical_path": "/tmp/ws-two",
                    "lease_epoch": 1
                }
            })
            .to_string(),
        ))
        .expect("create two request");
    let _ = app.clone().oneshot(create_one).await.expect("create one");
    let _ = app.clone().oneshot(create_two).await.expect("create two");

    let filtered_req = Request::builder()
        .method("GET")
        .uri("/context/runs?workspace=/tmp/ws-two&limit=1")
        .body(Body::empty())
        .expect("filtered list request");
    let filtered_resp = app
        .clone()
        .oneshot(filtered_req)
        .await
        .expect("filtered list response");
    assert_eq!(filtered_resp.status(), StatusCode::OK);
    let filtered_body = to_bytes(filtered_resp.into_body(), usize::MAX)
        .await
        .expect("filtered list body");
    let filtered_payload: Value =
        serde_json::from_slice(&filtered_body).expect("filtered list json");
    let rows = filtered_payload
        .get("runs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0]
            .get("workspace")
            .and_then(|v| v.get("canonical_path"))
            .and_then(Value::as_str),
        Some("/tmp/ws-two")
    );
}

#[tokio::test]
async fn context_run_lease_mismatch_pauses_run() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-lease",
                "objective": "lease mismatch",
                "workspace": {
                    "workspace_id": "ws-1",
                    "canonical_path": "/expected/path",
                    "lease_epoch": 1
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

    let validate_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-lease/lease/validate")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "phase": "pre_dispatch",
                "current_path": "/other/path",
                "step_id": "step-1"
            })
            .to_string(),
        ))
        .expect("validate request");
    let validate_resp = app
        .clone()
        .oneshot(validate_req)
        .await
        .expect("validate response");
    assert_eq!(validate_resp.status(), StatusCode::OK);
    let validate_body = to_bytes(validate_resp.into_body(), usize::MAX)
        .await
        .expect("validate body");
    let validate_payload: Value = serde_json::from_slice(&validate_body).expect("validate json");
    assert_eq!(
        validate_payload.get("mismatch").and_then(Value::as_bool),
        Some(true)
    );

    let get_run_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-lease")
        .body(Body::empty())
        .expect("get run request");
    let get_run_resp = app
        .clone()
        .oneshot(get_run_req)
        .await
        .expect("get run response");
    let get_run_body = to_bytes(get_run_resp.into_body(), usize::MAX)
        .await
        .expect("get run body");
    let get_run_payload: Value = serde_json::from_slice(&get_run_body).expect("get run json");
    assert_eq!(
        get_run_payload
            .get("run")
            .and_then(|run| run.get("status"))
            .and_then(Value::as_str),
        Some("paused")
    );
}

#[tokio::test]
async fn context_run_replay_matches_persisted_state_without_drift() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-replay-ok",
                "objective": "replay no drift"
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
        .uri("/context/runs/ctx-run-replay-ok/events")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "type": "step_started",
                "status": "running",
                "step_id": "s1",
                "payload": {
                    "step_title": "Plan",
                    "step_status": "in_progress",
                    "why_next_step": "Need active planning"
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

    let replay_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-replay-ok/replay")
        .body(Body::empty())
        .expect("replay request");
    let replay_resp = app
        .clone()
        .oneshot(replay_req)
        .await
        .expect("replay response");
    assert_eq!(replay_resp.status(), StatusCode::OK);
    let replay_body = to_bytes(replay_resp.into_body(), usize::MAX)
        .await
        .expect("replay body");
    let replay_payload: Value = serde_json::from_slice(&replay_body).expect("replay json");
    assert_eq!(
        replay_payload
            .get("drift")
            .and_then(|d| d.get("mismatch"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        replay_payload
            .get("replay")
            .and_then(|r| r.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
}

#[tokio::test]
async fn context_run_replay_detects_status_drift() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-replay-drift",
                "objective": "replay drift"
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
        .uri("/context/runs/ctx-run-replay-drift/events")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "type": "planning_started",
                "status": "planning",
                "payload": {}
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
        .uri("/context/runs/ctx-run-replay-drift")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let mut get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    get_payload["run"]["status"] = Value::String("failed".to_string());

    let put_req = Request::builder()
        .method("PUT")
        .uri("/context/runs/ctx-run-replay-drift")
        .header("content-type", "application/json")
        .body(Body::from(
            get_payload
                .get("run")
                .cloned()
                .expect("run payload")
                .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let replay_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-replay-drift/replay")
        .body(Body::empty())
        .expect("replay request");
    let replay_resp = app
        .clone()
        .oneshot(replay_req)
        .await
        .expect("replay response");
    assert_eq!(replay_resp.status(), StatusCode::OK);
    let replay_body = to_bytes(replay_resp.into_body(), usize::MAX)
        .await
        .expect("replay body");
    let replay_payload: Value = serde_json::from_slice(&replay_body).expect("replay json");
    assert_eq!(
        replay_payload
            .get("drift")
            .and_then(|d| d.get("mismatch"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        replay_payload
            .get("drift")
            .and_then(|d| d.get("status_mismatch"))
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn context_run_driver_next_selects_runnable_step_and_sets_why() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-driver-1",
                "objective": "meta manager select"
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
        .uri("/context/runs/ctx-run-driver-1")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let mut get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    get_payload["run"]["steps"] = json!([
        {"step_id":"s1","title":"Plan","status":"pending"},
        {"step_id":"s2","title":"Execute","status":"runnable"}
    ]);

    let put_req = Request::builder()
        .method("PUT")
        .uri("/context/runs/ctx-run-driver-1")
        .header("content-type", "application/json")
        .body(Body::from(
            get_payload
                .get("run")
                .cloned()
                .expect("run payload")
                .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let next_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-driver-1/driver/next")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("next request");
    let next_resp = app.clone().oneshot(next_req).await.expect("next response");
    assert_eq!(next_resp.status(), StatusCode::OK);
    let next_body = to_bytes(next_resp.into_body(), usize::MAX)
        .await
        .expect("next body");
    let next_payload: Value = serde_json::from_slice(&next_body).expect("next json");
    assert_eq!(
        next_payload.get("selected_step_id").and_then(Value::as_str),
        Some("s2")
    );
    assert!(next_payload
        .get("why_next_step")
        .and_then(Value::as_str)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false));

    let run_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-driver-1")
        .body(Body::empty())
        .expect("run request");
    let run_resp = app.clone().oneshot(run_req).await.expect("run response");
    let run_body = to_bytes(run_resp.into_body(), usize::MAX)
        .await
        .expect("run body");
    let run_payload: Value = serde_json::from_slice(&run_body).expect("run json");
    assert_eq!(
        run_payload
            .get("run")
            .and_then(|r| r.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    assert_eq!(
        run_payload
            .get("run")
            .and_then(|r| r.get("steps"))
            .and_then(Value::as_array)
            .and_then(|steps| steps.get(1))
            .and_then(|step| step.get("status"))
            .and_then(Value::as_str),
        Some("in_progress")
    );
}

#[tokio::test]
async fn context_run_driver_next_respects_terminal_state() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-driver-2",
                "objective": "terminal check"
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
        .uri("/context/runs/ctx-run-driver-2")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let mut get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    get_payload["run"]["status"] = Value::String("completed".to_string());

    let put_req = Request::builder()
        .method("PUT")
        .uri("/context/runs/ctx-run-driver-2")
        .header("content-type", "application/json")
        .body(Body::from(
            get_payload
                .get("run")
                .cloned()
                .expect("run payload")
                .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let next_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-driver-2/driver/next")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("next request");
    let next_resp = app.clone().oneshot(next_req).await.expect("next response");
    assert_eq!(next_resp.status(), StatusCode::OK);
    let next_body = to_bytes(next_resp.into_body(), usize::MAX)
        .await
        .expect("next body");
    let next_payload: Value = serde_json::from_slice(&next_body).expect("next json");
    assert_eq!(next_payload.get("selected_step_id"), Some(&Value::Null));
    assert_eq!(
        next_payload.get("target_status").and_then(Value::as_str),
        Some("completed")
    );
}

#[tokio::test]
async fn context_run_driver_next_dry_run_does_not_mutate_state_or_events() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-driver-dry",
                "objective": "dry run guardrail"
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
        .uri("/context/runs/ctx-run-driver-dry")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let mut get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    get_payload["run"]["steps"] = json!([
        {"step_id":"s1","title":"Plan","status":"runnable"}
    ]);
    let before_revision = get_payload["run"]["revision"]
        .as_u64()
        .expect("before revision");

    let put_req = Request::builder()
        .method("PUT")
        .uri("/context/runs/ctx-run-driver-dry")
        .header("content-type", "application/json")
        .body(Body::from(
            get_payload
                .get("run")
                .cloned()
                .expect("run payload")
                .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let dry_next_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-driver-dry/driver/next")
        .header("content-type", "application/json")
        .body(Body::from(json!({"dry_run": true}).to_string()))
        .expect("dry next request");
    let dry_next_resp = app
        .clone()
        .oneshot(dry_next_req)
        .await
        .expect("dry next response");
    assert_eq!(dry_next_resp.status(), StatusCode::OK);

    let run_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-driver-dry")
        .body(Body::empty())
        .expect("run request");
    let run_resp = app.clone().oneshot(run_req).await.expect("run response");
    let run_body = to_bytes(run_resp.into_body(), usize::MAX)
        .await
        .expect("run body");
    let run_payload: Value = serde_json::from_slice(&run_body).expect("run json");
    assert_eq!(
        run_payload
            .get("run")
            .and_then(|r| r.get("revision"))
            .and_then(Value::as_u64),
        Some(before_revision.saturating_add(1))
    );
    assert_eq!(
        run_payload
            .get("run")
            .and_then(|r| r.get("steps"))
            .and_then(Value::as_array)
            .and_then(|steps| steps.first())
            .and_then(|step| step.get("status"))
            .and_then(Value::as_str),
        Some("runnable")
    );

    let events_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-driver-dry/events")
        .body(Body::empty())
        .expect("events request");
    let events_resp = app
        .clone()
        .oneshot(events_req)
        .await
        .expect("events response");
    let events_body = to_bytes(events_resp.into_body(), usize::MAX)
        .await
        .expect("events body");
    let events_payload: Value = serde_json::from_slice(&events_body).expect("events json");
    let has_decision_event = events_payload
        .get("events")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("type")
                    .and_then(Value::as_str)
                    .map(|ty| ty == "meta_next_step_selected")
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    assert!(!has_decision_event);
}

#[tokio::test]
async fn context_run_driver_next_emits_decision_event_with_why() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-driver-event",
                "objective": "emit decision event"
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
        .uri("/context/runs/ctx-run-driver-event")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let mut get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    get_payload["run"]["steps"] = json!([
        {"step_id":"s1","title":"Plan","status":"runnable"}
    ]);

    let put_req = Request::builder()
        .method("PUT")
        .uri("/context/runs/ctx-run-driver-event")
        .header("content-type", "application/json")
        .body(Body::from(
            get_payload
                .get("run")
                .cloned()
                .expect("run payload")
                .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let next_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-driver-event/driver/next")
        .header("content-type", "application/json")
        .body(Body::from(json!({"dry_run": false}).to_string()))
        .expect("next request");
    let next_resp = app.clone().oneshot(next_req).await.expect("next response");
    assert_eq!(next_resp.status(), StatusCode::OK);

    let events_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-driver-event/events")
        .body(Body::empty())
        .expect("events request");
    let events_resp = app
        .clone()
        .oneshot(events_req)
        .await
        .expect("events response");
    let events_body = to_bytes(events_resp.into_body(), usize::MAX)
        .await
        .expect("events body");
    let events_payload: Value = serde_json::from_slice(&events_body).expect("events json");
    let decision_event = events_payload
        .get("events")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("type")
                    .and_then(Value::as_str)
                    .map(|ty| ty == "meta_next_step_selected")
                    .unwrap_or(false)
            })
        })
        .cloned()
        .expect("decision event");
    assert!(decision_event
        .get("payload")
        .and_then(|p| p.get("why_next_step"))
        .and_then(Value::as_str)
        .map(|why| !why.trim().is_empty())
        .unwrap_or(false));
}

#[tokio::test]
async fn context_run_fault_injection_workspace_mismatch_checkpoint_replay() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-fault-1",
                "objective": "fault injection path",
                "workspace": {
                    "workspace_id": "ws-fault",
                    "canonical_path": "/expected/path",
                    "lease_epoch": 1
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
        .uri("/context/runs/ctx-run-fault-1")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let mut get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    get_payload["run"]["steps"] = json!([
        {"step_id":"s1","title":"Plan","status":"runnable"}
    ]);

    let put_req = Request::builder()
        .method("PUT")
        .uri("/context/runs/ctx-run-fault-1")
        .header("content-type", "application/json")
        .body(Body::from(
            get_payload
                .get("run")
                .cloned()
                .expect("run payload")
                .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let next_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-fault-1/driver/next")
        .header("content-type", "application/json")
        .body(Body::from(json!({"dry_run": false}).to_string()))
        .expect("next request");
    let next_resp = app.clone().oneshot(next_req).await.expect("next response");
    assert_eq!(next_resp.status(), StatusCode::OK);

    let mismatch_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-fault-1/lease/validate")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "phase": "pre_tool_call",
                "current_path": "/other/path",
                "step_id": "s1"
            })
            .to_string(),
        ))
        .expect("mismatch request");
    let mismatch_resp = app
        .clone()
        .oneshot(mismatch_req)
        .await
        .expect("mismatch response");
    assert_eq!(mismatch_resp.status(), StatusCode::OK);
    let mismatch_body = to_bytes(mismatch_resp.into_body(), usize::MAX)
        .await
        .expect("mismatch body");
    let mismatch_payload: Value = serde_json::from_slice(&mismatch_body).expect("mismatch json");
    assert_eq!(
        mismatch_payload.get("mismatch").and_then(Value::as_bool),
        Some(true)
    );

    let checkpoint_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-fault-1/checkpoints")
        .header("content-type", "application/json")
        .body(Body::from(json!({"reason":"fault_injection"}).to_string()))
        .expect("checkpoint request");
    let checkpoint_resp = app
        .clone()
        .oneshot(checkpoint_req)
        .await
        .expect("checkpoint response");
    assert_eq!(checkpoint_resp.status(), StatusCode::OK);

    let events_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-fault-1/events")
        .body(Body::empty())
        .expect("events request");
    let events_resp = app
        .clone()
        .oneshot(events_req)
        .await
        .expect("events response");
    let events_body = to_bytes(events_resp.into_body(), usize::MAX)
        .await
        .expect("events body");
    let events_payload: Value = serde_json::from_slice(&events_body).expect("events json");
    let event_rows = events_payload
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(event_rows.iter().any(|row| {
        row.get("type")
            .and_then(Value::as_str)
            .map(|ty| ty == "meta_next_step_selected")
            .unwrap_or(false)
    }));
    assert!(event_rows.iter().any(|row| {
        row.get("type")
            .and_then(Value::as_str)
            .map(|ty| ty == "workspace_mismatch")
            .unwrap_or(false)
    }));
    assert!(event_rows.iter().any(|row| {
        row.get("status")
            .and_then(Value::as_str)
            .map(|status| status == "paused")
            .unwrap_or(false)
    }));

    let replay_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-fault-1/replay")
        .body(Body::empty())
        .expect("replay request");
    let replay_resp = app
        .clone()
        .oneshot(replay_req)
        .await
        .expect("replay response");
    assert_eq!(replay_resp.status(), StatusCode::OK);
    let replay_body = to_bytes(replay_resp.into_body(), usize::MAX)
        .await
        .expect("replay body");
    let replay_payload: Value = serde_json::from_slice(&replay_body).expect("replay json");
    assert_eq!(
        replay_payload
            .get("replay")
            .and_then(|r| r.get("status"))
            .and_then(Value::as_str),
        Some("paused")
    );
    assert_eq!(
        replay_payload
            .get("drift")
            .and_then(|d| d.get("mismatch"))
            .and_then(Value::as_bool),
        Some(false)
    );
}

#[tokio::test]
async fn context_run_todos_sync_maps_to_steps_and_emits_event() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-todos-sync",
                "objective": "sync todos"
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

    let sync_req = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-todos-sync/todos/sync")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "replace": true,
                "source_session_id": "s-1",
                "source_run_id": "r-1",
                "todos": [
                    {"id":"task-1","content":"Plan architecture","status":"in_progress"},
                    {"id":"task-2","content":"Implement endpoint","status":"pending"},
                    {"id":"task-3","content":"Write tests","status":"completed"}
                ]
            })
            .to_string(),
        ))
        .expect("sync request");
    let sync_resp = app.clone().oneshot(sync_req).await.expect("sync response");
    assert_eq!(sync_resp.status(), StatusCode::OK);

    let run_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-todos-sync")
        .body(Body::empty())
        .expect("run request");
    let run_resp = app.clone().oneshot(run_req).await.expect("run response");
    let run_body = to_bytes(run_resp.into_body(), usize::MAX)
        .await
        .expect("run body");
    let run_payload: Value = serde_json::from_slice(&run_body).expect("run json");
    assert_eq!(
        run_payload
            .get("run")
            .and_then(|r| r.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    assert_eq!(
        run_payload
            .get("run")
            .and_then(|r| r.get("steps"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(3)
    );

    let events_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-todos-sync/events")
        .body(Body::empty())
        .expect("events request");
    let events_resp = app
        .clone()
        .oneshot(events_req)
        .await
        .expect("events response");
    let events_body = to_bytes(events_resp.into_body(), usize::MAX)
        .await
        .expect("events body");
    let events_payload: Value = serde_json::from_slice(&events_body).expect("events json");
    let has_todo_synced = events_payload
        .get("events")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("type")
                    .and_then(Value::as_str)
                    .map(|v| v == "todo_synced")
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    assert!(has_todo_synced);
}

#[tokio::test]
async fn context_tasks_claim_is_single_winner_under_race() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-task-race",
                "objective": "claim race"
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
        .uri("/context/runs/ctx-run-task-race/tasks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tasks": [
                    {
                        "id": "task-1",
                        "task_type": "unit_work",
                        "status": "runnable",
                        "payload": {"title": "Only task"}
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

    let claim_one = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-race/tasks/claim")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "agent_id": "agent-a",
                "command_id": "claim-a"
            })
            .to_string(),
        ))
        .expect("claim one request");
    let claim_two = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-race/tasks/claim")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "agent_id": "agent-b",
                "command_id": "claim-b"
            })
            .to_string(),
        ))
        .expect("claim two request");

    let (resp_one, resp_two) = tokio::join!(
        app.clone().oneshot(claim_one),
        app.clone().oneshot(claim_two)
    );
    let resp_one = resp_one.expect("claim one response");
    let resp_two = resp_two.expect("claim two response");
    assert_eq!(resp_one.status(), StatusCode::OK);
    assert_eq!(resp_two.status(), StatusCode::OK);

    let body_one = to_bytes(resp_one.into_body(), usize::MAX)
        .await
        .expect("claim one body");
    let body_two = to_bytes(resp_two.into_body(), usize::MAX)
        .await
        .expect("claim two body");
    let payload_one: Value = serde_json::from_slice(&body_one).expect("claim one json");
    let payload_two: Value = serde_json::from_slice(&body_two).expect("claim two json");

    let winner_count = [payload_one.clone(), payload_two.clone()]
        .iter()
        .filter(|payload| !payload.get("task").unwrap_or(&Value::Null).is_null())
        .count();
    assert_eq!(winner_count, 1);
}

#[tokio::test]
async fn context_task_transition_command_id_is_idempotent() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-task-idempotent",
                "objective": "idempotent transition"
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
        .uri("/context/runs/ctx-run-task-idempotent/tasks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tasks": [
                    {
                        "id": "task-1",
                        "task_type": "unit_work",
                        "status": "in_progress",
                        "payload": {"title": "Task"}
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

    let transition_req_one = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-idempotent/tasks/task-1/transition")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "fail",
                "command_id": "cmd-fail-1",
                "error": "boom"
            })
            .to_string(),
        ))
        .expect("transition one request");
    let transition_resp_one = app
        .clone()
        .oneshot(transition_req_one)
        .await
        .expect("transition one response");
    assert_eq!(transition_resp_one.status(), StatusCode::OK);

    let transition_req_two = Request::builder()
        .method("POST")
        .uri("/context/runs/ctx-run-task-idempotent/tasks/task-1/transition")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "fail",
                "command_id": "cmd-fail-1",
                "error": "boom"
            })
            .to_string(),
        ))
        .expect("transition two request");
    let transition_resp_two = app
        .clone()
        .oneshot(transition_req_two)
        .await
        .expect("transition two response");
    assert_eq!(transition_resp_two.status(), StatusCode::OK);
    let transition_two_body = to_bytes(transition_resp_two.into_body(), usize::MAX)
        .await
        .expect("transition two body");
    let transition_two_payload: Value =
        serde_json::from_slice(&transition_two_body).expect("transition two json");
    assert_eq!(
        transition_two_payload
            .get("deduped")
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn context_task_transition_rejects_task_revision_mismatch() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_run_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-task-rev",
                "objective": "rev mismatch"
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
        .uri("/context/runs/ctx-run-task-rev/tasks")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tasks": [
                    {
                        "id": "task-1",
                        "task_type": "unit_work",
                        "status": "runnable"
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
        .uri("/context/runs/ctx-run-task-rev/tasks/task-1/transition")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "status",
                "status": "in_progress",
                "expected_task_rev": 999
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
    let transition_body = to_bytes(transition_resp.into_body(), usize::MAX)
        .await
        .expect("transition body");
    let transition_payload: Value =
        serde_json::from_slice(&transition_body).expect("transition json");
    assert_eq!(
        transition_payload.get("ok").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        transition_payload.get("code").and_then(Value::as_str),
        Some("TASK_REV_MISMATCH")
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
