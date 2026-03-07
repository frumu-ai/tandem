use super::*;

#[tokio::test]
async fn bug_monitor_runtime_creates_incident_and_draft_from_failure_event() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            require_approval_for_new_issues: true,
            ..Default::default()
        })
        .await
        .expect("config");

    let task = tokio::spawn(crate::run_bug_monitor(state.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    state.event_bus.publish(EngineEvent::new(
        "session.error",
        json!({
            "sessionID": "session-123",
            "runID": "run-123",
            "reason": "Prompt retry failed",
            "component": "swarm-orchestrator",
        }),
    ));

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let incidents = state.list_bug_monitor_incidents(10).await;
            let drafts = state.list_bug_monitor_drafts(10).await;
            if incidents
                .iter()
                .any(|row| row.draft_id.is_some() || row.last_error.is_some())
                || !drafts.is_empty()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("incident timeout");

    let incidents = state.list_bug_monitor_incidents(10).await;
    assert_eq!(incidents.len(), 1);
    let incident = &incidents[0];
    assert_eq!(incident.event_type, "session.error");
    assert_eq!(incident.repo, "acme/platform");
    assert_eq!(incident.workspace_root, "/tmp/acme");
    assert!(incident.draft_id.is_some());

    let drafts = state.list_bug_monitor_drafts(10).await;
    assert_eq!(drafts.len(), 1);
    assert_eq!(
        drafts[0].draft_id,
        incident.draft_id.clone().unwrap_or_default()
    );

    task.abort();
}

#[tokio::test]
async fn paused_bug_monitor_runtime_ignores_failure_events() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            paused: true,
            repo: Some("acme/platform".to_string()),
            ..Default::default()
        })
        .await
        .expect("config");

    let task = tokio::spawn(crate::run_bug_monitor(state.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    state.event_bus.publish(EngineEvent::new(
        "session.error",
        json!({
            "sessionID": "session-456",
            "reason": "Paused reporter should ignore this",
        }),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    assert!(state.list_bug_monitor_incidents(10).await.is_empty());
    assert!(state.list_bug_monitor_drafts(10).await.is_empty());
    task.abort();
}

#[tokio::test]
async fn bug_monitor_runtime_detects_real_context_task_failures() {
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

    let task = tokio::spawn(crate::run_bug_monitor(state.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    state.event_bus.publish(EngineEvent::new(
        "context.task.failed",
        json!({
            "runID": "ctx-run-fr-real-failure",
            "taskID": "task-3",
            "title": "Add levels, combat, UI, and audiovisual polish",
            "error": "PROMPT_RETRY_FAILED",
            "component": "swarm-agent-2",
            "task": {
                "id": "task-3",
                "payload": {
                    "title": "Add levels, combat, UI, and audiovisual polish"
                }
            }
        }),
    ));

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let incidents = state.list_bug_monitor_incidents(10).await;
            let drafts = state.list_bug_monitor_drafts(10).await;
            if incidents
                .iter()
                .any(|row| row.draft_id.is_some() || row.last_error.is_some())
                || !drafts.is_empty()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("incident timeout");

    let incidents = state.list_bug_monitor_incidents(10).await;
    assert_eq!(incidents.len(), 1);
    let incident = &incidents[0];
    assert_eq!(incident.event_type, "context.task.failed");
    assert!(
        incident.title.contains("context.task.failed")
            || incident
                .title
                .contains("Add levels, combat, UI, and audiovisual polish"),
        "unexpected incident title: {}",
        incident.title
    );
    assert!(
        incident.draft_id.is_some() || incident.last_error.is_some(),
        "expected either a draft or a recorded reporter error"
    );

    task.abort();
}

#[tokio::test]
async fn bug_monitor_report_creates_and_dedupes_draft() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            require_approval_for_new_issues: true,
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state.clone());
    let body = json!({
        "report": {
            "source": "desktop_logs",
            "event": "orchestrator.run_failed",
            "run_id": "run-123",
            "excerpt": ["boom", "stack trace"],
        }
    });
    let req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let payload: Value =
        serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.expect("body"))
            .expect("json");
    let draft = payload.get("draft").expect("draft");
    assert_eq!(
        draft.get("repo").and_then(Value::as_str),
        Some("acme/platform")
    );
    assert_eq!(
        draft.get("status").and_then(Value::as_str),
        Some("approval_required")
    );
    assert!(draft
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .contains("orchestrator.run_failed"));

    let req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let second_payload: Value =
        serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.expect("body"))
            .expect("json");
    assert_eq!(
        payload
            .get("draft")
            .and_then(|row| row.get("draft_id"))
            .and_then(Value::as_str),
        second_payload
            .get("draft")
            .and_then(|row| row.get("draft_id"))
            .and_then(Value::as_str)
    );

    let drafts = state.list_bug_monitor_drafts(10).await;
    assert_eq!(drafts.len(), 1);
}

#[tokio::test]
async fn bug_monitor_report_surfaces_duplicate_failure_patterns() {
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
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state.clone());
    let seed_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-failure-pattern-seed",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "acme/platform"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 301
                }
            })
            .to_string(),
        ))
        .expect("seed request");
    let seed_resp = app.clone().oneshot(seed_req).await.expect("seed response");
    assert_eq!(seed_resp.status(), StatusCode::OK);

    let candidate_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-failure-pattern-seed/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "failure_pattern",
                "summary": "Repeated orchestrator failure",
                "payload": {
                    "type": "failure.pattern",
                    "repo_slug": "acme/platform",
                    "fingerprint": "manual-failure-pattern",
                    "symptoms": ["orchestrator.run_failed"],
                    "canonical_markers": ["orchestrator.run_failed", "stack trace"],
                    "linked_issue_numbers": [301],
                    "linked_pr_numbers": [],
                    "affected_components": ["orchestrator"],
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

    let report_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "desktop_logs",
                    "event": "orchestrator.run_failed",
                    "fingerprint": "manual-failure-pattern",
                    "excerpt": ["stack trace"],
                }
            })
            .to_string(),
        ))
        .expect("report request");
    let report_resp = app
        .clone()
        .oneshot(report_req)
        .await
        .expect("report response");
    assert_eq!(report_resp.status(), StatusCode::OK);
    let report_payload: Value = serde_json::from_slice(
        &to_bytes(report_resp.into_body(), usize::MAX)
            .await
            .expect("report body"),
    )
    .expect("report json");
    let duplicate_matches = report_payload
        .get("duplicate_matches")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        report_payload.get("suppressed").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(duplicate_matches.len(), 1);
    assert_eq!(
        duplicate_matches[0]
            .get("fingerprint")
            .and_then(Value::as_str),
        Some("manual-failure-pattern")
    );
    assert!(state.list_bug_monitor_drafts(10).await.is_empty());
}

