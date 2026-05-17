#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_empty_triage_summary_synthesizes_file_refs_and_fix_points() {
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
    let create_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/bug-monitor/report")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "report": {
                            "source": "automation_v2",
                            "title": "Workflow run failed at read_contracts",
                            "detail": "required output `.tandem/runs/run-1/artifacts/read-contracts.md` was not created for node `read_contracts`",
                            "component": "automation_v2",
                            "event": "automation_v2.run.failed",
                            "excerpt": ["required output was not created"]
                        }
                    })
                    .to_string(),
                ))
                .expect("create request"),
        )
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
        .expect("draft id");

    let triage_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/bug-monitor/drafts/{draft_id}/triage-run"))
                .body(Body::empty())
                .expect("triage request"),
        )
        .await
        .expect("triage response");
    assert_eq!(triage_resp.status(), StatusCode::OK);

    let summary_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/bug-monitor/drafts/{draft_id}/triage-summary"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .expect("summary request"),
        )
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    let summary = summary_payload
        .get("triage_summary")
        .expect("triage summary");
    assert!(summary
        .get("file_references")
        .and_then(Value::as_array)
        .is_some_and(|rows| !rows.is_empty()));
    assert!(summary
        .get("fix_points")
        .and_then(Value::as_array)
        .is_some_and(|rows| !rows.is_empty()));
    assert!(summary_payload
        .get("issue_draft")
        .is_some_and(Value::is_object));
}

#[tokio::test]
#[serial_test::serial]
#[serial_test::serial(bug_monitor_http)]
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
