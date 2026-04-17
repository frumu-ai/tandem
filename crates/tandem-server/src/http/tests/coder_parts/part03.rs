
#[tokio::test]
async fn coder_issue_fix_summary_writes_patch_summary_without_changed_files() {
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
                "coder_run_id": "coder-issue-fix-diagnostic-summary",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "acme/platform"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 132
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
        .uri("/coder/runs/coder-issue-fix-diagnostic-summary/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "root_cause": "The startup fallback branch was intentionally not patched because the incident was configuration-only.",
                "validation_steps": ["cargo test -p tandem-server coder_issue_fix_summary_writes_patch_summary_without_changed_files -- --test-threads=1"],
                "validation_results": [{
                    "kind": "diagnostic",
                    "status": "passed",
                    "summary": "Configuration-only recovery path validated without code changes"
                }],
                "memory_hits_used": ["memory-hit-fix-diagnostic-1"],
                "notes": "No-op fix summary for operator follow-up."
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

    let blackboard = load_context_blackboard(&state, &linked_context_run_id);
    let patch_summary_path = blackboard
        .artifacts
        .iter()
        .find(|artifact| artifact.artifact_type == "coder_patch_summary")
        .map(|artifact| artifact.path.clone())
        .expect("patch summary path");
    let patch_summary_payload: Value = serde_json::from_str(
        &tokio::fs::read_to_string(&patch_summary_path)
            .await
            .expect("read patch summary artifact"),
    )
    .expect("parse patch summary artifact");
    assert_eq!(
        patch_summary_payload
            .get("root_cause")
            .and_then(Value::as_str),
        Some(
            "The startup fallback branch was intentionally not patched because the incident was configuration-only."
        )
    );
    assert_eq!(
        patch_summary_payload
            .get("changed_files")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        patch_summary_payload
            .get("validation_results")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        patch_summary_payload
            .get("worker_run_reference")
            .and_then(Value::as_str),
        patch_summary_payload
            .get("worker_session_context_run_id")
            .and_then(Value::as_str)
            .or_else(|| {
                patch_summary_payload
                    .get("worker_session_id")
                    .and_then(Value::as_str)
            })
    );
    assert_eq!(
        patch_summary_payload
            .get("validation_run_reference")
            .and_then(Value::as_str),
        patch_summary_payload
            .get("validation_session_context_run_id")
            .and_then(Value::as_str)
            .or_else(|| {
                patch_summary_payload
                    .get("validation_session_id")
                    .and_then(Value::as_str)
            })
    );
}

#[tokio::test]
async fn coder_issue_triage_prefers_failure_patterns_in_memory_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_first_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-triage-a",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 65
                }
            })
            .to_string(),
        ))
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let triage_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-triage-a/triage-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Crash loop traces point at startup recovery.",
                "confidence": "medium"
            })
            .to_string(),
        ))
        .expect("triage summary request");
    let triage_summary_resp = app
        .clone()
        .oneshot(triage_summary_req)
        .await
        .expect("triage summary response");
    assert_eq!(triage_summary_resp.status(), StatusCode::OK);

    let failure_pattern_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-triage-a/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "failure_pattern",
                "task_id": "attempt_reproduction",
                "summary": "Crash loop consistently starts in startup recovery.",
                "payload": {
                    "workflow_mode": "issue_triage",
                    "summary": "Crash loop consistently starts in startup recovery.",
                    "fingerprint": "triage-startup-recovery-loop",
                    "canonical_markers": ["startup recovery", "crash loop"]
                }
            })
            .to_string(),
        ))
        .expect("failure pattern request");
    let failure_pattern_resp = app
        .clone()
        .oneshot(failure_pattern_req)
        .await
        .expect("failure pattern response");
    assert_eq!(failure_pattern_resp.status(), StatusCode::OK);

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-triage-b",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 65
                }
            })
            .to_string(),
        ))
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-issue-triage-b/memory-hits")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("failure_pattern")
    );
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("triage_memory")
                && (row.get("source_coder_run_id").and_then(Value::as_str)
                    == Some("coder-issue-triage-a")
                    || row.get("run_id").and_then(Value::as_str) == Some("coder-issue-triage-a"))
        }))
        .unwrap_or(false));
}

