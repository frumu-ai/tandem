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

fn protected_audit_request(uri: &str, org_id: &str, workspace_id: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("x-tandem-org-id", org_id)
        .header("x-tandem-workspace-id", workspace_id)
        .header("x-tandem-actor-id", "audit-admin")
        .header("x-tandem-request-source", "api_token")
        .body(Body::empty())
        .expect("protected audit request")
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

#[tokio::test]
async fn protected_audit_query_filters_by_tenant_context() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let tenant_a =
        tandem_types::TenantContext::explicit("org-a", "workspace-a", Some("user-a".to_string()));
    let tenant_b =
        tandem_types::TenantContext::explicit("org-b", "workspace-b", Some("user-b".to_string()));

    crate::audit::append_protected_audit_event(
        &state,
        "automation_v2.internal_sweep.server_restart_failed_run",
        &tenant_a,
        Some("tandem-server:internal-sweep".to_string()),
        json!({
            "run_id": "run-tenant-a-secret",
            "automation_id": "automation-a",
            "tenantContext": tenant_a,
        }),
    )
    .await
    .expect("tenant a audit");
    crate::audit::append_protected_audit_event(
        &state,
        "automation_v2.internal_sweep.server_restart_failed_run",
        &tenant_b,
        Some("tandem-server:internal-sweep".to_string()),
        json!({
            "run_id": "run-tenant-b-visible",
            "automation_id": "automation-b",
            "tenantContext": tenant_b,
        }),
    )
    .await
    .expect("tenant b audit");

    let tenant_b_resp = app
        .clone()
        .oneshot(protected_audit_request(
            "/audit/protected?run_id=run-tenant-a-secret",
            "org-b",
            "workspace-b",
        ))
        .await
        .expect("tenant b protected audit response");
    assert_eq!(tenant_b_resp.status(), StatusCode::OK);
    let tenant_b_body = to_bytes(tenant_b_resp.into_body(), usize::MAX)
        .await
        .expect("tenant b body");
    let tenant_b_payload: Value =
        serde_json::from_slice(&tenant_b_body).expect("tenant b audit json");
    assert_eq!(
        tenant_b_payload.get("count").and_then(Value::as_u64),
        Some(0)
    );

    let tenant_a_resp = app
        .oneshot(protected_audit_request(
            "/audit/protected?run_id=run-tenant-a-secret",
            "org-a",
            "workspace-a",
        ))
        .await
        .expect("tenant a protected audit response");
    assert_eq!(tenant_a_resp.status(), StatusCode::OK);
    let tenant_a_body = to_bytes(tenant_a_resp.into_body(), usize::MAX)
        .await
        .expect("tenant a body");
    let tenant_a_payload: Value =
        serde_json::from_slice(&tenant_a_body).expect("tenant a audit json");
    assert_eq!(
        tenant_a_payload.get("count").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        tenant_a_payload["events"][0]["payload"]["run_id"].as_str(),
        Some("run-tenant-a-secret")
    );
}

#[tokio::test]
async fn protected_audit_query_filters_by_denial_event_type() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let tenant = tandem_types::TenantContext::explicit(
        "org-denial",
        "workspace-denial",
        Some("audit-admin".to_string()),
    );

    crate::audit::append_protected_audit_event(
        &state,
        "mcp.secret_tenant_mismatch",
        &tenant,
        Some("audit-admin".to_string()),
        json!({
            "reason": "store_secret_tenant_mismatch",
            "server_name": "tenant-mcp",
            "tool_name": "get_me",
        }),
    )
    .await
    .expect("mcp denial audit");
    crate::audit::append_protected_audit_event(
        &state,
        "authority.cross_tenant_denied",
        &tenant,
        Some("audit-admin".to_string()),
        json!({
            "reason": "cross_tenant_receipt_replay",
        }),
    )
    .await
    .expect("authority denial audit");

    let resp = app
        .oneshot(protected_audit_request(
            "/audit/protected?event_type=mcp.secret_tenant_mismatch",
            "org-denial",
            "workspace-denial",
        ))
        .await
        .expect("protected audit response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("protected audit body");
    let payload: Value = serde_json::from_slice(&body).expect("protected audit json");

    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        payload["events"][0]["event_type"].as_str(),
        Some("mcp.secret_tenant_mismatch")
    );
}

#[tokio::test]
async fn recover_in_flight_runs_records_attributed_protected_audit() {
    let state = test_state().await;
    let tenant = tandem_types::TenantContext::explicit(
        "org-recovery",
        "workspace-recovery",
        Some("user-recovery".to_string()),
    );
    let mut automation =
        super::global::create_test_automation_v2(&state, "auto-v2-restart-recovery-audit").await;
    automation.set_tenant_context(&tenant);
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("store tenant automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Running;
            row.active_session_ids = vec!["session-recovery-audit".to_string()];
            row.latest_session_id = Some("session-recovery-audit".to_string());
        })
        .await
        .expect("mark running");

    let recovered = state.recover_in_flight_runs().await;
    assert_eq!(recovered, 1);

    let events = crate::audit::load_protected_audit_events_for_tenant(&state, &tenant).await;
    let recovery_event = events
        .iter()
        .find(|event| {
            event.event_type == "automation_v2.internal_sweep.server_restart_queued_run_for_resume"
                && event.payload.get("run_id").and_then(Value::as_str) == Some(run.run_id.as_str())
        })
        .expect("protected restart recovery audit event");
    assert_eq!(recovery_event.tenant_context, tenant);
    assert_eq!(
        recovery_event.actor.as_deref(),
        Some("tandem-server:internal-sweep")
    );
    assert_eq!(
        recovery_event.payload.get("sweep").and_then(Value::as_str),
        Some("recover_in_flight_runs")
    );
    assert_eq!(
        recovery_event
            .payload
            .get("outcome")
            .and_then(Value::as_str),
        Some("queued_for_resume")
    );
}
