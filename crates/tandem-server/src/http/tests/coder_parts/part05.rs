
#[tokio::test]
async fn coder_merge_submit_blocks_when_execution_request_is_not_merge_ready() {
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
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-submit-blocked",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 315
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
        .uri("/coder/runs/coder-merge-submit-blocked/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "merge",
                "summary": "This looked merge-ready before downstream policy re-check.",
                "blockers": [],
                "required_checks": [],
                "required_approvals": []
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

    let approve_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-blocked/approve")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reason": "Operator approved merge execution."
            })
            .to_string(),
        ))
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
    let merge_execution_artifact_path = approve_payload
        .get("merge_execution_artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .expect("merge execution artifact path")
        .to_string();
    tokio::fs::write(
        &merge_execution_artifact_path,
        serde_json::to_string_pretty(&json!({
            "coder_run_id": "coder-merge-submit-blocked",
            "linked_context_run_id": "ctx-coder-merge-submit-blocked",
            "workflow_mode": "merge_recommendation",
            "repo_binding": {
                "project_id": "proj-engine",
                "workspace_id": "ws-tandem",
                "workspace_root": "/tmp/tandem-repo",
                "repo_slug": "user123/tandem"
            },
            "github_ref": {
                "kind": "pull_request",
                "number": 315
            },
            "recommendation": "hold",
            "blockers": ["Manual verification pending"],
            "required_checks": ["ci / test"],
            "required_approvals": ["codeowners"]
        }))
        .expect("merge execution artifact json"),
    )
    .await
    .expect("overwrite merge execution artifact");

    let submit_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-blocked/merge-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "user123",
                "reason": "Try to merge anyway",
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
    server.abort();
    assert_eq!(submit_resp.status(), StatusCode::OK);
    let submit_payload: Value = serde_json::from_slice(
        &to_bytes(submit_resp.into_body(), usize::MAX)
            .await
            .expect("submit body"),
    )
    .expect("submit json");
    assert_eq!(
        submit_payload.get("ok").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        submit_payload.get("code").and_then(Value::as_str),
        Some("CODER_MERGE_SUBMIT_POLICY_BLOCKED")
    );
    assert_eq!(
        submit_payload
            .get("policy")
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("merge_execution_request_not_merge_ready")
    );
}

#[tokio::test]
async fn coder_merge_submit_blocks_auto_mode_without_opt_in() {
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
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-submit-auto-blocked",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 316
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
        .uri("/coder/runs/coder-merge-submit-auto-blocked/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "merge",
                "summary": "Checks and approvals are complete.",
                "blockers": [],
                "required_checks": [],
                "required_approvals": []
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

    let approve_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-auto-blocked/approve")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reason": "Operator approved merge execution."
            })
            .to_string(),
        ))
        .expect("approve request");
    let approve_resp = app
        .clone()
        .oneshot(approve_req)
        .await
        .expect("approve response");
    assert_eq!(approve_resp.status(), StatusCode::OK);

    let submit_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-auto-blocked/merge-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "user123",
                "reason": "Try to auto-execute the merge",
                "submit_mode": "auto",
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
    server.abort();
    assert_eq!(submit_resp.status(), StatusCode::OK);
    let submit_payload: Value = serde_json::from_slice(
        &to_bytes(submit_resp.into_body(), usize::MAX)
            .await
            .expect("submit body"),
    )
    .expect("submit json");
    assert_eq!(
        submit_payload.get("ok").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        submit_payload.get("code").and_then(Value::as_str),
        Some("CODER_MERGE_SUBMIT_POLICY_BLOCKED")
    );
    assert_eq!(
        submit_payload
            .get("policy")
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_explicit_auto_merge_submit_opt_in")
    );
}

