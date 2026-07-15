// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

async fn spawn_fake_incident_monitor_github_mcp_server_with_issues(
    seeded_issues: Vec<Value>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake incident monitor github mcp listener");
    let addr = listener
        .local_addr()
        .expect("fake incident monitor github mcp addr");
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
                                    let owner = arguments
                                        .get("owner")
                                        .and_then(Value::as_str)
                                        .unwrap_or("acme");
                                    let repo = arguments
                                        .get("repo")
                                        .and_then(Value::as_str)
                                        .unwrap_or("platform");
                                    let issue = json!({
                                        "number": issue_number,
                                        "title": arguments.get("title").and_then(Value::as_str).unwrap_or("Incident Monitor issue"),
                                        "body": arguments.get("body").and_then(Value::as_str).unwrap_or(""),
                                        "state": "open",
                                        "html_url": format!("https://github.com/{owner}/{repo}/issues/{issue_number}")
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
                                    let owner = arguments
                                        .get("owner")
                                        .and_then(Value::as_str)
                                        .unwrap_or("acme");
                                    let repo = arguments
                                        .get("repo")
                                        .and_then(Value::as_str)
                                        .unwrap_or("platform");
                                    let comment = json!({
                                        "id": comment_id,
                                        "html_url": format!("https://github.com/{owner}/{repo}/issues/{issue_number}#issuecomment-{}", comment_rows.len() + 1)
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
            .expect("serve fake incident monitor github mcp");
    });
    (format!("http://{addr}"), server)
}

async fn spawn_fake_incident_monitor_github_mcp_server() -> (String, tokio::task::JoinHandle<()>) {
    spawn_fake_incident_monitor_github_mcp_server_with_issues(Vec::new()).await
}

async fn write_ready_incident_monitor_triage_summary(app: axum::Router, draft_id: &str) -> Value {
    let summary_req = Request::builder()
        .method("POST")
        .uri(format!("/incident-monitor/drafts/{draft_id}/triage-summary"))
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
                "acceptance_criteria": ["The failing workflow reports a useful Incident Monitor incident."],
                "verification_steps": ["Run the Incident Monitor regression tests."],
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

/// Spawn the incident monitor and wait until it has subscribed to the event bus before
/// returning. `run_incident_monitor` only subscribes after its startup-readiness check,
/// and the broadcast channel drops events published before a receiver exists, so
/// publishing on a fixed sleep races under load. Waiting on `receiver_count` makes
/// the "publish then expect an incident" tests deterministic.
async fn spawn_incident_monitor_ready(state: &AppState) -> tokio::task::JoinHandle<()> {
    let baseline = state.event_bus.receiver_count();
    let task = tokio::spawn(crate::run_incident_monitor(state.clone()));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while state.event_bus.receiver_count() <= baseline {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("incident monitor did not subscribe to the event bus in time");
    task
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_runtime_creates_incident_and_draft_from_failure_event() {
    let state = test_state().await;
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            require_approval_for_new_issues: true,
            ..Default::default()
        })
        .await
        .expect("config");

    let task = spawn_incident_monitor_ready(&state).await;
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
            let incidents = state.list_incident_monitor_incidents(10).await;
            let drafts = state.list_incident_monitor_drafts(10).await;
            let linked_draft = incidents
                .iter()
                .filter_map(|row| row.draft_id.as_deref())
                .any(|draft_id| drafts.iter().any(|draft| draft.draft_id == draft_id));
            if linked_draft {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("incident timeout");

    let incidents = state.list_incident_monitor_incidents(10).await;
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

    let drafts = state.list_incident_monitor_drafts(10).await;
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
#[serial_test::serial(incident_monitor_http)]
async fn paused_incident_monitor_runtime_ignores_failure_events() {
    let state = test_state().await;
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
            enabled: true,
            paused: true,
            repo: Some("acme/platform".to_string()),
            ..Default::default()
        })
        .await
        .expect("config");

    let task = spawn_incident_monitor_ready(&state).await;
    state.event_bus.publish(EngineEvent::new(
        "session.error",
        json!({
            "sessionID": "session-456",
            "reason": "Paused reporter should ignore this",
        }),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    assert!(state.list_incident_monitor_incidents(10).await.is_empty());
    assert!(state.list_incident_monitor_drafts(10).await.is_empty());
    task.abort();
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_runtime_detects_real_context_task_failures() {
    let state = test_state().await;
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some("/tmp/acme".to_string()),
            ..Default::default()
        })
        .await
        .expect("config");

    let task = spawn_incident_monitor_ready(&state).await;
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
            let incidents = state.list_incident_monitor_incidents(10).await;
            let drafts = state.list_incident_monitor_drafts(10).await;
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

    let incidents = state.list_incident_monitor_incidents(10).await;
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
    let drafts = state.list_incident_monitor_drafts(10).await;
    assert!(
        incident.draft_id.is_some() || incident.last_error.is_some() || !drafts.is_empty(),
        "expected either a linked draft, a draft row, or a recorded reporter error"
    );

    task.abort();
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_submission_extracts_rich_workflow_failure_metadata_and_redacts_secrets() {
    let state = test_state().await;
    let config = crate::IncidentMonitorConfig {
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

    let submission = crate::incident_monitor::service::build_incident_monitor_submission_from_event(
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
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_submission_preserves_prior_contract_failure_when_provider_stream_fails() {
    let state = test_state().await;
    let config = crate::IncidentMonitorConfig {
        enabled: true,
        repo: Some("acme/platform".to_string()),
        workspace_root: Some("/tmp/acme".to_string()),
        ..Default::default()
    };
    let event = EngineEvent::new(
        "automation_v2.run.failed",
        json!({
            "workflow_id": "review-workflow",
            "workflow_name": "Review workflow",
            "run_id": "run-review-1",
            "task_id": "research_sources",
            "node_id": "research_sources",
            "attempt": 3,
            "max_attempts": 3,
            "retry_exhausted": true,
            "error_kind": "validation_error",
            "reason": "provider stream chunk error: error decoding response body",
            "error": "provider stream chunk error: error decoding response body",
            "validation_errors": [
                "required_workspace_files_missing",
                "required workspace file `tandem-review.md` was not written in a prior attempt"
            ],
            "recent_node_attempt_evidence": [{
                "event": "workflow_state_changed",
                "attempt": 2,
                "reason": "required workspace files were not written in the current attempt: tandem-review.md",
                "unmet_requirements": ["required_workspace_files_missing"],
                "missing_workspace_files": ["tandem-review.md"],
                "required_next_tool_actions": [
                    "Write the required workspace file(s) `tandem-review.md` in this attempt before writing the run artifact; do not rely on the run artifact to satisfy this workspace-write contract."
                ]
            }],
            "attempt_review_chain": [{
                "attempt": 2,
                "node_id": "research_sources",
                "failure_class": "workspace_write_missing",
                "attempt_review": {
                    "tone": "calm_teammate_v1",
                    "progress_label": "partial",
                    "progress_score": 45,
                    "completed_correctly": ["Produced the required run artifact."],
                    "still_needed": ["Write the required workspace file(s) `tandem-review.md` approved for this node before ending the attempt."],
                    "why_it_matters": ["Downstream nodes and Incident Monitor need a real artifact or workspace file to inspect."],
                    "next_moves": ["Write `tandem-review.md`, then write the run artifact."]
                }
            }],
        }),
    );

    let submission = crate::incident_monitor::service::build_incident_monitor_submission_from_event(
        &state, &config, &event,
    )
    .await
    .expect("submission");
    let detail = submission.detail.expect("detail");

    assert!(detail.contains("provider stream chunk error: error decoding response body"));
    assert!(detail.contains("required_workspace_files_missing"));
    assert!(detail.contains("tandem-review.md"));
    assert!(detail.contains("recent_node_attempt_evidence:"));
    assert!(detail.contains("attempt_review_chain:"));
    assert!(detail.contains("calm_teammate_v1"));
    assert!(detail.contains("do not rely on the run artifact"));
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_submission_renders_workspace_file_repair_fields() {
    let state = test_state().await;
    let config = crate::IncidentMonitorConfig {
        enabled: true,
        repo: Some("acme/platform".to_string()),
        workspace_root: Some("/tmp/acme".to_string()),
        ..Default::default()
    };
    let event = EngineEvent::new(
        "automation_v2.run.failed",
        json!({
            "workflow_id": "review-workflow",
            "run_id": "run-review-2",
            "task_id": "research_sources",
            "node_id": "research_sources",
            "attempt": 2,
            "max_attempts": 3,
            "retry_exhausted": false,
            "error_kind": "validation_error",
            "reason": "automation run blocked by upstream node outcome",
            "validation_errors": ["required_workspace_files_missing"],
            "missing_workspace_files": ["tandem-review.md"],
            "required_next_tool_actions": [
                "Write `tandem-review.md` before updating the run artifact."
            ],
        }),
    );

    let submission = crate::incident_monitor::service::build_incident_monitor_submission_from_event(
        &state, &config, &event,
    )
    .await
    .expect("submission");
    let detail = submission.detail.expect("detail");

    assert!(detail.contains("retry_exhausted: false"));
    assert!(detail.contains("missing_workspace_files:"));
    assert!(detail.contains("tandem-review.md"));
    assert!(detail.contains("required_next_tool_actions:"));
    assert!(detail.contains("Write `tandem-review.md` before updating the run artifact."));
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_report_creates_and_dedupes_draft() {
    let state = test_state().await;
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
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
            "risk_category": "data_exfiltration",
            "actor": "agent-release",
            "model": "gpt-5",
            "tool_name": "slack.post_message",
            "action": "send_message",
            "policy": "approval.high_risk",
            "approval_state": "denied",
            "blast_radius": "customer channel",
            "external_correlation_ids": ["case-123", "token=SECRET_TOKEN_123"],
            "expected_destination": "incident_monitor_issue_draft",
            "evidence_refs": ["artifact://runs/run-123/logs"],
            "excerpt": ["boom", "stack trace"],
        }
    });
    let req = Request::builder()
        .method("POST")
        .uri("/incident-monitor/report")
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
    assert!(detail.contains("risk_category: data_exfiltration"));
    assert!(detail.contains("actor: agent-release"));
    assert!(detail.contains("model: gpt-5"));
    assert!(detail.contains("tool_name: slack.post_message"));
    assert!(detail.contains("action: send_message"));
    assert!(detail.contains("policy: approval.high_risk"));
    assert!(detail.contains("approval_state: denied"));
    assert!(detail.contains("blast_radius: customer channel"));
    assert!(detail.contains("external_correlation_ids: case-123, token=[redacted]"));
    assert!(!detail.contains("SECRET_TOKEN_123"));
    assert!(detail.contains("expected_destination: incident_monitor_issue_draft"));
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
        draft.get("risk_category").and_then(Value::as_str),
        Some("data_exfiltration")
    );
    assert_eq!(
        draft.get("actor").and_then(Value::as_str),
        Some("agent-release")
    );
    assert_eq!(draft.get("model").and_then(Value::as_str), Some("gpt-5"));
    assert_eq!(
        draft.get("tool_name").and_then(Value::as_str),
        Some("slack.post_message")
    );
    assert_eq!(
        draft.get("action").and_then(Value::as_str),
        Some("send_message")
    );
    assert_eq!(
        draft.get("policy").and_then(Value::as_str),
        Some("approval.high_risk")
    );
    assert_eq!(
        draft.get("approval_state").and_then(Value::as_str),
        Some("denied")
    );
    assert_eq!(
        draft.get("blast_radius").and_then(Value::as_str),
        Some("customer channel")
    );
    assert_eq!(
        draft
            .get("external_correlation_ids")
            .and_then(Value::as_array)
            .map(|rows| { rows.iter().filter_map(Value::as_str).collect::<Vec<_>>() }),
        Some(vec!["case-123", "token=[redacted]"])
    );
    assert_eq!(
        draft.get("expected_destination").and_then(Value::as_str),
        Some("incident_monitor_issue_draft")
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
        .uri("/incident-monitor/report")
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

    let drafts = state.list_incident_monitor_drafts(10).await;
    assert_eq!(drafts.len(), 1);
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_report_blocks_noisy_or_unevidenced_signals() {
    let state = test_state().await;
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/incident-monitor/report")
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
        Some("INCIDENT_MONITOR_REPORT_INVALID")
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
    assert!(state.list_incident_monitor_drafts(10).await.is_empty());
    let incidents = state.list_incident_monitor_incidents(10).await;
    assert_eq!(incidents.len(), 1);
    assert_eq!(incidents[0].status, "quality_gate_blocked");
    assert!(incidents[0].draft_id.is_none());
    assert!(incidents[0]
        .quality_gate
        .as_ref()
        .is_some_and(|gate| !gate.passed));
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_report_surfaces_duplicate_failure_patterns() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
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
        .uri("/incident-monitor/report")
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
    assert!(state.list_incident_monitor_drafts(10).await.is_empty());
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_runtime_suppresses_duplicate_failure_patterns() {
    let state = test_state().await;
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

    let task = spawn_incident_monitor_ready(&state).await;
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
            let incidents = state.list_incident_monitor_incidents(10).await;
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

    let incidents = state.list_incident_monitor_incidents(10).await;
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
    assert!(state.list_incident_monitor_drafts(10).await.is_empty());

    task.abort();
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_report_requires_repo() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/incident-monitor/report")
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
        Some("INCIDENT_MONITOR_REPORT_INVALID")
    );
}
#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn incident_monitor_draft_can_be_approved_and_denied() {
    let state = test_state().await;
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
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
        .uri("/incident-monitor/report")
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
        .uri(format!("/incident-monitor/drafts/{draft_id}/approve"))
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
        Some("incident_monitor_approval")
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
            path_has_suffix(path, "/artifacts/incident_monitor.approval_failure_pattern.json")
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
        Some("incident_monitor_triage_summary")
    );
    assert!(approve_payload
        .get("issue_draft_artifact")
        .is_some_and(Value::is_null));

    let duplicate_req = Request::builder()
        .method("POST")
        .uri("/incident-monitor/report")
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
        .uri(format!("/incident-monitor/drafts/{draft_id}/deny"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"reason":"too late"}).to_string()))
        .expect("deny request");
    let deny_resp = app.clone().oneshot(deny_req).await.expect("deny response");
    assert_eq!(deny_resp.status(), StatusCode::CONFLICT);

    let second_req = Request::builder()
        .method("POST")
        .uri("/incident-monitor/report")
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
        .uri(format!("/incident-monitor/drafts/{second_draft_id}/deny"))
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
