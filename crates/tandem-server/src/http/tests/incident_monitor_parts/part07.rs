// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

async fn spawn_fake_incident_monitor_linear_mcp_server_with_issues(
    seeded_issues: Vec<Value>,
    fail_create: bool,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake incident monitor linear mcp listener");
    let addr = listener
        .local_addr()
        .expect("fake incident monitor linear mcp addr");
    let issues = Arc::new(RwLock::new(seeded_issues));
    let app = axum::Router::new().route(
        "/",
        axum::routing::post({
            let issues = issues.clone();
            move |axum::Json(request): axum::Json<Value>| {
                let issues = issues.clone();
                async move {
                    let id = request.get("id").cloned().unwrap_or(Value::Null);
                    let method = request
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let result = match method {
                        "initialize" => json!({
                            "protocolVersion": "2024-11-05",
                            "capabilities": {},
                            "serverInfo": {
                                "name": "linear",
                                "version": "test"
                            }
                        }),
                        "tools/list" => json!({
                            "tools": [
                                {
                                    "name": "list_issues",
                                    "description": "List Linear issues",
                                    "inputSchema": {"type":"object"}
                                },
                                {
                                    "name": "create_issue",
                                    "description": "Create a Linear issue",
                                    "inputSchema": {"type":"object"}
                                }
                            ]
                        }),
                        "tools/call" => {
                            let name = request
                                .get("params")
                                .and_then(|row| row.get("name"))
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            let arguments = request
                                .get("params")
                                .and_then(|row| row.get("arguments"))
                                .cloned()
                                .unwrap_or(Value::Null);
                            match name {
                                "list_issues" => {
                                    let query = arguments
                                        .get("query")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .trim()
                                        .to_ascii_lowercase();
                                    let snapshot = issues
                                        .read()
                                        .await
                                        .iter()
                                        .filter(|issue| {
                                            if query.is_empty() {
                                                return true;
                                            }
                                            let haystack = format!(
                                                "{}\n{}\n{}",
                                                issue
                                                    .get("title")
                                                    .and_then(Value::as_str)
                                                    .unwrap_or_default(),
                                                issue
                                                    .get("description")
                                                    .or_else(|| issue.get("body"))
                                                    .and_then(Value::as_str)
                                                    .unwrap_or_default(),
                                                issue
                                                    .get("identifier")
                                                    .and_then(Value::as_str)
                                                    .unwrap_or_default()
                                            )
                                            .to_ascii_lowercase();
                                            haystack.contains(&query)
                                        })
                                        .cloned()
                                        .collect::<Vec<_>>();
                                    json!({ "issues": snapshot })
                                }
                                "create_issue" => {
                                    if fail_create {
                                        return axum::Json(json!({
                                            "jsonrpc": "2.0",
                                            "id": id,
                                            "error": {
                                                "code": -32000,
                                                "message": "Linear create failed"
                                            }
                                        }));
                                    }
                                    let mut issue_rows = issues.write().await;
                                    let issue_number = (issue_rows.len() as u64) + 101;
                                    let identifier = format!("TAN-{issue_number}");
                                    let issue = json!({
                                        "id": format!("linear-issue-{issue_number}"),
                                        "identifier": identifier,
                                        "title": arguments
                                            .get("title")
                                            .and_then(Value::as_str)
                                            .unwrap_or("Incident Monitor issue"),
                                        "description": arguments
                                            .get("description")
                                            .or_else(|| arguments.get("body"))
                                            .and_then(Value::as_str)
                                            .unwrap_or(""),
                                        "state": {"name": "Todo"},
                                        "url": format!("https://linear.app/frumu/issue/TAN-{issue_number}/incident-monitor")
                                    });
                                    issue_rows.push(issue.clone());
                                    json!({ "issue": issue })
                                }
                                other => json!({
                                    "content": [
                                        {
                                            "type": "text",
                                            "text": format!("unsupported tool {other}")
                                        }
                                    ]
                                }),
                            }
                        }
                        other => json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": format!("unsupported method {other}")
                                }
                            ]
                        }),
                    };
                    axum::Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": result,
                    }))
                }
            }
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve fake incident monitor linear mcp");
    });
    (format!("http://{addr}"), server)
}

async fn spawn_fake_incident_monitor_linear_mcp_server() -> (String, tokio::task::JoinHandle<()>) {
    spawn_fake_incident_monitor_linear_mcp_server_with_issues(Vec::new(), false).await
}