#[tokio::test]
async fn bug_monitor_runtime_suppresses_duplicate_failure_patterns() {
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
    let seed_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-runtime-duplicate-seed",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "acme/platform"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 401
                }
            })
            .to_string(),
        ))
        .expect("seed request");
    let seed_resp = app.clone().oneshot(seed_req).await.expect("seed response");
    assert_eq!(seed_resp.status(), StatusCode::OK);

    let candidate_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-runtime-duplicate-seed/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "failure_pattern",
                "summary": "Repeated orchestrator failure",
                "payload": {
                    "type": "failure.pattern",
                    "repo_slug": "acme/platform",
                    "fingerprint": "runtime-duplicate-fingerprint",
                    "symptoms": ["session.error"],
                    "canonical_markers": ["Prompt retry failed", "swarm-orchestrator"],
                    "linked_issue_numbers": [401],
                    "linked_pr_numbers": [],
                    "affected_components": ["orchestrator"],
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

    let task = tokio::spawn(crate::run_bug_monitor(state.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    state.event_bus.publish(EngineEvent::new(
        "session.error",
        json!({
            "sessionID": "session-duplicate",
            "runID": "run-duplicate",
            "reason": "Prompt retry failed",
            "component": "swarm-orchestrator",
        }),
    ));

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let incidents = state.list_bug_monitor_incidents(10).await;
            if incidents
                .iter()
                .any(|row| row.status.eq_ignore_ascii_case("duplicate_suppressed"))
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("duplicate timeout");

    let incidents = state.list_bug_monitor_incidents(10).await;
    assert_eq!(incidents.len(), 1);
    let incident = &incidents[0];
    assert_eq!(incident.status, "duplicate_suppressed");
    assert!(incident.draft_id.is_none());
    assert!(state.list_bug_monitor_drafts(10).await.is_empty());

    task.abort();
}

#[tokio::test]
async fn bug_monitor_report_requires_repo() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "desktop_logs",
                    "excerpt": ["something failed"]
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let payload: Value =
        serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.expect("body"))
            .expect("json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("BUG_MONITOR_REPORT_INVALID")
    );
}

