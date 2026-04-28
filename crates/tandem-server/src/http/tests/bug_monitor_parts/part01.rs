async fn spawn_fake_bug_monitor_github_mcp_server_with_issues(
    seeded_issues: Vec<Value>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake bug monitor github mcp listener");
    let addr = listener
        .local_addr()
        .expect("fake bug monitor github mcp addr");
    let issues = Arc::new(RwLock::new(seeded_issues));
    let comments = Arc::new(RwLock::new(Vec::<Value>::new()));
    let app = axum::Router::new().route(
        "/",
        axum::routing::post({
            let issues = issues.clone();
            let comments = comments.clone();
            move |axum::Json(request): axum::Json<Value>| {
                let issues = issues.clone();
                let comments = comments.clone();
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
                                "name": "github",
                                "version": "test"
                            }
                        }),
                        "tools/list" => json!({
                            "tools": [
                                {
                                    "name": "list_repository_issues",
                                    "description": "List repository issues",
                                    "inputSchema": {"type":"object"}
                                },
                                {
                                    "name": "get_issue",
                                    "description": "Get a GitHub issue",
                                    "inputSchema": {"type":"object"}
                                },
                                {
                                    "name": "mcp.github.create_issue",
                                    "description": "Create a GitHub issue",
                                    "inputSchema": {"type":"object"}
                                },
                                {
                                    "name": "mcp.github.create_issue_comment",
                                    "description": "Create a GitHub issue comment",
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
                                "list_repository_issues" => {
                                    let snapshot = issues.read().await.clone();
                                    json!({ "issues": snapshot })
                                }
                                "get_issue" => {
                                    let issue_number = arguments
                                        .get("issue_number")
                                        .and_then(Value::as_u64)
                                        .unwrap_or_default();
                                    let issue = issues
                                        .read()
                                        .await
                                        .iter()
                                        .find(|row| {
                                            row.get("number").and_then(Value::as_u64)
                                                == Some(issue_number)
                                        })
                                        .cloned()
                                        .unwrap_or_else(|| {
                                            json!({
                                                "number": issue_number,
                                                "title": "missing",
                                                "body": "",
                                                "state": "closed",
                                                "html_url": format!("https://github.com/acme/platform/issues/{issue_number}")
                                            })
                                        });
                                    json!({ "issue": issue })
                                }
                                "mcp.github.create_issue" => {
                                    let mut issue_rows = issues.write().await;
                                    let issue_number = (issue_rows.len() as u64) + 101;
                                    let issue = json!({
                                        "number": issue_number,
                                        "title": arguments.get("title").and_then(Value::as_str).unwrap_or("Bug Monitor issue"),
                                        "body": arguments.get("body").and_then(Value::as_str).unwrap_or(""),
                                        "state": "open",
                                        "html_url": format!("https://github.com/acme/platform/issues/{issue_number}")
                                    });
                                    issue_rows.push(issue.clone());
                                    json!({ "issue": issue })
                                }
                                "mcp.github.create_issue_comment" => {
                                    let mut comment_rows = comments.write().await;
                                    let comment_id = format!("comment-{}", comment_rows.len() + 1);
                                    let issue_number = arguments
                                        .get("issue_number")
                                        .and_then(Value::as_u64)
                                        .unwrap_or_default();
                                    let comment = json!({
                                        "id": comment_id,
                                        "html_url": format!("https://github.com/acme/platform/issues/{issue_number}#issuecomment-{}", comment_rows.len() + 1)
                                    });
                                    comment_rows.push(comment.clone());
                                    json!({ "comment": comment })
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
            .expect("serve fake bug monitor github mcp");
    });
    (format!("http://{addr}"), server)
}

async fn spawn_fake_bug_monitor_github_mcp_server() -> (String, tokio::task::JoinHandle<()>) {
    spawn_fake_bug_monitor_github_mcp_server_with_issues(Vec::new()).await
}

async fn write_ready_bug_monitor_triage_summary(app: axum::Router, draft_id: &str) -> Value {
    let summary_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/triage-summary"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "suggested_title": "Build failure in CI",
                "what_happened": "The CI build failed while running the Tandem engine workflow.",
                "why_it_likely_happened": "The orchestrator path is returning an error after repository inspection.",
                "root_cause_confidence": "medium",
                "failure_type": "code_defect",
                "affected_components": ["orchestrator"],
                "likely_files_to_edit": ["crates/tandem-server/src/app/tasks.rs"],
                "expected_behavior": "The orchestrator run should complete without a build failure.",
                "steps_to_reproduce": ["Run the affected Tandem workflow.", "Observe the CI build failure."],
                "environment": ["Repo: acme/platform", "Process: tandem-engine"],
                "logs": ["boom", "stack trace"],
                "related_existing_issues": [],
                "related_failure_patterns": [],
                "research_sources": [{"kind": "repo", "summary": "Inspected orchestrator failure handling."}],
                "recommended_fix": "Handle the orchestrator failure path and preserve the error evidence.",
                "acceptance_criteria": ["The failing workflow reports a useful Bug Monitor incident."],
                "verification_steps": ["Run the Bug Monitor regression tests."],
                "coder_ready": true,
                "risk_level": "medium",
                "required_tool_scopes": [],
                "missing_tool_scopes": [],
                "permissions_available": true,
                "notes": "Research and validation completed in the triage run.",
            })
            .to_string(),
        ))
        .expect("triage summary request");
    let summary_resp = app
        .oneshot(summary_req)
        .await
        .expect("triage summary response");
    let summary_status = summary_resp.status();
    let summary_body = to_bytes(summary_resp.into_body(), usize::MAX)
        .await
        .expect("triage summary body");
    if summary_status != StatusCode::OK {
        panic!("{}", String::from_utf8_lossy(&summary_body));
    }
    serde_json::from_slice(&summary_body).expect("triage summary json")
}

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
    assert!(incident
        .quality_gate
        .as_ref()
        .is_some_and(|gate| gate.passed));

    let drafts = state.list_bug_monitor_drafts(10).await;
    assert_eq!(drafts.len(), 1);
    assert_eq!(
        drafts[0].draft_id,
        incident.draft_id.clone().unwrap_or_default()
    );
    assert!(drafts[0]
        .quality_gate
        .as_ref()
        .is_some_and(|gate| gate.passed));

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
    let drafts = state.list_bug_monitor_drafts(10).await;
    assert!(
        incident.draft_id.is_some() || incident.last_error.is_some() || !drafts.is_empty(),
        "expected either a linked draft, a draft row, or a recorded reporter error"
    );

    task.abort();
}

