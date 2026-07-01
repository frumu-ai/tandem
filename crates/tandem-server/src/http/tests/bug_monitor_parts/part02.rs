fn age_bug_monitor_triage_run_state(
    state: &crate::app::state::AppState,
    triage_run_id: &str,
    started_at_ms: u64,
) {
    let path = super::super::context_runs::context_run_state_path(state, triage_run_id);
    let raw = std::fs::read_to_string(&path).expect("read triage run state");
    let mut run_state: Value = serde_json::from_str(&raw).expect("parse triage run state");
    run_state["created_at_ms"] = json!(started_at_ms);
    run_state["started_at_ms"] = json!(started_at_ms);
    run_state["updated_at_ms"] = json!(started_at_ms);
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&run_state).expect("serialize triage run"),
    )
    .expect("write triage run state");
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_post_idempotency_claim_allows_one_pending_create() {
    let state = test_state().await;
    let now = crate::now_ms();
    let post = crate::BugMonitorPostRecord {
        post_id: "failure-post-claim-1".to_string(),
        draft_id: "failure-draft-claim".to_string(),
        incident_id: Some("failure-incident-claim".to_string()),
        fingerprint: "fingerprint-claim".to_string(),
        repo: "acme/platform".to_string(),
        operation: "create_issue".to_string(),
        status: "pending".to_string(),
        issue_number: None,
        issue_url: None,
        comment_id: None,
        comment_url: None,
        destination_id: Some("legacy-github".to_string()),
        destination_kind: Some(crate::BugMonitorDestinationKind::GithubIssue),
        route_id: None,
        route_match_reason: Some("legacy_github".to_string()),
        external_id: None,
        external_url: None,
        external_title: None,
        target_ref: Some("acme/platform".to_string()),
        receipt: None,
        evidence_digest: Some("digest-claim".to_string()),
        confidence: None,
        risk_level: None,
        expected_destination: None,
        evidence_refs: Vec::new(),
        quality_gate: None,
        idempotency_key: "acme/platform:fingerprint-claim:create_issue:digest-claim".to_string(),
        response_excerpt: None,
        error: None,
        created_at_ms: now,
        updated_at_ms: now,
    };
    let (claimed_first, first) = state
        .try_claim_bug_monitor_post_idempotency(post.clone())
        .await
        .expect("first claim");
    assert!(claimed_first);
    assert_eq!(first.post_id, "failure-post-claim-1");

    let mut competing = post;
    competing.post_id = "failure-post-claim-2".to_string();
    let (claimed_second, existing) = state
        .try_claim_bug_monitor_post_idempotency(competing)
        .await
        .expect("second claim");
    assert!(!claimed_second);
    assert_eq!(existing.post_id, "failure-post-claim-1");
    assert_eq!(state.list_bug_monitor_posts(10).await.len(), 1);
    assert_eq!(
        state
            .list_bug_monitor_posts_by_destination(
                10,
                crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID
            )
            .await
            .len(),
        1
    );
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_route_preview_uses_legacy_default_without_posting() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state.clone());
    let preview_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/route-preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "manual",
                    "event": "manual.report",
                    "risk_level": "high",
                    "title": "Manual high risk report"
                }
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
        preview_payload
            .get("effective_destination_ids")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str),
        Some(crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID)
    );
    assert_eq!(
        preview_payload
            .get("matches")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("default_destination")
    );
    assert_eq!(
        preview_payload
            .get("approval_required")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        preview_payload.get("blocked").and_then(Value::as_bool),
        Some(true)
    );
    assert!(state.list_bug_monitor_posts(10).await.is_empty());
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_route_preview_matches_ordered_route_and_blocks_unready_destination() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            destinations: vec![crate::BugMonitorDestinationConfig {
                destination_id: "linear-prod".to_string(),
                name: "Linear production".to_string(),
                kind: crate::BugMonitorDestinationKind::LinearIssue,
                enabled: true,
                ..Default::default()
            }],
            routes: vec![crate::BugMonitorRouteConfig {
                route_id: "manual-linear".to_string(),
                name: "Manual reports to Linear".to_string(),
                priority: 10,
                destination_ids: vec!["linear-prod".to_string()],
                approval_policy: crate::BugMonitorApprovalPolicy::Always,
                match_sources: vec!["desktop_logs".to_string()],
                ..Default::default()
            }],
            default_destination_ids: vec![
                crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
            ],
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state.clone());
    let preview_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/route-preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "source": "desktop_logs",
                "event_type": "external_service_crash",
                "risk_level": "medium"
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_resp = app.oneshot(preview_req).await.expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_payload: Value = serde_json::from_slice(
        &to_bytes(preview_resp.into_body(), usize::MAX)
            .await
            .expect("preview body"),
    )
    .expect("preview json");

    assert_eq!(
        preview_payload
            .get("matches")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("route_id"))
            .and_then(Value::as_str),
        Some("manual-linear")
    );
    assert_eq!(
        preview_payload
            .get("effective_destination_ids")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str),
        Some("linear-prod")
    );
    assert_eq!(
        preview_payload
            .get("approval_required")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        preview_payload.get("blocked").and_then(Value::as_bool),
        Some(true)
    );
    assert!(preview_payload
        .get("blocked_reasons")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.as_str()
                    .is_some_and(|reason| reason.contains("not ready"))
            })
        })
        .unwrap_or(false));
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_router_blocks_manual_publish_to_mcp_destination_without_explicit_allow() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            destinations: vec![crate::BugMonitorDestinationConfig {
                destination_id: "mcp-tool-prod".to_string(),
                name: "MCP tool production".to_string(),
                kind: crate::BugMonitorDestinationKind::McpTool,
                enabled: true,
                mcp_server: Some("incident-router".to_string()),
                mcp_tool: Some("incident.create".to_string()),
                config: Some(json!({
                    "payload": {
                        "title": "$draft.title",
                        "fingerprint": "$draft.fingerprint"
                    }
                })),
                ..Default::default()
            }],
            ..Default::default()
        })
        .await
        .expect("config");
    let draft = state
        .submit_bug_monitor_draft(crate::BugMonitorSubmission {
            source: Some("manual".to_string()),
            title: Some("Blocked MCP destination publish".to_string()),
            detail: Some("A report that should not call an MCP tool by default".to_string()),
            risk_level: Some("medium".to_string()),
            confidence: Some("medium".to_string()),
            ..Default::default()
        })
        .await
        .expect("draft");

    let app = app_router(state.clone());
    let publish_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{}/publish", draft.draft_id))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "destination_ids": ["mcp-tool-prod"] }).to_string(),
        ))
        .expect("publish request");
    let publish_resp = app.oneshot(publish_req).await.expect("publish response");
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
            .is_some_and(|detail| detail.contains("MCP publish is not explicitly allowed")),
        "publish should explain missing MCP allow flag: {publish_payload:?}"
    );
    assert!(state.list_bug_monitor_posts(10).await.is_empty());
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_router_requires_route_approval_before_manual_publish() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            routes: vec![crate::BugMonitorRouteConfig {
                route_id: "manual-review-first".to_string(),
                name: "Manual review first".to_string(),
                priority: 10,
                approval_policy: crate::BugMonitorApprovalPolicy::Always,
                ..Default::default()
            }],
            ..Default::default()
        })
        .await
        .expect("config");
    let draft = state
        .submit_bug_monitor_draft(crate::BugMonitorSubmission {
            source: Some("manual".to_string()),
            title: Some("Route approval publish".to_string()),
            detail: Some("A ready draft should still pause for route approval".to_string()),
            risk_level: Some("medium".to_string()),
            confidence: Some("medium".to_string()),
            ..Default::default()
        })
        .await
        .expect("draft");
    assert_eq!(draft.status, "draft_ready");
    assert!(draft.approval_granted_at_ms.is_none());

    let app = app_router(state.clone());
    let publish_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{}/publish", draft.draft_id))
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
    let stored = state
        .get_bug_monitor_draft(&draft.draft_id)
        .await
        .expect("stored draft");
    assert_eq!(stored.status, "approval_required");
    assert_eq!(stored.github_status.as_deref(), Some("approval_required"));
    assert!(state.list_bug_monitor_posts(10).await.is_empty());
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_router_matches_project_route_from_persisted_draft_context() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            safety_defaults: crate::BugMonitorSafetyDefaults {
                block_unready_destinations: true,
                ..Default::default()
            },
            destinations: vec![crate::BugMonitorDestinationConfig {
                destination_id: "webhook-prod".to_string(),
                name: "Webhook production".to_string(),
                kind: crate::BugMonitorDestinationKind::Webhook,
                enabled: true,
                webhook_url: Some("https://example.invalid/bug-monitor".to_string()),
                ..Default::default()
            }],
            routes: vec![crate::BugMonitorRouteConfig {
                route_id: "project-webhook".to_string(),
                name: "Project incidents to webhook".to_string(),
                priority: 10,
                destination_ids: vec!["webhook-prod".to_string()],
                approval_policy: crate::BugMonitorApprovalPolicy::Never,
                match_project_ids: vec!["proj-engine".to_string()],
                ..Default::default()
            }],
            default_destination_ids: vec![
                crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
            ],
            ..Default::default()
        })
        .await
        .expect("config");
    let draft = state
        .submit_bug_monitor_draft(crate::BugMonitorSubmission {
            project_id: Some("proj-engine".to_string()),
            source: Some("desktop_logs".to_string()),
            title: Some("Project-routed publish".to_string()),
            detail: Some("A persisted draft should retain project routing context".to_string()),
            risk_level: Some("medium".to_string()),
            confidence: Some("medium".to_string()),
            ..Default::default()
        })
        .await
        .expect("draft");
    assert_eq!(draft.project_id.as_deref(), Some("proj-engine"));

    let app = app_router(state.clone());
    let publish_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{}/publish", draft.draft_id))
        .body(Body::empty())
        .expect("publish request");
    let publish_resp = app.oneshot(publish_req).await.expect("publish response");
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
            .is_some_and(|detail| {
                detail.contains("webhook-prod") && detail.contains("Webhook secret reference is missing")
            }),
        "publish should match the project route before webhook readiness fallback: {publish_payload:?}"
    );
    assert!(state.list_bug_monitor_posts(10).await.is_empty());
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_high_risk_draft_requires_approval_by_default() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            require_approval_for_new_issues: false,
            ..Default::default()
        })
        .await
        .expect("config");

    let draft = state
        .submit_bug_monitor_draft(crate::BugMonitorSubmission {
            source: Some("manual".to_string()),
            title: Some("High-risk route should pause".to_string()),
            detail: Some("A high-risk incident should require approval by default".to_string()),
            risk_level: Some("high".to_string()),
            confidence: Some("medium".to_string()),
            ..Default::default()
        })
        .await
        .expect("draft");

    assert_eq!(draft.status, "approval_required");
    assert_eq!(draft.risk_level.as_deref(), Some("high"));
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_publish_reuses_existing_post_on_duplicate_submit() {
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
                    "title": "Build failure in CI",
                    "detail": "event: orchestrator.run_failed\nprocess: tandem-engine\ncomponent: orchestrator",
                    "excerpt": ["boom", "stack trace"],
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
    let first_post_id = publish_payload
        .get("post")
        .and_then(|row| row.get("post_id"))
        .and_then(Value::as_str)
        .expect("first post id")
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
    assert_eq!(
        second_publish_payload
            .get("draft")
            .and_then(|row| row.get("github_status"))
            .and_then(Value::as_str),
        Some("duplicate_skipped")
    );

    let actions_req = Request::builder()
        .method("GET")
        .uri("/external-actions?limit=10")
        .body(Body::empty())
        .expect("actions request");
    let actions_resp = app
        .clone()
        .oneshot(actions_req)
        .await
        .expect("actions response");
    assert_eq!(actions_resp.status(), StatusCode::OK);
    let actions_payload: Value = serde_json::from_slice(
        &to_bytes(actions_resp.into_body(), usize::MAX)
            .await
            .expect("actions body"),
    )
    .expect("actions json");
    assert_eq!(
        actions_payload.get("count").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        actions_payload
            .get("actions")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("action_id"))
            .and_then(Value::as_str),
        Some(first_post_id.as_str())
    );
    assert_eq!(
        actions_payload
            .get("actions")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("capability_id"))
            .and_then(Value::as_str),
        Some("github.create_issue")
    );

    let posts_req = Request::builder()
        .method("GET")
        .uri("/bug-monitor/posts?limit=10")
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

    server.abort();
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_recovers_overdue_triage_run_after_status_refresh() {
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
            model_policy: Some(json!({
                "default_model": { "provider_id": "openai", "model_id": "gpt-4.1-mini" }
            })),
            triage_timeout_ms: Some(1),
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
                    "source": "automation_v2",
                    "title": "Paused workflow needs recovery",
                    "detail": "workflow.run.failed\nreason: automation node timed out",
                    "excerpt": ["automation node timed out"],
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
    let triage_payload: Value = serde_json::from_slice(
        &to_bytes(triage_resp.into_body(), usize::MAX)
            .await
            .expect("triage body"),
    )
    .expect("triage json");
    let triage_run_id = triage_payload
        .get("draft")
        .and_then(|row| row.get("triage_run_id"))
        .and_then(Value::as_str)
        .expect("triage run id")
        .to_string();

    age_bug_monitor_triage_run_state(&state, &triage_run_id, 1);

    let status_req = Request::builder()
        .method("GET")
        .uri("/bug-monitor/status")
        .body(Body::empty())
        .expect("status request");
    let status_resp = app
        .clone()
        .oneshot(status_req)
        .await
        .expect("status response");
    assert_eq!(status_resp.status(), StatusCode::OK);

    let draft = state
        .get_bug_monitor_draft(&draft_id)
        .await
        .expect("recovered draft");
    assert_eq!(draft.github_status.as_deref(), Some("github_issue_created"));
    assert!(draft.issue_number.is_some());

    assert_eq!(
        state.bug_monitor_posts.read().await.len(),
        1,
        "recovery should publish exactly one GitHub post"
    );

    let issues_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/bug-monitor/posts?limit=10")
                .body(Body::empty())
                .expect("posts request"),
        )
        .await
        .expect("posts response");
    assert_eq!(issues_resp.status(), StatusCode::OK);
    let posts_payload: Value = serde_json::from_slice(
        &to_bytes(issues_resp.into_body(), usize::MAX)
            .await
            .expect("posts body"),
    )
    .expect("posts json");
    assert_eq!(posts_payload.get("count").and_then(Value::as_u64), Some(1));

    let second_status_req = Request::builder()
        .method("GET")
        .uri("/bug-monitor/status")
        .body(Body::empty())
        .expect("second status request");
    let second_status_resp = app
        .clone()
        .oneshot(second_status_req)
        .await
        .expect("second status response");
    assert_eq!(second_status_resp.status(), StatusCode::OK);

    let second_posts_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/bug-monitor/posts?limit=10")
                .body(Body::empty())
                .expect("second posts request"),
        )
        .await
        .expect("second posts response");
    assert_eq!(second_posts_resp.status(), StatusCode::OK);
    let second_posts_payload: Value = serde_json::from_slice(
        &to_bytes(second_posts_resp.into_body(), usize::MAX)
            .await
            .expect("second posts body"),
    )
    .expect("second posts json");
    assert_eq!(
        second_posts_payload.get("count").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        state.bug_monitor_posts.read().await.len(),
        1,
        "repeated recovery must not create duplicate posts"
    );

    server.abort();
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_publish_skips_comment_when_matched_open_commenting_disabled() {
    let (endpoint, server) = spawn_fake_bug_monitor_github_mcp_server_with_issues(vec![json!({
        "number": 42,
        "title": "Build failure in CI",
        "body": "existing issue body\n<!-- tandem:fingerprint:v1:fingerprint-match-open-no-comment -->",
        "state": "open",
        "html_url": "https://github.com/acme/platform/issues/42"
    })])
    .await;

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
            auto_comment_on_matched_open_issues: false,
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
                    "title": "Build failure in CI",
                    "fingerprint": "fingerprint-match-open-no-comment",
                    "detail": "event: orchestrator.run_failed\nprocess: tandem-engine\ncomponent: orchestrator",
                    "excerpt": ["boom", "stack trace"],
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
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_payload: Value = serde_json::from_slice(
        &to_bytes(publish_resp.into_body(), usize::MAX)
            .await
            .expect("publish body"),
    )
    .expect("publish json");
    assert_eq!(
        publish_payload.get("action").and_then(Value::as_str),
        Some("matched_open_no_comment")
    );
    assert!(publish_payload.get("post").is_some_and(Value::is_null));
    assert_eq!(
        publish_payload
            .get("draft")
            .and_then(|row| row.get("github_status"))
            .and_then(Value::as_str),
        Some("draft_ready")
    );
    assert_eq!(
        publish_payload
            .get("draft")
            .and_then(|row| row.get("matched_issue_number"))
            .and_then(Value::as_u64),
        Some(42)
    );

    let posts_req = Request::builder()
        .method("GET")
        .uri("/bug-monitor/posts?limit=10")
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
    assert_eq!(posts_payload.get("count").and_then(Value::as_u64), Some(0));

    server.abort();
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(bug_monitor_http)]
async fn incident_monitor_issue_draft_prefers_structured_triage_summary() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
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
                    "title": "Noisy raw detail",
                    "detail": "raw detail should not win",
                    "excerpt": ["noisy line"],
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
    let seeded_draft = state
        .get_bug_monitor_draft(&draft_id)
        .await
        .expect("seeded draft");
    let mut enriched_draft = seeded_draft.clone();
    enriched_draft.issue_number = Some(1234);
    enriched_draft.matched_issue_number = Some(1234);
    state
        .put_bug_monitor_draft(enriched_draft.clone())
        .await
        .expect("update draft");
    state
        .put_bug_monitor_incident(crate::BugMonitorIncidentRecord {
            incident_id: "incident-structured-triage".to_string(),
            fingerprint: enriched_draft.fingerprint.clone(),
            event_type: "orchestrator.run_failed".to_string(),
            status: "queued".to_string(),
            repo: enriched_draft.repo.clone(),
            workspace_root: "/tmp/acme".to_string(),
            title: "Structured triage title".to_string(),
            detail: Some("structured incident detail".to_string()),
            excerpt: vec!["structured log line".to_string()],
            source: Some("desktop_logs".to_string()),
            run_id: None,
            session_id: None,
            correlation_id: None,
            component: Some("orchestrator".to_string()),
            level: Some("error".to_string()),
            occurrence_count: 3,
            created_at_ms: crate::now_ms(),
            updated_at_ms: crate::now_ms(),
            last_seen_at_ms: Some(crate::now_ms()),
            draft_id: Some(draft_id.clone()),
            triage_run_id: enriched_draft.triage_run_id.clone(),
            last_error: None,
            confidence: enriched_draft.confidence.clone(),
            risk_level: enriched_draft.risk_level.clone(),
            expected_destination: enriched_draft.expected_destination.clone(),
            evidence_refs: enriched_draft.evidence_refs.clone(),
            quality_gate: enriched_draft.quality_gate.clone(),
            duplicate_summary: None,
            duplicate_matches: None,
            event_payload: None,
            ..crate::BugMonitorIncidentRecord::default()
        })
        .await
        .expect("seed incident");

    let summary_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/triage-summary"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "suggested_title": "Structured triage title",
                "what_happened": "Structured triage summary",
                "expected_behavior": "The run should complete successfully.",
                "steps_to_reproduce": [
                    "Open the repo",
                    "Run the failing workflow",
                    "Observe the orchestration error"
                ],
                "environment": [
                    "Repo: acme/platform",
                    "Process: tandem-engine"
                ],
                "logs": [
                    "structured log line",
                    "second structured line"
                ],
                "why_it_likely_happened": "A GitHub label preflight is missing.",
                "root_cause_confidence": "medium",
                "failure_type": "tool_error",
                "affected_components": ["github publisher"],
                "likely_files_to_edit": ["crates/tandem-server/src/workflows.rs"],
                "recommended_fix": "Check or create labels before creating issues.",
                "acceptance_criteria": ["Missing labels are detected before publish"],
                "verification_steps": ["Run the GitHub publish smoke test"],
                "coder_ready": true,
                "risk_level": "medium"
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    let issue_draft = summary_payload.get("issue_draft").expect("issue draft");
    assert_eq!(
        summary_payload
            .get("triage_summary_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_triage_summary")
    );
    assert!(summary_payload
        .get("triage_summary_artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .is_some_and(|path| path_has_suffix(path, "/artifacts/bug_monitor.triage_summary.json")));
    assert_eq!(
        summary_payload
            .get("issue_draft_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("incident_monitor_issue_draft")
    );
    assert!(summary_payload
        .get("issue_draft_artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .is_some_and(|path| path_has_suffix(path, "/artifacts/bug_monitor.issue_draft.json")));
    let rendered_body = issue_draft
        .get("rendered_body")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(rendered_body.contains("Structured triage summary"));
    assert!(rendered_body.contains("The run should complete successfully."));
    assert!(rendered_body.contains("1. Open the repo"));
    assert!(rendered_body.contains("structured log line"));
    assert!(rendered_body.contains("A GitHub label preflight is missing."));
    assert!(rendered_body.contains("Check or create labels before creating issues."));
    assert!(rendered_body.contains("crates/tandem-server/src/workflows.rs"));
    assert!(rendered_body.contains("Missing labels are detected before publish"));
    assert!(rendered_body.contains("tandem_autonomous_coder_issue"));
    assert!(!rendered_body.contains("raw detail should not win"));
    assert_eq!(
        issue_draft.get("coder_ready").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        issue_draft
            .get("coder_ready_gate")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("passed")
    );
    assert_eq!(
        issue_draft.get("suggested_title").and_then(Value::as_str),
        Some("Structured triage title")
    );

    let failure_pattern_payload = summary_payload
        .get("failure_pattern_memory")
        .cloned()
        .expect("failure pattern memory");
    assert_eq!(
        failure_pattern_payload
            .get("stored")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        failure_pattern_payload
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("failure_pattern")
    );
    assert_eq!(
        failure_pattern_payload
            .get("metadata")
            .and_then(|row| row.get("repo_slug"))
            .and_then(Value::as_str),
        Some("acme/platform")
    );
    assert_eq!(
        failure_pattern_payload
            .get("metadata")
            .and_then(|row| row.get("recurrence_count"))
            .and_then(Value::as_u64),
        Some(3)
    );
    let regression_signal_payload = summary_payload
        .get("regression_signal_memory")
        .cloned()
        .expect("regression signal memory");
    assert_eq!(
        regression_signal_payload
            .get("stored")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        regression_signal_payload
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("regression_signal")
    );
    assert_eq!(
        regression_signal_payload
            .get("metadata")
            .and_then(|row| row.get("repo_slug"))
            .and_then(Value::as_str),
        Some("acme/platform")
    );
    assert_eq!(
        regression_signal_payload
            .get("metadata")
            .and_then(|row| row.get("expected_behavior"))
            .and_then(Value::as_str),
        Some("The run should complete successfully.")
    );
    let triage_run_id = summary_payload
        .get("draft")
        .and_then(|row| row.get("triage_run_id"))
        .and_then(Value::as_str)
        .expect("triage run id");
    let regression_signal_artifact = load_context_blackboard(&state, triage_run_id)
        .artifacts
        .into_iter()
        .find(|artifact| artifact.artifact_type == "bug_monitor_regression_signal_memory")
        .expect("regression signal artifact");
    assert!(path_has_suffix(
        &regression_signal_artifact.path,
        "/artifacts/bug_monitor.regression_signal_memory.json"
    ));

    let duplicate_report_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "desktop_logs",
                    "title": "Structured triage summary",
                    "detail": "Structured triage summary",
                    "excerpt": ["structured log line"],
                }
            })
            .to_string(),
        ))
        .expect("duplicate report request");
    let duplicate_report_resp = app
        .clone()
        .oneshot(duplicate_report_req)
        .await
        .expect("duplicate report response");
    assert_eq!(duplicate_report_resp.status(), StatusCode::OK);
    let duplicate_report_payload: Value = serde_json::from_slice(
        &to_bytes(duplicate_report_resp.into_body(), usize::MAX)
            .await
            .expect("duplicate report body"),
    )
    .expect("duplicate report json");
    assert_eq!(
        duplicate_report_payload
            .get("suppressed")
            .and_then(Value::as_bool),
        Some(true)
    );
}
#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(bug_monitor_http)]
async fn incident_monitor_issue_draft_blocks_coder_ready_without_required_evidence() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
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
                    "title": "Ambiguous workflow failure",
                    "detail": "event: workflow.run.failed",
                    "excerpt": ["workflow failed without enough triage evidence"]
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

    let summary_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/triage-summary"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "suggested_title": "Ambiguous workflow failure",
                "what_happened": "The workflow failed, but triage has not identified an edit scope.",
                "why_it_likely_happened": "Repo research found the failure is real, but not which component owns it.",
                "root_cause_confidence": "medium",
                "failure_type": "unknown",
                "recommended_fix": "Investigate further before editing code.",
                "steps_to_reproduce": ["Run the affected workflow"],
                "logs": ["workflow failed without enough triage evidence"],
                "research_sources": [{"kind":"repo","summary":"Inspected workflow failure reports"}],
                "acceptance_criteria": ["A scoped owner is identified before coding"],
                "verification_steps": ["Run the workflow failure smoke test"],
                "coder_ready": true,
                "risk_level": "medium"
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    assert_eq!(
        summary_payload
            .get("triage_summary")
            .and_then(|row| row.get("coder_ready"))
            .and_then(Value::as_bool),
        Some(false)
    );
    let issue_draft = summary_payload.get("issue_draft").expect("issue draft");
    assert_eq!(
        issue_draft.get("coder_ready").and_then(Value::as_bool),
        Some(false)
    );
    let missing = issue_draft
        .get("coder_ready_gate")
        .and_then(|row| row.get("missing"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(missing
        .iter()
        .any(|row| row.as_str() == Some("scope_identified")));
    assert!(!missing
        .iter()
        .any(|row| row.as_str() == Some("acceptance_criteria")));
    assert!(!missing
        .iter()
        .any(|row| row.as_str() == Some("verification_steps")));
    let rendered_body = issue_draft
        .get("rendered_body")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(!rendered_body.contains("tandem_autonomous_coder_issue"));
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(bug_monitor_http)]
async fn incident_monitor_issue_draft_blocks_coder_ready_when_tool_scope_missing() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
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
                    "title": "Workflow failure needs repo edit",
                    "detail": "event: workflow.run.failed",
                    "excerpt": ["workflow failed with a clear code fix"]
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

    let summary_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/triage-summary"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "suggested_title": "Workflow failure needs repo edit",
                "what_happened": "A workflow failure has a scoped code fix.",
                "why_it_likely_happened": "Repo research points to workflow failure handling.",
                "root_cause_confidence": "medium",
                "failure_type": "code_defect",
                "affected_components": ["workflow executor"],
                "likely_files_to_edit": ["crates/tandem-server/src/workflows.rs"],
                "recommended_fix": "Patch the workflow failure handling.",
                "steps_to_reproduce": ["Run the affected workflow"],
                "logs": ["workflow failed with a clear code fix"],
                "research_sources": [{"kind":"repo","summary":"Inspected workflow executor code"}],
                "acceptance_criteria": ["Terminal failure is reported once"],
                "verification_steps": ["Run bug_monitor_ tests"],
                "coder_ready": true,
                "risk_level": "medium",
                "required_tool_scopes": ["repo:write"],
                "missing_tool_scopes": ["repo:write"],
                "permissions_available": false
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    let issue_draft = summary_payload.get("issue_draft").expect("issue draft");
    assert_eq!(
        issue_draft.get("coder_ready").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        issue_draft
            .get("permissions_available")
            .and_then(Value::as_bool),
        Some(false)
    );
    let missing = issue_draft
        .get("coder_ready_gate")
        .and_then(|row| row.get("missing"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(missing
        .iter()
        .any(|row| row.as_str() == Some("tool_scope_available")));
    let rendered_body = issue_draft
        .get("rendered_body")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(!rendered_body.contains("tandem_autonomous_coder_issue"));
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_triage_run_created_from_approved_draft() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
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
                    "title": "Build failure in CI",
                    "excerpt": ["boom"],
                }
            })
            .to_string(),
        ))
        .expect("request");
    let create_resp = app.clone().oneshot(create_req).await.expect("response");
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
    let draft_status = create_payload
        .get("draft")
        .and_then(|row| row.get("status"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if draft_status.eq_ignore_ascii_case("approval_required") {
        let approve_req = Request::builder()
            .method("POST")
            .uri(format!("/bug-monitor/drafts/{draft_id}/approve"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .expect("approve request");
        let approve_resp = app
            .clone()
            .oneshot(approve_req)
            .await
            .expect("approve response");
        assert_eq!(approve_resp.status(), StatusCode::OK);
    }

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
    let triage_payload: Value = serde_json::from_slice(
        &to_bytes(triage_resp.into_body(), usize::MAX)
            .await
            .expect("triage body"),
    )
    .expect("triage json");
    let run_id = triage_payload
        .get("run")
        .and_then(|row| row.get("run_id"))
        .and_then(Value::as_str)
        .expect("run id")
        .to_string();
    let automation_id = triage_payload
        .get("run")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation id")
        .to_string();
    let automation = state
        .get_automation_v2(&automation_id)
        .await
        .expect("stored triage automation");
    let bug_monitor_nodes = automation
        .flow
        .nodes
        .iter()
        .filter(|node| {
            node.metadata
                .as_ref()
                .and_then(|metadata| metadata.get("bug_monitor"))
                .and_then(|metadata| metadata.get("artifact_type"))
                .and_then(Value::as_str)
                .is_some()
        })
        .collect::<Vec<_>>();
    assert_eq!(bug_monitor_nodes.len(), 4);
    for node in bug_monitor_nodes {
        assert!(node
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("builder"))
            .and_then(|builder| builder.get("output_path"))
            .is_none());
        assert!(node
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("bug_monitor"))
            .and_then(|bug_monitor| bug_monitor.get("context_artifact_path"))
            .and_then(Value::as_str)
            .is_some_and(|path| path.starts_with("artifacts/bug_monitor.")));
        assert_eq!(
            crate::app::state::automation_node_required_output_path(node),
            None
        );
    }
    assert_eq!(
        triage_payload
            .get("draft")
            .and_then(|row| row.get("triage_run_id"))
            .and_then(Value::as_str),
        Some(run_id.as_str())
    );
    assert_eq!(
        triage_payload
            .get("draft")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("triage_queued")
    );
    assert!(triage_payload
        .get("triage_summary")
        .is_some_and(Value::is_null));
    assert!(triage_payload
        .get("triage_summary_artifact")
        .is_some_and(Value::is_null));
    assert!(triage_payload
        .get("issue_draft")
        .is_some_and(Value::is_null));
    assert!(triage_payload
        .get("issue_draft_artifact")
        .is_some_and(Value::is_null));

    let get_run_req = Request::builder()
        .method("GET")
        .uri(format!("/context/runs/{run_id}"))
        .body(Body::empty())
        .expect("get run request");
    let get_run_resp = app
        .clone()
        .oneshot(get_run_req)
        .await
        .expect("get run response");
    assert_eq!(get_run_resp.status(), StatusCode::OK);
    let get_run_payload: Value = serde_json::from_slice(
        &to_bytes(get_run_resp.into_body(), usize::MAX)
            .await
            .expect("get run body"),
    )
    .expect("get run json");
    assert_eq!(
        get_run_payload
            .get("run")
            .and_then(|row| row.get("run_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_triage")
    );
    assert_eq!(
        get_run_payload
            .get("run")
            .and_then(|row| row.get("tasks"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(4)
    );
    let tasks = get_run_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .expect("triage tasks");
    let task_by_kind = |kind: &str| {
        tasks.iter().find(|task| {
            task.get("payload")
                .and_then(|payload| payload.get("task_kind"))
                .and_then(Value::as_str)
                == Some(kind)
        })
    };
    let inspect = task_by_kind("inspection").expect("inspection task");
    let research = task_by_kind("research").expect("research task");
    let validate = task_by_kind("validation").expect("validation task");
    let fix = task_by_kind("fix_proposal").expect("fix proposal task");
    let task_id = |task: &Value| {
        task.get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    assert_eq!(
        research
            .get("depends_on_task_ids")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str)
            .map(ToString::to_string),
        Some(task_id(inspect))
    );
    assert_eq!(
        validate
            .get("depends_on_task_ids")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str)
            .map(ToString::to_string),
        Some(task_id(research))
    );
    assert_eq!(
        fix.get("depends_on_task_ids")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str)
            .map(ToString::to_string),
        Some(task_id(validate))
    );
    assert_eq!(
        research
            .get("payload")
            .and_then(|payload| payload.get("expected_artifact"))
            .and_then(Value::as_str),
        Some("bug_monitor_research_report")
    );
    let duplicate_artifact_present = get_run_payload
        .get("run")
        .and_then(|row| row.get("artifacts"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("artifact_type").and_then(Value::as_str)
                    == Some("failure_duplicate_matches")
            })
        })
        .unwrap_or(false);
    assert!(!duplicate_artifact_present);

    let second_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/triage-run"))
        .body(Body::empty())
        .expect("second triage request");
    let second_resp = app
        .clone()
        .oneshot(second_req)
        .await
        .expect("second triage response");
    assert_eq!(second_resp.status(), StatusCode::OK);
    let second_payload: Value = serde_json::from_slice(
        &to_bytes(second_resp.into_body(), usize::MAX)
            .await
            .expect("second triage body"),
    )
    .expect("second triage json");
    assert_eq!(
        second_payload
            .get("run")
            .and_then(|row| row.get("run_id"))
            .and_then(Value::as_str),
        Some(run_id.as_str())
    );
    assert_eq!(
        second_payload.get("deduped").and_then(Value::as_bool),
        Some(true)
    );
    assert!(second_payload
        .get("issue_draft_artifact")
        .is_some_and(Value::is_null));
    assert!(
        second_payload.get("duplicate_matches_artifact").is_none()
            || second_payload
                .get("duplicate_matches_artifact")
                .is_some_and(Value::is_null)
    );

    state
        .put_bug_monitor_incident(crate::BugMonitorIncidentRecord {
            incident_id: "incident-replay-approved-draft".to_string(),
            fingerprint: triage_payload
                .get("draft")
                .and_then(|row| row.get("fingerprint"))
                .and_then(Value::as_str)
                .expect("draft fingerprint")
                .to_string(),
            event_type: "orchestrator.run_failed".to_string(),
            status: "queued".to_string(),
            repo: "acme/platform".to_string(),
            workspace_root: "/tmp/acme".to_string(),
            title: "Build failure in CI".to_string(),
            detail: Some("boom".to_string()),
            excerpt: vec!["boom".to_string()],
            source: Some("desktop_logs".to_string()),
            run_id: None,
            session_id: None,
            correlation_id: None,
            component: Some("orchestrator".to_string()),
            level: Some("error".to_string()),
            occurrence_count: 1,
            created_at_ms: crate::now_ms(),
            updated_at_ms: crate::now_ms(),
            last_seen_at_ms: Some(crate::now_ms()),
            draft_id: Some(draft_id.clone()),
            triage_run_id: Some(run_id.clone()),
            last_error: None,
            confidence: None,
            risk_level: None,
            expected_destination: None,
            evidence_refs: Vec::new(),
            quality_gate: None,
            duplicate_summary: None,
            duplicate_matches: None,
            event_payload: None,
            ..crate::BugMonitorIncidentRecord::default()
        })
        .await
        .expect("seed replay incident");

    let replay_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/incidents/incident-replay-approved-draft/replay")
        .body(Body::empty())
        .expect("replay request");
    let replay_resp = app
        .clone()
        .oneshot(replay_req)
        .await
        .expect("replay response");
    assert_eq!(replay_resp.status(), StatusCode::OK);
    let replay_payload: Value = serde_json::from_slice(
        &to_bytes(replay_resp.into_body(), usize::MAX)
            .await
            .expect("replay body"),
    )
    .expect("replay json");
    assert_eq!(
        replay_payload
            .get("run")
            .and_then(|row| row.get("run_id"))
            .and_then(Value::as_str),
        Some(run_id.as_str())
    );
    assert_eq!(
        replay_payload.get("deduped").and_then(Value::as_bool),
        Some(true)
    );
    assert!(replay_payload
        .get("issue_draft")
        .is_some_and(Value::is_null));
    assert!(replay_payload
        .get("issue_draft_artifact")
        .is_some_and(Value::is_null));
    assert!(replay_payload
        .get("triage_summary")
        .is_some_and(Value::is_null));
    assert!(replay_payload
        .get("triage_summary_artifact")
        .is_some_and(Value::is_null));
    assert_eq!(
        replay_payload
            .get("duplicate_summary")
            .and_then(|row| row.get("match_count"))
            .and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        replay_payload
            .get("duplicate_matches")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
}