async fn configure_linear_incident_monitor_destination(state: &AppState, endpoint: String) {
    state
        .mcp
        .add_or_update(
            "linear".to_string(),
            endpoint,
            std::collections::HashMap::new(),
            true,
        )
        .await;
    assert!(state.mcp.connect("linear").await);
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            mcp_server: Some("linear".to_string()),
            destinations: vec![crate::IncidentMonitorDestinationConfig {
                destination_id: "linear-primary".to_string(),
                name: "Primary Linear".to_string(),
                kind: crate::IncidentMonitorDestinationKind::LinearIssue,
                mcp_server: Some("linear".to_string()),
                linear_team: Some("Tandem".to_string()),
                linear_project: Some("Incident Monitor".to_string()),
                ..Default::default()
            }],
            default_destination_ids: vec!["linear-primary".to_string()],
            ..Default::default()
        })
        .await
        .expect("config");
}

async fn create_ready_linear_incident_monitor_draft(app: axum::Router, fingerprint: &str) -> String {
    let create_req = Request::builder()
        .method("POST")
        .uri("/incident-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "desktop_logs",
                    "title": "Linear destination failure",
                    "fingerprint": fingerprint,
                    "detail": "event: linear.destination.failure\ncomponent: incident-router",
                    "risk_level": "medium",
                    "confidence": "medium",
                    "excerpt": ["linear destination publish"]
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
        .uri(format!("/incident-monitor/drafts/{draft_id}/triage-run"))
        .body(Body::empty())
        .expect("triage request");
    let triage_resp = app
        .clone()
        .oneshot(triage_req)
        .await
        .expect("triage response");
    assert_eq!(triage_resp.status(), StatusCode::OK);
    write_ready_incident_monitor_triage_summary(app, &draft_id).await;
    draft_id
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_router_publishes_linear_destination_metadata_and_skips_duplicate() {
    let (endpoint, server) = spawn_fake_incident_monitor_linear_mcp_server().await;
    let state = test_state().await;
    configure_linear_incident_monitor_destination(&state, endpoint).await;

    let app = app_router(state.clone());
    let draft_id =
        create_ready_linear_incident_monitor_draft(app.clone(), "fingerprint-linear-create").await;

    let publish_req = Request::builder()
        .method("POST")
        .uri(format!("/incident-monitor/drafts/{draft_id}/publish"))
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
        publish_payload.get("issue_url").and_then(Value::as_str),
        Some("https://linear.app/frumu/issue/TAN-101/incident-monitor")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("destination_kind"))
            .and_then(Value::as_str),
        Some("linear_issue")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("target_ref"))
            .and_then(Value::as_str),
        Some("Tandem/Incident Monitor")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("external_id"))
            .and_then(Value::as_str),
        Some("TAN-101")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("receipt"))
            .and_then(|row| row.get("provider"))
            .and_then(Value::as_str),
        Some("linear")
    );
    assert_eq!(
        publish_payload
            .get("external_action")
            .and_then(|row| row.get("capability_id"))
            .and_then(Value::as_str),
        Some("linear.create_issue")
    );
    assert_eq!(
        publish_payload
            .get("external_action")
            .and_then(|row| row.get("target"))
            .and_then(Value::as_str),
        Some("Tandem/Incident Monitor")
    );
    let first_post_id = publish_payload
        .get("post")
        .and_then(|row| row.get("post_id"))
        .and_then(Value::as_str)
        .expect("post id")
        .to_string();

    let second_publish_req = Request::builder()
        .method("POST")
        .uri(format!("/incident-monitor/drafts/{draft_id}/publish"))
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
        .uri("/incident-monitor/posts?limit=10&destination_id=linear-primary")
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
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_router_links_existing_linear_issue_before_create() {
    let existing = json!({
        "id": "linear-existing",
        "identifier": "TAN-900",
        "title": "Existing Linear destination failure",
        "description": "Existing issue body includes fingerprint-linear-existing",
        "state": {"name": "In Progress"},
        "url": "https://linear.app/frumu/issue/TAN-900/existing-linear-destination-failure"
    });
    let (endpoint, server) =
        spawn_fake_incident_monitor_linear_mcp_server_with_issues(vec![existing], true).await;
    let state = test_state().await;
    configure_linear_incident_monitor_destination(&state, endpoint).await;

    let app = app_router(state.clone());
    let draft_id =
        create_ready_linear_incident_monitor_draft(app.clone(), "fingerprint-linear-existing").await;

    let publish_req = Request::builder()
        .method("POST")
        .uri(format!("/incident-monitor/drafts/{draft_id}/publish"))
        .body(Body::empty())
        .expect("publish request");
    let publish_resp = app
        .clone()
        .oneshot(publish_req)
        .await
        .expect("publish response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_payload: Value = serde_json::from_slice(
        &to_bytes(publish_resp.into_body(), usize::MAX)
            .await
            .expect("publish body"),
    )
    .expect("publish json");
    assert_eq!(
        publish_payload.get("action").and_then(Value::as_str),
        Some("matched_linear_issue")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("operation"))
            .and_then(Value::as_str),
        Some("match_issue")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("external_id"))
            .and_then(Value::as_str),
        Some("TAN-900")
    );
    assert_eq!(
        publish_payload
            .get("external_action")
            .and_then(|row| row.get("capability_id"))
            .and_then(Value::as_str),
        Some("linear.list_issues")
    );

    let second_publish_req = Request::builder()
        .method("POST")
        .uri(format!("/incident-monitor/drafts/{draft_id}/publish"))
        .body(Body::empty())
        .expect("second publish request");
    let second_publish_resp = app
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
            .get("draft")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("linear_issue_matched")
    );
    assert_eq!(
        second_publish_payload
            .get("draft")
            .and_then(|row| row.get("github_status"))
            .and_then(Value::as_str),
        Some("linear_issue_matched")
    );
    assert_eq!(
        second_publish_payload
            .get("post")
            .and_then(|row| row.get("operation"))
            .and_then(Value::as_str),
        Some("match_issue")
    );

    server.abort();
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_router_records_linear_create_failure_receipt() {
    let (endpoint, server) =
        spawn_fake_incident_monitor_linear_mcp_server_with_issues(Vec::new(), true).await;
    let state = test_state().await;
    configure_linear_incident_monitor_destination(&state, endpoint).await;

    let app = app_router(state.clone());
    let draft_id =
        create_ready_linear_incident_monitor_draft(app.clone(), "fingerprint-linear-failure").await;

    let publish_req = Request::builder()
        .method("POST")
        .uri(format!("/incident-monitor/drafts/{draft_id}/publish"))
        .body(Body::empty())
        .expect("publish request");
    let publish_resp = app
        .clone()
        .oneshot(publish_req)
        .await
        .expect("publish response");
    assert_eq!(publish_resp.status(), StatusCode::BAD_REQUEST);
    let publish_payload: Value = serde_json::from_slice(
        &to_bytes(publish_resp.into_body(), usize::MAX)
            .await
            .expect("publish body"),
    )
    .expect("publish json");
    assert!(
        publish_payload
            .get("detail")
            .and_then(Value::as_str)
            .is_some_and(|detail| detail.contains("Linear create failed")),
        "publish should return Linear failure detail: {publish_payload:?}"
    );

    let posts = state.list_incident_monitor_posts(10).await;
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].status, "failed");
    let failed_post_id = posts[0].post_id.clone();
    assert_eq!(
        posts[0].destination_kind,
        Some(crate::IncidentMonitorDestinationKind::LinearIssue)
    );
    assert!(posts[0]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("Linear create failed")));
    let stored = state
        .get_incident_monitor_draft(&draft_id)
        .await
        .expect("stored draft");
    assert_eq!(stored.github_status.as_deref(), Some("linear_post_failed"));

    let second_publish_req = Request::builder()
        .method("POST")
        .uri(format!("/incident-monitor/drafts/{draft_id}/publish"))
        .body(Body::empty())
        .expect("second publish request");
    let second_publish_resp = app
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
        Some("create_issue_retry_suppressed")
    );
    assert_eq!(
        second_publish_payload
            .get("post")
            .and_then(|row| row.get("post_id"))
            .and_then(Value::as_str),
        Some(failed_post_id.as_str())
    );
    assert_eq!(state.list_incident_monitor_posts(10).await.len(), 1);

    server.abort();
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_router_requires_linear_destination_approval() {
    let (endpoint, server) = spawn_fake_incident_monitor_linear_mcp_server().await;
    let state = test_state().await;
    configure_linear_incident_monitor_destination(&state, endpoint).await;
    let mut config = state.incident_monitor_config().await;
    config.destinations[0].require_approval = true;
    state.put_incident_monitor_config(config).await.expect("config");

    let draft = state
        .submit_incident_monitor_draft(crate::IncidentMonitorSubmission {
            source: Some("manual".to_string()),
            title: Some("Linear approval publish".to_string()),
            detail: Some("A Linear destination requiring approval should pause".to_string()),
            risk_level: Some("medium".to_string()),
            confidence: Some("medium".to_string()),
            ..Default::default()
        })
        .await
        .expect("draft");

    let app = app_router(state.clone());
    let publish_req = Request::builder()
        .method("POST")
        .uri(format!("/incident-monitor/drafts/{}/publish", draft.draft_id))
        .body(Body::empty())
        .expect("publish request");
    let publish_resp = app.oneshot(publish_req).await.expect("publish response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_payload: Value = serde_json::from_slice(
        &to_bytes(publish_resp.into_body(), usize::MAX)
            .await
            .expect("publish body"),
    )
    .expect("publish json");
    assert_eq!(
        publish_payload.get("action").and_then(Value::as_str),
        Some("approval_required")
    );
    assert!(publish_payload.get("post").is_some_and(Value::is_null));
    assert!(state.list_incident_monitor_posts(10).await.is_empty());

    server.abort();
}
