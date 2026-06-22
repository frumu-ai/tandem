#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_issue_draft_blocks_weak_proposal_without_completed_triage() {
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

    let draft_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/issue-draft"))
        .body(Body::empty())
        .expect("issue draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("issue draft response");
    assert_eq!(draft_resp.status(), StatusCode::BAD_REQUEST);
    let issue_draft_payload: Value = serde_json::from_slice(
        &to_bytes(draft_resp.into_body(), usize::MAX)
            .await
            .expect("issue draft body"),
    )
    .expect("issue draft json");
    let proposal_gate = issue_draft_payload
        .get("proposal_quality_gate")
        .expect("proposal quality gate");
    assert_eq!(
        proposal_gate.get("status").and_then(Value::as_str),
        Some("blocked")
    );
    let missing = proposal_gate
        .get("missing")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(missing
        .iter()
        .any(|row| row.as_str() == Some("research_performed")));
    assert!(missing
        .iter()
        .any(|row| row.as_str() == Some("bounded_action")));
    assert!(missing
        .iter()
        .any(|row| row.as_str() == Some("verification_steps")));
    assert_eq!(
        issue_draft_payload
            .get("draft")
            .and_then(|row| row.get("github_status"))
            .and_then(Value::as_str),
        Some("proposal_blocked")
    );
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_issue_draft_renders_repo_template() {
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
    let draft_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/issue-draft"))
        .body(Body::empty())
        .expect("issue draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("issue draft response");
    assert_eq!(draft_resp.status(), StatusCode::OK);
    let issue_draft_payload: Value = serde_json::from_slice(
        &to_bytes(draft_resp.into_body(), usize::MAX)
            .await
            .expect("issue draft body"),
    )
    .expect("issue draft json");
    let rendered_body = issue_draft_payload
        .get("issue_draft")
        .and_then(|row| row.get("rendered_body"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert_eq!(
        issue_draft_payload
            .get("draft")
            .and_then(|row| row.get("draft_id"))
            .and_then(Value::as_str),
        Some(draft_id.as_str())
    );
    assert_eq!(
        issue_draft_payload
            .get("triage_summary")
            .and_then(|row| row.get("suggested_title"))
            .and_then(Value::as_str),
        Some("Build failure in CI")
    );
    assert_eq!(
        issue_draft_payload
            .get("triage_summary_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_triage_summary")
    );
    assert!(issue_draft_payload
        .get("triage_summary_artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .is_some_and(|path| path_has_suffix(path, "/artifacts/bug_monitor.triage_summary.json")));
    assert_eq!(
        issue_draft_payload
            .get("issue_draft_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_issue_draft")
    );
    assert!(issue_draft_payload
        .get("issue_draft_artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .is_some_and(|path| path_has_suffix(path, "/artifacts/bug_monitor.issue_draft.json")));
    assert!(rendered_body.contains("## What happened?"));
    assert!(rendered_body.contains("## What did you expect to happen?"));
    assert!(rendered_body.contains("## Steps to reproduce"));
    assert!(rendered_body.contains("## Environment"));
    assert!(rendered_body.contains("## Logs / screenshots"));
    assert!(rendered_body.contains("<!-- tandem:fingerprint:v1:"));
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_publish_and_recheck_fail_with_issue_draft_context() {
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
    assert_eq!(publish_resp.status(), StatusCode::BAD_REQUEST);
    let publish_payload: Value = serde_json::from_slice(
        &to_bytes(publish_resp.into_body(), usize::MAX)
            .await
            .expect("publish body"),
    )
    .expect("publish json");
    assert_eq!(
        publish_payload.get("code").and_then(Value::as_str),
        Some("BUG_MONITOR_DRAFT_PUBLISH_FAILED")
    );
    assert_eq!(
        publish_payload
            .get("draft")
            .and_then(|row| row.get("draft_id"))
            .and_then(Value::as_str),
        Some(draft_id.as_str())
    );
    assert!(publish_payload
        .get("issue_draft")
        .and_then(|row| row.get("rendered_body"))
        .and_then(Value::as_str)
        .is_some_and(|body| body.contains("CI build failed")));
    assert!(matches!(
        publish_payload
            .get("triage_summary")
            .and_then(|row| row.get("suggested_title"))
            .and_then(Value::as_str),
        None | Some("Build failure in CI")
    ));
    assert!(matches!(
        publish_payload
            .get("triage_summary_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        None | Some("bug_monitor_triage_summary")
    ));
    assert_eq!(
        publish_payload
            .get("issue_draft_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_issue_draft")
    );

    let recheck_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/recheck-match"))
        .body(Body::empty())
        .expect("recheck request");
    let recheck_resp = app
        .clone()
        .oneshot(recheck_req)
        .await
        .expect("recheck response");
    assert_eq!(recheck_resp.status(), StatusCode::BAD_REQUEST);
    let recheck_payload: Value = serde_json::from_slice(
        &to_bytes(recheck_resp.into_body(), usize::MAX)
            .await
            .expect("recheck body"),
    )
    .expect("recheck json");
    assert_eq!(
        recheck_payload.get("code").and_then(Value::as_str),
        Some("BUG_MONITOR_DRAFT_RECHECK_FAILED")
    );
    assert_eq!(
        recheck_payload
            .get("draft")
            .and_then(|row| row.get("draft_id"))
            .and_then(Value::as_str),
        Some(draft_id.as_str())
    );
    assert!(recheck_payload
        .get("issue_draft")
        .and_then(|row| row.get("rendered_body"))
        .and_then(Value::as_str)
        .is_some_and(|body| body.contains("CI build failed")));
    assert!(matches!(
        recheck_payload
            .get("triage_summary")
            .and_then(|row| row.get("suggested_title"))
            .and_then(Value::as_str),
        None | Some("Build failure in CI")
    ));
    assert!(matches!(
        recheck_payload
            .get("triage_summary_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        None | Some("bug_monitor_triage_summary")
    ));
    assert_eq!(
        recheck_payload
            .get("issue_draft_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_issue_draft")
    );
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_publish_and_recheck_succeed_with_triage_context() {
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
        publish_payload.get("ok").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        publish_payload.get("action").and_then(Value::as_str),
        Some("create_issue")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("operation"))
            .and_then(Value::as_str),
        Some("create_issue")
    );
    assert_eq!(
        publish_payload
            .get("external_action")
            .and_then(|row| row.get("operation"))
            .and_then(Value::as_str),
        Some("create_issue")
    );
    assert_eq!(
        publish_payload
            .get("external_action")
            .and_then(|row| row.get("source_kind"))
            .and_then(Value::as_str),
        Some("bug_monitor")
    );
    assert!(matches!(
        publish_payload
            .get("triage_summary")
            .and_then(|row| row.get("suggested_title"))
            .and_then(Value::as_str),
        None | Some("Build failure in CI")
    ));
    assert!(matches!(
        publish_payload
            .get("triage_summary_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        None | Some("bug_monitor_triage_summary")
    ));
    assert!(publish_payload
        .get("issue_draft")
        .and_then(|row| row.get("rendered_body"))
        .and_then(Value::as_str)
        .is_some_and(|body| body.contains("CI build failed")));
    assert_eq!(
        publish_payload
            .get("issue_draft_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_issue_draft")
    );
    assert!(matches!(
        publish_payload
            .get("duplicate_matches")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        None | Some(0)
    ));

    let recheck_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/recheck-match"))
        .body(Body::empty())
        .expect("recheck request");
    let recheck_resp = app
        .clone()
        .oneshot(recheck_req)
        .await
        .expect("recheck response");
    let recheck_status = recheck_resp.status();
    let recheck_body = to_bytes(recheck_resp.into_body(), usize::MAX)
        .await
        .expect("recheck body");
    if recheck_status != StatusCode::OK {
        panic!("{}", String::from_utf8_lossy(&recheck_body));
    }
    let recheck_payload: Value = serde_json::from_slice(&recheck_body).expect("recheck json");
    assert_eq!(
        recheck_payload.get("ok").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        recheck_payload.get("action").and_then(Value::as_str),
        Some("matched_open")
    );
    assert!(recheck_payload.get("post").is_some_and(Value::is_null));
    assert!(matches!(
        recheck_payload
            .get("triage_summary")
            .and_then(|row| row.get("suggested_title"))
            .and_then(Value::as_str),
        None | Some("Build failure in CI")
    ));
    assert!(matches!(
        recheck_payload
            .get("triage_summary_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        None | Some("bug_monitor_triage_summary")
    ));
    assert!(recheck_payload
        .get("issue_draft")
        .and_then(|row| row.get("rendered_body"))
        .and_then(Value::as_str)
        .is_some_and(|body| body.contains("CI build failed")));
    assert_eq!(
        recheck_payload
            .get("issue_draft_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_issue_draft")
    );
    assert!(matches!(
        recheck_payload
            .get("duplicate_matches")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        None | Some(0)
    ));

    server.abort();
}
#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_publish_comments_on_matched_open_issue_and_lists_post() {
    let (endpoint, server) = spawn_fake_bug_monitor_github_mcp_server_with_issues(vec![json!({
        "number": 42,
        "title": "Build failure in CI",
        "body": "existing issue body\n<!-- tandem:fingerprint:v1:fingerprint-match-open -->",
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
            auto_comment_on_matched_open_issues: true,
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
                    "fingerprint": "fingerprint-match-open",
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
        Some("comment_issue")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("operation"))
            .and_then(Value::as_str),
        Some("comment_issue")
    );
    assert_eq!(
        publish_payload
            .get("external_action")
            .and_then(|row| row.get("capability_id"))
            .and_then(Value::as_str),
        Some("github.comment_on_issue")
    );
    assert_eq!(
        publish_payload
            .get("post")
            .and_then(|row| row.get("issue_number"))
            .and_then(Value::as_u64),
        Some(42)
    );
    assert_eq!(
        publish_payload
            .get("draft")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("github_comment_posted")
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
    assert_eq!(
        posts_payload
            .get("posts")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("draft_id"))
            .and_then(Value::as_str),
        Some(draft_id.as_str())
    );
    assert_eq!(
        posts_payload
            .get("posts")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("operation"))
            .and_then(Value::as_str),
        Some("comment_issue")
    );
    assert_eq!(
        posts_payload
            .get("posts")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("issue_number"))
            .and_then(Value::as_u64),
        Some(42)
    );

    server.abort();
}