#[tokio::test]
async fn coder_merge_submit_blocks_auto_mode_for_manual_follow_on() {
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
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-submit-manual-follow-on-parent",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 319
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
        .uri("/coder/runs/coder-merge-submit-manual-follow-on-parent/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add startup fallback for nil config handling.",
                "root_cause": "Missing fallback in recovery path.",
                "fix_strategy": "Restore fallback and add regression coverage.",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup fallback regression passed"
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
        .uri("/coder/runs/coder-merge-submit-manual-follow-on-parent/pr-draft")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("draft response");
    assert_eq!(draft_resp.status(), StatusCode::OK);

    let submit_pr_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-manual-follow-on-parent/pr-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "user123",
                "reason": "Open the draft PR",
                "dry_run": false,
                "mcp_server": "github",
                "allow_auto_merge_recommendation": true
            })
            .to_string(),
        ))
        .expect("submit pr request");
    let submit_pr_resp = app
        .clone()
        .oneshot(submit_pr_req)
        .await
        .expect("submit pr response");
    assert_eq!(submit_pr_resp.status(), StatusCode::OK);

    let merge_follow_on_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-manual-follow-on-parent/follow-on-run")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_mode": "merge_recommendation",
                "coder_run_id": "coder-merge-submit-manual-follow-on-merge"
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
            .get("coder_run")
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("spawn_mode"))
            .and_then(Value::as_str),
        Some("manual")
    );
    assert_eq!(
        merge_follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("merge_auto_spawn_opted_in"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let merge_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-manual-follow-on-merge/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "merge",
                "summary": "Everything looks ready from the merge side.",
                "blockers": [],
                "required_checks": [],
                "required_approvals": []
            })
            .to_string(),
        ))
        .expect("merge summary request");
    let merge_summary_resp = app
        .clone()
        .oneshot(merge_summary_req)
        .await
        .expect("merge summary response");
    assert_eq!(merge_summary_resp.status(), StatusCode::OK);

    let approve_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-manual-follow-on-merge/approve")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reason": "Operator approved merge execution."
            })
            .to_string(),
        ))
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
            .get("merge_submit_policy")
            .and_then(|row| row.get("preferred_submit_mode"))
            .and_then(Value::as_str),
        Some("manual")
    );
    assert_eq!(
        approve_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("explicit_submit_required"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        approve_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto_execute_eligible"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        approve_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto_execute_policy_enabled"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        approve_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto_execute_block_reason"))
            .and_then(Value::as_str),
        Some("requires_approved_pr_review_follow_on")
    );
    assert_eq!(
        approve_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto"))
            .and_then(|row| row.get("policy"))
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_auto_spawned_merge_follow_on")
    );

    let submit_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-manual-follow-on-merge/merge-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "user123",
                "reason": "Try to auto-execute the manual follow-on merge",
                "submit_mode": "auto",
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
    server.abort();
    assert_eq!(submit_resp.status(), StatusCode::OK);
    let submit_payload: Value = serde_json::from_slice(
        &to_bytes(submit_resp.into_body(), usize::MAX)
            .await
            .expect("submit body"),
    )
    .expect("submit json");
    assert_eq!(
        submit_payload.get("ok").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        submit_payload.get("code").and_then(Value::as_str),
        Some("CODER_MERGE_SUBMIT_POLICY_BLOCKED")
    );
    assert_eq!(
        submit_payload
            .get("policy")
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_auto_spawned_merge_follow_on")
    );
    assert_eq!(
        submit_payload
            .get("policy")
            .and_then(|row| row.get("spawn_mode"))
            .and_then(Value::as_str),
        Some("manual")
    );
}

