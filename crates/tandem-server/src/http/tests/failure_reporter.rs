use super::*;

#[tokio::test]
async fn failure_reporter_runtime_creates_incident_and_draft_from_failure_event() {
    let state = test_state().await;
    state
        .put_failure_reporter_config(crate::FailureReporterConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            ..Default::default()
        })
        .await
        .expect("config");

    let task = tokio::spawn(crate::run_failure_reporter(state.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    state.event_bus.publish(EngineEvent::new(
        "session.error",
        json!({
            "sessionID": "session-123",
            "runID": "run-123",
            "reason": "Prompt retry failed",
            "component": "swarm-orchestrator",
        }),
    ));

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let incidents = state.list_failure_reporter_incidents(10).await;
            let drafts = state.list_failure_reporter_drafts(10).await;
            if incidents
                .iter()
                .any(|row| row.draft_id.is_some() || row.last_error.is_some())
                || !drafts.is_empty()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("incident timeout");

    let incidents = state.list_failure_reporter_incidents(10).await;
    assert_eq!(incidents.len(), 1);
    let incident = &incidents[0];
    assert_eq!(incident.event_type, "session.error");
    assert_eq!(incident.repo, "acme/platform");
    assert_eq!(incident.workspace_root, "/tmp/acme");
    assert!(incident.draft_id.is_some());

    let drafts = state.list_failure_reporter_drafts(10).await;
    assert_eq!(drafts.len(), 1);
    assert_eq!(
        drafts[0].draft_id,
        incident.draft_id.clone().unwrap_or_default()
    );

    task.abort();
}

#[tokio::test]
async fn paused_failure_reporter_runtime_ignores_failure_events() {
    let state = test_state().await;
    state
        .put_failure_reporter_config(crate::FailureReporterConfig {
            enabled: true,
            paused: true,
            repo: Some("acme/platform".to_string()),
            ..Default::default()
        })
        .await
        .expect("config");

    let task = tokio::spawn(crate::run_failure_reporter(state.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    state.event_bus.publish(EngineEvent::new(
        "session.error",
        json!({
            "sessionID": "session-456",
            "reason": "Paused reporter should ignore this",
        }),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    assert!(state.list_failure_reporter_incidents(10).await.is_empty());
    assert!(state.list_failure_reporter_drafts(10).await.is_empty());
    task.abort();
}

#[tokio::test]
async fn failure_reporter_report_creates_and_dedupes_draft() {
    let state = test_state().await;
    state
        .put_failure_reporter_config(crate::FailureReporterConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state.clone());
    let body = json!({
        "report": {
            "source": "desktop_logs",
            "event": "orchestrator.run_failed",
            "run_id": "run-123",
            "excerpt": ["boom", "stack trace"],
        }
    });
    let req = Request::builder()
        .method("POST")
        .uri("/failure-reporter/report")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let payload: Value =
        serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.expect("body"))
            .expect("json");
    let draft = payload.get("draft").expect("draft");
    assert_eq!(
        draft.get("repo").and_then(Value::as_str),
        Some("acme/platform")
    );
    assert_eq!(
        draft.get("status").and_then(Value::as_str),
        Some("approval_required")
    );
    assert!(draft
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .contains("orchestrator.run_failed"));

    let req = Request::builder()
        .method("POST")
        .uri("/failure-reporter/report")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let second_payload: Value =
        serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.expect("body"))
            .expect("json");
    assert_eq!(
        payload
            .get("draft")
            .and_then(|row| row.get("draft_id"))
            .and_then(Value::as_str),
        second_payload
            .get("draft")
            .and_then(|row| row.get("draft_id"))
            .and_then(Value::as_str)
    );

    let drafts = state.list_failure_reporter_drafts(10).await;
    assert_eq!(drafts.len(), 1);
}

#[tokio::test]
async fn failure_reporter_report_requires_repo() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/failure-reporter/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "desktop_logs",
                    "excerpt": ["something failed"]
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let payload: Value =
        serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.expect("body"))
            .expect("json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("FAILURE_REPORTER_REPORT_INVALID")
    );
}

#[tokio::test]
async fn failure_reporter_draft_can_be_approved_and_denied() {
    let state = test_state().await;
    state
        .put_failure_reporter_config(crate::FailureReporterConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/failure-reporter/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "desktop_logs",
                    "excerpt": ["boom"],
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    let payload: Value =
        serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.expect("body"))
            .expect("json");
    let draft_id = payload
        .get("draft")
        .and_then(|row| row.get("draft_id"))
        .and_then(Value::as_str)
        .expect("draft_id")
        .to_string();

    let approve_req = Request::builder()
        .method("POST")
        .uri(format!("/failure-reporter/drafts/{draft_id}/approve"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"reason":"looks valid"}).to_string()))
        .expect("approve request");
    let approve_resp = app
        .clone()
        .oneshot(approve_req)
        .await
        .expect("approve response");
    assert_eq!(approve_resp.status(), StatusCode::OK);
    let approve_payload: Value = serde_json::from_slice(
        &to_bytes(approve_resp.into_body(), usize::MAX)
            .await
            .expect("approve body"),
    )
    .expect("approve json");
    assert_eq!(
        approve_payload
            .get("draft")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("draft_ready")
    );

    let deny_req = Request::builder()
        .method("POST")
        .uri(format!("/failure-reporter/drafts/{draft_id}/deny"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"reason":"too late"}).to_string()))
        .expect("deny request");
    let deny_resp = app.clone().oneshot(deny_req).await.expect("deny response");
    assert_eq!(deny_resp.status(), StatusCode::CONFLICT);

    let second_req = Request::builder()
        .method("POST")
        .uri("/failure-reporter/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "desktop_logs",
                    "title": "another failure",
                    "excerpt": ["oops"],
                    "fingerprint": "manual-second"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let second_resp = app.clone().oneshot(second_req).await.expect("response");
    let second_payload: Value = serde_json::from_slice(
        &to_bytes(second_resp.into_body(), usize::MAX)
            .await
            .expect("second body"),
    )
    .expect("second json");
    let second_draft_id = second_payload
        .get("draft")
        .and_then(|row| row.get("draft_id"))
        .and_then(Value::as_str)
        .expect("second draft id");

    let deny_req = Request::builder()
        .method("POST")
        .uri(format!("/failure-reporter/drafts/{second_draft_id}/deny"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"reason":"noise"}).to_string()))
        .expect("deny request");
    let deny_resp = app.clone().oneshot(deny_req).await.expect("deny response");
    assert_eq!(deny_resp.status(), StatusCode::OK);
    let deny_payload: Value = serde_json::from_slice(
        &to_bytes(deny_resp.into_body(), usize::MAX)
            .await
            .expect("deny body"),
    )
    .expect("deny json");
    assert_eq!(
        deny_payload
            .get("draft")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("denied")
    );
}