#[tokio::test]
async fn bug_monitor_submission_extracts_rich_workflow_failure_metadata_and_redacts_secrets() {
    let state = test_state().await;
    let config = crate::BugMonitorConfig {
        enabled: true,
        repo: Some("acme/platform".to_string()),
        workspace_root: Some("/tmp/acme".to_string()),
        ..Default::default()
    };
    let event = EngineEvent::new(
        "workflow.run.failed",
        json!({
            "workflow_id": "feature-archaeology",
            "workflowID": "feature-archaeology",
            "workflow_name": "Feature archaeology",
            "run_id": "test-run-1",
            "runID": "test-run-1",
            "task_id": "github-publish",
            "taskID": "github-publish",
            "stage_id": "publish_issues",
            "agent_role": "issue_publisher",
            "attempt": 3,
            "max_attempts": 3,
            "retry_exhausted": true,
            "error_kind": "tool_error",
            "reason": "GitHub create issue failed with 422",
            "error": "Validation failed: label does not exist",
            "expected_output": "Create or update GitHub issue",
            "actual_output": "GitHub rejected issue creation",
            "tool_name": "github.create_issue",
            "tool_args_summary": {"repo": "acme/platform", "token": "ghp_secret"},
            "artifact_refs": ["artifacts/feature_archaeology.issue_candidates.json"],
            "files_touched": ["crates/tandem-server/src/workflows.rs"],
            "validation_errors": ["label does not exist"],
            "suggested_next_action": "Check labels before creating issues or create missing labels first",
            "api_key": "secret-api-key"
        }),
    );

    let submission = crate::bug_monitor::service::build_bug_monitor_submission_from_event(
        &state, &config, &event,
    )
    .await
    .expect("submission");
    let title = submission.title.expect("title");
    let detail = submission.detail.expect("detail");

    assert!(title.contains("Workflow feature-archaeology failed at publish_issues"));
    assert!(detail.contains("workflow_id: feature-archaeology"));
    assert!(detail.contains("run_id: test-run-1"));
    assert!(detail.contains("task_id: github-publish"));
    assert!(detail.contains("error_kind: tool_error"));
    assert!(detail.contains("artifact_refs:"));
    assert!(detail.contains("artifacts/feature_archaeology.issue_candidates.json"));
    assert!(detail.contains("files_touched:"));
    assert!(detail.contains("crates/tandem-server/src/workflows.rs"));
    assert!(detail.contains("Check labels before creating issues"));
    assert!(!detail.contains("ghp_secret"));
    assert!(!detail.contains("secret-api-key"));
    assert!(detail.contains("[redacted]"));
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
            "confidence": "medium",
            "risk_level": "medium",
            "expected_destination": "bug_monitor_issue_draft",
            "evidence_refs": ["artifact://runs/run-123/logs"],
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
    let detail = draft
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(detail.contains("confidence: medium"));
    assert!(detail.contains("risk_level: medium"));
    assert!(detail.contains("expected_destination: bug_monitor_issue_draft"));
    assert!(detail.contains("evidence_refs:"));
    assert!(detail.contains("artifact://runs/run-123/logs"));
    assert_eq!(
        draft.get("confidence").and_then(Value::as_str),
        Some("medium")
    );
    assert_eq!(
        draft.get("risk_level").and_then(Value::as_str),
        Some("medium")
    );
    assert_eq!(
        draft.get("expected_destination").and_then(Value::as_str),
        Some("bug_monitor_issue_draft")
    );
    assert_eq!(
        draft
            .get("evidence_refs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str),
        Some("artifact://runs/run-123/logs")
    );
    assert_eq!(
        draft
            .get("quality_gate")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("passed")
    );

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
async fn bug_monitor_report_blocks_noisy_or_unevidenced_signals() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "desktop_logs",
                    "event": "workflow.run.progress",
                    "title": "Still running"
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
    assert!(payload
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .contains("quality gate"));
    assert_eq!(
        payload
            .get("quality_gate")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        payload
            .get("incident")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("quality_gate_blocked")
    );
    assert!(state.list_bug_monitor_drafts(10).await.is_empty());
    let incidents = state.list_bug_monitor_incidents(10).await;
    assert_eq!(incidents.len(), 1);
    assert_eq!(incidents[0].status, "quality_gate_blocked");
    assert!(incidents[0].draft_id.is_none());
    assert!(incidents[0]
        .quality_gate
        .as_ref()
        .is_some_and(|gate| !gate.passed));
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
        duplicate_summary
            .get("best_match")
            .and_then(|value| value.get("artifact_refs"))
            .and_then(Value::as_array)
            .cloned(),
        Some(vec![Value::from(
            "artifact://ctx/manual/triage.summary.json"
        )])
    );
    assert_eq!(
        duplicate_summary
            .get("best_match")
            .and_then(|value| value.get("candidate_id"))
            .and_then(Value::as_str),
        duplicate_summary
            .get("best_match")
            .and_then(|value| value.get("candidate_id"))
            .and_then(Value::as_str)
    );
    assert!(duplicate_summary
        .get("best_match")
        .and_then(|value| value.get("candidate_id"))
        .and_then(Value::as_str)
        .is_some_and(|value| value.starts_with("memcand-")));
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
    assert_eq!(
        incident
            .duplicate_summary
            .as_ref()
            .and_then(|value| value.get("best_match"))
            .and_then(|value| value.get("artifact_refs"))
            .and_then(Value::as_array)
            .cloned(),
        Some(vec![Value::from(
            "artifact://ctx/manual/triage.summary.json"
        )])
    );
    assert_eq!(
        incident
            .duplicate_summary
            .as_ref()
            .and_then(|value| value.get("best_match"))
            .and_then(|value| value.get("candidate_id"))
            .and_then(Value::as_str),
        incident
            .duplicate_summary
            .as_ref()
            .and_then(|value| value.get("best_match"))
            .and_then(|value| value.get("candidate_id"))
            .and_then(Value::as_str)
    );
    assert!(incident
        .duplicate_summary
        .as_ref()
        .and_then(|value| value.get("best_match"))
        .and_then(|value| value.get("candidate_id"))
        .and_then(Value::as_str)
        .is_some_and(|value| value.starts_with("memcand-")));
    let duplicate_matches = incident.duplicate_matches.clone().unwrap_or_default();
    assert_eq!(duplicate_matches.len(), 1);
    assert_eq!(
        duplicate_matches[0].get("source").and_then(Value::as_str),
        Some("coder_candidate")
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
        Some("triage_queued")
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
        .is_some_and(Value::is_null));
    assert_eq!(
        approve_payload
            .get("duplicate_summary")
            .and_then(|row| row.get("match_count"))
            .and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        approve_payload
            .get("duplicate_matches")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        approve_payload
            .get("triage_summary_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("bug_monitor_triage_summary")
    );
    assert!(approve_payload
        .get("issue_draft_artifact")
        .is_some_and(Value::is_null));

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
        .is_some_and(|path| path.ends_with("/artifacts/bug_monitor.triage_summary.json")));
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