#[tokio::test]
async fn coder_merge_policy_reports_auto_execute_eligibility_when_project_enabled() {
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
    let app = app_router(state.clone());

    let policy_req = Request::builder()
        .method("PUT")
        .uri("/coder/projects/proj-engine/policy")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "auto_merge_enabled": true
            })
            .to_string(),
        ))
        .expect("policy request");
    let policy_resp = app
        .clone()
        .oneshot(policy_req)
        .await
        .expect("policy response");
    assert_eq!(policy_resp.status(), StatusCode::OK);

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-auto-eligible-parent",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 320
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
        .uri("/coder/runs/coder-merge-auto-eligible-parent/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add missing fallback to startup recovery.",
                "root_cause": "Recovery skipped the nil-config guard.",
                "fix_strategy": "restore startup fallback and add a targeted regression",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
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
        .uri("/coder/runs/coder-merge-auto-eligible-parent/pr-draft")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("draft response");
    assert_eq!(draft_resp.status(), StatusCode::OK);

    let submit_pr_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-auto-eligible-parent/pr-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "user123",
                "reason": "Open the draft PR and queue review plus merge follow-ons",
                "dry_run": false,
                "mcp_server": "github",
                "allow_auto_merge_recommendation": true,
                "spawn_follow_on_runs": ["merge_recommendation"]
            })
            .to_string(),
        ))
        .expect("submit pr request");
    let submit_pr_resp = app
        .clone()
        .oneshot(submit_pr_req)
        .await
        .expect("submit pr response");
    assert_eq!(submit_pr_resp.status(), StatusCode::OK);
    let submit_pr_payload: Value = serde_json::from_slice(
        &to_bytes(submit_pr_resp.into_body(), usize::MAX)
            .await
            .expect("submit pr body"),
    )
    .expect("submit pr json");
    let spawned_runs = submit_pr_payload
        .get("spawned_follow_on_runs")
        .and_then(Value::as_array)
        .expect("spawned follow-on runs");
    assert_eq!(spawned_runs.len(), 2);
    let review_run_id = spawned_runs[0]
        .get("coder_run")
        .and_then(|row| row.get("coder_run_id"))
        .and_then(Value::as_str)
        .expect("review run id");
    let merge_run_id = spawned_runs[1]
        .get("coder_run")
        .and_then(|row| row.get("coder_run_id"))
        .and_then(Value::as_str)
        .expect("merge run id");

    let review_summary_req = Request::builder()
        .method("POST")
        .uri(&format!("/coder/runs/{review_run_id}/pr-review-summary"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "approve",
                "summary": "Looks good to merge.",
                "risk_level": "low",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "blockers": [],
                "requested_changes": [],
                "regression_signals": []
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

    let merge_summary_req = Request::builder()
        .method("POST")
        .uri(&format!(
            "/coder/runs/{merge_run_id}/merge-recommendation-summary"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "merge",
                "summary": "Everything looks ready from the merge side.",
                "blockers": [],
                "required_checks": [],
                "required_approvals": []
            })
            .to_string(),
        ))
        .expect("merge summary request");
    let merge_summary_resp = app
        .clone()
        .oneshot(merge_summary_req)
        .await
        .expect("merge summary response");
    assert_eq!(merge_summary_resp.status(), StatusCode::OK);

    let approve_req = Request::builder()
        .method("POST")
        .uri(&format!("/coder/runs/{merge_run_id}/approve"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reason": "Operator approved merge execution."
            })
            .to_string(),
        ))
        .expect("approve request");
    let approve_resp = app
        .clone()
        .oneshot(approve_req)
        .await
        .expect("approve response");
    server.abort();
    assert_eq!(approve_resp.status(), StatusCode::OK);
    let approve_payload: Value = serde_json::from_slice(
        &to_bytes(approve_resp.into_body(), usize::MAX)
            .await
            .expect("approve body"),
    )
    .expect("approve json");
    assert_eq!(
        approve_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("preferred_submit_mode"))
            .and_then(Value::as_str),
        Some("auto")
    );
    assert_eq!(
        approve_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto_execute_policy_enabled"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        approve_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto_execute_eligible"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        approve_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto_execute_after_approval"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        approve_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto_execute_block_reason"))
            .and_then(Value::as_str),
        Some("explicit_submit_required_policy")
    );
    assert_eq!(
        approve_payload
            .get("merge_submit_policy")
            .and_then(|row| row.get("auto"))
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(false)
    );
}

