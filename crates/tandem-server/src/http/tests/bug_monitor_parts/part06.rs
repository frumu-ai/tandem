#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_router_publishes_configured_github_destination_metadata() {
    let (endpoint, server) = spawn_fake_bug_monitor_github_mcp_server().await;

    let state = test_state().await;
    state
        .mcp
        .add_or_update(
            "github".to_string(),
            endpoint,
            std::collections::HashMap::new(),
            true,
        )
        .await;
    assert!(state.mcp.connect("github").await);
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            mcp_server: Some("github".to_string()),
            destinations: vec![crate::BugMonitorDestinationConfig {
                destination_id: "github-primary".to_string(),
                name: "Primary GitHub".to_string(),
                kind: crate::BugMonitorDestinationKind::GithubIssue,
                repo: Some("acme/incidents".to_string()),
                mcp_server: Some("github".to_string()),
                ..Default::default()
            }],
            default_destination_ids: vec!["github-primary".to_string()],
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state.clone());
    let create_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "desktop_logs",
                    "title": "Configured GitHub destination failure",
                    "fingerprint": "fingerprint-configured-github",
                    "detail": "event: configured.github.destination\ncomponent: incident-router",
                    "risk_level": "medium",
                    "confidence": "medium",
                    "excerpt": ["configured destination publish"]
                }
            })
            .to_string(),
        ))
        .expect("request");
    let create_resp = app.clone().oneshot(create_req).await.expect("response");
    assert_eq!(create_resp.status(), StatusCode::OK);
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

    let triage_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/triage-run"))
        .body(Body::empty())
        .expect("triage request");
    let triage_resp = app
        .clone()
        .oneshot(triage_req)
        .await
        .expect("triage response");
    assert_eq!(triage_resp.status(), StatusCode::OK);
    write_ready_bug_monitor_triage_summary(app.clone(), &draft_id).await;

    let mut event_rx = state.event_bus.subscribe();
    let publish_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/publish"))
        .body(Body::empty())
        .expect("publish request");
    let publish_resp = app
        .clone()
        .oneshot(publish_req)
        .await
        .expect("publish response");
    let publish_status = publish_resp.status();
    let publish_body = to_bytes(publish_resp.into_body(), usize::MAX)
        .await
        .expect("publish body");
    if publish_status != StatusCode::OK {
        panic!("{}", String::from_utf8_lossy(&publish_body));
    }
    let publish_payload: Value = serde_json::from_slice(&publish_body).expect("publish json");
    assert_eq!(
        publish_payload.get("action").and_then(Value::as_str),
        Some("create_issue")
    );
    let attempted = next_event_of_type(&mut event_rx, "bug_monitor.publish.attempted").await;
    assert_eq!(
        attempted.properties.get("draft_id").and_then(Value::as_str),
        Some(draft_id.as_str())
    );
    assert_eq!(
        attempted
            .properties
            .get("selected_destination_id")
            .and_then(Value::as_str),
        Some("github-primary")
    );
    let completed = next_event_of_type(&mut event_rx, "bug_monitor.publish.completed").await;
    assert_eq!(
        completed.properties.get("action").and_then(Value::as_str),
        Some("create_issue")
    );
    let protected_audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit log");
    assert!(protected_audit.contains("\"event_type\":\"bug_monitor.publish.attempted\""));
    assert!(protected_audit.contains("\"event_type\":\"bug_monitor.publish.completed\""));
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("destination_id"))
            .and_then(Value::as_str),
        Some("github-primary")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("route_match_reason"))
            .and_then(Value::as_str),
        Some("default_destination")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("repo"))
            .and_then(Value::as_str),
        Some("acme/incidents")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("target_ref"))
            .and_then(Value::as_str),
        Some("acme/incidents")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("issue_url"))
            .and_then(Value::as_str),
        Some("https://github.com/acme/incidents/issues/101")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("receipt"))
            .and_then(|row| row.get("destination_id"))
            .and_then(Value::as_str),
        Some("github-primary")
    );
    assert_eq!(
        publish_payload
            .get("external_action")
            .and_then(|row| row.get("metadata"))
            .and_then(|row| row.get("destination_id"))
            .and_then(Value::as_str),
        Some("github-primary")
    );
    assert_eq!(
        publish_payload
            .get("external_action")
            .and_then(|row| row.get("target"))
            .and_then(Value::as_str),
        Some("acme/incidents")
    );
    assert_eq!(
        publish_payload
            .get("external_action")
            .and_then(|row| row.get("metadata"))
            .and_then(|row| row.get("target_ref"))
            .and_then(Value::as_str),
        Some("acme/incidents")
    );
    let first_post_id = publish_payload
        .get("post")
        .and_then(|row| row.get("post_id"))
        .and_then(Value::as_str)
        .expect("post id")
        .to_string();

    let second_publish_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/publish"))
        .body(Body::empty())
        .expect("second publish request");
    let second_publish_resp = app
        .clone()
        .oneshot(second_publish_req)
        .await
        .expect("second publish response");
    assert_eq!(second_publish_resp.status(), StatusCode::OK);
    let second_publish_payload: Value = serde_json::from_slice(
        &to_bytes(second_publish_resp.into_body(), usize::MAX)
            .await
            .expect("second publish body"),
    )
    .expect("second publish json");
    assert_eq!(
        second_publish_payload.get("action").and_then(Value::as_str),
        Some("skip_duplicate")
    );
    assert_eq!(
        second_publish_payload
            .get("post")
            .and_then(|row| row.get("post_id"))
            .and_then(Value::as_str),
        Some(first_post_id.as_str())
    );

    let posts_req = Request::builder()
        .method("GET")
        .uri("/bug-monitor/posts?limit=10&destination_id=github-primary")
        .body(Body::empty())
        .expect("posts request");
    let posts_resp = app.oneshot(posts_req).await.expect("posts response");
    assert_eq!(posts_resp.status(), StatusCode::OK);
    let posts_payload: Value = serde_json::from_slice(
        &to_bytes(posts_resp.into_body(), usize::MAX)
            .await
            .expect("posts body"),
    )
    .expect("posts json");
    assert_eq!(posts_payload.get("count").and_then(Value::as_u64), Some(1));

    server.abort();
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_config_patch_emits_redacted_admin_audit() {
    let state = test_state().await;
    state.set_api_token(Some("tk_admin".to_string())).await;
    let app = app_router(state.clone());
    let mut event_rx = state.event_bus.subscribe();

    let patch_req = Request::builder()
        .method("PATCH")
        .uri("/config/bug-monitor")
        .header("content-type", "application/json")
        .header("x-tandem-token", "tk_admin")
        .body(Body::from(
            json!({
                "bug_monitor": {
                    "enabled": true,
                    "repo": "acme/platform",
                    "workspace_root": "/tmp/acme",
                    "destinations": [{
                        "destination_id": "webhook-audit",
                        "name": "Webhook Audit",
                        "kind": "webhook",
                        "webhook_url": "https://hooks.example.test/incidents",
                        "webhook_secret_ref": "env:BUG_MONITOR_AUDIT_SECRET"
                    }],
                    "routes": [{
                        "route_id": "audit-route",
                        "name": "Audit Route",
                        "priority": 10,
                        "destination_ids": ["webhook-audit"],
                        "match_tenant_ids": ["tenant-a"],
                        "match_workspace_ids": ["workspace-a"]
                    }],
                    "default_destination_ids": ["webhook-audit"]
                }
            })
            .to_string(),
        ))
        .expect("patch config request");
    let patch_resp = app.oneshot(patch_req).await.expect("patch config response");
    assert_eq!(patch_resp.status(), StatusCode::OK);

    let event = next_event_of_type(&mut event_rx, "bug_monitor.config.updated").await;
    assert_eq!(
        event.properties.get("route_count").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        event
            .properties
            .pointer("/destinations/0/has_webhook_secret_ref")
            .and_then(Value::as_bool),
        Some(true)
    );
    let event_text = serde_json::to_string(&event.properties).expect("event json");
    assert!(!event_text.contains("BUG_MONITOR_AUDIT_SECRET"));

    let protected_audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit log");
    assert!(protected_audit.contains("\"event_type\":\"bug_monitor.config.updated\""));
    assert!(!protected_audit.contains("BUG_MONITOR_AUDIT_SECRET"));
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_scoped_intake_key_cannot_call_privileged_routes() {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("bug monitor scoped intake workspace");
    std::fs::create_dir_all(workspace.path().join("logs")).expect("logs dir");
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some(workspace.path().display().to_string()),
            monitored_projects: vec![crate::BugMonitorMonitoredProject {
                project_id: "payments".to_string(),
                name: "Payments".to_string(),
                repo: "acme/payments".to_string(),
                workspace_root: workspace.path().display().to_string(),
                log_sources: vec![crate::BugMonitorLogSource {
                    source_id: "ci".to_string(),
                    path: "logs/ci.jsonl".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        })
        .await
        .expect("config");
    let raw_key = "tbm_intake_report_only_scope_test";
    state
        .put_bug_monitor_intake_key(crate::BugMonitorProjectIntakeKey {
            key_id: "intake-key-report-only".to_string(),
            project_id: "payments".to_string(),
            name: "Report only".to_string(),
            key_hash: crate::sha256_hex(&[raw_key]),
            enabled: true,
            scopes: vec!["bug_monitor:report".to_string()],
            created_at_ms: Some(crate::now_ms()),
            last_used_at_ms: None,
        })
        .await
        .expect("intake key");
    state.set_api_token(Some("tk_admin".to_string())).await;

    let app = app_router(state.clone());
    let intake_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/intake/report")
        .header("content-type", "application/json")
        .header("x-tandem-bug-monitor-intake-key", raw_key)
        .body(Body::from(
            json!({
                "project_id": "payments",
                "source_id": "ci",
                "report": {
                    "title": "Scoped intake report",
                    "fingerprint": "scoped-intake-report-only",
                    "detail": "event: scoped.intake\ncomponent: auth-boundary",
                    "risk_level": "medium",
                    "confidence": "medium",
                    "excerpt": ["scoped intake can create a draft"]
                }
            })
            .to_string(),
        ))
        .expect("intake report request");
    let intake_resp = app
        .clone()
        .oneshot(intake_req)
        .await
        .expect("intake response");
    assert_eq!(intake_resp.status(), StatusCode::OK);
    let intake_payload: Value = serde_json::from_slice(
        &to_bytes(intake_resp.into_body(), usize::MAX)
            .await
            .expect("intake body"),
    )
    .expect("intake json");
    let draft_id = intake_payload
        .get("draft")
        .and_then(|row| row.get("draft_id"))
        .and_then(Value::as_str)
        .expect("draft id")
        .to_string();

    let privileged_requests = vec![
        Request::builder()
            .method("GET")
            .uri("/config/bug-monitor")
            .header("x-tandem-bug-monitor-intake-key", raw_key)
            .body(Body::empty())
            .expect("config request"),
        Request::builder()
            .method("GET")
            .uri("/bug-monitor/security/authority-inventory")
            .header("x-tandem-bug-monitor-intake-key", raw_key)
            .body(Body::empty())
            .expect("authority inventory request"),
        Request::builder()
            .method("GET")
            .uri("/failure-reporter/security/authority-inventory")
            .header("x-tandem-bug-monitor-intake-key", raw_key)
            .body(Body::empty())
            .expect("failure reporter authority inventory request"),
        Request::builder()
            .method("POST")
            .uri("/bug-monitor/route-preview")
            .header("content-type", "application/json")
            .header("x-tandem-bug-monitor-intake-key", raw_key)
            .body(Body::from(
                json!({
                    "draft_id": draft_id.as_str(),
                    "destination_ids": ["legacy-github"]
                })
                .to_string(),
            ))
            .expect("preview request"),
        Request::builder()
            .method("POST")
            .uri("/bug-monitor/report")
            .header("content-type", "application/json")
            .header("x-tandem-bug-monitor-intake-key", raw_key)
            .body(Body::from(
                json!({
                    "report": {
                        "title": "privileged report",
                        "fingerprint": "scoped-intake-privileged-report",
                        "detail": "should be rejected"
                    }
                })
                .to_string(),
            ))
            .expect("report request"),
        Request::builder()
            .method("POST")
            .uri("/bug-monitor/intake/keys")
            .header("content-type", "application/json")
            .header("x-tandem-bug-monitor-intake-key", raw_key)
            .body(Body::from(
                json!({
                    "project_id": "payments",
                    "name": "should-not-create"
                })
                .to_string(),
            ))
            .expect("key request"),
        Request::builder()
            .method("POST")
            .uri(format!("/bug-monitor/drafts/{draft_id}/publish"))
            .header("x-tandem-bug-monitor-intake-key", raw_key)
            .body(Body::empty())
            .expect("publish request"),
    ];

    for request in privileged_requests {
        let resp = app
            .clone()
            .oneshot(request)
            .await
            .expect("privileged response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