#[tokio::test]
async fn failure_reporter_triage_run_created_from_approved_draft() {
    let state = test_state().await;
    state
        .put_failure_reporter_config(crate::FailureReporterConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state.clone());
    let create_req = Request::builder()
        .method("POST")
        .uri("/failure-reporter/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "desktop_logs",
                    "title": "Build failure in CI",
                    "excerpt": ["boom"],
                }
            })
            .to_string(),
        ))
        .expect("request");
    let create_resp = app.clone().oneshot(create_req).await.expect("response");
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let draft_id = create_payload
        .get("draft")
        .and_then(|row| row.get("draft_id"))
        .and_then(Value::as_str)
        .expect("draft id")
        .to_string();

    let approve_req = Request::builder()
        .method("POST")
        .uri(format!("/failure-reporter/drafts/{draft_id}/approve"))
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .expect("approve request");
    let approve_resp = app
        .clone()
        .oneshot(approve_req)
        .await
        .expect("approve response");
    assert_eq!(approve_resp.status(), StatusCode::OK);

    let triage_req = Request::builder()
        .method("POST")
        .uri(format!("/failure-reporter/drafts/{draft_id}/triage-run"))
        .body(Body::empty())
        .expect("triage request");
    let triage_resp = app
        .clone()
        .oneshot(triage_req)
        .await
        .expect("triage response");
    assert_eq!(triage_resp.status(), StatusCode::OK);
    let triage_payload: Value = serde_json::from_slice(
        &to_bytes(triage_resp.into_body(), usize::MAX)
            .await
            .expect("triage body"),
    )
    .expect("triage json");
    let run_id = triage_payload
        .get("run")
        .and_then(|row| row.get("run_id"))
        .and_then(Value::as_str)
        .expect("run id")
        .to_string();
    assert_eq!(
        triage_payload
            .get("draft")
            .and_then(|row| row.get("triage_run_id"))
            .and_then(Value::as_str),
        Some(run_id.as_str())
    );
    assert_eq!(
        triage_payload
            .get("draft")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("triage_queued")
    );

    let get_run_req = Request::builder()
        .method("GET")
        .uri(format!("/context/runs/{run_id}"))
        .body(Body::empty())
        .expect("get run request");
    let get_run_resp = app
        .clone()
        .oneshot(get_run_req)
        .await
        .expect("get run response");
    assert_eq!(get_run_resp.status(), StatusCode::OK);
    let get_run_payload: Value = serde_json::from_slice(
        &to_bytes(get_run_resp.into_body(), usize::MAX)
            .await
            .expect("get run body"),
    )
    .expect("get run json");
    assert_eq!(
        get_run_payload
            .get("run")
            .and_then(|row| row.get("run_type"))
            .and_then(Value::as_str),
        Some("failure_reporter_triage")
    );
    assert_eq!(
        get_run_payload
            .get("run")
            .and_then(|row| row.get("tasks"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(2)
    );

    let second_req = Request::builder()
        .method("POST")
        .uri(format!("/failure-reporter/drafts/{draft_id}/triage-run"))
        .body(Body::empty())
        .expect("second triage request");
    let second_resp = app
        .clone()
        .oneshot(second_req)
        .await
        .expect("second triage response");
    assert_eq!(second_resp.status(), StatusCode::OK);
    let second_payload: Value = serde_json::from_slice(
        &to_bytes(second_resp.into_body(), usize::MAX)
            .await
            .expect("second triage body"),
    )
    .expect("second triage json");
    assert_eq!(
        second_payload
            .get("run")
            .and_then(|row| row.get("run_id"))
            .and_then(Value::as_str),
        Some(run_id.as_str())
    );
    assert_eq!(
        second_payload.get("deduped").and_then(Value::as_bool),
        Some(true)
    );
}
