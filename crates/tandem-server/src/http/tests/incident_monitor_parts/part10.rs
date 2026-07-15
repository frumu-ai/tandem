// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[derive(Clone, Copy)]
enum FakeMcpToolCallMode {
    Success,
    Failure,
    AuthRequired,
}

async fn spawn_fake_incident_monitor_mcp_tool_server(
    tool_name: &str,
    fail_call: bool,
) -> (String, Arc<RwLock<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    spawn_fake_incident_monitor_mcp_tool_server_with_mode(
        tool_name,
        if fail_call {
            FakeMcpToolCallMode::Failure
        } else {
            FakeMcpToolCallMode::Success
        },
    )
    .await
}

async fn spawn_fake_incident_monitor_mcp_tool_server_with_mode(
    tool_name: &str,
    mode: FakeMcpToolCallMode,
) -> (String, Arc<RwLock<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake incident monitor mcp tool listener");
    let addr = listener
        .local_addr()
        .expect("fake incident monitor mcp tool addr");
    let calls = Arc::new(RwLock::new(Vec::<Value>::new()));
    let tool_name = tool_name.to_string();
    let app = axum::Router::new().route(
        "/",
        axum::routing::post({
            let calls = calls.clone();
            move |axum::Json(request): axum::Json<Value>| {
                let calls = calls.clone();
                let tool_name = tool_name.clone();
                let mode = mode;
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
                                "name": "incident-router",
                                "version": "test"
                            }
                        }),
                        "tools/list" => json!({
                            "tools": [{
                                "name": tool_name,
                                "description": "Publish a mapped Incident Monitor payload",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "title": {"type": "string"},
                                        "fingerprint": {"type": "string"},
                                        "detail": {"type": "string"},
                                        "evidence_digest": {"type": "string"}
                                    }
                                }
                            }]
                        }),
                        "tools/call" => {
                            let name = request
                                .get("params")
                                .and_then(|row| row.get("name"))
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            let arguments = request
                                .get("params")
                                .and_then(|row| row.get("arguments"))
                                .cloned()
                                .unwrap_or(Value::Null);
                            calls.write().await.push(json!({
                                "name": name,
                                "arguments": arguments,
                            }));
                            match mode {
                                FakeMcpToolCallMode::Failure => {
                                    return axum::Json(json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "error": {
                                            "code": -32000,
                                            "message": "remote tool exploded token=SECRET_TOKEN_123"
                                        }
                                    }));
                                }
                                FakeMcpToolCallMode::AuthRequired => {
                                    return axum::Json(json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "result": {
                                            "structuredContent": {
                                                "authorizationUrl": "https://auth.example.test/authorize",
                                                "message": "Authorize this MCP connector first"
                                            }
                                        }
                                    }));
                                }
                                FakeMcpToolCallMode::Success => {}
                            }
                            json!({
                                "content": [{
                                    "type": "text",
                                    "text": "incident routed token=SECRET_TOKEN_123"
                                }]
                            })
                        }
                        other => json!({
                            "content": [{
                                "type": "text",
                                "text": format!("unsupported method {other}")
                            }]
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
            .expect("serve fake incident monitor mcp tool");
    });
    (format!("http://{addr}"), calls, server)
}

async fn configure_mcp_tool_incident_monitor_destination(
    state: &AppState,
    endpoint: String,
    destination_tool: &str,
    config: Value,
) {
    state
        .mcp
        .add_or_update(
            "incident-router".to_string(),
            endpoint,
            std::collections::HashMap::new(),
            true,
        )
        .await;
    assert!(
        state
            .mcp
            .set_allowed_tools(
                "incident-router",
                Some(vec!["incident_publish".to_string()])
            )
            .await
    );
    assert!(state.mcp.connect("incident-router").await);
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            mcp_server: Some("incident-router".to_string()),
            destinations: vec![crate::IncidentMonitorDestinationConfig {
                destination_id: "mcp-primary".to_string(),
                name: "Primary MCP tool".to_string(),
                kind: crate::IncidentMonitorDestinationKind::McpTool,
                mcp_server: Some("incident-router".to_string()),
                mcp_tool: Some(destination_tool.to_string()),
                config: Some(config),
                ..Default::default()
            }],
            default_destination_ids: vec!["mcp-primary".to_string()],
            safety_defaults: crate::IncidentMonitorSafetyDefaults {
                block_unready_destinations: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .expect("config");
}

async fn publish_incident_monitor_mcp_draft(app: axum::Router, draft_id: &str) -> (StatusCode, Value) {
    let publish_req = Request::builder()
        .method("POST")
        .uri(format!("/incident-monitor/drafts/{draft_id}/publish"))
        .body(Body::empty())
        .expect("publish request");
    let publish_resp = app.oneshot(publish_req).await.expect("publish response");
    let status = publish_resp.status();
    let body = to_bytes(publish_resp.into_body(), usize::MAX)
        .await
        .expect("publish body");
    (
        status,
        serde_json::from_slice(&body)
            .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&body))),
    )
}

