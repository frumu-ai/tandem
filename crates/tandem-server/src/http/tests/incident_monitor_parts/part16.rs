// Adversarial scenario pack endpoint tests (TAN-487).

async fn configure_scenario_pack_incident_monitor(state: &AppState, require_approval_for_high_risk: bool) {
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            destinations: vec![crate::IncidentMonitorDestinationConfig {
                destination_id: "gh-default".to_string(),
                name: "Default GitHub".to_string(),
                kind: crate::IncidentMonitorDestinationKind::GithubIssue,
                enabled: true,
                repo: Some("acme/platform".to_string()),
                ..Default::default()
            }],
            default_destination_ids: vec!["gh-default".to_string()],
            safety_defaults: crate::IncidentMonitorSafetyDefaults {
                require_approval_for_high_risk,
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .expect("config");
}

async fn run_scenario_packs(app: axum::Router, body: Value, token: Option<&str>) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/incident-monitor/security/scenario-packs")
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("x-tandem-token", token);
    }
    let resp = app
        .oneshot(builder.body(Body::from(body.to_string())).expect("scenario request"))
        .await
        .expect("scenario response");
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("scenario body");
    let value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&bytes)));
    (status, value)
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_scenario_packs_pass_on_governed_config() {
    // TAN-487: with a governed config (default destination + high-risk approval),
    // every built-in adversarial scenario's control expectation is met, in
    // dry-run, with no external mutation.
    let state = test_state().await;
    configure_scenario_pack_incident_monitor(&state, true).await;
    let app = app_router(state);

    let (status, payload) = run_scenario_packs(app, json!({}), None).await;
    assert_eq!(status, StatusCode::OK, "{payload:?}");
    assert_eq!(payload["pack"]["mutates_external_systems"], json!(false));
    assert_eq!(payload["scope"]["dry_run"], json!(true));

    let total = payload["pack"]["counts"]["total"].as_u64().expect("total");
    let passed = payload["pack"]["counts"]["passed"].as_u64().expect("passed");
    assert!(total >= 8, "default pack should have >= 8 scenarios: {payload}");
    assert_eq!(
        passed, total,
        "a governed config should pass every scenario: {}",
        payload["pack"]["results"]
    );
    // The unsafe/unknown-destination scenarios must fail closed (blocked).
    let results = payload["pack"]["results"].as_array().expect("results");
    let unsafe_scenario = results
        .iter()
        .find(|row| row["scenario_id"] == json!("unsafe_unready_destination_blocked"))
        .expect("unsafe scenario present");
    assert_eq!(unsafe_scenario["route_preview"]["blocked"], json!(true));
    assert_eq!(unsafe_scenario["status"], json!("pass"));
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_scenario_packs_surface_gap_when_approval_disabled() {
    // With the high-risk approval gate disabled, the escalation scenarios fail,
    // surfacing the governance gap.
    let state = test_state().await;
    configure_scenario_pack_incident_monitor(&state, false).await;
    let app = app_router(state);

    let (status, payload) = run_scenario_packs(app, json!({}), None).await;
    assert_eq!(status, StatusCode::OK, "{payload:?}");
    let failed = payload["pack"]["counts"]["failed"].as_u64().expect("failed");
    assert!(
        failed >= 1,
        "disabling high-risk approval must surface at least one failed scenario: {}",
        payload["pack"]["results"]
    );
    // A failed scenario carries a finding id for evidence linkage.
    let results = payload["pack"]["results"].as_array().expect("results");
    assert!(
        results
            .iter()
            .any(|row| row["status"] == json!("fail") && row["finding_id"].is_string()),
        "failed scenarios must generate finding ids: {}",
        payload["pack"]["results"]
    );
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_scenario_packs_can_filter_by_scenario_id() {
    let state = test_state().await;
    configure_scenario_pack_incident_monitor(&state, true).await;
    let app = app_router(state);

    let (status, payload) = run_scenario_packs(
        app,
        json!({ "scenario_ids": ["prompt_injection_requires_approval"] }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{payload:?}");
    let results = payload["pack"]["results"].as_array().expect("results");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["scenario_id"], json!("prompt_injection_requires_approval"));
    assert_eq!(results[0]["category"], json!("prompt_injection"));
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_scenario_packs_require_admin_token() {
    let state = test_state().await;
    configure_scenario_pack_incident_monitor(&state, true).await;
    state.set_api_token(Some("tk_admin".to_string())).await;
    let app = app_router(state);

    // Missing token → unauthorized.
    let (status, _payload) = run_scenario_packs(app.clone(), json!({}), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Correct admin token → ok.
    let (status, _payload) = run_scenario_packs(app, json!({}), Some("tk_admin")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_scenario_packs_reject_scoped_intake_key() {
    // With no admin API token configured, the handler's own guard must still
    // reject a scoped intake key (scenario packs are an admin-only surface).
    let state = test_state().await;
    configure_scenario_pack_incident_monitor(&state, true).await;
    let app = app_router(state);
    let denied = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/incident-monitor/security/scenario-packs")
                .header("content-type", "application/json")
                .header("x-tandem-incident-monitor-intake-key", "tim_scoped")
                .body(Body::from(json!({}).to_string()))
                .expect("scoped request"),
        )
        .await
        .expect("scoped response");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_scenario_packs_listing_describes_pack_without_running() {
    let state = test_state().await;
    let app = app_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/incident-monitor/security/scenario-packs")
                .body(Body::empty())
                .expect("list request"),
        )
        .await
        .expect("list response");
    assert_eq!(resp.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("list body"),
    )
    .expect("list json");
    let scenarios = payload["packs"][0]["scenarios"]
        .as_array()
        .expect("scenarios");
    assert!(scenarios.len() >= 8);
    // Listing must not execute anything (no results field on the pack).
    assert!(payload["packs"][0].get("results").is_none());
}