#[tokio::test]
async fn coder_issue_fix_reuses_prior_fix_pattern_memory_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_first_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-a",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 79
                }
            })
            .to_string(),
        ))
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-a/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add the missing startup fallback guard and cover it with a targeted regression test.",
                "root_cause": "Startup recovery skipped the nil-config fallback path.",
                "fix_strategy": "add startup fallback guard",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup recovery regression is now covered"
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

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-b",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 79
                }
            })
            .to_string(),
        ))
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-issue-fix-b/memory-hits")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    assert_eq!(
        hits_payload.get("query").and_then(Value::as_str),
        Some("user123/tandem issue #79")
    );
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("fix_pattern")
    );
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("validation_memory")
                && (row.get("source_coder_run_id").and_then(Value::as_str)
                    == Some("coder-issue-fix-a")
                    || row.get("run_id").and_then(Value::as_str) == Some("coder-issue-fix-a"))
        }))
        .unwrap_or(false));
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("fix_pattern")
                && (row.get("source_coder_run_id").and_then(Value::as_str)
                    == Some("coder-issue-fix-a")
                    || row.get("run_id").and_then(Value::as_str) == Some("coder-issue-fix-a"))
        }))
        .unwrap_or(false));
}

#[tokio::test]
async fn coder_pr_review_evidence_advances_review_run() {
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
                "coder_run_id": "coder-pr-review-evidence",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 87
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

    let evidence_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-pr-review-evidence/pr-review-evidence")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "changes_requested",
                "summary": "Inspection found a risky migration path and missing rollback test.",
                "risk_level": "high",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "blockers": ["Rollback test missing"],
                "requested_changes": ["Add rollback coverage"],
                "regression_signals": [{
                    "kind": "historical_failure_pattern",
                    "summary": "Migrations without rollback have failed before"
                }],
                "memory_hits_used": ["memory-hit-pr-evidence-1"],
                "notes": "Evidence recorded before final verdict summary."
            })
            .to_string(),
        ))
        .expect("evidence request");
    let evidence_resp = app
        .clone()
        .oneshot(evidence_req)
        .await
        .expect("evidence response");
    assert_eq!(evidence_resp.status(), StatusCode::OK);
    let evidence_payload: Value = serde_json::from_slice(
        &to_bytes(evidence_resp.into_body(), usize::MAX)
            .await
            .expect("evidence body"),
    )
    .expect("evidence json");
    assert_eq!(
        evidence_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_review_evidence")
    );
    assert_eq!(
        evidence_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    assert_eq!(
        evidence_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("artifact_write")
    );
    assert!(evidence_payload
        .get("worker_run_reference")
        .is_some_and(Value::is_null));
    assert!(evidence_payload
        .get("worker_session_context_run_id")
        .is_some_and(Value::is_null));

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Running);
    for workflow_node_id in [
        "inspect_pull_request",
        "retrieve_memory",
        "review_pull_request",
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
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("write_review_artifact"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Runnable)
    );
}

#[tokio::test]
async fn coder_pr_review_execute_next_drives_task_runtime_to_completion() {
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
                "coder_run_id": "coder-pr-review-execute-next",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 200
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

    for expected in [
        "inspect_pull_request",
        "review_pull_request",
        "write_review_artifact",
    ] {
        let execute_req = Request::builder()
            .method("POST")
            .uri("/coder/runs/coder-pr-review-execute-next/execute-next")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "agent_id": "coder_engine_worker_test"
                })
                .to_string(),
            ))
            .expect("execute request");
        let execute_resp = app
            .clone()
            .oneshot(execute_req)
            .await
            .expect("execute response");
        assert_eq!(execute_resp.status(), StatusCode::OK);
        let execute_payload: Value = serde_json::from_slice(
            &to_bytes(execute_resp.into_body(), usize::MAX)
                .await
                .expect("execute body"),
        )
        .expect("execute json");
        assert_eq!(
            execute_payload
                .get("task")
                .and_then(|row| row.get("workflow_node_id"))
                .and_then(Value::as_str),
            Some(expected)
        );
        if expected != "inspect_pull_request" {
            assert_eq!(
                execute_payload
                    .get("dispatch_result")
                    .and_then(|row| row.get("worker_run_reference"))
                    .and_then(Value::as_str),
                execute_payload
                    .get("dispatch_result")
                    .and_then(|row| row.get("worker_session_context_run_id"))
                    .and_then(Value::as_str)
                    .or_else(|| {
                        execute_payload
                            .get("dispatch_result")
                            .and_then(|row| row.get("worker_session_id"))
                            .and_then(Value::as_str)
                    })
            );
        }
    }

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Completed);
    let blackboard = load_context_blackboard(&state, &linked_context_run_id);
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_pr_review_worker_session"));
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_review_evidence"));
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_pr_review_summary"));
    for workflow_node_id in [
        "inspect_pull_request",
        "retrieve_memory",
        "review_pull_request",
        "write_review_artifact",
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
}

