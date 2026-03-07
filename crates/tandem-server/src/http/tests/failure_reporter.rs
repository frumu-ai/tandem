use super::*;

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
