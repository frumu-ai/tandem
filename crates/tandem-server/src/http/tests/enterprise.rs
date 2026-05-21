use super::*;

#[tokio::test]
async fn enterprise_status_returns_public_safe_summary() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/status")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("mode").and_then(Value::as_str),
        Some("disabled")
    );
    assert_eq!(
        payload.get("bridge_state").and_then(Value::as_str),
        Some("absent")
    );
    assert_eq!(
        payload
            .get("tenant_context")
            .and_then(|row| row.get("source"))
            .and_then(Value::as_str),
        Some("local_implicit")
    );
    assert_eq!(
        payload
            .get("tenant_context")
            .and_then(|row| row.get("org_id"))
            .and_then(Value::as_str),
        Some("local")
    );
    assert!(payload
        .get("capabilities")
        .and_then(Value::as_array)
        .is_some_and(|caps| !caps.is_empty()));
}

#[tokio::test]
async fn enterprise_org_units_noop_threads_request_tenant() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/org-units")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .header("x-tandem-actor-id", "admin-user")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("status").and_then(Value::as_str), Some("noop"));
    assert_eq!(
        payload.get("bridge_state").and_then(Value::as_str),
        Some("absent")
    );
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));
    assert_eq!(
        payload
            .get("tenant_context")
            .and_then(|row| row.get("org_id"))
            .and_then(Value::as_str),
        Some("clinic-co")
    );
    assert_eq!(
        payload
            .get("tenant_context")
            .and_then(|row| row.get("workspace_id"))
            .and_then(Value::as_str),
        Some("care-delivery")
    );
    assert_eq!(
        payload
            .get("request_principal")
            .and_then(|row| row.get("actor_id"))
            .and_then(Value::as_str),
        Some("admin-user")
    );
    assert_eq!(
        payload
            .get("org_units")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn enterprise_source_bindings_noop_threads_request_tenant() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/source-bindings")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("status").and_then(Value::as_str), Some("noop"));
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));
    assert_eq!(
        payload
            .get("tenant_context")
            .and_then(|row| row.get("org_id"))
            .and_then(Value::as_str),
        Some("acme")
    );
    assert_eq!(
        payload
            .get("source_bindings")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}