#[tokio::test]
async fn coder_pr_review_summary_create_writes_artifact_and_outcome() {
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
                "coder_run_id": "coder-pr-review-summary",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 89,
                    "url": "https://github.com/user123/tandem/pull/89"
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
        .uri("/coder/runs/coder-pr-review-summary/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "changes_requested",
                "summary": "The PR introduces a migration risk and is missing rollback coverage.",
                "risk_level": "high",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "blockers": ["Missing rollback test"],
                "requested_changes": ["Add rollback coverage for the migration path"],
                "regression_signals": [{
                    "kind": "historical_failure_pattern",
                    "summary": "Similar rollout failed without rollback coverage"
                }],
                "validation_steps": ["cargo test -p tandem-server coder_pr_review_summary_create_writes_artifact_and_outcome -- --test-threads=1"],
                "validation_results": [{
                    "kind": "targeted_review_validation",
                    "status": "passed",
                    "summary": "Targeted review validation passed"
                }],
                "memory_hits_used": ["memory-hit-1"],
                "notes": "Review memory suggests prior migration regressions."
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
        Some("coder_pr_review_summary")
    );
    assert_eq!(
        summary_payload
            .get("review_evidence_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_review_evidence")
    );
    assert_eq!(
        summary_payload
            .get("validation_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_validation_report")
    );
    assert!(summary_payload
        .get("worker_run_reference")
        .is_some_and(Value::is_null));
    assert!(summary_payload
        .get("review_evidence_artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .is_some_and(|path| path.ends_with(
            "/context_runs/ctx-coder-pr-review-summary/artifacts/pr_review.evidence.json"
        )));
    assert!(summary_payload
        .get("validation_artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .is_some_and(|path| path.ends_with(
            "/context_runs/ctx-coder-pr-review-summary/artifacts/pr_review.validation.json"
        )));
    let summary_artifact_id = summary_payload
        .get("artifact")
        .and_then(|row| row.get("id"))
        .and_then(Value::as_str)
        .expect("summary artifact id")
        .to_string();
    let review_evidence_artifact_id = summary_payload
        .get("review_evidence_artifact")
        .and_then(|row| row.get("id"))
        .and_then(Value::as_str)
        .expect("review evidence artifact id")
        .to_string();
    let validation_artifact_id = summary_payload
        .get("validation_artifact")
        .and_then(|row| row.get("id"))
        .and_then(Value::as_str)
        .expect("validation artifact id")
        .to_string();
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows
                .iter()
                .any(|row| { row.get("kind").and_then(Value::as_str) == Some("review_memory") })),
        Some(true)
    );
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().any(|row| {
                row.get("kind").and_then(Value::as_str) == Some("regression_signal")
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

    let artifacts_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-pr-review-summary/artifacts")
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
            row.get("id").and_then(Value::as_str) == Some(summary_artifact_id.as_str())
                && row.get("artifact_type").and_then(Value::as_str)
                    == Some("coder_pr_review_summary")
        }))
        .unwrap_or(false));
    assert!(artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("id").and_then(Value::as_str) == Some(review_evidence_artifact_id.as_str())
                && row.get("artifact_type").and_then(Value::as_str) == Some("coder_review_evidence")
        }))
        .unwrap_or(false));
    assert!(artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("id").and_then(Value::as_str) == Some(validation_artifact_id.as_str())
                && row.get("artifact_type").and_then(Value::as_str)
                    == Some("coder_validation_report")
        }))
        .unwrap_or(false));

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-pr-review-summary/memory-candidates")
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
    assert!(candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("review_memory")
                && row
                    .get("payload")
                    .and_then(|payload| payload.get("review_evidence_artifact_path"))
                    .and_then(Value::as_str)
                    .is_some_and(|path| path.ends_with("/artifacts/pr_review.evidence.json"))
        }))
        .unwrap_or(false));

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.run_type, "coder_pr_review");
    assert_eq!(run.status, ContextRunStatus::Completed);
    let workflow_nodes = run
        .tasks
        .iter()
        .filter_map(|task| task.workflow_node_id.clone())
        .collect::<Vec<_>>();
    for workflow_node_id in [
        "inspect_pull_request",
        "retrieve_memory",
        "review_pull_request",
        "write_review_artifact",
    ] {
        assert_eq!(
            run.tasks
                .iter()
                .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
                .map(|task| &task.status),
            Some(&ContextBlackboardTaskStatus::Done),
            "expected {workflow_node_id} to be done; saw workflow nodes: {workflow_nodes:?}"
        );
    }
}

