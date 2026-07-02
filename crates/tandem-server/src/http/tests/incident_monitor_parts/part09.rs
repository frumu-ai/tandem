async fn configure_local_incident_monitor_destination(
    state: &AppState,
    destination_id: &str,
    kind: crate::IncidentMonitorDestinationKind,
    telemetry_path: Option<&str>,
    memory_category: Option<&str>,
    enabled: bool,
) {
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            destinations: vec![crate::IncidentMonitorDestinationConfig {
                destination_id: destination_id.to_string(),
                name: format!("{destination_id} destination"),
                kind,
                enabled,
                telemetry_path: telemetry_path.map(str::to_string),
                memory_category: memory_category.map(str::to_string),
                ..Default::default()
            }],
            default_destination_ids: vec![destination_id.to_string()],
            ..Default::default()
        })
        .await
        .expect("config");
}

async fn publish_incident_monitor_local_draft(app: axum::Router, draft_id: &str) -> (StatusCode, Value) {
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

async fn create_ready_secret_incident_monitor_draft(app: axum::Router, fingerprint: &str) -> String {
    let create_req = Request::builder()
        .method("POST")
        .uri("/incident-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "desktop_logs",
                    "title": "Memory destination failure",
                    "fingerprint": fingerprint,
                    "detail": "event: memory.destination.failure\ncomponent: incident-router\ntoken=SECRET_TOKEN_123",
                    "risk_level": "medium",
                    "confidence": "medium",
                    "excerpt": ["authorization: Bearer SECRET_TOKEN_123"]
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
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_telemetry_destination_records_filters_and_skips_duplicate() {
    let state = test_state().await;
    configure_local_incident_monitor_destination(
        &state,
        "telemetry-primary",
        crate::IncidentMonitorDestinationKind::Telemetry,
        Some("incidents"),
        None,
        true,
    )
    .await;

    let app = app_router(state.clone());
    let draft_id =
        create_ready_linear_incident_monitor_draft(app.clone(), "fingerprint-telemetry-local").await;

    let (publish_status, publish_payload) =
        publish_incident_monitor_local_draft(app.clone(), &draft_id).await;
    assert_eq!(publish_status, StatusCode::OK, "{publish_payload:?}");
    assert_eq!(
        publish_payload.get("action").and_then(Value::as_str),
        Some("record_telemetry")
    );
    let post = publish_payload.get("post").expect("post");
    assert_eq!(
        post.get("destination_id").and_then(Value::as_str),
        Some("telemetry-primary")
    );
    assert_eq!(
        post.get("destination_kind").and_then(Value::as_str),
        Some("telemetry")
    );
    assert_eq!(
        post.get("target_ref").and_then(Value::as_str),
        Some("telemetry:incidents")
    );
    assert_eq!(
        post.get("receipt")
            .and_then(|row| row.get("provider"))
            .and_then(Value::as_str),
        Some("incident_monitor_telemetry")
    );
    assert!(
        post.get("external_id")
            .and_then(Value::as_str)
            .is_some_and(|value| value.starts_with("bmtel_")),
        "expected deterministic telemetry id: {post:?}"
    );
    let first_post_id = post
        .get("post_id")
        .and_then(Value::as_str)
        .expect("post id")
        .to_string();

    // TAN-556: the telemetry record is actually written to the sink, not just
    // acknowledged by the receipt. The relative path resolves under the
    // incident-monitor data dir.
    let telemetry_sink = state
        .incident_monitor_log_evidence_dir
        .parent()
        .expect("evidence dir parent")
        .join("incidents");
    let telemetry_contents =
        std::fs::read_to_string(&telemetry_sink).expect("telemetry sink written");
    assert!(
        telemetry_contents.contains("incident_monitor_telemetry"),
        "telemetry sink should contain the record: {telemetry_contents}"
    );

    let (second_status, second_payload) =
        publish_incident_monitor_local_draft(app.clone(), &draft_id).await;
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

    let posts_req = Request::builder()
        .method("GET")
        .uri("/incident-monitor/posts?limit=10&destination_id=telemetry-primary")
        .body(Body::empty())
        .expect("posts request");
    let posts_resp = app
        .clone()
        .oneshot(posts_req)
        .await
        .expect("posts response");
    assert_eq!(posts_resp.status(), StatusCode::OK);
    let posts_payload: Value = serde_json::from_slice(
        &to_bytes(posts_resp.into_body(), usize::MAX)
            .await
            .expect("posts body"),
    )
    .expect("posts json");
    assert_eq!(posts_payload.get("count").and_then(Value::as_u64), Some(1));

    let empty_posts_req = Request::builder()
        .method("GET")
        .uri("/incident-monitor/posts?limit=10&destination_id=memory-primary")
        .body(Body::empty())
        .expect("empty posts request");
    let empty_posts_resp = app
        .oneshot(empty_posts_req)
        .await
        .expect("empty posts response");
    assert_eq!(empty_posts_resp.status(), StatusCode::OK);
    let empty_posts_payload: Value = serde_json::from_slice(
        &to_bytes(empty_posts_resp.into_body(), usize::MAX)
            .await
            .expect("empty posts body"),
    )
    .expect("empty posts json");
    assert_eq!(
        empty_posts_payload.get("count").and_then(Value::as_u64),
        Some(0)
    );
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_local_destination_idempotency_ignores_incident_entrypoint() {
    let state = test_state().await;
    configure_local_incident_monitor_destination(
        &state,
        "telemetry-primary",
        crate::IncidentMonitorDestinationKind::Telemetry,
        Some("incidents"),
        None,
        true,
    )
    .await;

    let app = app_router(state.clone());
    let draft_id =
        create_ready_linear_incident_monitor_draft(app.clone(), "fingerprint-telemetry-incident").await;
    let draft = state
        .get_incident_monitor_draft(&draft_id)
        .await
        .expect("stored draft");
    let incident_id = format!("failure-incident-{}", uuid::Uuid::new_v4().simple());
    state
        .put_incident_monitor_incident(crate::IncidentMonitorIncidentRecord {
            incident_id: incident_id.clone(),
            fingerprint: draft.fingerprint.clone(),
            event_type: "incident_monitor.failure".to_string(),
            status: "triaged".to_string(),
            repo: draft.repo.clone(),
            workspace_root: "/tmp/acme".to_string(),
            title: draft
                .title
                .clone()
                .unwrap_or_else(|| "Telemetry destination failure".to_string()),
            draft_id: Some(draft_id.clone()),
            triage_run_id: draft.triage_run_id.clone(),
            detail: Some(
                "incident-specific context should not affect local idempotency".to_string(),
            ),
            confidence: draft.confidence.clone(),
            risk_level: draft.risk_level.clone(),
            expected_destination: draft.expected_destination.clone(),
            created_at_ms: crate::now_ms(),
            updated_at_ms: crate::now_ms(),
            ..Default::default()
        })
        .await
        .expect("incident");

    let first = crate::incident_monitor::router::publish_draft(
        &state,
        crate::incident_monitor::router::IncidentMonitorPublishRequest {
            draft_id: draft_id.clone(),
            incident_id: Some(incident_id),
            mode: crate::incident_monitor_github::PublishMode::Auto,
            destination_ids: Vec::new(),
        },
    )
    .await
    .expect("incident publish");
    assert_eq!(first.action, "record_telemetry");
    let first_post_id = first.post.expect("first post").post_id;

    let second = crate::incident_monitor::router::publish_draft(
        &state,
        crate::incident_monitor::router::IncidentMonitorPublishRequest {
            draft_id,
            incident_id: None,
            mode: crate::incident_monitor_github::PublishMode::ManualPublish,
            destination_ids: Vec::new(),
        },
    )
    .await
    .expect("manual publish");
    assert_eq!(second.action, "skip_duplicate");
    assert_eq!(
        second.post.as_ref().map(|post| post.post_id.as_str()),
        Some(first_post_id.as_str())
    );
    assert_eq!(
        state
            .list_incident_monitor_posts_by_destination(10, "telemetry-primary")
            .await
            .len(),
        1
    );
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_internal_memory_destination_stores_redacted_summary_and_skips_duplicate() {
    let state = test_state().await;
    configure_local_incident_monitor_destination(
        &state,
        "memory-primary",
        crate::IncidentMonitorDestinationKind::InternalMemory,
        None,
        Some("policy_gap"),
        true,
    )
    .await;

    let app = app_router(state.clone());
    let draft_id =
        create_ready_secret_incident_monitor_draft(app.clone(), "fingerprint-memory-local").await;

    let (publish_status, publish_payload) =
        publish_incident_monitor_local_draft(app.clone(), &draft_id).await;
    assert_eq!(publish_status, StatusCode::OK, "{publish_payload:?}");
    assert_eq!(
        publish_payload.get("action").and_then(Value::as_str),
        Some("store_memory_summary")
    );
    let post = publish_payload.get("post").expect("post");
    assert_eq!(
        post.get("destination_kind").and_then(Value::as_str),
        Some("internal_memory")
    );
    assert_eq!(
        post.get("target_ref").and_then(Value::as_str),
        Some("memory:policy_gap")
    );
    assert!(
        post.get("external_id")
            .and_then(Value::as_str)
            .is_some_and(|value| value.starts_with("bmmem_")),
        "expected deterministic memory ref: {post:?}"
    );
    assert_eq!(
        post.get("receipt")
            .and_then(|row| row.get("provider"))
            .and_then(Value::as_str),
        Some("incident_monitor_internal_memory")
    );
    assert_eq!(
        post.get("receipt")
            .and_then(|row| row.get("category"))
            .and_then(Value::as_str),
        Some("policy_gap")
    );
    assert_eq!(
        post.get("receipt")
            .and_then(|row| row.get("recurrence_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    let post_text = serde_json::to_string(post).expect("post json");
    assert!(!post_text.contains("SECRET_TOKEN_123"), "{post_text}");
    assert!(
        !post_text.contains("Bearer SECRET_TOKEN_123"),
        "{post_text}"
    );
    let first_post_id = post
        .get("post_id")
        .and_then(Value::as_str)
        .expect("post id")
        .to_string();

    let (second_status, second_payload) =
        publish_incident_monitor_local_draft(app.clone(), &draft_id).await;
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

    let posts = state
        .list_incident_monitor_posts_by_destination(10, "memory-primary")
        .await;
    assert_eq!(posts.len(), 1);
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_local_destination_disabled_blocks_publish_without_post() {
    let state = test_state().await;
    configure_local_incident_monitor_destination(
        &state,
        "telemetry-disabled",
        crate::IncidentMonitorDestinationKind::Telemetry,
        Some("incidents"),
        None,
        false,
    )
    .await;
    let draft = state
        .submit_incident_monitor_draft(crate::IncidentMonitorSubmission {
            source: Some("manual".to_string()),
            title: Some("Disabled local destination".to_string()),
            fingerprint: Some("fingerprint-disabled-local".to_string()),
            detail: Some("A disabled destination must not write a post.".to_string()),
            risk_level: Some("medium".to_string()),
            confidence: Some("medium".to_string()),
            ..Default::default()
        })
        .await
        .expect("draft");

    let app = app_router(state.clone());
    let (publish_status, publish_payload) =
        publish_incident_monitor_local_draft(app, &draft.draft_id).await;
    assert_eq!(publish_status, StatusCode::BAD_REQUEST);
    assert!(
        publish_payload
            .get("detail")
            .and_then(Value::as_str)
            .is_some_and(|detail| detail.contains("disabled")),
        "publish should explain disabled destination: {publish_payload:?}"
    );
    assert!(state.list_incident_monitor_posts(10).await.is_empty());
}