#[tokio::test]
async fn coder_merge_submit_blocks_without_approved_sibling_pr_review() {
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
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-submit-review-block-parent",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 317
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
        .uri("/coder/runs/coder-merge-submit-review-block-parent/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add startup fallback for nil config handling.",
                "root_cause": "Missing fallback in recovery path.",
                "fix_strategy": "Restore fallback and add regression coverage.",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup fallback regression passed"
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
        .uri("/coder/runs/coder-merge-submit-review-block-parent/pr-draft")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("draft response");
    assert_eq!(draft_resp.status(), StatusCode::OK);

    let submit_pr_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-review-block-parent/pr-submit")
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
        .expect("submit pr request");
    let submit_pr_resp = app
        .clone()
        .oneshot(submit_pr_req)
        .await
        .expect("submit pr response");
    assert_eq!(submit_pr_resp.status(), StatusCode::OK);

    let review_follow_on_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-review-block-parent/follow-on-run")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_mode": "pr_review",
                "coder_run_id": "coder-merge-submit-review-block-review"
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
        .uri("/coder/runs/coder-merge-submit-review-block-review/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "changes_requested",
                "summary": "Rollback coverage is still missing.",
                "risk_level": "medium",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "blockers": [],
                "requested_changes": ["Add rollback coverage"],
                "regression_signals": []
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

    let merge_follow_on_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-review-block-parent/follow-on-run")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_mode": "merge_recommendation",
                "coder_run_id": "coder-merge-submit-review-block-merge"
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

    let merge_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-review-block-merge/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "merge",
                "summary": "Everything looks ready from the merge side.",
                "blockers": [],
                "required_checks": [],
                "required_approvals": []
            })
            .to_string(),
        ))
        .expect("merge summary request");
    let merge_summary_resp = app
        .clone()
        .oneshot(merge_summary_req)
        .await
        .expect("merge summary response");
    assert_eq!(merge_summary_resp.status(), StatusCode::OK);

    let approve_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-review-block-merge/approve")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reason": "Operator approved merge execution."
            })
            .to_string(),
        ))
        .expect("approve request");
    let approve_resp = app
        .clone()
        .oneshot(approve_req)
        .await
        .expect("approve response");
    assert_eq!(approve_resp.status(), StatusCode::OK);

    let submit_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-review-block-merge/merge-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "user123",
                "reason": "Try to merge despite review objections",
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
    server.abort();
    assert_eq!(submit_resp.status(), StatusCode::OK);
    let submit_payload: Value = serde_json::from_slice(
        &to_bytes(submit_resp.into_body(), usize::MAX)
            .await
            .expect("submit body"),
    )
    .expect("submit json");
    assert_eq!(
        submit_payload.get("ok").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        submit_payload.get("code").and_then(Value::as_str),
        Some("CODER_MERGE_SUBMIT_POLICY_BLOCKED")
    );
    assert_eq!(
        submit_payload
            .get("policy")
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_approved_pr_review_follow_on")
    );
    assert_eq!(
        submit_payload
            .get("policy")
            .and_then(|row| row.get("review_verdict"))
            .and_then(Value::as_str),
        Some("changes_requested")
    );
    assert_eq!(
        submit_payload
            .get("policy")
            .and_then(|row| row.get("has_requested_changes"))
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn coder_merge_submit_uses_latest_completed_sibling_pr_review() {
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
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-submit-latest-review-parent",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 318
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
        .uri("/coder/runs/coder-merge-submit-latest-review-parent/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add startup fallback for nil config handling.",
                "root_cause": "Missing fallback in recovery path.",
                "fix_strategy": "Restore fallback and add regression coverage.",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup fallback regression passed"
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
        .uri("/coder/runs/coder-merge-submit-latest-review-parent/pr-draft")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("draft response");
    assert_eq!(draft_resp.status(), StatusCode::OK);

    let submit_pr_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-latest-review-parent/pr-submit")
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
        .expect("submit pr request");
    let submit_pr_resp = app
        .clone()
        .oneshot(submit_pr_req)
        .await
        .expect("submit pr response");
    assert_eq!(submit_pr_resp.status(), StatusCode::OK);

    let first_review_follow_on_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-latest-review-parent/follow-on-run")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_mode": "pr_review",
                "coder_run_id": "coder-merge-submit-latest-review-approve"
            })
            .to_string(),
        ))
        .expect("first review follow-on request");
    let first_review_follow_on_resp = app
        .clone()
        .oneshot(first_review_follow_on_req)
        .await
        .expect("first review follow-on response");
    assert_eq!(first_review_follow_on_resp.status(), StatusCode::OK);

    let first_review_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-latest-review-approve/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "approve",
                "summary": "Looks good to merge from the first pass.",
                "risk_level": "low",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "blockers": [],
                "requested_changes": [],
                "regression_signals": []
            })
            .to_string(),
        ))
        .expect("first review summary request");
    let first_review_summary_resp = app
        .clone()
        .oneshot(first_review_summary_req)
        .await
        .expect("first review summary response");
    assert_eq!(first_review_summary_resp.status(), StatusCode::OK);

    let second_review_follow_on_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-latest-review-parent/follow-on-run")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_mode": "pr_review",
                "coder_run_id": "coder-merge-submit-latest-review-block"
            })
            .to_string(),
        ))
        .expect("second review follow-on request");
    let second_review_follow_on_resp = app
        .clone()
        .oneshot(second_review_follow_on_req)
        .await
        .expect("second review follow-on response");
    assert_eq!(second_review_follow_on_resp.status(), StatusCode::OK);

    let second_review_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-latest-review-block/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "changes_requested",
                "summary": "A newer review found rollback coverage gaps.",
                "risk_level": "medium",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "blockers": [],
                "requested_changes": ["Add rollback coverage"],
                "regression_signals": []
            })
            .to_string(),
        ))
        .expect("second review summary request");
    let second_review_summary_resp = app
        .clone()
        .oneshot(second_review_summary_req)
        .await
        .expect("second review summary response");
    assert_eq!(second_review_summary_resp.status(), StatusCode::OK);

    let merge_follow_on_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-latest-review-parent/follow-on-run")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_mode": "merge_recommendation",
                "coder_run_id": "coder-merge-submit-latest-review-merge"
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

    let merge_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-latest-review-merge/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "merge",
                "summary": "Everything looks ready from the merge side.",
                "blockers": [],
                "required_checks": [],
                "required_approvals": []
            })
            .to_string(),
        ))
        .expect("merge summary request");
    let merge_summary_resp = app
        .clone()
        .oneshot(merge_summary_req)
        .await
        .expect("merge summary response");
    assert_eq!(merge_summary_resp.status(), StatusCode::OK);

    let approve_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-latest-review-merge/approve")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reason": "Operator approved merge execution."
            })
            .to_string(),
        ))
        .expect("approve request");
    let approve_resp = app
        .clone()
        .oneshot(approve_req)
        .await
        .expect("approve response");
    assert_eq!(approve_resp.status(), StatusCode::OK);

    let submit_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-submit-latest-review-merge/merge-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "user123",
                "reason": "Try to merge using the older approval",
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
    server.abort();
    assert_eq!(submit_resp.status(), StatusCode::OK);
    let submit_payload: Value = serde_json::from_slice(
        &to_bytes(submit_resp.into_body(), usize::MAX)
            .await
            .expect("submit body"),
    )
    .expect("submit json");
    assert_eq!(
        submit_payload.get("ok").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        submit_payload.get("code").and_then(Value::as_str),
        Some("CODER_MERGE_SUBMIT_POLICY_BLOCKED")
    );
    assert_eq!(
        submit_payload
            .get("policy")
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_approved_pr_review_follow_on")
    );
    assert_eq!(
        submit_payload
            .get("policy")
            .and_then(|row| row.get("review_verdict"))
            .and_then(Value::as_str),
        Some("changes_requested")
    );
}

