
#[tokio::test]
async fn coder_merge_recommendation_execute_all_runs_to_completion() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-execute-all",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 301
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

    let execute_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-execute-all/execute-all")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "agent_id": "coder_engine_worker_test",
                "max_steps": 8
            })
            .to_string(),
        ))
        .expect("execute-all request");
    let execute_resp = app
        .clone()
        .oneshot(execute_req)
        .await
        .expect("execute-all response");
    assert_eq!(execute_resp.status(), StatusCode::OK);
    let execute_payload: Value = serde_json::from_slice(
        &to_bytes(execute_resp.into_body(), usize::MAX)
            .await
            .expect("execute-all body"),
    )
    .expect("execute-all json");
    assert_eq!(
        execute_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        execute_payload
            .get("stopped_reason")
            .and_then(Value::as_str),
        Some("run_completed")
    );
    assert!(execute_payload
        .get("executed_steps")
        .and_then(Value::as_u64)
        .is_some_and(|count| count >= 3));
}

#[tokio::test]
async fn coder_issue_fix_summary_create_writes_artifact() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-summary",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 78
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
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-summary/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Guard the missing config branch and add a regression test for startup recovery.",
                "root_cause": "Nil config fallback was skipped during startup recovery.",
                "fix_strategy": "add startup fallback guard",
                "changed_files": [
                    "crates/tandem-server/src/http/coder.rs",
                    "crates/tandem-server/src/http/tests/coder.rs"
                ],
                "validation_steps": ["cargo test -p tandem-server coder_issue_fix_summary_create_writes_artifact -- --test-threads=1"],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "targeted coder issue-fix regression passed"
                }],
                "memory_hits_used": ["memory-hit-fix-1"],
                "notes": "Prior triage memory pointed to startup recovery flow."
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
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_issue_fix_summary")
    );
    assert_eq!(
        summary_payload
            .get("validation_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_validation_report")
    );
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows
                .iter()
                .any(|row| { row.get("kind").and_then(Value::as_str) == Some("fix_pattern") })),
        Some(true)
    );
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().any(|row| {
                row.get("kind").and_then(Value::as_str) == Some("validation_memory")
            })),
        Some(true)
    );
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows
                .iter()
                .any(|row| { row.get("kind").and_then(Value::as_str) == Some("run_outcome") })),
        Some(true)
    );
    assert_eq!(
        summary_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("completed")
    );
    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Completed);
    for workflow_node_id in [
        "inspect_issue_context",
        "retrieve_memory",
        "prepare_fix",
        "validate_fix",
        "write_fix_artifact",
    ] {
        assert_eq!(
            run.tasks
                .iter()
                .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
                .map(|task| &task.status),
            Some(&ContextBlackboardTaskStatus::Done),
            "expected {workflow_node_id} to be done"
        );
    }

    let artifacts_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-issue-fix-summary/artifacts")
        .body(Body::empty())
        .expect("artifacts request");
    let artifacts_resp = app
        .clone()
        .oneshot(artifacts_req)
        .await
        .expect("artifacts response");
    assert_eq!(artifacts_resp.status(), StatusCode::OK);
    let artifacts_payload: Value = serde_json::from_slice(
        &to_bytes(artifacts_resp.into_body(), usize::MAX)
            .await
            .expect("artifacts body"),
    )
    .expect("artifacts json");
    assert!(artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("artifact_type").and_then(Value::as_str) == Some("coder_issue_fix_summary")
        }))
        .unwrap_or(false));
    assert!(artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("artifact_type").and_then(Value::as_str) == Some("coder_validation_report")
        }))
        .unwrap_or(false));
}

