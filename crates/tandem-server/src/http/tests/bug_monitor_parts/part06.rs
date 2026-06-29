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
                repo: Some("acme/platform".to_string()),
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
