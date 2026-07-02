#[derive(Debug, Clone)]
struct FakeIncidentMonitorWebhookRequest {
    headers: std::collections::BTreeMap<String, String>,
    body: Vec<u8>,
}

async fn spawn_fake_incident_monitor_webhook_server(
    statuses: Vec<u16>,
    delay_ms: u64,
) -> (
    String,
    Arc<RwLock<Vec<FakeIncidentMonitorWebhookRequest>>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake incident monitor webhook listener");
    let addr = listener
        .local_addr()
        .expect("fake incident monitor webhook addr");
    let requests = Arc::new(RwLock::new(Vec::<FakeIncidentMonitorWebhookRequest>::new()));
    let statuses = Arc::new(RwLock::new(if statuses.is_empty() {
        vec![202]
    } else {
        statuses
    }));
    let app = axum::Router::new().route(
        "/incident",
        axum::routing::post({
            let requests = requests.clone();
            let statuses = statuses.clone();
            move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
                let requests = requests.clone();
                let statuses = statuses.clone();
                async move {
                    let header_snapshot = headers
                        .iter()
                        .filter_map(|(name, value)| {
                            value.to_str().ok().map(|value| {
                                (name.as_str().to_ascii_lowercase(), value.to_string())
                            })
                        })
                        .collect::<std::collections::BTreeMap<_, _>>();
                    requests.write().await.push(FakeIncidentMonitorWebhookRequest {
                        headers: header_snapshot,
                        body: body.to_vec(),
                    });
                    if delay_ms > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                    let status = {
                        let mut rows = statuses.write().await;
                        if rows.len() > 1 {
                            rows.remove(0)
                        } else {
                            rows.first().copied().unwrap_or(202)
                        }
                    };
                    (
                        axum::http::StatusCode::from_u16(status).expect("fake webhook status"),
                        "ok",
                    )
                }
            }
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve fake incident monitor webhook");
    });
    (format!("http://{addr}/incident"), requests, server)
}

async fn configure_webhook_incident_monitor_destination(
    state: &AppState,
    endpoint: String,
    destination_config: Value,
) {
    std::env::set_var(
        "TANDEM_TEST_INCIDENT_MONITOR_WEBHOOK_SECRET",
        "test-webhook-signing-secret",
    );
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            destinations: vec![crate::IncidentMonitorDestinationConfig {
                destination_id: "webhook-primary".to_string(),
                name: "Primary webhook".to_string(),
                kind: crate::IncidentMonitorDestinationKind::Webhook,
                webhook_url: Some(endpoint),
                webhook_secret_ref: Some("env:TANDEM_TEST_INCIDENT_MONITOR_WEBHOOK_SECRET".to_string()),
                config: Some(destination_config),
                ..Default::default()
            }],
            default_destination_ids: vec!["webhook-primary".to_string()],
            ..Default::default()
        })
        .await
        .expect("config");
}

