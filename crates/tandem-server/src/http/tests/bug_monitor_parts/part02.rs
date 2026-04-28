#[tokio::test]
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
async fn bug_monitor_issue_draft_prefers_structured_triage_summary() {
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
        .is_some_and(|path| path.ends_with("/artifacts/bug_monitor.triage_summary.json")));
    assert_eq!(
        summary_payload
            .get("issue_draft_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_issue_draft")
    );
    assert!(summary_payload
        .get("issue_draft_artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .is_some_and(|path| path.ends_with("/artifacts/bug_monitor.issue_draft.json")));
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
    assert!(regression_signal_artifact
        .path
        .ends_with("/artifacts/bug_monitor.regression_signal_memory.json"));

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
async fn bug_monitor_issue_draft_blocks_coder_ready_without_required_evidence() {
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
async fn bug_monitor_issue_draft_blocks_coder_ready_when_tool_scope_missing() {
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
    assert_eq!(
        triage_payload
            .get("triage_summary")
            .and_then(|row| row.get("suggested_title"))
            .and_then(Value::as_str),
        Some("Bug Monitor issue")
    );
    assert_eq!(
        triage_payload
            .get("triage_summary_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_triage_summary")
    );
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
    assert_eq!(
        replay_payload
            .get("triage_summary")
            .and_then(|row| row.get("suggested_title"))
            .and_then(Value::as_str),
        Some("Bug Monitor issue")
    );
    assert_eq!(
        replay_payload
            .get("triage_summary_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_triage_summary")
    );
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

#[tokio::test]
async fn bug_monitor_triage_run_writes_duplicate_match_artifact() {
    let state = test_state().await;
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
                    "fingerprint": "manual-artifact-fingerprint-source",
                    "excerpt": ["Build failure in CI"],
                }
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let draft_fingerprint = create_payload
        .get("draft")
        .and_then(|row| row.get("fingerprint"))
        .and_then(Value::as_str)
        .expect("draft fingerprint")
        .to_string();
    let draft_id = create_payload
        .get("draft")
        .and_then(|row| row.get("draft_id"))
        .and_then(Value::as_str)
        .expect("draft id")
        .to_string();
    let seed_run_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-failure-pattern-artifact",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "acme/platform"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 302
                }
            })
            .to_string(),
        ))
        .expect("seed request");
    let seed_run_resp = app
        .clone()
        .oneshot(seed_run_req)
        .await
        .expect("seed response");
    assert_eq!(seed_run_resp.status(), StatusCode::OK);

    let candidate_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-failure-pattern-artifact/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "failure_pattern",
                "summary": "Repeated orchestrator failure",
                "payload": {
                    "type": "failure.pattern",
                    "repo_slug": "acme/platform",
                    "fingerprint": draft_fingerprint,
                    "symptoms": ["Build failure in CI"],
                    "canonical_markers": ["Build failure in CI"],
                    "linked_issue_numbers": [302],
                    "linked_pr_numbers": [],
                    "affected_components": ["ci"],
                    "artifact_refs": ["artifact://ctx/manual/triage.summary.json"]
                }
            })
            .to_string(),
        ))
        .expect("candidate request");
    let candidate_resp = app
        .clone()
        .oneshot(candidate_req)
        .await
        .expect("candidate response");
    assert_eq!(candidate_resp.status(), StatusCode::OK);
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
        .expect("run id");
    assert_eq!(
        triage_payload
            .get("duplicate_matches_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("failure_duplicate_matches")
    );
    assert_eq!(
        triage_payload
            .get("duplicate_summary")
            .and_then(|row| row.get("match_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        triage_payload
            .get("duplicate_matches")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert!(triage_payload
        .get("duplicate_matches_artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .is_some_and(|path| path.ends_with("/artifacts/failure_duplicate_matches.json")));
    write_ready_bug_monitor_triage_summary(app.clone(), &draft_id).await;

    let issue_draft_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/issue-draft"))
        .body(Body::empty())
        .expect("issue draft request");
    let issue_draft_resp = app
        .clone()
        .oneshot(issue_draft_req)
        .await
        .expect("issue draft response");
    assert_eq!(issue_draft_resp.status(), StatusCode::OK);
    let issue_draft_payload: Value = serde_json::from_slice(
        &to_bytes(issue_draft_resp.into_body(), usize::MAX)
            .await
            .expect("issue draft body"),
    )
    .expect("issue draft json");
    assert_eq!(
        issue_draft_payload
            .get("duplicate_matches_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("failure_duplicate_matches")
    );
    assert_eq!(
        issue_draft_payload
            .get("duplicate_summary")
            .and_then(|row| row.get("match_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        issue_draft_payload
            .get("duplicate_matches")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert!(issue_draft_payload
        .get("duplicate_matches_artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .is_some_and(|path| path.ends_with("/artifacts/failure_duplicate_matches.json")));

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
        publish_payload
            .get("duplicate_matches_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("failure_duplicate_matches")
    );
    assert_eq!(
        publish_payload
            .get("duplicate_summary")
            .and_then(|row| row.get("match_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        publish_payload
            .get("duplicate_matches")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert!(publish_payload
        .get("duplicate_matches_artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .is_some_and(|path| path.ends_with("/artifacts/failure_duplicate_matches.json")));

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
        recheck_payload
            .get("duplicate_matches_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("failure_duplicate_matches")
    );
    assert_eq!(
        recheck_payload
            .get("duplicate_summary")
            .and_then(|row| row.get("match_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        recheck_payload
            .get("duplicate_matches")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert!(recheck_payload
        .get("issue_draft")
        .and_then(|row| row.get("rendered_body"))
        .and_then(Value::as_str)
        .is_some_and(|body| body.contains("Repeated orchestrator failure")));
    assert!(recheck_payload
        .get("duplicate_matches_artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .is_some_and(|path| path.ends_with("/artifacts/failure_duplicate_matches.json")));

    let get_blackboard_req = Request::builder()
        .method("GET")
        .uri(format!("/context/runs/{run_id}/blackboard"))
        .body(Body::empty())
        .expect("get blackboard request");
    let get_blackboard_resp = app
        .clone()
        .oneshot(get_blackboard_req)
        .await
        .expect("get blackboard response");
    assert_eq!(get_blackboard_resp.status(), StatusCode::OK);
    let get_blackboard_payload: Value = serde_json::from_slice(
        &to_bytes(get_blackboard_resp.into_body(), usize::MAX)
            .await
            .expect("get blackboard body"),
    )
    .expect("get blackboard json");
    let duplicate_artifact_present = get_blackboard_payload
        .get("blackboard")
        .and_then(|row| row.get("artifacts"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("artifact_type").and_then(Value::as_str)
                    == Some("failure_duplicate_matches")
            })
        })
        .unwrap_or(false);
    assert!(duplicate_artifact_present);
}
