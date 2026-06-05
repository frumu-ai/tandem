// TAN-9 / CT-04: cross-tenant audit visibility negative tests for `/audit/stream`.
//
// These drive the real HTTP audit read path end to end. The handler subscribes to
// `state.event_bus` when the request runs and streams an unbounded NDJSON body, so the
// harness must (1) issue the request first to establish the subscription, (2) publish the
// audit events afterwards, and (3) read the streaming body under a deadline rather than
// draining it to EOF (the stream never closes while `state` is alive).
use super::*;
use tandem_types::EngineEvent;
use tokio_stream::StreamExt as _;

/// `/audit/stream` requires an admin principal (`api_token` | `control_panel`). In test
/// mode the request source is taken from `x-tandem-request-source`, and the tenant context
/// from the `x-tandem-*` headers.
fn audit_stream_request(org_id: &str, workspace_id: &str, actor_id: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri("/audit/stream")
        .header("x-tandem-org-id", org_id)
        .header("x-tandem-workspace-id", workspace_id)
        .header("x-tandem-actor-id", actor_id)
        .header("x-tandem-request-source", "api_token")
        .body(Body::empty())
        .expect("audit stream request")
}

/// A fintech protected-action denial audit event tagged with an explicit tenant. `run_marker`
/// is echoed into the streamed record's `result.run_id`, giving each event a unique probe.
fn tenant_audit_event(org_id: &str, workspace_id: &str, run_marker: &str) -> EngineEvent {
    EngineEvent::new(
        "fintech.protected_action.denied",
        json!({
            "org_id": org_id,
            "workspace_id": workspace_id,
            "runID": run_marker,
            "automationID": "automation-1",
            "tool": "mcp.bank.release_funds",
            "classification": "requires_approval",
            "category": "money_movement",
            "reason": "approval required",
        }),
    )
}

/// Read the streaming NDJSON body until `stop_marker` is seen or the deadline elapses,
/// returning everything captured so far. Never drains to EOF (the stream is unbounded).
async fn capture_until(resp: axum::response::Response, stop_marker: &str) -> String {
    let mut body = resp.into_body().into_data_stream();
    let mut captured = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Ok(Some(chunk)) = tokio::time::timeout(remaining, body.next()).await else {
            break;
        };
        let chunk = chunk.expect("audit stream chunk");
        captured.push_str(&String::from_utf8_lossy(&chunk));
        if captured.contains(stop_marker) {
            break;
        }
    }
    captured
}

#[tokio::test]
async fn audit_stream_hides_other_tenants_events() {
    let state = test_state().await;
    let app = app_router(state.clone());

    // Subscribe as tenant B first so the handler's broadcast receiver exists before we
    // publish. The streaming response is returned as soon as the subscription is set up.
    let resp = app
        .clone()
        .oneshot(audit_stream_request("org-b", "workspace-b", "user-b"))
        .await
        .expect("audit stream response");
    assert_eq!(resp.status(), StatusCode::OK);

    // Tenant A's protected event must never reach tenant B; tenant B's own event must.
    state.event_bus.publish(tenant_audit_event(
        "org-a",
        "workspace-a",
        "run-tenant-a-secret",
    ));
    state.event_bus.publish(tenant_audit_event(
        "org-b",
        "workspace-b",
        "run-tenant-b-visible",
    ));

    let captured = capture_until(resp, "run-tenant-b-visible").await;

    assert!(
        captured.contains("run-tenant-b-visible"),
        "tenant B should see its own audit event, got: {captured:?}"
    );
    assert!(
        !captured.contains("run-tenant-a-secret"),
        "tenant B must NOT see tenant A's audit event, got: {captured:?}"
    );
}

#[tokio::test]
async fn audit_stream_hides_untagged_events_from_explicit_tenant() {
    // End-to-end guard for the TAN-9 fail-closed fix: an event with no org tag cannot be
    // attributed to a tenant, so an explicit (multi-tenant) reader must not receive it.
    let state = test_state().await;
    let app = app_router(state.clone());

    let resp = app
        .clone()
        .oneshot(audit_stream_request("org-b", "workspace-b", "user-b"))
        .await
        .expect("audit stream response");
    assert_eq!(resp.status(), StatusCode::OK);

    // Untagged event (no org_id/workspace_id) followed by a tenant-B-tagged probe so the
    // reader has a deterministic stop marker even though the untagged event is filtered.
    state.event_bus.publish(EngineEvent::new(
        "fintech.protected_action.denied",
        json!({
            "runID": "run-untagged-secret",
            "automationID": "automation-1",
            "tool": "mcp.bank.release_funds",
            "classification": "requires_approval",
            "category": "money_movement",
            "reason": "approval required",
        }),
    ));
    state.event_bus.publish(tenant_audit_event(
        "org-b",
        "workspace-b",
        "run-tenant-b-probe",
    ));

    let captured = capture_until(resp, "run-tenant-b-probe").await;

    assert!(
        captured.contains("run-tenant-b-probe"),
        "tenant B should see its own tagged probe event, got: {captured:?}"
    );
    assert!(
        !captured.contains("run-untagged-secret"),
        "an untagged audit event must fail closed for an explicit tenant, got: {captured:?}"
    );
}

#[tokio::test]
async fn audit_stream_requires_admin_principal() {
    // A non-admin source (default `local_control_panel`) must be refused outright.
    let state = test_state().await;
    let app = app_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/audit/stream")
        .header("x-tandem-org-id", "org-b")
        .header("x-tandem-workspace-id", "workspace-b")
        .header("x-tandem-actor-id", "user-b")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