#[tokio::test]
async fn coder_merge_recommendation_reuses_prior_memory_hits() {
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
                "coder_run_id": "coder-merge-recommendation-baseline",
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
        .expect("baseline create request");
    let create_baseline_resp = app
        .clone()
        .oneshot(create_baseline_req)
        .await
        .expect("baseline create response");
    assert_eq!(create_baseline_resp.status(), StatusCode::OK);

    let baseline_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-recommendation-baseline/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "hold",
                "summary": "Hold merge pending final manual verification."
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
                "coder_run_id": "coder-merge-recommendation-a",
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
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-recommendation-a/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "hold",
                "summary": "Hold merge until the final approval lands and the rollout note is attached.",
                "risk_level": "medium",
                "blockers": ["Required reviewer approval missing"],
                "required_checks": ["ci / test"],
                "required_approvals": ["codeowners"],
                "memory_hits_used": ["memory-hit-merge-a"]
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
                "coder_run_id": "coder-merge-recommendation-b",
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
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-merge-recommendation-b/memory-hits")
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
        Some(
            "user123/tandem pull request #93 merge recommendation regressions blockers required checks approvals"
        )
    );
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("merge_recommendation_memory")
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
        Some("coder-merge-recommendation-a")
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
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("merge_recommendation_memory")
                && (row.get("source_coder_run_id").and_then(Value::as_str)
                    == Some("coder-merge-recommendation-a")
                    || row.get("run_id").and_then(Value::as_str)
                        == Some("coder-merge-recommendation-a"))
        }))
        .unwrap_or(false));
}

