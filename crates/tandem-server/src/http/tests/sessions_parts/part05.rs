// TAN-400: enterprise strict-mode fail-closed regression tests. These prove
// the boundary refuses to dispatch — before the provider is ever called —
// when the strict posture cannot positively establish tenant context or
// provider classification, and that denied decisions leave only audit-safe
// evidence behind. Shares the env-guard + DEFAULT serial group discipline
// documented at the top of part04.rs.

#[tokio::test]
#[serial_test::serial]
async fn strict_enforce_blocks_missing_tenant_context_even_for_classified_provider() {
    let _mode = DataBoundaryEnvGuard::set("TANDEM_DATA_BOUNDARY_MODE", Some("enforce"));
    let _strict = DataBoundaryEnvGuard::set("TANDEM_DATA_BOUNDARY_STRICT", Some("1"));
    let _classes = DataBoundaryEnvGuard::set(
        "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
        Some("boundary-test=approved_external"),
    );
    let state = test_state().await;
    let (session_id, captured) = capturing_boundary_session(&state).await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state);

    // Payload is deliberately clean: strict mode fails closed on the missing
    // tenant posture alone, not on detected findings.
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/session/{session_id}/prompt_async"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "parts": [{"type": "text", "text": "summarize the release notes"}],
                        "model": {"provider_id": "boundary-test", "model_id": "boundary-test-1"},
                    })
                    .to_string(),
                ))
                .expect("prompt request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let events = collect_events_until_run_finished(&mut rx).await;
    assert_eq!(run_finished_status(&events), "error");
    let blocked = events
        .iter()
        .find(|event| event.event_type == "data_boundary.blocked")
        .expect("strict block event");
    let reason_codes = serde_json::to_string(&blocked.properties["reason_codes"]).expect("json");
    assert!(
        reason_codes.contains("missing_tenant_context"),
        "strict block must cite missing tenant context: {reason_codes}"
    );
    assert!(
        captured.lock().expect("captured lock").is_none(),
        "strict fail-closed dispatch must never reach the provider"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn strict_enforce_blocks_unclassified_provider() {
    let _mode = DataBoundaryEnvGuard::set("TANDEM_DATA_BOUNDARY_MODE", Some("enforce"));
    let _strict = DataBoundaryEnvGuard::set("TANDEM_DATA_BOUNDARY_STRICT", Some("1"));
    // No TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES: the provider stays Unknown.
    let _classes = DataBoundaryEnvGuard::set("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES", None);
    let state = test_state().await;
    let (session_id, captured) = capturing_boundary_session(&state).await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state);

    let resp = app
        .oneshot(boundary_prompt_request(&session_id))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let events = collect_events_until_run_finished(&mut rx).await;
    assert_eq!(run_finished_status(&events), "error");
    let blocked = events
        .iter()
        .find(|event| event.event_type == "data_boundary.blocked")
        .expect("strict block event");
    let reason_codes = serde_json::to_string(&blocked.properties["reason_codes"]).expect("json");
    assert!(
        reason_codes.contains("unknown_provider_boundary_class"),
        "strict block must cite the unknown provider class: {reason_codes}"
    );
    assert_eq!(
        blocked.properties["classificationSource"], "unclassified",
        "audit trail must show why the provider was Unknown"
    );
    assert!(captured.lock().expect("captured lock").is_none());
}

#[tokio::test]
#[serial_test::serial]
async fn enforce_blocks_prohibited_provider_before_dispatch() {
    let _mode = DataBoundaryEnvGuard::set("TANDEM_DATA_BOUNDARY_MODE", Some("enforce"));
    let _classes = DataBoundaryEnvGuard::set(
        "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
        Some("boundary-test=prohibited"),
    );
    let state = test_state().await;
    let (session_id, captured) = capturing_boundary_session(&state).await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state);

    let resp = app
        .oneshot(boundary_prompt_request(&session_id))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let events = collect_events_until_run_finished(&mut rx).await;
    assert_eq!(run_finished_status(&events), "error");
    let blocked = events
        .iter()
        .find(|event| event.event_type == "data_boundary.blocked")
        .expect("prohibited provider block event");
    let reason_codes = serde_json::to_string(&blocked.properties["reason_codes"]).expect("json");
    assert!(
        reason_codes.contains("prohibited_provider"),
        "block must cite the prohibited provider: {reason_codes}"
    );
    assert!(
        captured.lock().expect("captured lock").is_none(),
        "a prohibited provider must never be dispatched to"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn strict_denied_decision_leaves_only_safe_evidence_in_protected_audit() {
    let _mode = DataBoundaryEnvGuard::set("TANDEM_DATA_BOUNDARY_MODE", Some("enforce"));
    let _classes = DataBoundaryEnvGuard::set(
        "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
        Some("boundary-test=prohibited"),
    );
    let state = test_state().await;
    let (session_id, _captured) = capturing_boundary_session(&state).await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let resp = app
        .clone()
        .oneshot(boundary_prompt_request(&session_id))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let events = collect_events_until_run_finished(&mut rx).await;
    let blocked = events
        .iter()
        .find(|event| event.event_type == "data_boundary.blocked")
        .expect("blocked event");
    // The bridge task is spawned by the serve loop, not by test_state, so
    // route the event through the bridge directly — same code path.
    assert!(
        crate::data_boundary_bridge::record_data_boundary_protected_audit(&state, blocked)
            .await
            .expect("record blocked protected audit"),
        "blocked decisions must be appended to the protected ledger"
    );

    // Denied decision is queryable through the admin audit read — and the
    // durable record carries classes/hashes/reason codes, never the payload.
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/audit/protected?event_type=data_boundary.blocked")
                .header("x-tandem-actor-id", "audit-admin")
                .header("x-tandem-request-source", "api_token")
                .body(Body::empty())
                .expect("audit request"),
        )
        .await
        .expect("audit response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("audit body");
    let body: serde_json::Value = serde_json::from_slice(&body).expect("audit json");
    let rows = body["events"].as_array().expect("events array");
    assert!(!rows.is_empty(), "denied decision missing from ledger");
    let serialized = serde_json::to_string(&body).expect("json");
    assert!(
        !serialized.contains(BOUNDARY_TEST_SECRET),
        "protected audit must never contain the raw payload"
    );
    assert!(serialized.contains("prohibited_provider"));
    assert!(serialized.contains("payload_hash"));
}