#[tokio::test]
async fn bug_monitor_draft_can_be_approved_and_denied() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            require_approval_for_new_issues: true,
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "desktop_logs",
                    "excerpt": ["boom"],
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    let payload: Value =
        serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.expect("body"))
            .expect("json");
    let draft_id = payload
        .get("draft")
        .and_then(|row| row.get("draft_id"))
        .and_then(Value::as_str)
        .expect("draft_id")
        .to_string();

    let approve_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/approve"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"reason":"looks valid"}).to_string()))
        .expect("approve request");
    let approve_resp = app
        .clone()
        .oneshot(approve_req)
        .await
        .expect("approve response");
    assert_eq!(approve_resp.status(), StatusCode::OK);
    let approve_payload: Value = serde_json::from_slice(
        &to_bytes(approve_resp.into_body(), usize::MAX)
            .await
            .expect("approve body"),
    )
    .expect("approve json");
    assert_eq!(
        approve_payload
            .get("draft")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("draft_ready")
    );

    let deny_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/deny"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"reason":"too late"}).to_string()))
        .expect("deny request");
    let deny_resp = app.clone().oneshot(deny_req).await.expect("deny response");
    assert_eq!(deny_resp.status(), StatusCode::CONFLICT);

    let second_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "desktop_logs",
                    "title": "another failure",
                    "excerpt": ["oops"],
                    "fingerprint": "manual-second"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let second_resp = app.clone().oneshot(second_req).await.expect("response");
    let second_payload: Value = serde_json::from_slice(
        &to_bytes(second_resp.into_body(), usize::MAX)
            .await
            .expect("second body"),
    )
    .expect("second json");
    let second_draft_id = second_payload
        .get("draft")
        .and_then(|row| row.get("draft_id"))
        .and_then(Value::as_str)
        .expect("second draft id");

    let deny_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{second_draft_id}/deny"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"reason":"noise"}).to_string()))
        .expect("deny request");
    let deny_resp = app.clone().oneshot(deny_req).await.expect("deny response");
    assert_eq!(deny_resp.status(), StatusCode::OK);
    let deny_payload: Value = serde_json::from_slice(
        &to_bytes(deny_resp.into_body(), usize::MAX)
            .await
            .expect("deny body"),
    )
    .expect("deny json");
    assert_eq!(
        deny_payload
            .get("draft")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("denied")
    );
}

#[tokio::test]
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
    assert!(rendered_body.contains("## What happened?"));
    assert!(rendered_body.contains("## What did you expect to happen?"));
    assert!(rendered_body.contains("## Steps to reproduce"));
    assert!(rendered_body.contains("## Environment"));
    assert!(rendered_body.contains("## Logs / screenshots"));
    assert!(rendered_body.contains("<!-- tandem:fingerprint:v1:"));
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
                ]
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
    let rendered_body = issue_draft
        .get("rendered_body")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(rendered_body.contains("Structured triage summary"));
    assert!(rendered_body.contains("The run should complete successfully."));
    assert!(rendered_body.contains("1. Open the repo"));
    assert!(rendered_body.contains("structured log line"));
    assert!(!rendered_body.contains("raw detail should not win"));
    assert_eq!(
        issue_draft.get("suggested_title").and_then(Value::as_str),
        Some("Structured triage title")
    );
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
        Some(2)
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
                    "fingerprint": "manual-artifact-fingerprint",
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

    let create_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "desktop_logs",
                    "title": "Build failure in CI",
                    "fingerprint": "manual-artifact-fingerprint",
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
        .expect("run id");

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