fn mcp_tool_destination_config() -> Value {
    json!({
        "allow_publish": true,
        "payload": {
            "title": "$draft.title",
            "fingerprint": "$draft.fingerprint",
            "detail": "$draft.detail",
            "repo": "$draft.repo",
            "evidence_digest": "$evidence.digest",
            "route_reason": "$destination.route_match_reason",
            "static_source": "incident-monitor"
        }
    })
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_mcp_tool_destination_publishes_mapped_payload_and_skips_duplicate() {
    let (endpoint, calls, server) =
        spawn_fake_incident_monitor_mcp_tool_server("incident_publish", false).await;
    let state = test_state().await;
    configure_mcp_tool_incident_monitor_destination(
        &state,
        endpoint,
        "incident_publish",
        mcp_tool_destination_config(),
    )
    .await;

    let app = app_router(state.clone());
    let preview_req = Request::builder()
        .method("POST")
        .uri("/incident-monitor/route-preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "source": "desktop_logs",
                "risk_level": "medium",
                "destination_ids": ["mcp-primary"]
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_resp = app
        .clone()
        .oneshot(preview_req)
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_payload: Value = serde_json::from_slice(
        &to_bytes(preview_resp.into_body(), usize::MAX)
            .await
            .expect("preview body"),
    )
    .expect("preview json");
    assert_eq!(
        preview_payload.get("blocked").and_then(Value::as_bool),
        Some(false)
    );
    assert!(
        calls.read().await.is_empty(),
        "route preview must not call tools"
    );

    let draft_id =
        create_ready_secret_incident_monitor_draft(app.clone(), "fingerprint-mcp-tool-success").await;
    let (publish_status, publish_payload) =
        publish_incident_monitor_mcp_draft(app.clone(), &draft_id).await;
    assert_eq!(publish_status, StatusCode::OK, "{publish_payload:?}");
    assert_eq!(
        publish_payload.get("action").and_then(Value::as_str),
        Some("call_mcp_tool")
    );
    let post = publish_payload.get("post").expect("post");
    assert_eq!(
        post.get("destination_kind").and_then(Value::as_str),
        Some("mcp_tool")
    );
    assert_eq!(
        post.get("receipt")
            .and_then(|row| row.get("provider"))
            .and_then(Value::as_str),
        Some("mcp_tool")
    );
    assert_eq!(
        post.get("receipt")
            .and_then(|row| row.get("arguments_redacted"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(
        post.get("external_id")
            .and_then(Value::as_str)
            .is_some_and(|value| value.starts_with("bmmcp_")),
        "expected deterministic MCP record id: {post:?}"
    );
    let post_text = serde_json::to_string(post).expect("post json");
    assert!(!post_text.contains("SECRET_TOKEN_123"), "{post_text}");

    let call_rows = calls.read().await;
    assert_eq!(call_rows.len(), 1);
    assert_eq!(
        call_rows[0].get("name").and_then(Value::as_str),
        Some("incident_publish")
    );
    assert_eq!(
        call_rows[0]
            .get("arguments")
            .and_then(|row| row.get("fingerprint"))
            .and_then(Value::as_str),
        Some("fingerprint-mcp-tool-success")
    );
    assert!(
        call_rows[0]
            .get("arguments")
            .and_then(|row| row.get("detail"))
            .and_then(Value::as_str)
            .is_some_and(|value| {
                value.contains("[redacted]") && !value.contains("SECRET_TOKEN_123")
            }),
        "outbound MCP tool arguments must be redacted, not carry raw secrets (TAN-540): {call_rows:?}"
    );
    drop(call_rows);

    let first_post_id = post
        .get("post_id")
        .and_then(Value::as_str)
        .expect("post id")
        .to_string();
    let (second_status, second_payload) =
        publish_incident_monitor_mcp_draft(app.clone(), &draft_id).await;
    assert_eq!(second_status, StatusCode::OK, "{second_payload:?}");
    assert_eq!(
        second_payload.get("action").and_then(Value::as_str),
        Some("skip_duplicate")
    );
    assert_eq!(
        second_payload
            .get("post")
            .and_then(|row| row.get("post_id"))
            .and_then(Value::as_str),
        Some(first_post_id.as_str())
    );
    assert_eq!(calls.read().await.len(), 1);

    server.abort();
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_mcp_tool_destination_blocks_missing_tool_before_execution() {
    let (endpoint, calls, server) =
        spawn_fake_incident_monitor_mcp_tool_server("incident_publish", false).await;
    let state = test_state().await;
    configure_mcp_tool_incident_monitor_destination(
        &state,
        endpoint,
        "missing_tool",
        mcp_tool_destination_config(),
    )
    .await;

    let app = app_router(state.clone());
    let preview_req = Request::builder()
        .method("POST")
        .uri("/incident-monitor/route-preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "source": "desktop_logs",
                "destination_ids": ["mcp-primary"]
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_resp = app
        .clone()
        .oneshot(preview_req)
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_payload: Value = serde_json::from_slice(
        &to_bytes(preview_resp.into_body(), usize::MAX)
            .await
            .expect("preview body"),
    )
    .expect("preview json");
    assert_eq!(
        preview_payload.get("blocked").and_then(Value::as_bool),
        Some(true)
    );
    assert!(preview_payload
        .get("blocked_reasons")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| row
            .as_str()
            .is_some_and(|reason| reason.contains("MCP tool is not available"))))
        .unwrap_or(false));

    let draft_id =
        create_ready_secret_incident_monitor_draft(app.clone(), "fingerprint-mcp-tool-missing").await;
    let (publish_status, publish_payload) =
        publish_incident_monitor_mcp_draft(app.clone(), &draft_id).await;
    assert_eq!(publish_status, StatusCode::BAD_REQUEST);
    assert!(
        publish_payload
            .get("detail")
            .and_then(Value::as_str)
            .is_some_and(|detail| detail.contains("MCP tool is not available")),
        "publish should explain missing tool: {publish_payload:?}"
    );
    assert!(calls.read().await.is_empty());
    assert!(state.list_incident_monitor_posts(10).await.is_empty());

    server.abort();
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_mcp_tool_destination_blocks_malformed_mapping_before_execution() {
    let (endpoint, calls, server) =
        spawn_fake_incident_monitor_mcp_tool_server("incident_publish", false).await;
    let state = test_state().await;
    configure_mcp_tool_incident_monitor_destination(
        &state,
        endpoint,
        "incident_publish",
        json!({
            "allow_publish": true,
            "payload": "fingerprint=$draft.fingerprint"
        }),
    )
    .await;

    let app = app_router(state.clone());
    let draft_id =
        create_ready_secret_incident_monitor_draft(app.clone(), "fingerprint-mcp-tool-mapping").await;
    let (publish_status, publish_payload) =
        publish_incident_monitor_mcp_draft(app.clone(), &draft_id).await;
    assert_eq!(publish_status, StatusCode::BAD_REQUEST);
    assert!(
        publish_payload
            .get("detail")
            .and_then(Value::as_str)
            .is_some_and(|detail| detail.contains("MCP payload mapping must be an object")),
        "publish should explain malformed mapping: {publish_payload:?}"
    );
    assert!(calls.read().await.is_empty());
    assert!(state.list_incident_monitor_posts(10).await.is_empty());

    server.abort();
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_mcp_tool_destination_records_mapping_render_failure_after_claim() {
    let (endpoint, calls, server) =
        spawn_fake_incident_monitor_mcp_tool_server("incident_publish", false).await;
    let state = test_state().await;
    configure_mcp_tool_incident_monitor_destination(
        &state,
        endpoint,
        "incident_publish",
        json!({
            "allow_publish": true,
            "payload": {
                "title": "$draft.title",
                "unsupported": "$draft.url"
            }
        }),
    )
    .await;

    let app = app_router(state.clone());
    let draft_id =
        create_ready_secret_incident_monitor_draft(app.clone(), "fingerprint-mcp-tool-placeholder")
            .await;
    let (publish_status, publish_payload) =
        publish_incident_monitor_mcp_draft(app.clone(), &draft_id).await;
    assert_eq!(publish_status, StatusCode::BAD_REQUEST);
    assert!(
        publish_payload
            .get("detail")
            .and_then(Value::as_str)
            .is_some_and(|detail| detail.contains("Unsupported MCP payload mapping placeholder")),
        "publish should explain unsupported placeholder: {publish_payload:?}"
    );
    assert!(calls.read().await.is_empty());
    let posts = state
        .list_incident_monitor_posts_by_destination(10, "mcp-primary")
        .await;
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].status, "failed");
    assert!(
        posts[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("$draft.url")),
        "{posts:?}"
    );

    let (second_status, second_payload) =
        publish_incident_monitor_mcp_draft(app.clone(), &draft_id).await;
    assert_eq!(second_status, StatusCode::BAD_REQUEST);
    assert!(
        !second_payload
            .get("action")
            .and_then(Value::as_str)
            .is_some_and(|action| action == "publish_in_progress"),
        "mapping failures must not leave a pending claim: {second_payload:?}"
    );
    assert_eq!(
        state
            .list_incident_monitor_posts_by_destination(10, "mcp-primary")
            .await
            .len(),
        2
    );

    server.abort();
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_mcp_tool_destination_records_tool_failure_without_leaking_arguments() {
    let (endpoint, calls, server) =
        spawn_fake_incident_monitor_mcp_tool_server("incident_publish", true).await;
    let state = test_state().await;
    configure_mcp_tool_incident_monitor_destination(
        &state,
        endpoint,
        "incident_publish",
        mcp_tool_destination_config(),
    )
    .await;

    let app = app_router(state.clone());
    let draft_id =
        create_ready_secret_incident_monitor_draft(app.clone(), "fingerprint-mcp-tool-failure").await;
    let (publish_status, publish_payload) =
        publish_incident_monitor_mcp_draft(app.clone(), &draft_id).await;
    assert_eq!(publish_status, StatusCode::BAD_REQUEST);
    assert!(
        publish_payload
            .get("detail")
            .and_then(Value::as_str)
            .is_some_and(|detail| detail.contains("remote tool exploded")),
        "publish should explain MCP tool failure: {publish_payload:?}"
    );
    assert_eq!(calls.read().await.len(), 1);

    let posts = state
        .list_incident_monitor_posts_by_destination(10, "mcp-primary")
        .await;
    assert_eq!(posts.len(), 1);
    let post = &posts[0];
    assert_eq!(post.status, "failed");
    assert_eq!(
        post.destination_kind.as_ref(),
        Some(&crate::IncidentMonitorDestinationKind::McpTool)
    );
    assert!(
        post.error
            .as_deref()
            .is_some_and(|error| error.contains("remote tool exploded")),
        "{post:?}"
    );
    assert_eq!(
        post.receipt
            .as_ref()
            .and_then(|row| row.get("arguments_redacted"))
            .and_then(Value::as_bool),
        Some(true)
    );
    let post_text = serde_json::to_string(post).expect("post json");
    assert!(!post_text.contains("SECRET_TOKEN_123"), "{post_text}");

    server.abort();
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_mcp_tool_destination_blocks_auth_required_without_posting() {
    let (endpoint, calls, server) = spawn_fake_incident_monitor_mcp_tool_server_with_mode(
        "incident_publish",
        FakeMcpToolCallMode::AuthRequired,
    )
    .await;
    let state = test_state().await;
    configure_mcp_tool_incident_monitor_destination(
        &state,
        endpoint,
        "incident_publish",
        mcp_tool_destination_config(),
    )
    .await;

    let app = app_router(state.clone());
    let draft_id =
        create_ready_secret_incident_monitor_draft(app.clone(), "fingerprint-mcp-tool-auth").await;
    let (publish_status, publish_payload) =
        publish_incident_monitor_mcp_draft(app.clone(), &draft_id).await;
    assert_eq!(publish_status, StatusCode::BAD_REQUEST);
    assert!(
        publish_payload
            .get("detail")
            .and_then(Value::as_str)
            .is_some_and(|detail| detail.contains("MCP authorization required")),
        "publish should explain MCP auth requirement: {publish_payload:?}"
    );
    assert_eq!(calls.read().await.len(), 1);

    let posts = state
        .list_incident_monitor_posts_by_destination(10, "mcp-primary")
        .await;
    assert_eq!(posts.len(), 1);
    let post = &posts[0];
    assert_eq!(post.status, "blocked");
    assert_eq!(
        post.receipt
            .as_ref()
            .and_then(|row| row.get("mcp_auth_required"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(post
        .error
        .as_deref()
        .is_some_and(|error| error.contains("MCP authorization required")));

    let (second_status, second_payload) =
        publish_incident_monitor_mcp_draft(app.clone(), &draft_id).await;
    assert_eq!(second_status, StatusCode::BAD_REQUEST);
    assert!(
        !second_payload
            .get("action")
            .and_then(Value::as_str)
            .is_some_and(|action| action == "skip_duplicate"),
        "auth-required results must not be treated as posted duplicates: {second_payload:?}"
    );
    assert!(state
        .list_incident_monitor_posts_by_destination(10, "mcp-primary")
        .await
        .iter()
        .all(|row| row.status != "posted"));

    server.abort();
}