#[tokio::test]
async fn coder_pr_review_reuses_prior_review_memory_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_baseline_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-pr-review-baseline",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 90
                }
            })
            .to_string(),
        ))
        .expect("baseline create request");
    let create_baseline_resp = app
        .clone()
        .oneshot(create_baseline_req)
        .await
        .expect("baseline create response");
    assert_eq!(create_baseline_resp.status(), StatusCode::OK);

    let baseline_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-pr-review-baseline/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "comment",
                "summary": "Initial review requested one more pass before merge."
            })
            .to_string(),
        ))
        .expect("baseline summary request");
    let baseline_summary_resp = app
        .clone()
        .oneshot(baseline_summary_req)
        .await
        .expect("baseline summary response");
    assert_eq!(baseline_summary_resp.status(), StatusCode::OK);

    let create_first_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-pr-review-a",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 90
                }
            })
            .to_string(),
        ))
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-pr-review-a/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "changes_requested",
                "summary": "Previous review flagged missing rollback coverage.",
                "risk_level": "high",
                "requested_changes": ["Add rollback coverage"],
                "regression_signals": [{
                    "kind": "historical_failure_pattern",
                    "summary": "Rollback-free migrations regressed previously"
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

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-pr-review-b",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 90
                }
            })
            .to_string(),
        ))
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);
    let _create_second_payload: Value = serde_json::from_slice(
        &to_bytes(create_second_resp.into_body(), usize::MAX)
            .await
            .expect("second create body"),
    )
    .expect("second create json");

    let get_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-pr-review-b")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_payload: Value = serde_json::from_slice(
        &to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("get body"),
    )
    .expect("get json");
    assert!(get_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_hits")
        }))
        .unwrap_or(false));

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-pr-review-b/memory-hits")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    assert_eq!(
        hits_payload.get("query").and_then(Value::as_str),
        Some("user123/tandem pull request #90 review regressions blockers requested changes")
    );
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("review_memory")
    );
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("source_coder_run_id"))
            .and_then(Value::as_str)
            .or_else(|| {
                hits_payload
                    .get("hits")
                    .and_then(Value::as_array)
                    .and_then(|rows| rows.first())
                    .and_then(|row| row.get("run_id"))
                    .and_then(Value::as_str)
            }),
        Some("coder-pr-review-a")
    );
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("same_ref"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(get_payload
        .get("memory_hits")
        .and_then(|row| row.get("hits"))
        .and_then(Value::as_array)
        .map(|rows| !rows.is_empty())
        .unwrap_or(false));
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("regression_signal")
                && (row.get("source_coder_run_id").and_then(Value::as_str)
                    == Some("coder-pr-review-a")
                    || row.get("run_id").and_then(Value::as_str) == Some("coder-pr-review-a"))
        }))
        .unwrap_or(false));
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("review_memory")
                && (row.get("source_coder_run_id").and_then(Value::as_str)
                    == Some("coder-pr-review-a")
                    || row.get("run_id").and_then(Value::as_str) == Some("coder-pr-review-a"))
        }))
        .unwrap_or(false));
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("source_coder_run_id").and_then(Value::as_str) == Some("coder-pr-review-a")
                || row.get("run_id").and_then(Value::as_str) == Some("coder-pr-review-a")
        }))
        .unwrap_or(false));
}