async fn publish_incident_monitor_webhook_draft(
    app: axum::Router,
    draft_id: &str,
) -> (StatusCode, Value) {
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

fn assert_tandem_webhook_signature(
    headers: &std::collections::BTreeMap<String, String>,
    body: &[u8],
) {
    let signature = headers.get("x-tandem-signature").expect("signature header");
    let timestamp = signature
        .strip_prefix("t=")
        .and_then(|rest| rest.split_once(",v1=").map(|(timestamp, _)| timestamp))
        .and_then(|value| value.parse::<u64>().ok())
        .expect("signature timestamp");
    let expected = crate::app::state::automation_webhook_signature_header(
        "test-webhook-signing-secret",
        timestamp,
        body,
    );
    assert_eq!(signature, &expected);
    assert_eq!(
        headers.get("x-tandem-signature-scheme").map(String::as_str),
        Some("tandem_hmac_sha256_v1")
    );
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_webhook_destination_publishes_signed_payload_and_skips_duplicate() {
    let (endpoint, requests, server) = spawn_fake_incident_monitor_webhook_server(vec![202], 0).await;
    let state = test_state().await;
    configure_webhook_incident_monitor_destination(
        &state,
        endpoint,
        json!({
            "allow_private_networks": true,
            "allow_insecure_http": true,
            "max_attempts": 2
        }),
    )
    .await;

    let app = app_router(state.clone());
    let draft_id =
        create_ready_linear_incident_monitor_draft(app.clone(), "fingerprint-webhook-signed").await;

    let (publish_status, publish_payload) =
        publish_incident_monitor_webhook_draft(app.clone(), &draft_id).await;
    assert_eq!(publish_status, StatusCode::OK, "{publish_payload:?}");
    assert_eq!(
        publish_payload.get("action").and_then(Value::as_str),
        Some("post_webhook")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("destination_kind"))
            .and_then(Value::as_str),
        Some("webhook")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("receipt"))
            .and_then(|row| row.get("provider"))
            .and_then(Value::as_str),
        Some("webhook")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("receipt"))
            .and_then(|row| row.get("status_code"))
            .and_then(Value::as_u64),
        Some(202)
    );
    assert_eq!(
        publish_payload
            .get("external_action")
            .and_then(|row| row.get("capability_id"))
            .and_then(Value::as_str),
        Some("webhook.post")
    );

    let request_snapshot = requests.read().await.clone();
    assert_eq!(request_snapshot.len(), 1);
    let request = &request_snapshot[0];
    assert_tandem_webhook_signature(&request.headers, &request.body);
    assert_eq!(
        request.headers.get("x-tandem-event").map(String::as_str),
        Some("incident_monitor.incident")
    );
    let body_text = String::from_utf8(request.body.clone()).expect("webhook body utf8");
    assert!(!body_text.contains("test-webhook-signing-secret"));
    assert!(!body_text.contains("TANDEM_TEST_INCIDENT_MONITOR_WEBHOOK_SECRET"));
    let body_json: Value = serde_json::from_str(&body_text).expect("webhook json");
    assert_eq!(
        body_json
            .get("destination")
            .and_then(|row| row.get("destination_id"))
            .and_then(Value::as_str),
        Some("webhook-primary")
    );
    assert_eq!(
        body_json
            .get("draft")
            .and_then(|row| row.get("fingerprint"))
            .and_then(Value::as_str),
        Some("fingerprint-webhook-signed")
    );
    assert_eq!(
        body_json
            .get("issue_draft")
            .and_then(|row| row.get("suggested_title"))
            .and_then(Value::as_str),
        Some("Build failure in CI")
    );

    let first_post_id = publish_payload
        .get("post")
        .and_then(|row| row.get("post_id"))
        .and_then(Value::as_str)
        .expect("post id")
        .to_string();
    let (second_status, second_payload) =
        publish_incident_monitor_webhook_draft(app.clone(), &draft_id).await;
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
    assert_eq!(requests.read().await.len(), 1);

    server.abort();
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_webhook_destination_blocks_private_url_by_default() {
    let (endpoint, requests, server) = spawn_fake_incident_monitor_webhook_server(vec![202], 0).await;
    let state = test_state().await;
    configure_webhook_incident_monitor_destination(
        &state,
        endpoint,
        json!({
            "allow_insecure_http": true,
            "max_attempts": 1
        }),
    )
    .await;

    let app = app_router(state.clone());
    let draft_id =
        create_ready_linear_incident_monitor_draft(app.clone(), "fingerprint-webhook-private").await;

    let (publish_status, publish_payload) =
        publish_incident_monitor_webhook_draft(app.clone(), &draft_id).await;
    assert_eq!(publish_status, StatusCode::BAD_REQUEST);
    assert!(
        publish_payload
            .get("detail")
            .and_then(Value::as_str)
            .is_some_and(|detail| detail.contains("localhost/private network")
                || detail.contains("private or internal address")),
        "private URL should be blocked: {publish_payload:?}"
    );
    assert_eq!(requests.read().await.len(), 0);
    let posts = state.list_incident_monitor_posts(10).await;
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].status, "failed");
    assert_eq!(
        posts[0]
            .receipt
            .as_ref()
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("blocked")
    );

    server.abort();
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_webhook_destination_blocks_ipv4_mapped_private_ipv6_url() {
    let (endpoint, requests, server) = spawn_fake_incident_monitor_webhook_server(vec![202], 0).await;
    let port = reqwest::Url::parse(&endpoint)
        .expect("parse fake webhook endpoint")
        .port()
        .expect("fake webhook port");
    let mapped_endpoint = format!("http://[::ffff:127.0.0.1]:{port}/incident");
    let state = test_state().await;
    configure_webhook_incident_monitor_destination(
        &state,
        mapped_endpoint,
        json!({
            "allow_insecure_http": true,
            "max_attempts": 1
        }),
    )
    .await;

    let app = app_router(state.clone());
    let draft_id = create_ready_linear_incident_monitor_draft(
        app.clone(),
        "fingerprint-webhook-ipv4-mapped-private",
    )
    .await;

    let (publish_status, publish_payload) =
        publish_incident_monitor_webhook_draft(app.clone(), &draft_id).await;
    assert_eq!(publish_status, StatusCode::BAD_REQUEST);
    assert!(
        publish_payload
            .get("detail")
            .and_then(Value::as_str)
            .is_some_and(|detail| detail.contains("private or internal address")),
        "IPv4-mapped private IPv6 URL should be blocked: {publish_payload:?}"
    );
    assert_eq!(requests.read().await.len(), 0);

    server.abort();
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_webhook_destination_retries_retryable_failure() {
    let (endpoint, requests, server) =
        spawn_fake_incident_monitor_webhook_server(vec![500, 202], 0).await;
    let state = test_state().await;
    configure_webhook_incident_monitor_destination(
        &state,
        endpoint,
        json!({
            "allow_private_networks": true,
            "allow_insecure_http": true,
            "max_attempts": 2
        }),
    )
    .await;

    let app = app_router(state.clone());
    let draft_id =
        create_ready_linear_incident_monitor_draft(app.clone(), "fingerprint-webhook-retry").await;

    let (publish_status, publish_payload) =
        publish_incident_monitor_webhook_draft(app.clone(), &draft_id).await;
    assert_eq!(publish_status, StatusCode::OK, "{publish_payload:?}");
    assert_eq!(requests.read().await.len(), 2);
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("receipt"))
            .and_then(|row| row.get("attempt_count"))
            .and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("receipt"))
            .and_then(|row| row.get("status_code"))
            .and_then(Value::as_u64),
        Some(202)
    );

    server.abort();
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_webhook_destination_records_non_retryable_failure_receipt() {
    let (endpoint, requests, server) = spawn_fake_incident_monitor_webhook_server(vec![400], 0).await;
    let state = test_state().await;
    configure_webhook_incident_monitor_destination(
        &state,
        endpoint,
        json!({
            "allow_private_networks": true,
            "allow_insecure_http": true,
            "max_attempts": 3
        }),
    )
    .await;

    let app = app_router(state.clone());
    let draft_id =
        create_ready_linear_incident_monitor_draft(app.clone(), "fingerprint-webhook-400").await;

    let (publish_status, publish_payload) =
        publish_incident_monitor_webhook_draft(app.clone(), &draft_id).await;
    assert_eq!(publish_status, StatusCode::BAD_REQUEST);
    assert_eq!(requests.read().await.len(), 1);
    let posts = state.list_incident_monitor_posts(10).await;
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].status, "failed");
    assert_eq!(
        posts[0]
            .receipt
            .as_ref()
            .and_then(|row| row.get("status_code"))
            .and_then(Value::as_u64),
        Some(400)
    );
    assert_eq!(
        posts[0]
            .receipt
            .as_ref()
            .and_then(|row| row.get("attempt_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert!(
        publish_payload
            .get("detail")
            .and_then(Value::as_str)
            .is_some_and(|detail| detail.contains("HTTP status 400")),
        "publish should expose non-retryable status: {publish_payload:?}"
    );

    server.abort();
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_webhook_destination_records_timeout_failure_receipt() {
    let (endpoint, requests, server) = spawn_fake_incident_monitor_webhook_server(vec![202], 400).await;
    let state = test_state().await;
    configure_webhook_incident_monitor_destination(
        &state,
        endpoint,
        json!({
            "allow_private_networks": true,
            "allow_insecure_http": true,
            "max_attempts": 1,
            "timeout_ms": 250
        }),
    )
    .await;

    let app = app_router(state.clone());
    let draft_id =
        create_ready_linear_incident_monitor_draft(app.clone(), "fingerprint-webhook-timeout").await;

    let (publish_status, publish_payload) =
        publish_incident_monitor_webhook_draft(app.clone(), &draft_id).await;
    assert_eq!(publish_status, StatusCode::BAD_REQUEST);
    assert_eq!(requests.read().await.len(), 1);
    let posts = state.list_incident_monitor_posts(10).await;
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].status, "failed");
    assert_eq!(
        posts[0]
            .receipt
            .as_ref()
            .and_then(|row| row.get("attempt_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert!(
        publish_payload
            .get("detail")
            .and_then(Value::as_str)
            .is_some_and(|detail| detail.contains("timed out")),
        "publish should expose timeout: {publish_payload:?}"
    );

    server.abort();
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_failure_receipt_attributes_actual_destination() {
    // TAN-552: a failed publish routed to a non-GitHub destination must be
    // recorded against that destination, not always attributed to legacy GitHub
    // (which would also risk suppressing a later real GitHub create).
    let state = test_state().await;
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            destinations: vec![crate::IncidentMonitorDestinationConfig {
                destination_id: "webhook-primary".to_string(),
                name: "Primary webhook".to_string(),
                kind: crate::IncidentMonitorDestinationKind::Webhook,
                webhook_url: Some("https://example.com/incident-hook".to_string()),
                ..Default::default()
            }],
            default_destination_ids: vec!["webhook-primary".to_string()],
            ..Default::default()
        })
        .await
        .expect("config");

    let draft = state
        .submit_incident_monitor_draft(crate::IncidentMonitorSubmission {
            source: Some("manual".to_string()),
            title: Some("Webhook destination failure".to_string()),
            detail: Some("delivery failed".to_string()),
            risk_level: Some("medium".to_string()),
            confidence: Some("medium".to_string()),
            ..Default::default()
        })
        .await
        .expect("draft");

    let post = crate::incident_monitor::router::record_publish_failure(
        &state,
        &draft,
        None,
        "auto_post",
        None,
        "boom",
    )
    .await
    .expect("record failure");

    assert_eq!(post.destination_id.as_deref(), Some("webhook-primary"));
    assert_eq!(
        post.destination_kind,
        Some(crate::IncidentMonitorDestinationKind::Webhook)
    );
    assert!(
        post.receipt
            .as_ref()
            .and_then(|receipt| receipt.get("provider"))
            .and_then(Value::as_str)
            == Some("webhook"),
        "receipt should record the webhook provider: {:?}",
        post.receipt
    );
}