#[tokio::test]
async fn coder_issue_fix_pr_draft_create_writes_artifact() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-pr-draft",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 312
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

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-draft/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Guard startup recovery config loading.",
                "root_cause": "Recovery skipped the nil-config fallback branch.",
                "fix_strategy": "restore the fallback guard and add a regression test",
                "changed_files": [
                    "crates/tandem-server/src/http/coder.rs",
                    "crates/tandem-server/src/http/tests/coder.rs"
                ],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "targeted issue-fix regression passed"
                }]
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

    let draft_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-draft/pr-draft")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "base_branch": "main"
            })
            .to_string(),
        ))
        .expect("draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("draft response");
    assert_eq!(draft_resp.status(), StatusCode::OK);
    let draft_payload: Value = serde_json::from_slice(
        &to_bytes(draft_resp.into_body(), usize::MAX)
            .await
            .expect("draft body"),
    )
    .expect("draft json");
    assert_eq!(
        draft_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_pr_draft")
    );
    assert_eq!(
        draft_payload
            .get("approval_required")
            .and_then(Value::as_bool),
        Some(true)
    );

    let artifact_path = draft_payload
        .get("artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .expect("draft artifact path");
    let artifact_payload: Value = serde_json::from_str(
        &tokio::fs::read_to_string(artifact_path)
            .await
            .expect("read draft artifact"),
    )
    .expect("parse draft artifact");
    assert_eq!(
        artifact_payload.get("title").and_then(Value::as_str),
        Some("Guard startup recovery config loading.")
    );
    assert!(artifact_payload
        .get("body")
        .and_then(Value::as_str)
        .is_some_and(|body| body.contains("Closes #312")));
    assert!(artifact_payload
        .get("body")
        .and_then(Value::as_str)
        .is_some_and(|body| body.contains("coder.rs")));
}

#[tokio::test]
async fn coder_issue_fix_pr_submit_dry_run_writes_submission_artifact() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-pr-submit",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 313
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

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add missing fallback to startup recovery.",
                "root_cause": "Recovery skipped the nil-config guard.",
                "fix_strategy": "restore startup fallback and add a targeted regression",
                "changed_files": [
                    "crates/tandem-server/src/http/coder.rs"
                ],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup recovery regression passed"
                }]
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

    let draft_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit/pr-draft")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("draft response");
    assert_eq!(draft_resp.status(), StatusCode::OK);

    let submit_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit/pr-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "user123",
                "reason": "Looks good for a draft PR",
                "dry_run": true
            })
            .to_string(),
        ))
        .expect("submit request");
    let submit_resp = app
        .clone()
        .oneshot(submit_req)
        .await
        .expect("submit response");
    assert_eq!(submit_resp.status(), StatusCode::OK);
    let submit_payload: Value = serde_json::from_slice(
        &to_bytes(submit_resp.into_body(), usize::MAX)
            .await
            .expect("submit body"),
    )
    .expect("submit json");
    assert_eq!(
        submit_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_pr_submission")
    );
    assert_eq!(
        submit_payload
            .get("external_action")
            .and_then(|row| row.get("capability_id"))
            .and_then(Value::as_str),
        Some("github.create_pull_request")
    );
    assert_eq!(
        submit_payload
            .get("external_action")
            .and_then(|row| row.get("source_kind"))
            .and_then(Value::as_str),
        Some("coder")
    );
    assert!(submit_payload
        .get("duplicate_linkage_candidate")
        .is_some_and(Value::is_null));
    assert_eq!(
        submit_payload.get("submitted").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        submit_payload.get("dry_run").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        submit_payload
            .get("worker_run_reference")
            .and_then(Value::as_str),
        submit_payload
            .get("worker_session_context_run_id")
            .and_then(Value::as_str)
    );
    assert_eq!(
        submit_payload
            .get("validation_run_reference")
            .and_then(Value::as_str),
        submit_payload
            .get("validation_session_context_run_id")
            .and_then(Value::as_str)
    );
    assert!(submit_payload
        .get("submitted_github_ref")
        .is_some_and(Value::is_null));
    assert_eq!(
        submit_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        submit_payload
            .get("skipped_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    let submission_artifact_payload = submit_payload
        .get("artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|body| serde_json::from_str::<Value>(&body).ok())
        .expect("submission artifact payload");
    assert_eq!(
        submission_artifact_payload
            .get("worker_run_reference")
            .and_then(Value::as_str),
        submission_artifact_payload
            .get("worker_session_context_run_id")
            .and_then(Value::as_str)
    );
    assert_eq!(
        submission_artifact_payload
            .get("validation_run_reference")
            .and_then(Value::as_str),
        submission_artifact_payload
            .get("validation_session_context_run_id")
            .and_then(Value::as_str)
    );
    assert_eq!(
        submission_artifact_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
}

#[tokio::test]
async fn coder_issue_fix_pr_submit_real_submit_writes_canonical_pr_identity() {
    let (endpoint, server) = spawn_fake_github_mcp_server().await;

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
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-pr-submit-real",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 313
                },
                "mcp_servers": ["github"]
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

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit-real/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add missing fallback to startup recovery.",
                "root_cause": "Recovery skipped the nil-config guard.",
                "fix_strategy": "restore startup fallback and add a targeted regression",
                "changed_files": [
                    "crates/tandem-server/src/http/coder.rs"
                ],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup recovery regression passed"
                }]
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

    let draft_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit-real/pr-draft")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("draft response");
    assert_eq!(draft_resp.status(), StatusCode::OK);

    let submit_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit-real/pr-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "user123",
                "reason": "Ready to open the draft PR",
                "dry_run": false,
                "mcp_server": "github",
                "spawn_follow_on_runs": ["pr_review"]
            })
            .to_string(),
        ))
        .expect("submit request");
    let submit_resp = app
        .clone()
        .oneshot(submit_req)
        .await
        .expect("submit response");
    server.abort();

    assert_eq!(submit_resp.status(), StatusCode::OK);
    let submit_payload: Value = serde_json::from_slice(
        &to_bytes(submit_resp.into_body(), usize::MAX)
            .await
            .expect("submit body"),
    )
    .expect("submit json");
    assert_eq!(
        submit_payload.get("submitted").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        submit_payload
            .get("submitted_github_ref")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("pull_request")
    );
    assert_eq!(
        submit_payload
            .get("pull_request")
            .and_then(|row| row.get("number"))
            .and_then(Value::as_u64),
        Some(314)
    );
    assert_eq!(
        submit_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(2)
    );
    assert_eq!(
        submit_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("parent_coder_run_id"))
            .and_then(Value::as_str),
        Some("coder-issue-fix-pr-submit-real")
    );
    assert_eq!(
        submit_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("spawn_mode"))
            .and_then(Value::as_str),
        Some("template")
    );
    assert_eq!(
        submit_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.get(1))
            .and_then(|row| row.get("required_completed_workflow_modes"))
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["pr_review"])
    );
    assert_eq!(
        submit_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.get(1))
            .and_then(|row| row.get("execution_policy_preview"))
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        submit_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.get(1))
            .and_then(|row| row.get("merge_submit_policy_preview"))
            .and_then(|row| row.get("auto_execute_policy_enabled"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        submit_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.get(1))
            .and_then(|row| row.get("merge_submit_policy_preview"))
            .and_then(|row| row.get("auto_execute_block_reason"))
            .and_then(Value::as_str),
        Some("project_auto_merge_policy_disabled")
    );
    assert_eq!(
        submit_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.get(1))
            .and_then(|row| row.get("merge_submit_policy_preview"))
            .and_then(|row| row.get("manual"))
            .and_then(|row| row.get("policy"))
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_merge_execution_request")
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("pr_review")
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("parent_coder_run_id"))
            .and_then(Value::as_str),
        Some("coder-issue-fix-pr-submit-real")
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("origin"))
            .and_then(Value::as_str),
        Some("issue_fix_pr_submit_auto")
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("spawn_mode"))
            .and_then(Value::as_str),
        Some("auto")
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("execution_policy"))
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        submit_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_pr_submission")
    );

    let artifact_path = submit_payload
        .get("artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .expect("submit artifact path");
    let artifact_payload: Value = serde_json::from_str(
        &tokio::fs::read_to_string(artifact_path)
            .await
            .expect("read submit artifact"),
    )
    .expect("parse submit artifact");
    assert_eq!(
        artifact_payload
            .get("submitted_github_ref")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("pull_request")
    );
    assert_eq!(
        artifact_payload
            .get("submitted_github_ref")
            .and_then(|row| row.get("number"))
            .and_then(Value::as_u64),
        Some(314)
    );
    assert_eq!(
        artifact_payload
            .get("pull_request")
            .and_then(|row| row.get("number"))
            .and_then(Value::as_u64),
        Some(314)
    );
    assert_eq!(
        artifact_payload.get("owner").and_then(Value::as_str),
        Some("user123")
    );
    assert_eq!(
        artifact_payload.get("repo").and_then(Value::as_str),
        Some("tandem")
    );
    assert_eq!(
        artifact_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(2)
    );
    assert_eq!(
        artifact_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("spawn_mode"))
            .and_then(Value::as_str),
        Some("template")
    );
    assert_eq!(
        artifact_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        artifact_payload
            .get("skipped_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        artifact_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("pr_review")
    );
    assert_eq!(
        artifact_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.get(1))
            .and_then(|row| row.get("execution_policy_preview"))
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        artifact_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.get(1))
            .and_then(|row| row.get("merge_submit_policy_preview"))
            .and_then(|row| row.get("auto_execute_policy_enabled"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        artifact_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("execution_policy"))
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        artifact_payload
            .get("duplicate_linkage_candidate")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("duplicate_linkage")
    );

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-issue-fix-pr-submit-real/memory-candidates")
        .body(Body::empty())
        .expect("candidates request");
    let candidates_resp = app
        .clone()
        .oneshot(candidates_req)
        .await
        .expect("candidates response");
    assert_eq!(candidates_resp.status(), StatusCode::OK);
    let candidates_payload: Value = serde_json::from_slice(
        &to_bytes(candidates_resp.into_body(), usize::MAX)
            .await
            .expect("candidates body"),
    )
    .expect("candidates json");
    let duplicate_linkage = candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("kind").and_then(Value::as_str) == Some("duplicate_linkage"))
        })
        .expect("duplicate linkage candidate");
    assert_eq!(
        duplicate_linkage
            .get("payload")
            .and_then(|row| row.get("linked_issue_numbers"))
            .cloned(),
        Some(json!([313]))
    );
    assert_eq!(
        duplicate_linkage
            .get("payload")
            .and_then(|row| row.get("linked_pr_numbers"))
            .cloned(),
        Some(json!([314]))
    );

    let follow_on_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit-real/follow-on-run")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_mode": "pr_review",
                "coder_run_id": "coder-follow-on-pr-review"
            })
            .to_string(),
        ))
        .expect("follow-on request");
    let follow_on_resp = app
        .clone()
        .oneshot(follow_on_req)
        .await
        .expect("follow-on response");
    assert_eq!(follow_on_resp.status(), StatusCode::OK);
    let follow_on_payload: Value = serde_json::from_slice(
        &to_bytes(follow_on_resp.into_body(), usize::MAX)
            .await
            .expect("follow-on body"),
    )
    .expect("follow-on json");
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("pr_review")
    );
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("github_ref"))
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("pull_request")
    );
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("github_ref"))
            .and_then(|row| row.get("number"))
            .and_then(Value::as_u64),
        Some(314)
    );
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("parent_coder_run_id"))
            .and_then(Value::as_str),
        Some("coder-issue-fix-pr-submit-real")
    );
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("origin"))
            .and_then(Value::as_str),
        Some("issue_fix_pr_submit_manual_follow_on")
    );
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("origin_artifact_type"))
            .and_then(Value::as_str),
        Some("coder_pr_submission")
    );
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("spawn_mode"))
            .and_then(Value::as_str),
        Some("manual")
    );
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("required_completed_workflow_modes"))
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(Vec::<&str>::new())
    );
    assert_eq!(
        follow_on_payload
            .get("execution_policy")
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        follow_on_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("duplicate_linkage")
    );

    let follow_on_candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-follow-on-pr-review/memory-candidates")
        .body(Body::empty())
        .expect("follow-on candidates request");
    let follow_on_candidates_resp = app
        .clone()
        .oneshot(follow_on_candidates_req)
        .await
        .expect("follow-on candidates response");
    assert_eq!(follow_on_candidates_resp.status(), StatusCode::OK);
    let follow_on_candidates_payload: Value = serde_json::from_slice(
        &to_bytes(follow_on_candidates_resp.into_body(), usize::MAX)
            .await
            .expect("follow-on candidates body"),
    )
    .expect("follow-on candidates json");
    let follow_on_duplicate_linkage = follow_on_candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("kind").and_then(Value::as_str) == Some("duplicate_linkage"))
        })
        .expect("follow-on duplicate linkage candidate");
    assert_eq!(
        follow_on_duplicate_linkage
            .get("payload")
            .and_then(|row| row.get("linked_issue_numbers"))
            .cloned(),
        Some(json!([313]))
    );
    assert_eq!(
        follow_on_duplicate_linkage
            .get("payload")
            .and_then(|row| row.get("linked_pr_numbers"))
            .cloned(),
        Some(json!([314]))
    );

    let review_hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-follow-on-pr-review/memory-hits")
        .body(Body::empty())
        .expect("review hits request");
    let review_hits_resp = app
        .clone()
        .oneshot(review_hits_req)
        .await
        .expect("review hits response");
    assert_eq!(review_hits_resp.status(), StatusCode::OK);
    let review_hits_payload: Value = serde_json::from_slice(
        &to_bytes(review_hits_resp.into_body(), usize::MAX)
            .await
            .expect("review hits body"),
    )
    .expect("review hits json");
    let review_duplicate_linkage = review_hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("kind").and_then(Value::as_str) == Some("duplicate_linkage"))
        })
        .expect("review duplicate linkage hit");
    assert_eq!(
        review_duplicate_linkage
            .get("same_linked_pr")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        review_hits_payload
            .get("retrieval_policy")
            .and_then(|row| row.get("prioritized_kinds"))
            .cloned(),
        Some(json!([
            "review_memory",
            "merge_recommendation_memory",
            "duplicate_linkage",
            "regression_signal",
            "run_outcome"
        ]))
    );

    let submitted_event = next_event_of_type(&mut rx, "coder.pr.submitted").await;
    assert_eq!(
        submitted_event
            .properties
            .get("submitted_github_ref")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("pull_request")
    );
    assert_eq!(
        submitted_event
            .properties
            .get("pull_request_number")
            .and_then(Value::as_u64),
        Some(314)
    );
    assert_eq!(
        submitted_event
            .properties
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(2)
    );
    assert_eq!(
        submitted_event
            .properties
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.get(1))
            .and_then(|row| row.get("execution_policy_preview"))
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        submitted_event
            .properties
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.get(1))
            .and_then(|row| row.get("merge_submit_policy_preview"))
            .and_then(|row| row.get("auto_execute_policy_enabled"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        submitted_event
            .properties
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        submitted_event
            .properties
            .get("duplicate_linkage_candidate")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("duplicate_linkage")
    );
    assert_eq!(
        submitted_event
            .properties
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("execution_policy"))
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        submitted_event
            .properties
            .get("skipped_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
}

#[tokio::test]
async fn coder_issue_fix_pr_submit_merge_auto_spawn_requires_opt_in() {
    let (endpoint, server) = spawn_fake_github_mcp_server().await;

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
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-pr-submit-merge-policy",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 313
                },
                "mcp_servers": ["github"]
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

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit-merge-policy/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add missing fallback to startup recovery.",
                "root_cause": "Recovery skipped the nil-config guard.",
                "fix_strategy": "restore startup fallback and add a targeted regression",
                "changed_files": [
                    "crates/tandem-server/src/http/coder.rs"
                ],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup recovery regression passed"
                }]
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

    let draft_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit-merge-policy/pr-draft")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("draft response");
    assert_eq!(draft_resp.status(), StatusCode::OK);

    let submit_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit-merge-policy/pr-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "user123",
                "reason": "Open the draft PR and queue review",
                "dry_run": false,
                "mcp_server": "github",
                "spawn_follow_on_runs": ["merge_recommendation"]
            })
            .to_string(),
        ))
        .expect("submit request");
    let submit_resp = app
        .clone()
        .oneshot(submit_req)
        .await
        .expect("submit response");
    server.abort();

    assert_eq!(submit_resp.status(), StatusCode::OK);
    let submit_payload: Value = serde_json::from_slice(
        &to_bytes(submit_resp.into_body(), usize::MAX)
            .await
            .expect("submit body"),
    )
    .expect("submit json");
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("pr_review")
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("origin"))
            .and_then(Value::as_str),
        Some("issue_fix_pr_submit_auto")
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("merge_auto_spawn_opted_in"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        submit_payload
            .get("skipped_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        submit_payload
            .get("skipped_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("merge_recommendation")
    );
    assert_eq!(
        submit_payload
            .get("skipped_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_explicit_auto_merge_recommendation_opt_in")
    );

    let submitted_event = next_event_of_type(&mut rx, "coder.pr.submitted").await;
    assert_eq!(
        submitted_event
            .properties
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        submitted_event
            .properties
            .get("skipped_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
}

#[tokio::test]
async fn coder_merge_follow_on_execution_waits_for_completed_review() {
    let (endpoint, server) = spawn_fake_github_mcp_server().await;

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
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-follow-on-policy-parent",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 313
                },
                "mcp_servers": ["github"]
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

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-policy-parent/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add missing fallback to startup recovery.",
                "root_cause": "Recovery skipped the nil-config guard.",
                "fix_strategy": "restore startup fallback and add a targeted regression",
                "changed_files": [
                    "crates/tandem-server/src/http/coder.rs"
                ],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup recovery regression passed"
                }]
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

    let draft_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-policy-parent/pr-draft")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("draft response");
    assert_eq!(draft_resp.status(), StatusCode::OK);

    let submit_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-policy-parent/pr-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "user123",
                "reason": "Open the draft PR",
                "dry_run": false,
                "mcp_server": "github"
            })
            .to_string(),
        ))
        .expect("submit request");
    let submit_resp = app
        .clone()
        .oneshot(submit_req)
        .await
        .expect("submit response");
    assert_eq!(submit_resp.status(), StatusCode::OK);

    let merge_follow_on_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-policy-parent/follow-on-run")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_mode": "merge_recommendation",
                "coder_run_id": "coder-follow-on-merge"
            })
            .to_string(),
        ))
        .expect("merge follow-on request");
    let merge_follow_on_resp = app
        .clone()
        .oneshot(merge_follow_on_req)
        .await
        .expect("merge follow-on response");
    assert_eq!(merge_follow_on_resp.status(), StatusCode::OK);
    let merge_follow_on_payload: Value = serde_json::from_slice(
        &to_bytes(merge_follow_on_resp.into_body(), usize::MAX)
            .await
            .expect("merge follow-on body"),
    )
    .expect("merge follow-on json");
    assert_eq!(
        merge_follow_on_payload
            .get("execution_policy")
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        merge_follow_on_payload
            .get("execution_policy")
            .and_then(|row| row.get("policy"))
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_completed_pr_review_follow_on")
    );
    assert_eq!(
        merge_follow_on_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto_execute_policy_enabled"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        merge_follow_on_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto_execute_block_reason"))
            .and_then(Value::as_str),
        Some("requires_merge_execution_request")
    );
    assert_eq!(
        merge_follow_on_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("manual"))
            .and_then(|row| row.get("policy"))
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_merge_execution_request")
    );

    let blocked_execute_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-merge/execute-next")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("blocked execute request");
    let blocked_execute_resp = app
        .clone()
        .oneshot(blocked_execute_req)
        .await
        .expect("blocked execute response");
    assert_eq!(blocked_execute_resp.status(), StatusCode::OK);
    let blocked_execute_payload: Value = serde_json::from_slice(
        &to_bytes(blocked_execute_resp.into_body(), usize::MAX)
            .await
            .expect("blocked execute body"),
    )
    .expect("blocked execute json");
    assert_eq!(
        blocked_execute_payload.get("code").and_then(Value::as_str),
        Some("CODER_EXECUTION_POLICY_BLOCKED")
    );
    assert_eq!(
        blocked_execute_payload
            .get("policy")
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_completed_pr_review_follow_on")
    );
    assert_eq!(
        blocked_execute_payload
            .get("execution_policy")
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        blocked_execute_payload
            .get("coder_run")
            .and_then(|row| row.get("coder_run_id"))
            .and_then(Value::as_str),
        Some("coder-follow-on-merge")
    );
    assert_eq!(
        blocked_execute_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    let blocked_event = loop {
        let event = next_event_of_type(&mut rx, "coder.run.phase_changed").await;
        if event.properties.get("event_type").and_then(Value::as_str)
            == Some("execution_policy_blocked")
        {
            break event;
        }
    };
    assert_eq!(
        blocked_event
            .properties
            .get("policy")
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_completed_pr_review_follow_on")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("coder_run_id")
            .and_then(Value::as_str),
        Some("coder-follow-on-merge")
    );
    assert!(blocked_event
        .properties
        .get("linked_context_run_id")
        .and_then(Value::as_str)
        .is_some());
    assert_eq!(
        blocked_event
            .properties
            .get("workflow_mode")
            .and_then(Value::as_str),
        Some("merge_recommendation")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("repo_binding")
            .and_then(|row| row.get("repo_slug"))
            .and_then(Value::as_str),
        Some("user123/tandem")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("github_ref")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("pull_request")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("phase")
            .and_then(Value::as_str),
        Some("policy_blocked")
    );
    let merge_run_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-follow-on-merge")
        .body(Body::empty())
        .expect("merge get request");
    let merge_run_resp = app
        .clone()
        .oneshot(merge_run_req)
        .await
        .expect("merge get response");
    assert_eq!(merge_run_resp.status(), StatusCode::OK);
    let merge_run_payload: Value = serde_json::from_slice(
        &to_bytes(merge_run_resp.into_body(), usize::MAX)
            .await
            .expect("merge get body"),
    )
    .expect("merge get json");
    assert_eq!(
        merge_run_payload
            .get("coder_run")
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("required_completed_workflow_modes"))
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["pr_review"])
    );
    assert_eq!(
        merge_run_payload
            .get("execution_policy")
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        merge_run_payload
            .get("execution_policy")
            .and_then(|row| row.get("policy"))
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_completed_pr_review_follow_on")
    );

    let review_follow_on_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-policy-parent/follow-on-run")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_mode": "pr_review",
                "coder_run_id": "coder-follow-on-review"
            })
            .to_string(),
        ))
        .expect("review follow-on request");
    let review_follow_on_resp = app
        .clone()
        .oneshot(review_follow_on_req)
        .await
        .expect("review follow-on response");
    assert_eq!(review_follow_on_resp.status(), StatusCode::OK);

    let review_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-review/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "approve",
                "summary": "Looks good after targeted review.",
                "risk_level": "low",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "blockers": [],
                "requested_changes": []
            })
            .to_string(),
        ))
        .expect("review summary request");
    let review_summary_resp = app
        .clone()
        .oneshot(review_summary_req)
        .await
        .expect("review summary response");
    assert_eq!(review_summary_resp.status(), StatusCode::OK);

    let allowed_execute_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-merge/execute-next")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("allowed execute request");
    let allowed_execute_resp = app
        .clone()
        .oneshot(allowed_execute_req)
        .await
        .expect("allowed execute response");
    server.abort();

    assert_eq!(allowed_execute_resp.status(), StatusCode::OK);
    let allowed_execute_payload: Value = serde_json::from_slice(
        &to_bytes(allowed_execute_resp.into_body(), usize::MAX)
            .await
            .expect("allowed execute body"),
    )
    .expect("allowed execute json");
    assert_eq!(
        allowed_execute_payload.get("ok").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        allowed_execute_payload
            .get("dispatched")
            .and_then(Value::as_bool),
        Some(true)
    );
}