#[tokio::test]
async fn coder_merge_recommendation_run_create_gets_seeded_tasks() {
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
                "coder_run_id": "coder-merge-recommendation-1",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 91
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

    let get_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-merge-recommendation-1")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_payload: Value = serde_json::from_slice(
        &to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("get body"),
    )
    .expect("get json");
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("run_type"))
            .and_then(Value::as_str),
        Some("coder_merge_recommendation")
    );
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    let tasks = get_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .cloned()
        .expect("tasks");
    assert_eq!(
        tasks
            .iter()
            .find(|row| row.get("workflow_node_id").and_then(Value::as_str)
                == Some("retrieve_memory"))
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("done")
    );
    assert_eq!(
        tasks
            .iter()
            .find(|row| row.get("workflow_node_id").and_then(Value::as_str)
                == Some("inspect_pull_request"))
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("runnable")
    );
    assert!(get_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("workflow_node_id").and_then(Value::as_str) == Some("assess_merge_readiness")
        }))
        .unwrap_or(false));
    assert!(get_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_hits")
        }))
        .unwrap_or(false));
    assert_eq!(
        get_payload
            .get("memory_hits")
            .and_then(|row| row.get("query"))
            .and_then(Value::as_str),
        Some(
            "user123/tandem pull request #91 merge recommendation regressions blockers required checks approvals"
        )
    );
}

#[tokio::test]
async fn coder_merge_readiness_report_advances_merge_run() {
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
                "coder_run_id": "coder-merge-readiness",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 93
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

    let readiness_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-readiness/merge-readiness-report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "hold",
                "summary": "The PR is close, but CODEOWNERS approval is still required.",
                "risk_level": "medium",
                "blockers": ["Required CODEOWNERS approval missing"],
                "required_checks": ["ci / test", "ci / lint"],
                "required_approvals": ["codeowners"],
                "memory_hits_used": ["memory-hit-merge-readiness-1"],
                "notes": "Readiness captured before final merge summary."
            })
            .to_string(),
        ))
        .expect("readiness request");
    let readiness_resp = app
        .clone()
        .oneshot(readiness_req)
        .await
        .expect("readiness response");
    assert_eq!(readiness_resp.status(), StatusCode::OK);
    let readiness_payload: Value = serde_json::from_slice(
        &to_bytes(readiness_resp.into_body(), usize::MAX)
            .await
            .expect("readiness body"),
    )
    .expect("readiness json");
    assert_eq!(
        readiness_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_merge_readiness_report")
    );
    assert_eq!(
        readiness_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    assert!(readiness_payload
        .get("worker_run_reference")
        .is_some_and(Value::is_null));
    assert!(readiness_payload
        .get("worker_session_context_run_id")
        .is_some_and(Value::is_null));
    assert_eq!(
        readiness_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("artifact_write")
    );

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Running);
    for workflow_node_id in [
        "inspect_pull_request",
        "retrieve_memory",
        "assess_merge_readiness",
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
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("write_merge_artifact"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Runnable)
    );
}

