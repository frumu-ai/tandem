use super::*;

#[tokio::test]
async fn approve_tool_by_call_route_replies_permission() {
    let state = test_state().await;
    let request = state
        .permissions
        .ask_for_session(Some("s1"), "bash", json!({"command":"echo hi"}))
        .await;
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri(format!("/sessions/s1/tools/{}/approve", request.id))
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("ok").and_then(|v| v.as_bool()), Some(true));
}

#[tokio::test]
async fn permission_reply_route_rejects_invalid_reply() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/permission/some-request/reply")
        .header("content-type", "application/json")
        .body(Body::from(json!({"reply":"invalid"}).to_string()))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(|v| v.as_str()),
        Some("APPROVAL_REPLY_INVALID")
    );
    assert_eq!(
        payload.get("retryable").and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[tokio::test]
async fn permission_reply_route_returns_not_found_for_unknown_request() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/permission/missing-request/reply")
        .header("content-type", "application/json")
        .body(Body::from(json!({"reply":"always"}).to_string()))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(|v| v.as_str()),
        Some("APPROVAL_REQUEST_NOT_FOUND")
    );
    assert_eq!(
        payload.get("retryable").and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[tokio::test]
async fn permission_reply_route_applies_and_persists_allow_rule() {
    let state = test_state().await;
    let request = state
        .permissions
        .ask_for_session(Some("s1"), "glob", json!({"pattern":"*"}))
        .await;
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri(format!("/permission/{}/reply", request.id))
        .header("content-type", "application/json")
        .body(Body::from(json!({"reply":"always"}).to_string()))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        payload.get("reply").and_then(|v| v.as_str()),
        Some("always")
    );
    assert_eq!(
        payload.get("persistedRule").and_then(|v| v.as_bool()),
        Some(true)
    );
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"permission.decision\""));
    assert!(audit.contains("\"permission\":\"glob\""));
    assert!(audit.contains("\"standingRulePersisted\":true"));
    assert!(audit.contains("\"sourceRequestID\""));
}