#[tokio::test]
async fn coder_run_approve_and_cancel_project_context_run_controls() {
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
                "coder_run_id": "coder-run-controls",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 15
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
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("create json");
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run")
        .to_string();

    let plan_req = Request::builder()
        .method("POST")
        .uri(format!("/context/runs/{linked_context_run_id}/events"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "type": "planning_started",
                "status": "awaiting_approval",
                "payload": {}
            })
            .to_string(),
        ))
        .expect("plan request");
    let plan_resp = app.clone().oneshot(plan_req).await.expect("plan response");
    assert_eq!(plan_resp.status(), StatusCode::OK);

    let approve_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-controls/approve")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reason": "approve coder plan"
            })
            .to_string(),
        ))
        .expect("approve request");
    let approve_resp = app
        .clone()
        .oneshot(approve_req)
        .await
        .expect("approve response");
    assert_eq!(approve_resp.status(), StatusCode::OK);
    let approve_body = to_bytes(approve_resp.into_body(), usize::MAX)
        .await
        .expect("approve body");
    let approve_payload: Value = serde_json::from_slice(&approve_body).expect("approve json");
    assert_eq!(
        approve_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("repo_inspection")
    );
    assert_eq!(
        approve_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );

    let cancel_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-controls/cancel")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reason": "stop this coder run"
            })
            .to_string(),
        ))
        .expect("cancel request");
    let cancel_resp = app
        .clone()
        .oneshot(cancel_req)
        .await
        .expect("cancel response");
    assert_eq!(cancel_resp.status(), StatusCode::OK);
    let cancel_body = to_bytes(cancel_resp.into_body(), usize::MAX)
        .await
        .expect("cancel body");
    let cancel_payload: Value = serde_json::from_slice(&cancel_body).expect("cancel json");
    assert_eq!(
        cancel_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("cancelled")
    );
    assert_eq!(
        cancel_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("cancelled")
    );
    assert_eq!(
        cancel_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("run_outcome")
    );

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-controls/memory-candidates")
        .body(Body::empty())
        .expect("candidates request");
    let candidates_resp = app
        .clone()
        .oneshot(candidates_req)
        .await
        .expect("candidates response");
    assert_eq!(candidates_resp.status(), StatusCode::OK);
    let candidates_body = to_bytes(candidates_resp.into_body(), usize::MAX)
        .await
        .expect("candidates body");
    let candidates_payload: Value =
        serde_json::from_slice(&candidates_body).expect("candidates json");
    let run_outcome = candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("kind").and_then(Value::as_str) == Some("run_outcome"))
        })
        .expect("run outcome candidate");
    assert_eq!(
        run_outcome
            .get("payload")
            .and_then(|row| row.get("result"))
            .and_then(Value::as_str),
        Some("cancelled")
    );
    assert_eq!(
        run_outcome
            .get("payload")
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("stop this coder run")
    );
}

#[tokio::test]
async fn coder_issue_triage_run_replay_matches_persisted_state_and_checkpoint() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let (_create_payload, linked_context_run_id) = create_coder_run_for_replay(
        app.clone(),
        json!({
            "coder_run_id": "coder-run-replay",
            "workflow_mode": "issue_triage",
            "repo_binding": {
                "project_id": "proj-engine",
                "workspace_id": "ws-tandem",
                "workspace_root": "/tmp/tandem-repo",
                "repo_slug": "user123/tandem",
                "default_branch": "main"
            },
            "github_ref": {
                "kind": "issue",
                "number": 404,
                "url": "https://github.com/user123/tandem/issues/404"
            }
        }),
    )
    .await;
    let replay_payload = checkpoint_and_replay_coder_run(app.clone(), &linked_context_run_id).await;

    assert_eq!(
        replay_payload
            .get("drift")
            .and_then(|row| row.get("mismatch"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        replay_payload
            .get("from_checkpoint")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        replay_payload
            .get("replay")
            .and_then(|row| row.get("run_type"))
            .and_then(Value::as_str),
        Some("coder_issue_triage")
    );
    assert_eq!(
        replay_payload
            .get("replay_blackboard")
            .and_then(|row| row.get("artifacts"))
            .and_then(Value::as_array)
            .map(|rows| rows.iter().any(|row| {
                row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_hits")
            })),
        Some(true)
    );
    assert_eq!(
        replay_payload
            .get("replay_blackboard")
            .and_then(|row| row.get("tasks"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(5)
    );
}
