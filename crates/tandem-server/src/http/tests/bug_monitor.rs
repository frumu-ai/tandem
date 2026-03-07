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
                    "recurrence_count": 5,
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

    let second_seed_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-failure-pattern-seed-2",
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
        .expect("second seed request");
    let second_seed_resp = app
        .clone()
        .oneshot(second_seed_req)
        .await
        .expect("second seed response");
    assert_eq!(second_seed_resp.status(), StatusCode::OK);

    let second_candidate_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-failure-pattern-seed-2/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "failure_pattern",
                "summary": "stack trace orchestrator.run_failed desktop_logs",
                "payload": {
                    "type": "failure.pattern",
                    "repo_slug": "acme/platform",
                    "fingerprint": "manual-failure-pattern",
                    "symptoms": ["desktop_logs", "orchestrator.run_failed"],
                    "canonical_markers": ["orchestrator.run_failed", "stack trace", "desktop_logs"],
                    "linked_issue_numbers": [302],
                    "recurrence_count": 1,
                    "linked_pr_numbers": [],
                    "affected_components": ["orchestrator"],
                    "artifact_refs": ["artifact://ctx/manual/triage.summary.json"]
                }
            })
            .to_string(),
        ))
        .expect("second candidate request");
    let second_candidate_resp = app
        .clone()
        .oneshot(second_candidate_req)
        .await
        .expect("second candidate response");
    assert_eq!(second_candidate_resp.status(), StatusCode::OK);

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
    let duplicate_summary = report_payload
        .get("duplicate_summary")
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(
        report_payload.get("suppressed").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        duplicate_summary.get("match_count").and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        duplicate_summary
            .get("best_match")
            .and_then(|value| value.get("fingerprint"))
            .and_then(Value::as_str),
        Some("manual-failure-pattern")
    );
    assert_eq!(
        duplicate_summary
            .get("best_match")
            .and_then(|value| value.get("match_reason"))
            .and_then(Value::as_str),
        Some("exact_fingerprint")
    );
    assert_eq!(
        duplicate_summary
            .get("best_match")
            .and_then(|value| value.get("recurrence_count"))
            .and_then(Value::as_u64),
        Some(5)
    );
    assert_eq!(
        duplicate_summary
            .get("best_match")
            .and_then(|value| value.get("linked_issue_numbers"))
            .and_then(Value::as_array)
            .cloned(),
        Some(vec![Value::from(301_u64)])
    );
    assert_eq!(
        report_payload
            .get("duplicate_matches")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(2)
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
                    "recurrence_count": 5,
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
    let duplicate_summary = incident
        .duplicate_summary
        .as_ref()
        .and_then(|value| value.get("matches"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(duplicate_summary.len(), 1);
    assert_eq!(
        incident
            .duplicate_summary
            .as_ref()
            .and_then(|value| value.get("match_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        incident
            .duplicate_summary
            .as_ref()
            .and_then(|value| value.get("best_match"))
            .and_then(|value| value.get("recurrence_count"))
            .and_then(Value::as_u64),
        Some(5)
    );
    assert_eq!(
        incident
            .duplicate_summary
            .as_ref()
            .and_then(|value| value.get("best_match"))
            .and_then(|value| value.get("linked_issue_numbers"))
            .and_then(Value::as_array)
            .cloned(),
        Some(vec![Value::from(401_u64)])
    );
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
    assert_eq!(
        approve_payload
            .get("failure_pattern_memory")
            .and_then(|row| row.get("stored"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        approve_payload
            .get("failure_pattern_memory")
            .and_then(|row| row.get("metadata"))
            .and_then(|row| row.get("source"))
            .and_then(Value::as_str),
        Some("bug_monitor_approval")
    );
    assert_eq!(
        approve_payload
            .get("failure_pattern_memory")
            .and_then(|row| row.get("metadata"))
            .and_then(|row| row.get("recurrence_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert!(approve_payload
        .get("failure_pattern_memory")
        .and_then(|row| row.get("metadata"))
        .and_then(|row| row.get("artifact_refs"))
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(Value::as_str)
        .is_some_and(|path| {
            path.ends_with("/artifacts/bug_monitor.approval_failure_pattern.json")
        }));
    assert!(approve_payload
        .get("issue_draft")
        .and_then(|row| row.get("rendered_body"))
        .and_then(Value::as_str)
        .is_some_and(|body| body.contains("boom")));
    assert!(
        approve_payload.get("issue_draft_artifact").is_none()
            || approve_payload
                .get("issue_draft_artifact")
                .is_some_and(Value::is_null)
    );

    let duplicate_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "desktop_logs",
                    "fingerprint": approve_payload
                        .get("draft")
                        .and_then(|row| row.get("fingerprint"))
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    "excerpt": ["boom"],
                }
            })
            .to_string(),
        ))
        .expect("duplicate request");
    let duplicate_resp = app
        .clone()
        .oneshot(duplicate_req)
        .await
        .expect("duplicate response");
    assert_eq!(duplicate_resp.status(), StatusCode::OK);
    let duplicate_payload: Value = serde_json::from_slice(
        &to_bytes(duplicate_resp.into_body(), usize::MAX)
            .await
            .expect("duplicate body"),
    )
    .expect("duplicate json");
    assert_eq!(
        duplicate_payload.get("suppressed").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        duplicate_payload
            .get("duplicate_matches")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
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
                    "title": "billing worker checksum mismatch 0xdeadbeef",
                    "detail": "billing worker checksum mismatch 0xdeadbeef while loading monthly ledger",
                    "excerpt": ["billing-worker", "checksum mismatch 0xdeadbeef", "monthly ledger"],
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
        .is_some_and(|path| path.ends_with("/artifacts/bug_monitor.issue_draft.json")));
    assert!(rendered_body.contains("## What happened?"));
    assert!(rendered_body.contains("## What did you expect to happen?"));
    assert!(rendered_body.contains("## Steps to reproduce"));
    assert!(rendered_body.contains("## Environment"));
    assert!(rendered_body.contains("## Logs / screenshots"));
    assert!(rendered_body.contains("<!-- tandem:fingerprint:v1:"));
}

#[tokio::test]
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
        .is_some_and(|body| body.contains("Build failure in CI")));
    assert_eq!(
        publish_payload
            .get("triage_summary_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_triage_summary")
    );
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
    assert_eq!(
        recheck_payload
            .get("triage_summary_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_triage_summary")
    );
    assert_eq!(
        recheck_payload
            .get("issue_draft_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_issue_draft")
    );
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
            duplicate_summary: None,
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
    assert!(!rendered_body.contains("raw detail should not win"));
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
    assert!(triage_payload
        .get("issue_draft")
        .and_then(|row| row.get("rendered_body"))
        .and_then(Value::as_str)
        .is_some_and(|body| body.contains("Build failure in CI")));
    assert_eq!(
        triage_payload
            .get("issue_draft_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_issue_draft")
    );
    assert!(triage_payload
        .get("issue_draft_artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .is_some_and(|path| path.ends_with("/artifacts/bug_monitor.issue_draft.json")));

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
    assert_eq!(
        second_payload
            .get("issue_draft_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_issue_draft")
    );
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
            duplicate_summary: None,
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
        .and_then(|row| row.get("rendered_body"))
        .and_then(Value::as_str)
        .is_some_and(|body| body.contains("Build failure in CI")));
    assert_eq!(
        replay_payload
            .get("issue_draft_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_issue_draft")
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
    assert_eq!(
        triage_payload
            .get("duplicate_matches_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("failure_duplicate_matches")
    );
    assert!(triage_payload
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