#[tokio::test]
async fn coder_merge_recommendation_execute_next_drives_task_runtime_to_completion() {
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
                "coder_run_id": "coder-merge-execute-next",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 201
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

    for expected in [
        "inspect_pull_request",
        "assess_merge_readiness",
        "write_merge_artifact",
    ] {
        let execute_req = Request::builder()
            .method("POST")
            .uri("/coder/runs/coder-merge-execute-next/execute-next")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "agent_id": "coder_engine_worker_test"
                })
                .to_string(),
            ))
            .expect("execute request");
        let execute_resp = app
            .clone()
            .oneshot(execute_req)
            .await
            .expect("execute response");
        assert_eq!(execute_resp.status(), StatusCode::OK);
        let execute_payload: Value = serde_json::from_slice(
            &to_bytes(execute_resp.into_body(), usize::MAX)
                .await
                .expect("execute body"),
        )
        .expect("execute json");
        assert_eq!(
            execute_payload
                .get("task")
                .and_then(|row| row.get("workflow_node_id"))
                .and_then(Value::as_str),
            Some(expected)
        );
        if expected != "inspect_repo" {
            assert_eq!(
                execute_payload
                    .get("dispatch_result")
                    .and_then(|row| row.get("worker_run_reference"))
                    .and_then(Value::as_str),
                execute_payload
                    .get("dispatch_result")
                    .and_then(|row| row.get("worker_session_context_run_id"))
                    .and_then(Value::as_str)
                    .or_else(|| {
                        execute_payload
                            .get("dispatch_result")
                            .and_then(|row| row.get("worker_session_id"))
                            .and_then(Value::as_str)
                    })
            );
        }
    }

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Completed);
    let blackboard = load_context_blackboard(&state, &linked_context_run_id);
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_merge_recommendation_worker_session"));
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_merge_readiness_report"));
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_merge_recommendation_summary"));
    for workflow_node_id in [
        "inspect_pull_request",
        "retrieve_memory",
        "assess_merge_readiness",
        "write_merge_artifact",
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
}

#[tokio::test]
async fn coder_merge_recommendation_summary_create_writes_artifact() {
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
                "coder_run_id": "coder-merge-recommendation-summary",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 92
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
        .uri("/coder/runs/coder-merge-recommendation-summary/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "hold",
                "summary": "Checks are mostly green but one required approval is still missing.",
                "risk_level": "medium",
                "blockers": ["Required reviewer approval missing"],
                "required_checks": ["ci / test", "ci / lint"],
                "required_approvals": ["codeowners"],
                "validation_steps": ["gh pr checks 92"],
                "validation_results": [{
                    "kind": "merge_gate_validation",
                    "status": "pending",
                    "summary": "Required approval still pending"
                }],
                "memory_hits_used": ["memory-hit-merge-1"],
                "notes": "Wait for CODEOWNERS approval before merge."
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
        Some("coder_merge_recommendation_summary")
    );
    assert_eq!(
        summary_payload
            .get("readiness_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_merge_readiness_report")
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
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("completed")
    );
    assert!(summary_payload
        .get("worker_run_reference")
        .is_some_and(Value::is_null));
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().any(|row| {
                row.get("kind").and_then(Value::as_str) == Some("merge_recommendation_memory")
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
    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Completed);
    for workflow_node_id in [
        "inspect_pull_request",
        "retrieve_memory",
        "assess_merge_readiness",
        "write_merge_artifact",
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
    let readiness_artifact_id = summary_payload
        .get("readiness_artifact")
        .and_then(|row| row.get("id"))
        .and_then(Value::as_str)
        .expect("readiness artifact id")
        .to_string();
    let validation_artifact_id = summary_payload
        .get("validation_artifact")
        .and_then(|row| row.get("id"))
        .and_then(Value::as_str)
        .expect("validation artifact id")
        .to_string();

    let artifacts_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-merge-recommendation-summary/artifacts")
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
            row.get("artifact_type").and_then(Value::as_str)
                == Some("coder_merge_recommendation_summary")
        }))
        .unwrap_or(false));
    assert!(artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("id").and_then(Value::as_str) == Some(readiness_artifact_id.as_str())
                && row.get("artifact_type").and_then(Value::as_str)
                    == Some("coder_merge_readiness_report")
        }))
        .unwrap_or(false));
    assert!(artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("id").and_then(Value::as_str) == Some(validation_artifact_id.as_str())
                && row.get("artifact_type").and_then(Value::as_str)
                    == Some("coder_validation_report")
        }))
        .unwrap_or(false));

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-merge-recommendation-summary/memory-candidates")
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
    assert!(candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("merge_recommendation_memory")
                && row
                    .get("payload")
                    .and_then(|payload| payload.get("readiness_artifact_path"))
                    .and_then(Value::as_str)
                    .is_some_and(|path| {
                        path.ends_with("/artifacts/merge_recommendation.readiness.json")
                    })
        }))
        .unwrap_or(false));
}
